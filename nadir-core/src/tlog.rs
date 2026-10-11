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
use std::time::Duration;

use crate::time::Instant;

use crate::mav::{Frame, FrameError, SystemId, V2};
use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};

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

/// Anything stamped before this is a lost record boundary rather than a real time, unless the whole
/// log is stamped since boot.
const EARLIEST_PLAUSIBLE_MICROS: i64 = 946_684_800_000_000; // 2000-01-01T00:00:00Z

/// Longest uptime a log stamped since boot can cover: zenith's u32 milliseconds wrap after 49.7 days.
const LATEST_BOOT_RELATIVE_MICROS: i64 = 50 * 24 * 3600 * 1_000_000;

/// Give up rather than scan an entire file that was never a telemetry log in the first place.
const MAX_SCAN: u64 = 1024 * 1024;

const READ_CHUNK: usize = 16 * 1024;

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
    micros_of(Utc::now())
}

fn micros_of(time: DateTime<Utc>) -> u64 {
    // pymavlink stores a link index in the low two bits of the timestamp, so leave them clear.
    u64::try_from(time.timestamp_micros()).unwrap_or(0) & !3
}

/// Lays out one record stamped with the time the frame was received, rather than the current time.
pub fn record(received_at: DateTime<Utc>, frame: &Frame<V2>) -> Result<Vec<u8>, FrameError> {
    encode(micros_of(received_at), frame)
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

/// Splits a byte stream into records, however the bytes happen to be chopped up.
///
/// A log can lose its record boundary: it may have been truncated mid-record, or written by another
/// tool. When the bytes at the cursor are not a plausible record the decoder scans forward one byte
/// at a time for a position that is, as pymavlink's `scan_timestamp` does, and reports how much it
/// skipped.
///
/// Flight controllers without a wall clock stamp records since boot instead. A stream whose first
/// record has such a stamp is read as one throughout.
///
/// Running out of bytes part-way through a record means "not yet" here; [`Reader`] is this decoder
/// plus a reader to pull from, for the case where it means end of data.
pub struct Decoder {
    window: Vec<u8>,
    /// Start of the undecoded bytes in `window`, which is only compacted once in a while.
    start: usize,
    /// Timestamps a record boundary may carry, in unix microseconds.
    plausible: std::ops::RangeInclusive<i64>,
    consumed: u64,
    skipped: u64,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            window: Vec::new(),
            start: 0,
            // Clocks are not guaranteed to agree, so allow a day of slack.
            plausible: EARLIEST_PLAUSIBLE_MICROS
                ..=(Utc::now() + TimeDelta::days(1)).timestamp_micros(),
            consumed: 0,
            skipped: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > self.window.len() / 2 {
            self.window.drain(..self.start);
            self.start = 0;
        }

        self.window.extend_from_slice(bytes);
    }

    /// The next complete record, or `None` if more bytes are needed first.
    pub fn pop(&mut self) -> Option<Result<Record, ReadError>> {
        loop {
            let pending = &self.window[self.start..];

            if pending.len() < TIMESTAMP_LEN + HEADER_LEN {
                return None;
            }

            if let Some(len) = self.boundary() {
                if pending.len() < len {
                    return None;
                }

                if let Ok(record) = decode(&pending[..len]) {
                    if self.consumed == 0
                        && record.received_at.timestamp_micros() < EARLIEST_PLAUSIBLE_MICROS
                    {
                        self.plausible = 0..=LATEST_BOOT_RELATIVE_MICROS;
                    }
                    self.take(len);

                    if self.skipped > 0 {
                        let skipped = std::mem::take(&mut self.skipped);
                        tracing::warn!("Skipped {skipped} bytes of telemetry to resync");
                    }

                    return Some(Ok(record));
                }
            }

            // Not a record boundary after all.
            self.take(1);
            self.skipped += 1;

            if self.skipped >= MAX_SCAN {
                return Some(Err(ReadError::Desynchronised(std::mem::take(
                    &mut self.skipped,
                ))));
            }
        }
    }

    /// Bytes dealt with so far: the records returned, plus anything skipped over while
    /// resynchronising.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    fn pending(&self) -> usize {
        self.window.len() - self.start
    }

    fn take(&mut self, len: usize) {
        self.start += len;
        self.consumed += len as u64;
    }

    /// Length of the record at the cursor, if the bytes there look like one at all.
    fn boundary(&self) -> Option<usize> {
        let pending = &self.window[self.start..];
        let micros = timestamp_of(pending)?.timestamp_micros();

        let plausible = self.plausible.contains(&micros)
            || (self.consumed == 0 && (0..=LATEST_BOOT_RELATIVE_MICROS).contains(&micros));

        plausible.then(|| record_len(pending)).flatten()
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads records back out of a telemetry log, resynchronising as [`Decoder`] describes.
pub struct Reader<R: Read> {
    inner: R,
    decoder: Decoder,
    chunk: Vec<u8>,
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
            decoder: Decoder::new(),
            chunk: vec![0; READ_CHUNK],
            finished: false,
        }
    }

    pub fn consumed(&self) -> u64 {
        self.decoder.consumed()
    }

    /// End of file. Leftover bytes are a partial record, which is routine rather than damage: the
    /// writer flushes on a timer, so a log from a running instance ends mid-record.
    fn at_end(&mut self) -> Option<Result<Record, ReadError>> {
        self.finished = true;

        if self.decoder.pending() > 0 {
            return Some(Err(ReadError::Truncated));
        }

        let skipped = self.decoder.skipped;
        (skipped > 0).then_some(Err(ReadError::Desynchronised(skipped)))
    }
}

impl<R: Read> Iterator for Reader<R> {
    type Item = Result<Record, ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        loop {
            if let Some(record) = self.decoder.pop() {
                if matches!(record, Err(ReadError::Desynchronised(_))) {
                    self.finished = true;
                }

                return Some(record);
            }

            match self.inner.read(&mut self.chunk) {
                Ok(0) => return self.at_end(),
                Ok(read) => self.decoder.push(&self.chunk[..read]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    self.finished = true;
                    return Some(Err(e.into()));
                }
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

/// `None` on wasm, where there is no directory to hold them in. Downstream already treats that as
/// "logging is unavailable" rather than as a failure.
#[cfg(target_arch = "wasm32")]
pub fn log_dir() -> Option<PathBuf> {
    None
}

/// Directory holding the telemetry logs, created if it does not exist yet.
#[cfg(not(target_arch = "wasm32"))]
pub fn log_dir() -> Option<PathBuf> {
    let Some(dirs) = directories::ProjectDirs::from("space", "tudsat", "nadir") else {
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

    use crate::mav::{Endpoint, MavLinkId};
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
    fn a_log_stamped_since_boot_reads_through() {
        let mut log = Vec::new();
        for i in 0..3u64 {
            log.extend_from_slice(&encode(168_000_000 + i * 4000, &frame()).unwrap());
        }
        log.extend_from_slice(&[0xab; 37]);
        log.extend_from_slice(&encode(169_000_000, &frame()).unwrap());

        let read: Vec<Record> = Reader::new(log.as_slice()).flatten().collect();

        assert_eq!(read.len(), 4);
        assert_eq!(read[3].received_at.timestamp_micros(), 169_000_000);
    }

    #[test]
    fn a_stamp_since_boot_is_only_accepted_at_the_start() {
        let mut log = encode(stamp(), &frame()).unwrap();
        log.extend_from_slice(&encode(168_000_000, &frame()).unwrap());

        let read: Vec<Record> = Reader::new(log.as_slice()).flatten().collect();

        assert_eq!(read.len(), 1);
    }

    #[test]
    fn a_decoder_reassembles_records_pushed_a_byte_at_a_time() {
        let mut log = encode(stamp(), &frame()).unwrap();
        log.extend_from_slice(&encode(stamp(), &frame()).unwrap());

        let mut decoder = Decoder::new();
        let mut read = 0;
        for byte in &log {
            decoder.push(std::slice::from_ref(byte));
            while let Some(record) = decoder.pop() {
                record.unwrap();
                read += 1;
            }
        }

        assert_eq!(read, 2);
        assert_eq!(decoder.consumed(), log.len() as u64);
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
