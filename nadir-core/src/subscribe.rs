//! Reading a session shared through a `meridian` service.

use std::sync::Arc;
use std::time::Duration;

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::mav::SystemId;
use crate::publish::SessionAddr;
use crate::source::{INGEST_BATCH, Origin, Source, StreamProgress};
use crate::tlog;

/// Telemetry never pauses this long on a live session, so a silent connection is a dead one.
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);
/// How recently a session must have been appended to to count as live.
const LIVE_WITHIN: Duration = Duration::from_secs(10);

/// One line of the service's session listing.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub id: String,
    pub bytes: u64,
    pub idle: Duration,
}

impl SessionSummary {
    /// Parses `GET /sessions`, skipping lines it does not understand.
    pub fn parse_listing(listing: &str) -> Vec<Self> {
        listing
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let id = fields.next()?.to_owned();
                let bytes = fields.next()?.parse().ok()?;
                let idle = Duration::from_secs(fields.next()?.parse().ok()?);

                Some(Self { id, bytes, idle })
            })
            .collect()
    }

    pub fn live(&self) -> bool {
        self.idle < LIVE_WITHIN
    }

    /// Station, system and start time, if the id was made by [`SessionAddr::new`].
    pub fn parts(&self) -> Option<(&str, SystemId, DateTime<Utc>)> {
        let mut parts = self.id.rsplitn(3, '-');
        let started =
            NaiveDateTime::parse_from_str(parts.next()?, SessionAddr::STARTED_FORMAT).ok()?;
        let system_id = SystemId::from_str_radix(parts.next()?, 16).ok()?;
        let station = parts.next()?;

        Some((station, system_id, started.and_utc()))
    }
}

impl StreamProgress {
    pub(crate) fn new(session: String, url: String) -> Self {
        Self {
            session,
            url,
            cursor_micros: AtomicI64::new(0),
            records: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            connected: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub(crate) fn set_bytes(&self, bytes: u64) {
        self.bytes.store(bytes, Ordering::Relaxed);
    }

    pub(crate) fn push_record(&self, received_at: DateTime<Utc>) {
        self.cursor_micros
            .store(received_at.timestamp_micros(), Ordering::Relaxed);
        self.records.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn push_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
}

impl Source {
    /// Starts filling a read-only source from a shared session, on a thread of its own.
    ///
    /// `notify` is called after each batch of records, for a UI that needs telling to redraw.
    pub fn stream(addr: &SessionAddr, notify: impl Fn() + Send + 'static) -> Self {
        let progress = Arc::new(StreamProgress::new(addr.id.clone(), addr.url()));
        let source = Self::fed(Origin::Stream(Arc::clone(&progress)), Utc::now());

        let feed = source.clone();
        std::thread::spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(feed.subscribe(&progress, &notify)),
                Err(e) => tracing::error!("Failed to start reading {}: {e}", progress.session),
            }
        });

        source
    }

    async fn subscribe(self, progress: &StreamProgress, notify: &dyn Fn()) {
        let client = match reqwest::Client::builder()
            .read_timeout(READ_TIMEOUT)
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                tracing::error!("Failed to start reading {}: {e}", progress.session);
                return;
            }
        };

        let mut logged_failure = false;

        while !progress.cancelled() {
            match self
                .read_session(&client, progress, notify, &mut logged_failure)
                .await
            {
                Ok(()) => tracing::info!("Session {} ended, reconnecting", progress.session),
                Err(e) if !logged_failure => {
                    tracing::warn!(
                        "Cannot read session {}, will keep trying: {e}",
                        progress.session
                    );
                    logged_failure = true;
                }
                Err(_) => {}
            }

            progress.set_connected(false);
            crate::time::sleep(RECONNECT_INTERVAL).await;
        }
    }

    async fn read_session(
        &self,
        client: &reqwest::Client,
        progress: &StreamProgress,
        notify: &dyn Fn(),
        logged_failure: &mut bool,
    ) -> Result<(), reqwest::Error> {
        let url = match progress.epoch.load(Ordering::Relaxed) {
            0 => format!("{}?offset={}", progress.url, progress.bytes()),
            epoch => format!("{}?offset={}&epoch={epoch}", progress.url, progress.bytes()),
        };

        let mut response = client.get(url).send().await?.error_for_status()?;

        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
        };
        let resume_from = header("x-session-offset").unwrap_or(0);
        let epoch = header("x-session-epoch").unwrap_or(0);

        if resume_from != progress.bytes() {
            tracing::warn!(
                "Session {} restarted on the service, reading it from the start",
                progress.session
            );
            progress.set_bytes(resume_from);
        }
        progress.epoch.store(epoch, Ordering::Relaxed);

        *logged_failure = false;
        progress.set_connected(true);
        tracing::info!("Reading session {}", progress.session);

        let mut decoder = tlog::Decoder::new();
        let mut in_batch = 0;

        while let Some(chunk) = response.chunk().await? {
            if progress.cancelled() {
                return Ok(());
            }

            decoder.push(&chunk);

            while let Some(record) = decoder.pop() {
                match record {
                    Ok(record) => {
                        self.ingest(&record.frame, record.received_at, None);
                        progress.push_record(record.received_at);
                    }
                    Err(e) => {
                        tracing::warn!("{e} in session {}", progress.session);
                        progress.push_error();
                    }
                }

                in_batch += 1;
                if in_batch >= INGEST_BATCH {
                    in_batch = 0;
                    progress.set_bytes(resume_from + decoder.consumed());
                    notify();
                    crate::task::yield_now().await;
                }
            }

            progress.set_bytes(resume_from + decoder.consumed());
            notify();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_parses_line_by_line() {
        let listing = "a-01-20261011T120000\t1234\t3\nnonsense\nb-02-20261011T130000\t0\t600\n";

        assert_eq!(
            SessionSummary::parse_listing(listing),
            vec![
                SessionSummary {
                    id: "a-01-20261011T120000".to_owned(),
                    bytes: 1234,
                    idle: Duration::from_secs(3),
                },
                SessionSummary {
                    id: "b-02-20261011T130000".to_owned(),
                    bytes: 0,
                    idle: Duration::from_secs(600),
                },
            ]
        );
    }

    #[test]
    fn an_id_splits_into_station_system_and_start() {
        let summary = |id: &str| SessionSummary {
            id: id.to_owned(),
            bytes: 0,
            idle: Duration::ZERO,
        };

        let made = summary(&SessionAddr::new(String::new(), "pad 2/north", 0x2a).id);
        let (station, system_id, started) = made.parts().unwrap();
        assert_eq!((station, system_id), ("pad-2-north", 0x2a));
        assert!((Utc::now() - started).num_seconds() < 5);

        assert_eq!(summary("handmade").parts(), None);
        assert_eq!(summary("a-zz-20261011T120000").parts(), None);
    }
}
