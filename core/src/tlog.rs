//! Telemetry logging in the `.tlog` format written by `QGroundControl` and Mission Planner: a bare
//! concatenation of records, each an 8-byte big-endian unix-microsecond timestamp followed by one
//! complete `MAVLink` frame. There is no file header, and records are self-delimiting because the
//! frame header carries its own payload length.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use directories::ProjectDirs;
use maviola::error::FrameError;
use maviola::prelude::{Frame, V2};
use maviola::protocol::SystemId;

const QUEUE_CAPACITY: usize = 4096;
const BUFFER_CAPACITY: usize = 64 * 1024;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const STAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";
const EXTENSION: &str = "tlog";

const TIMESTAMP_LEN: usize = 8;
const HEADER_LEN: usize = 10;
const CHECKSUM_LEN: usize = 2;
const SIGNATURE_LEN: usize = 13;
const MAGIC_V2: u8 = 0xfd;
const INCOMPAT_SIGNED: u8 = 0x01;

/// Anything stamped before this is a lost record boundary rather than a real time.
const EARLIEST_PLAUSIBLE_MICROS: i64 = 946_684_800_000_000; // 2000-01-01T00:00:00Z

/// Give up rather than scan an entire file that was never a telemetry log in the first place.
const MAX_SCAN: u64 = 1024 * 1024;

struct Queued {
    system_id: SystemId,
    bytes: Vec<u8>,
}

/// Handle for appending frames to the telemetry log, cheap to clone.
#[derive(Clone)]
pub struct Writer {
    records: Option<SyncSender<Queued>>,
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

        if records.try_send(Queued { system_id, bytes }).is_err() {
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
    fn run(mut self, records: &Receiver<Queued>) {
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

    fn write(&mut self, record: &Queued) {
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
            .join(format!("{}-{system_id:02x}.{EXTENSION}", self.stamp));

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

/// One record read back out of a telemetry log.
#[derive(Clone, Debug)]
pub struct Record {
    pub received_at: DateTime<Utc>,
    pub frame: Frame<V2>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("Failed to read the telemetry log: {0}")]
    Io(#[from] std::io::Error),
    #[error("The telemetry log ends mid-record")]
    Truncated,
    #[error("Found no record boundary in {0} bytes")]
    Desynchronised(u64),
    #[error("Malformed frame in the telemetry log: {0:?}")]
    Frame(FrameError),
}

/// Reads records back out of a telemetry log.
///
/// A log can lose its record boundary: it may have been truncated mid-record, or written by another
/// tool. When the bytes at the cursor are not a plausible record the reader scans forward one byte
/// at a time for a position that is, as pymavlink's `scan_timestamp` does, and reports how much it
/// skipped.
pub struct Reader<R: Read> {
    inner: R,
    window: Vec<u8>,
    latest_plausible: DateTime<Utc>,
    consumed: u64,
    finished: bool,
}

impl Reader<BufReader<File>> {
    pub fn open(path: &Path) -> Result<Self, std::io::Error> {
        Ok(Reader::new(BufReader::new(File::open(path)?)))
    }
}

impl<R: Read> Reader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            window: Vec::new(),
            // Clocks are not guaranteed to agree, so allow a day of slack.
            latest_plausible: Utc::now() + TimeDelta::days(1),
            consumed: 0,
            finished: false,
        }
    }

    /// Bytes of the log dealt with so far: the records returned, plus anything skipped over while
    /// resynchronising.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Reads until the window holds `want` bytes. `false` at end of file, with the window holding
    /// whatever was left.
    fn fill(&mut self, want: usize) -> Result<bool, std::io::Error> {
        while self.window.len() < want {
            let have = self.window.len();
            self.window.resize(want, 0);

            let read = self.inner.read(&mut self.window[have..]);
            self.window
                .truncate(have + read.as_ref().copied().unwrap_or(0));

            match read {
                Ok(0) => return Ok(false),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }

        Ok(true)
    }

    /// Length of the record at the cursor, if the bytes there look like one at all.
    fn boundary(&self) -> Option<usize> {
        let received_at = timestamp_of(&self.window)?;

        let plausible = received_at.timestamp_micros() >= EARLIEST_PLAUSIBLE_MICROS
            && received_at <= self.latest_plausible;

        plausible.then(|| record_len(&self.window)).flatten()
    }

    /// End of file. Leftover bytes are a partial record, which is routine rather than damage: the
    /// writer flushes on a timer, so a log from a running instance ends mid-record.
    fn at_end(&mut self, skipped: u64) -> Option<Result<Record, ReadError>> {
        self.finished = true;

        if !self.window.is_empty() {
            return Some(Err(ReadError::Truncated));
        }

        (skipped > 0).then_some(Err(ReadError::Desynchronised(skipped)))
    }
}

impl<R: Read> Iterator for Reader<R> {
    type Item = Result<Record, ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let mut skipped = 0u64;

        loop {
            match self.fill(TIMESTAMP_LEN + HEADER_LEN) {
                Ok(true) => {}
                Ok(false) => return self.at_end(skipped),
                Err(e) => {
                    self.finished = true;
                    return Some(Err(e.into()));
                }
            }

            if let Some(len) = self.boundary() {
                match self.fill(len) {
                    Ok(true) => {}
                    Ok(false) => return self.at_end(skipped),
                    Err(e) => {
                        self.finished = true;
                        return Some(Err(e.into()));
                    }
                }

                if let Ok(record) = decode(&self.window[..len]) {
                    self.window.drain(..len);
                    self.consumed += len as u64;

                    if skipped > 0 {
                        tracing::warn!("Skipped {skipped} bytes of the telemetry log to resync");
                    }

                    return Some(Ok(record));
                }
            }

            // Not a record boundary after all.
            self.window.drain(..1);
            self.consumed += 1;
            skipped += 1;

            if skipped >= MAX_SCAN {
                self.finished = true;
                return Some(Err(ReadError::Desynchronised(skipped)));
            }
        }
    }
}

/// The low two bits hold a link index rather than part of the time, see [`now_micros`].
fn timestamp_of(record: &[u8]) -> Option<DateTime<Utc>> {
    let mut stamp = [0u8; TIMESTAMP_LEN];
    stamp.copy_from_slice(record.get(..TIMESTAMP_LEN)?);

    let micros = i64::try_from(u64::from_be_bytes(stamp) & !3).ok()?;
    DateTime::from_timestamp_micros(micros)
}

/// Length of the record starting at `record`, or `None` if it does not begin with a frame header we
/// recognise. Only the timestamp and the header itself have to be present.
fn record_len(record: &[u8]) -> Option<usize> {
    let header = record.get(TIMESTAMP_LEN..TIMESTAMP_LEN + HEADER_LEN)?;

    // A V1 record is skipped like garbage: the ground station is V2 throughout, so there is nothing
    // to hand one to.
    if header[0] != MAGIC_V2 {
        return None;
    }

    let signature_len = if header[2] & INCOMPAT_SIGNED == 0 {
        0
    } else {
        SIGNATURE_LEN
    };

    Some(TIMESTAMP_LEN + HEADER_LEN + header[1] as usize + CHECKSUM_LEN + signature_len)
}

/// Reads back one record laid out by [`encode`]. `record` must hold at least the whole record, as
/// measured by [`record_len`].
fn decode(record: &[u8]) -> Result<Record, ReadError> {
    let received_at = timestamp_of(record).ok_or(ReadError::Truncated)?;
    let frame_bytes = record.get(TIMESTAMP_LEN..).ok_or(ReadError::Truncated)?;

    // `deserialize` is marked unsafe to flag building frames out of arbitrary bytes rather than
    // because it can misbehave; mavio's own docs note the implementation is entirely safe Rust. Its
    // safe counterpart `mavio::io::Receiver` owns its reader, which leaves the timestamps between
    // frames unreachable.
    let frame = unsafe { Frame::<V2>::deserialize(frame_bytes) }.map_err(ReadError::Frame)?;

    Ok(Record { received_at, frame })
}

/// A telemetry log on disk.
#[derive(Clone, Debug)]
pub struct LogFile {
    pub path: PathBuf,
    pub recorded_at: DateTime<Utc>,
    /// The system the log is named for, absent for a file we did not write ourselves.
    pub system_id: Option<SystemId>,
    pub bytes: u64,
}

/// The telemetry logs in [`log_dir`], newest first.
pub fn recent(limit: usize) -> Vec<LogFile> {
    let Some(dir) = log_dir() else {
        return Vec::new();
    };

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!("Failed to list {}: {e}", dir.display());
            return Vec::new();
        }
    };

    let mut logs: Vec<LogFile> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == EXTENSION))
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let path = entry.path();

            let (stamp, system_id) = parse_name(&path);

            Some(LogFile {
                recorded_at: stamp.or_else(|| metadata.modified().ok().map(Into::into))?,
                system_id,
                bytes: metadata.len(),
                path,
            })
        })
        .collect();

    logs.sort_unstable_by_key(|log| std::cmp::Reverse(log.recorded_at));
    logs.truncate(limit);
    logs
}

/// Splits a `<stamp>-<hex system id>.tlog` name back apart. Both halves are optional so that a log
/// from another ground station still lists.
fn parse_name(path: &Path) -> (Option<DateTime<Utc>>, Option<SystemId>) {
    let Some((stamp, system_id)) = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.rsplit_once('-'))
    else {
        return (None, None);
    };

    (
        NaiveDateTime::parse_from_str(stamp, STAMP_FORMAT)
            .ok()
            .map(|naive| naive.and_utc()),
        SystemId::from_str_radix(system_id, 16).ok(),
    )
}

/// Directory holding the telemetry logs, created if it does not exist yet.
pub fn log_dir() -> Option<PathBuf> {
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

    /// A timestamp the reader's plausibility gate will accept, unlike an arbitrary bit pattern.
    fn stamp() -> u64 {
        u64::try_from(Utc::now().timestamp_micros()).unwrap() & !3
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

    #[test]
    fn encode_and_decode_round_trip() {
        let frame = frame();
        let micros = stamp();

        let record = decode(&encode(micros, &frame).unwrap()).unwrap();

        assert_eq!(record.received_at.timestamp_micros(), micros as i64);
        assert_eq!(record.frame.sequence(), frame.sequence());
        assert_eq!(record.frame.system_id(), frame.system_id());
        assert_eq!(record.frame.component_id(), frame.component_id());
        assert_eq!(record.frame.message_id(), frame.message_id());
        assert_eq!(record.frame.checksum(), frame.checksum());
        assert_eq!(record.frame.payload().bytes(), frame.payload().bytes());
    }

    #[test]
    fn reader_walks_consecutive_records() {
        let mut log = Vec::new();
        let mut written = Vec::new();

        for i in 0..3u64 {
            let frame = frame();
            let micros = (stamp() + i * 1000) & !3;
            log.extend_from_slice(&encode(micros, &frame).unwrap());
            written.push((micros, frame.sequence()));
        }

        let read: Vec<Record> = Reader::new(log.as_slice()).map(Result::unwrap).collect();

        assert_eq!(read.len(), 3);
        for (record, (micros, sequence)) in read.iter().zip(written) {
            assert_eq!(record.received_at.timestamp_micros(), micros as i64);
            assert_eq!(record.frame.sequence(), sequence);
        }
    }

    #[test]
    fn a_truncated_tail_yields_the_whole_records_then_one_error() {
        let mut log = encode(stamp(), &frame()).unwrap();
        let partial = encode(stamp(), &frame()).unwrap();
        log.extend_from_slice(&partial[..partial.len() - 3]);

        let mut reader = Reader::new(log.as_slice());

        assert!(reader.next().is_some_and(|r| r.is_ok()));
        assert!(matches!(reader.next(), Some(Err(ReadError::Truncated))));
        assert!(reader.next().is_none());
    }

    #[test]
    fn garbage_between_records_is_skipped() {
        let mut log = encode(stamp(), &frame()).unwrap();
        log.extend_from_slice(&[0xab; 37]);
        log.extend_from_slice(&encode(stamp(), &frame()).unwrap());

        let read: Vec<Record> = Reader::new(log.as_slice()).flatten().collect();

        assert_eq!(read.len(), 2);
    }

    #[test]
    fn a_magic_byte_in_a_timestamp_does_not_derail_the_walk() {
        let frame = frame();
        // Microseconds are the low bytes of the stamp, so this puts 0xfd where a scan for a bare
        // header byte would trip over it, while keeping the time itself plausible.
        let micros = stamp() / 1_000_000 * 1_000_000 + 0x00fd_fdfc;

        let log = encode(micros, &frame).unwrap();
        let read: Vec<Record> = Reader::new(log.as_slice()).flatten().collect();

        assert_eq!(read.len(), 1);
        assert_eq!(read[0].received_at.timestamp_micros(), micros as i64 & !3);
    }

    #[test]
    fn an_empty_log_reads_as_no_records() {
        assert_eq!(Reader::new([].as_slice()).count(), 0);
    }

    #[test]
    fn a_name_splits_into_a_timestamp_and_a_system_id() {
        let (stamp, system_id) = parse_name(Path::new("/tmp/2026-07-25T21:23:31Z-14.tlog"));

        assert_eq!(
            stamp.map(|s| s.to_rfc3339()),
            Some("2026-07-25T21:23:31+00:00".to_owned())
        );
        assert_eq!(system_id, Some(0x14));
    }

    #[test]
    fn a_foreign_name_parses_to_nothing_rather_than_failing() {
        assert_eq!(parse_name(Path::new("/tmp/flight.tlog")), (None, None));
    }
}
