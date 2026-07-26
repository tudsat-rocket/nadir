//! A set of systems and the message store they share.
//!
//! Telemetry reaches the ground station two ways: live, off the links owned by [`Core`], or read
//! back out of a recording. Both end up in a [`Source`], and everything above this layer - the
//! protocol state machines, the database queries, the whole GUI - works the same either way.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, mpsc};

use maviola::asnc::node::Callback;
use maviola::prelude::{Frame, V2};
use maviola::protocol::SystemId;
use maviola::protocol::dialects::{Ardupilotmega, Common};
use mavspec::rust::dialects::common::enums::MavSeverity;

use db::Db;

use crate::system::System;
use crate::tlog;

/// Half the per-system broadcast capacity in `system.rs`.
const INGEST_BATCH: usize = 256;

/// Where a [`Source`]'s data comes from.
#[derive(Clone)]
pub enum Origin {
    /// Fed by the links, in real time.
    Live,
    /// Loaded from a telemetry log on disk.
    Log(Arc<LogProgress>),
}

impl Origin {
    /// The latest moment this source knows about.
    pub fn now(&self) -> DateTime<Utc> {
        match self {
            Self::Live => Utc::now(),
            Self::Log(progress) => progress.cursor(),
        }
    }
}

/// How far through a telemetry log the loader has got.
pub struct LogProgress {
    pub path: PathBuf,
    pub total_bytes: u64,
    read_bytes: AtomicU64,
    cursor_micros: AtomicI64,
    records: AtomicU64,
    errors: AtomicU64,
    done: AtomicBool,
    cancelled: AtomicBool,
}

impl LogProgress {
    /// Timestamp of the most recent record ingested.
    pub fn cursor(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(self.cursor_micros.load(Ordering::Relaxed))
            .unwrap_or_default()
    }

    pub fn read(&self) -> u64 {
        self.read_bytes.load(Ordering::Relaxed)
    }

    pub fn records(&self) -> u64 {
        self.records.load(Ordering::Relaxed)
    }

    /// Records the reader could not make sense of. Nonzero means the file was damaged or truncated,
    /// not that loading failed.
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    pub fn done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    /// Fraction of the file consumed.
    pub fn fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 1.0;
        }

        (self.read() as f32 / self.total_bytes as f32).clamp(0.0, 1.0)
    }

    /// Asks the loader to stop early, for a log closed while it is still loading.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("Failed to open the telemetry log: {0}")]
    Open(#[from] std::io::Error),
    #[error("{0} holds no telemetry")]
    Empty(PathBuf),
}

#[derive(Clone)]
pub struct Source {
    pub db: Db,
    /// Absent for a recording, which is already on disk and has nothing to record.
    pub tlog: Option<tlog::Writer>,
    pub systems: Arc<Mutex<HashMap<SystemId, System>>>,
    /// Zero point of the plot time axis. Fixed for the life of the source, so that panning a plot
    /// does not shift the data under the cursor.
    pub plot_origin: DateTime<Utc>,
    pub origin: Origin,
    can_proxy: Option<(
        mpsc::Sender<socketcan::CanFrame>,
        broadcast::Sender<socketcan::CanFrame>,
    )>,
}

impl Source {
    /// The source the links feed.
    pub(crate) fn live(
        can_proxy: Option<(
            mpsc::Sender<socketcan::CanFrame>,
            broadcast::Sender<socketcan::CanFrame>,
        )>,
    ) -> Self {
        Self {
            db: Db::init(),
            tlog: Some(tlog::Writer::spawn()),
            systems: Arc::new(Mutex::new(HashMap::new())),
            plot_origin: Utc::now(),
            origin: Origin::Live,
            can_proxy,
        }
    }

    /// Opens a telemetry log and starts loading it.
    ///
    /// Returns as soon as the file is known to hold telemetry; the rest arrives on a thread of its
    /// own.
    pub fn open_log(path: &Path) -> Result<Self, LogError> {
        let total_bytes = std::fs::metadata(path)?.len();

        // The plot origin has to be known before anything is drawn against it, and reading the
        // first record is also what tells us this is a telemetry log at all.
        let first = tlog::Reader::open(path)?
            .flatten()
            .next()
            .ok_or_else(|| LogError::Empty(path.to_path_buf()))?;

        let progress = Arc::new(LogProgress {
            path: path.to_path_buf(),
            total_bytes,
            read_bytes: AtomicU64::new(0),
            cursor_micros: AtomicI64::new(first.received_at.timestamp_micros()),
            records: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            done: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        });

        let source = Self {
            db: Db::init(),
            tlog: None,
            systems: Arc::new(Mutex::new(HashMap::new())),
            plot_origin: first.received_at,
            origin: Origin::Log(Arc::clone(&progress)),
            can_proxy: None,
        };

        let loader = source.clone();
        let reader = tlog::Reader::open(path)?;

        std::thread::spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(loader.load(reader, &progress)),
                Err(e) => tracing::error!("Failed to start the telemetry log loader: {e}"),
            }
        });

        Ok(source)
    }

    /// The latest moment this source knows about: see [`Origin::now`].
    pub fn now(&self) -> DateTime<Utc> {
        self.origin.now()
    }

    pub fn known_system_ids(&self) -> Vec<SystemId> {
        let mut system_ids: Vec<SystemId> = self.systems.lock().unwrap().keys().copied().collect();
        system_ids.sort_unstable();
        system_ids.dedup();
        system_ids
    }

    pub fn system(&self, id: SystemId) -> Option<System> {
        self.systems.lock().unwrap().get(&id).cloned()
    }

    async fn load<R: std::io::Read>(self, mut reader: tlog::Reader<R>, progress: &LogProgress) {
        let mut since_yield = 0;

        while let Some(result) = reader.next() {
            if progress.cancelled.load(Ordering::Relaxed) {
                tracing::info!("Cancelled loading {}", progress.path.display());
                break;
            }

            match result {
                Ok(record) => {
                    self.ingest(&record.frame, record.received_at, None);

                    progress
                        .cursor_micros
                        .store(record.received_at.timestamp_micros(), Ordering::Relaxed);
                    progress.records.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    tracing::warn!("{} in {}", e, progress.path.display());
                    progress.errors.fetch_add(1, Ordering::Relaxed);
                }
            }

            progress
                .read_bytes
                .store(reader.consumed(), Ordering::Relaxed);

            since_yield += 1;
            if since_yield >= INGEST_BATCH {
                since_yield = 0;
                tokio::task::yield_now().await;
            }
        }

        progress.done.store(true, Ordering::Relaxed);
        progress
            .read_bytes
            .store(progress.total_bytes, Ordering::Relaxed);

        tracing::info!(
            "Loaded {} records from {} ({} unreadable)",
            progress.records(),
            progress.path.display(),
            progress.errors(),
        );
    }

    /// Files one frame: decodes it, stores it, and hands it to its system's protocol tasks.
    pub(crate) fn ingest(
        &self,
        frame: &Frame<V2>,
        received_at: DateTime<Utc>,
        callback: Option<&Callback<V2>>,
    ) {
        let mut systems = self.systems.lock().unwrap();
        let mut system = match frame.system_id() {
            crate::GROUND_STATION_SYSTEM_ID | crate::OTHER_GROUND_STATION_SYSTEM_ID => None,
            system_id => Some(systems.entry(system_id).or_insert_with(|| {
                System::new(
                    system_id,
                    self.db.clone(),
                    self.tlog.clone(),
                    self.origin.clone(),
                    callback.cloned(),
                    self.can_proxy.clone(),
                )
            })),
        };

        if let Ok(message) = frame.decode::<Common>() {
            if let Common::Statustext(inner) = &message {
                log_statustext(frame, inner.severity, &inner.text);
            }

            self.write(frame, &message, received_at);

            if let Some(system) = &mut system {
                system.notify_of_common_message(message, frame, callback);
            }
        } else if let Ok(message) = frame.decode::<Ardupilotmega>() {
            self.write(frame, &message, received_at);

            if let Some(system) = &mut system {
                system.notify_of_frame(frame, callback);
            }
        } else if let Ok(message) = frame.decode::<rapid_dialect::Rapid>() {
            self.write(frame, &message, received_at);

            if let Some(system) = &mut system {
                system.notify_of_frame(frame, callback);
            }
        }
    }

    fn write<M: db::MessageExt>(&self, frame: &Frame<V2>, message: &M, received_at: DateTime<Utc>) {
        if let Err(e) = self.db.write_message_at(
            frame.system_id(),
            frame.component_id(),
            message,
            received_at,
        ) {
            tracing::error!("Failed to process message: {e:?}");
        }
    }
}

// TODO: move this to its own protocol task as well to get it out of here
fn log_statustext(frame: &Frame<V2>, severity: MavSeverity, text: &[u8]) {
    let system_id = frame.system_id();
    let component_id = frame.component_id();
    let text = String::from_utf8_lossy(text);

    match severity {
        MavSeverity::Debug => tracing::debug!(system_id, component_id, "{}", &text),
        MavSeverity::Info | MavSeverity::Notice => {
            tracing::info!(system_id, component_id, "{}", &text);
        }
        MavSeverity::Warning => tracing::warn!(system_id, component_id, "{}", &text),
        MavSeverity::Error
        | MavSeverity::Alert
        | MavSeverity::Critical
        | MavSeverity::Emergency => tracing::error!(system_id, component_id, "{}", &text),
    }
}
