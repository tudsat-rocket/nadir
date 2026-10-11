//! Mirroring one system's telemetry up to a `meridian` session.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use mavspec::rust::dialects::common::messages::{CanFrame, CanfdFrame};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{Receiver, Sender, channel};

use crate::mav::{Frame, SystemId, V2};
use crate::tlog;

/// Enough to ride out a request timing out at full telemetry rate.
const QUEUE_CAPACITY: usize = 16 * 1024;
const MAX_BATCH_BYTES: usize = 1024 * 1024;
/// Records waiting for the service beyond this are dropped, oldest first.
const MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAddr {
    /// Base URL of the service, e.g. `http://meridian:7654`.
    pub server: String,
    pub id: String,
}

impl SessionAddr {
    /// A new session for `system_id`, named so that a list of them reads well.
    pub fn new(server: String, station: &str, system_id: SystemId) -> Self {
        let station: String = station
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .take(32)
            .collect();
        let started = Utc::now().format("%Y%m%dT%H%M%S");

        Self {
            server,
            id: format!("{station}-{system_id:02x}-{started}"),
        }
    }

    pub fn url(&self) -> String {
        format!("{}/sessions/{}", self.server.trim_end_matches('/'), self.id)
    }
}

/// Mirrors frames to a session for as long as it is held.
pub struct Publisher {
    pub addr: SessionAddr,
    records: Sender<Vec<u8>>,
    status: Arc<PublishStatus>,
}

#[derive(Default)]
pub struct PublishStatus {
    connected: AtomicBool,
    acked_bytes: AtomicU64,
    pending_bytes: AtomicU64,
    dropped: AtomicU64,
}

impl PublishStatus {
    /// Whether the last request reached the service.
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn acked_bytes(&self) -> u64 {
        self.acked_bytes.load(Ordering::Relaxed)
    }

    pub fn pending_bytes(&self) -> u64 {
        self.pending_bytes.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Publisher {
    /// Starts publishing on a thread of its own, which ends when the publisher is dropped.
    pub fn spawn(addr: SessionAddr) -> Self {
        let (records, receiver) = channel(QUEUE_CAPACITY);
        let status = Arc::new(PublishStatus::default());

        let task = Task {
            url: addr.url(),
            receiver,
            status: Arc::clone(&status),
            pending: VecDeque::new(),
            pending_bytes: 0,
            acked: 0,
        };

        std::thread::spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(task.run()),
                Err(e) => tracing::error!("Failed to start publishing: {e}"),
            }
        });

        Self {
            addr,
            records,
            status,
        }
    }

    pub fn status(&self) -> Arc<PublishStatus> {
        Arc::clone(&self.status)
    }

    /// Queues one frame. Never blocks: a stalled service costs records, not the caller's time.
    pub fn publish(&self, received_at: DateTime<Utc>, frame: &Frame<V2>) {
        // Bulky, and only of use on a bus the reader is attached to.
        if matches!(frame.message_id(), CanFrame::ID | CanfdFrame::ID) {
            return;
        }

        let record = match tlog::record(received_at, frame) {
            Ok(record) => record,
            Err(e) => {
                tracing::error!("Failed to serialize a frame for publishing: {e:?}");
                return;
            }
        };

        if self.records.try_send(record).is_err() {
            self.status.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

struct Task {
    url: String,
    receiver: Receiver<Vec<u8>>,
    status: Arc<PublishStatus>,
    /// Records not yet acknowledged by the service, oldest first.
    pending: VecDeque<Vec<u8>>,
    pending_bytes: usize,
    /// Where the first pending record goes in the session.
    acked: u64,
}

impl Task {
    async fn run(mut self) {
        let url = self.url.clone();
        let client = match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build() {
            Ok(client) => client,
            Err(e) => {
                tracing::error!("Failed to start publishing: {e}");
                return;
            }
        };

        tracing::info!("Publishing to {url}");
        let mut logged_failure = false;

        while self.receive().await {
            let (batch, records) = self.batch();

            let result = client
                .post(format!("{url}?offset={}", self.acked))
                .body(batch)
                .send()
                .await;

            let failure = match result {
                Ok(response) if response.status().is_success() => {
                    self.acknowledge(records);
                    None
                }
                Ok(response) if response.status() == reqwest::StatusCode::CONFLICT => {
                    match response.text().await.map(|body| body.trim().parse::<u64>()) {
                        Ok(Ok(len)) => {
                            self.resume_at(len);
                            None
                        }
                        _ => Some("unreadable conflict response".to_owned()),
                    }
                }
                Ok(response) => Some(format!("HTTP {}", response.status())),
                Err(e) => Some(e.to_string()),
            };

            self.status
                .connected
                .store(failure.is_none(), Ordering::Relaxed);
            self.status.acked_bytes.store(self.acked, Ordering::Relaxed);
            self.status
                .pending_bytes
                .store(self.pending_bytes as u64, Ordering::Relaxed);

            match failure {
                None if logged_failure => {
                    tracing::info!("Publishing to {url} again");
                    logged_failure = false;
                }
                None => {}
                Some(e) => {
                    if !logged_failure {
                        tracing::warn!("Failed to publish to {url}, will keep trying: {e}");
                        logged_failure = true;
                    }
                    crate::time::sleep(RETRY_INTERVAL).await;
                }
            }
        }

        tracing::info!("Stopped publishing to {url}");
    }

    /// Moves queued records into `pending`, waiting for one if there are none. `false` once the
    /// publisher has been dropped.
    async fn receive(&mut self) -> bool {
        if self.pending.is_empty() {
            let Some(record) = self.receiver.recv().await else {
                return false;
            };
            self.push(record);
        }

        loop {
            match self.receiver.try_recv() {
                Ok(record) => self.push(record),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn push(&mut self, record: Vec<u8>) {
        self.pending_bytes += record.len();
        self.pending.push_back(record);

        while self.pending_bytes > MAX_PENDING_BYTES {
            let Some(oldest) = self.pending.pop_front() else {
                break;
            };
            self.pending_bytes -= oldest.len();
            self.status.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The oldest pending records, concatenated, and how many there are.
    fn batch(&self) -> (Vec<u8>, usize) {
        let mut batch = Vec::new();
        let mut records = 0;

        for record in &self.pending {
            if !batch.is_empty() && batch.len() + record.len() > MAX_BATCH_BYTES {
                break;
            }
            batch.extend_from_slice(record);
            records += 1;
        }

        (batch, records)
    }

    fn acknowledge(&mut self, records: usize) {
        for record in self.pending.drain(..records) {
            self.pending_bytes -= record.len();
            self.acked += record.len() as u64;
        }
    }

    /// Continues from where the service says the session ends.
    ///
    /// Longer than expected means a batch arrived whose response did not. Shorter means the service
    /// lost what it had, and the pending records follow on with a gap.
    fn resume_at(&mut self, len: u64) {
        while len > self.acked {
            let Some(record) = self.pending.pop_front() else {
                break;
            };
            self.pending_bytes -= record.len();
            self.acked += record.len() as u64;
        }

        if self.acked != len {
            tracing::warn!(
                "{} holds {len} bytes where {} were expected, continuing with a gap",
                self.url,
                self.acked
            );
        }

        self.acked = len;
    }
}
