//! Telemetry logging in the `.tlog` format written by `QGroundControl` and Mission Planner: a bare
//! concatenation of records, each an 8-byte big-endian unix-microsecond timestamp followed by one
//! complete `MAVLink` frame. There is no file header, and records are self-delimiting because the
//! frame header carries its own payload length.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use chrono::Utc;
use directories::ProjectDirs;
use maviola::error::FrameError;
use maviola::prelude::{Frame, V2};
use maviola::protocol::SystemId;

const QUEUE_CAPACITY: usize = 4096;
const BUFFER_CAPACITY: usize = 64 * 1024;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const TIMESTAMP_LEN: usize = 8;
const STAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

struct Record {
    system_id: SystemId,
    bytes: Vec<u8>,
}

/// Handle for appending frames to the telemetry log, cheap to clone.
#[derive(Clone)]
pub struct Writer {
    records: Option<SyncSender<Record>>,
    dropped: Arc<AtomicU64>,
}

impl Writer {
    /// Starts the writer thread, creating the log directory if it does not exist yet. Without a
    /// usable directory the returned writer discards everything: failing to log must never keep the
    /// ground station from operating.
    pub fn spawn() -> Self {
        let dropped = Arc::new(AtomicU64::new(0));

        let Some(dir) = log_dir() else {
            return Self {
                records: None,
                dropped,
            };
        };

        let (sender, receiver) = sync_channel(QUEUE_CAPACITY);

        let sink = Sink {
            dir,
            stamp: Utc::now().format(STAMP_FORMAT).to_string(),
            files: HashMap::new(),
            dropped: Arc::clone(&dropped),
        };

        std::thread::spawn(move || sink.run(&receiver));

        Self {
            records: Some(sender),
            dropped,
        }
    }

    /// Appends a frame to the log of `system_id`.
    ///
    /// Received frames belong to the system that sent them; sent frames belong to the system they
    /// are addressed to, so that one file holds both halves of a conversation.
    pub fn log(&self, system_id: SystemId, frame: &Frame<V2>) {
        let Some(records) = &self.records else {
            return;
        };

        let bytes = match encode(now_micros(), frame) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to serialize frame for the telemetry log: {e:?}");
                return;
            }
        };

        if records.try_send(Record { system_id, bytes }).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Owns every open log file. Runs on its own thread so that a slow or full disk can only ever
/// delay the log, never the frames flowing through `Core`.
struct Sink {
    dir: PathBuf,
    stamp: String,
    files: HashMap<SystemId, Option<BufWriter<File>>>,
    dropped: Arc<AtomicU64>,
}

impl Sink {
    fn run(mut self, records: &Receiver<Record>) {
        let mut last_flush = Instant::now();

        loop {
            match records.recv_timeout(FLUSH_INTERVAL) {
                Ok(record) => self.write(&record),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            // Elapsed time is checked separately from the receive timeout, which never fires while
            // frames keep arriving.
            if last_flush.elapsed() >= FLUSH_INTERVAL {
                self.flush();
                self.report_dropped();
                last_flush = Instant::now();
            }
        }

        self.flush();
    }

    fn write(&mut self, record: &Record) {
        if !self.files.contains_key(&record.system_id) {
            let file = self.open(record.system_id);
            self.files.insert(record.system_id, file);
        }

        let Some(Some(file)) = self.files.get_mut(&record.system_id) else {
            return;
        };

        if let Err(e) = file.write_all(&record.bytes) {
            let system_id = record.system_id;
            tracing::error!("Failed to write telemetry log for system {system_id:#04x}: {e}");
            self.files.insert(system_id, None);
        }
    }

    fn open(&self, system_id: SystemId) -> Option<BufWriter<File>> {
        let path = self
            .dir
            .join(format!("{}-{system_id:02x}.tlog", self.stamp));

        match File::options().create(true).append(true).open(&path) {
            Ok(file) => {
                tracing::info!(
                    "Logging telemetry for system {system_id:#04x} to {}",
                    path.display()
                );
                Some(BufWriter::with_capacity(BUFFER_CAPACITY, file))
            }
            Err(e) => {
                tracing::error!("Failed to open {}: {e}", path.display());
                None
            }
        }
    }

    fn flush(&mut self) {
        for file in self.files.values_mut().flatten() {
            if let Err(e) = file.flush() {
                tracing::error!("Failed to flush telemetry log: {e}");
            }
        }
    }

    fn report_dropped(&self) {
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            tracing::warn!("Telemetry log queue is full, dropped {dropped} frames");
        }
    }
}

fn now_micros() -> u64 {
    // pymavlink stores a link index in the low two bits of the timestamp, so leave them clear.
    u64::try_from(Utc::now().timestamp_micros()).unwrap_or(0) & !3
}

/// Lays out one record: timestamp, then the frame verbatim.
fn encode(received_at_micros: u64, frame: &Frame<V2>) -> Result<Vec<u8>, FrameError> {
    // The buffer has to start zeroed: `serialize` writes the checksum at the offset the header
    // declares and leaves the gap alone where the payload was truncated on the wire.
    let mut record = vec![0u8; TIMESTAMP_LEN + frame.size()];

    record[..TIMESTAMP_LEN].copy_from_slice(&received_at_micros.to_be_bytes());
    frame.serialize(&mut record[TIMESTAMP_LEN..])?;

    Ok(record)
}

fn log_dir() -> Option<PathBuf> {
    let Some(dirs) = ProjectDirs::from("space", "tudsat", "nadir") else {
        tracing::error!("Could not determine a data directory, telemetry logging is disabled");
        return None;
    };

    let dir = dirs.data_dir().join("telemetry");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::error!(
            "Failed to create {}: {e}, telemetry logging is disabled",
            dir.display()
        );
        return None;
    }

    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    use maviola::prelude::{Endpoint, MavLinkId};
    use mavspec::rust::dialects::common::messages::Heartbeat;

    const HEADER_LEN: usize = 10;
    const MAGIC_V2: u8 = 0xfd;

    fn frame() -> Frame<V2> {
        let endpoint: Endpoint<V2> = Endpoint::new(MavLinkId {
            system: 0x2a,
            component: 0x07,
        });

        endpoint
            .next_frame(&Heartbeat {
                mavlink_version: 3,
                ..Default::default()
            })
            .unwrap()
    }

    #[test]
    fn record_is_a_timestamp_followed_by_the_frame() {
        let frame = frame();
        let record = encode(0x0011_2233_4455_6670, &frame).unwrap();

        assert_eq!(record.len(), TIMESTAMP_LEN + frame.size());
        assert_eq!(
            &record[..TIMESTAMP_LEN],
            &[0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x70]
        );

        let body = &record[TIMESTAMP_LEN..];
        assert_eq!(body[0], MAGIC_V2);
        assert_eq!(body[1], frame.payload_length());
        assert_eq!(body[4], frame.sequence());
        assert_eq!(body[5], 0x2a);
        assert_eq!(body[6], 0x07);
    }

    #[test]
    fn checksum_lands_at_the_declared_payload_length() {
        let frame = frame();
        let record = encode(0, &frame).unwrap();

        let body = &record[TIMESTAMP_LEN..];
        let checksum_at = HEADER_LEN + frame.payload_length() as usize;

        assert_eq!(
            &body[checksum_at..checksum_at + 2],
            &frame.checksum().to_le_bytes()
        );
    }

    #[test]
    fn timestamps_leave_the_link_index_bits_clear() {
        assert_eq!(now_micros() & 3, 0);
    }

    #[test]
    fn a_writer_without_a_log_directory_discards_frames() {
        let writer = Writer {
            records: None,
            dropped: Arc::new(AtomicU64::new(0)),
        };

        writer.log(1, &frame());

        assert_eq!(writer.dropped.load(Ordering::Relaxed), 0);
    }
}
