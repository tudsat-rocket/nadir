use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use axum::body::Bytes;
use tokio::sync::broadcast;

const TAIL_CAPACITY: usize = 1024;

const MAX_ID: usize = 64;

const EXTENSION: &str = "tlog";

#[derive(Debug)]
pub enum AppendError {
    /// The offset is not where the session ends, which is this many bytes in.
    Conflict(u64),
    Io(std::io::Error),
}

/// One telemetry session, kept in memory once read and appended to its file on disk.
pub struct Session {
    log: Mutex<Log>,
    tail: broadcast::Sender<Bytes>,
}

struct Log {
    bytes: Vec<u8>,
    file: File,
}

impl Session {
    fn load(path: &Path) -> std::io::Result<Self> {
        let mut file = File::options()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        Ok(Self {
            log: Mutex::new(Log { bytes, file }),
            tail: broadcast::channel(TAIL_CAPACITY).0,
        })
    }

    pub fn append(&self, offset: u64, batch: Bytes) -> Result<(), AppendError> {
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);

        let len = log.bytes.len() as u64;
        if offset != len {
            return Err(AppendError::Conflict(len));
        }

        log.file.write_all(&batch).map_err(AppendError::Io)?;
        log.bytes.extend_from_slice(&batch);

        // Sent under the lock, so that `attach` cannot see a batch in both the history and the tail.
        let _ = self.tail.send(batch);

        Ok(())
    }

    /// Where the history starts, the history from there, a tail starting exactly where it ends, and
    /// the session's epoch. An offset past the end reads from the start.
    pub fn attach(&self, offset: u64) -> (u64, Vec<u8>, broadcast::Receiver<Bytes>, u64) {
        let log = self.log.lock().unwrap();

        let start = if offset <= log.bytes.len() as u64 {
            offset
        } else {
            0
        };

        (
            start,
            log.bytes[start as usize..].to_vec(),
            self.tail.subscribe(),
            epoch(&log.bytes),
        )
    }
}

/// The first record's timestamp, which tells a session apart from an earlier one under the same id
/// whose data was lost. Zero while empty.
fn epoch(bytes: &[u8]) -> u64 {
    bytes
        .first_chunk::<8>()
        .map_or(0, |stamp| u64::from_be_bytes(*stamp))
}

/// Every session in a directory, one `<id>.tlog` each, read into memory as they are asked for.
pub struct Sessions {
    dir: PathBuf,
    loaded: Mutex<BTreeMap<String, Arc<Session>>>,
}

impl Sessions {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            loaded: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn get(&self, id: &str) -> std::io::Result<Option<Arc<Session>>> {
        if !self.path(id).exists() {
            return Ok(None);
        }

        self.open(id).map(Some)
    }

    pub fn open(&self, id: &str) -> std::io::Result<Arc<Session>> {
        let mut loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(session) = loaded.get(id) {
            return Ok(Arc::clone(session));
        }

        let path = self.path(id);
        if !path.exists() {
            tracing::info!("opened session {id}");
        }

        let session = Arc::new(Session::load(&path)?);
        loaded.insert(id.to_owned(), Arc::clone(&session));

        Ok(session)
    }

    /// Id, length in bytes, and time since the last append of every session on disk.
    pub fn summaries(&self) -> std::io::Result<Vec<(String, u64, Duration)>> {
        let mut summaries = Vec::new();

        for entry in std::fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != EXTENSION) {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };

            let metadata = std::fs::metadata(&path)?;
            let idle = SystemTime::now()
                .duration_since(metadata.modified()?)
                .unwrap_or_default();

            summaries.push((id.to_owned(), metadata.len(), idle));
        }

        summaries.sort_unstable();
        Ok(summaries)
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{EXTENSION}"))
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (tempfile::TempDir, Sessions) {
        let dir = tempfile::tempdir().unwrap();
        let sessions = Sessions::new(dir.path().to_owned());
        (dir, sessions)
    }

    #[test]
    fn an_append_must_start_where_the_session_ends() {
        let (_dir, sessions) = session();
        let session = sessions.open("s").unwrap();

        session.append(0, Bytes::from_static(b"one")).unwrap();
        assert!(matches!(
            session.append(0, Bytes::from_static(b"two")),
            Err(AppendError::Conflict(3))
        ));
        assert!(matches!(
            session.append(5, Bytes::from_static(b"two")),
            Err(AppendError::Conflict(3))
        ));
        session.append(3, Bytes::from_static(b"two")).unwrap();

        assert_eq!(session.attach(0).1, b"onetwo");
    }

    #[tokio::test]
    async fn attaching_snapshots_the_past_and_subscribes_to_the_future() {
        let (_dir, sessions) = session();
        let session = sessions.open("s").unwrap();
        session.append(0, Bytes::from_static(b"before")).unwrap();

        let (_, history, mut tail, _) = session.attach(0);
        session.append(6, Bytes::from_static(b"after")).unwrap();

        assert_eq!(history, b"before");
        assert_eq!(tail.recv().await.unwrap(), Bytes::from_static(b"after"));
    }

    #[test]
    fn attaching_past_the_end_reads_from_the_start() {
        let (_dir, sessions) = session();
        let session = sessions.open("s").unwrap();
        session.append(0, Bytes::from_static(b"abcdef")).unwrap();

        assert_eq!(session.attach(2).0, 2);
        assert_eq!(session.attach(2).1, b"cdef");
        assert_eq!(session.attach(7).0, 0);
        assert_eq!(session.attach(7).1, b"abcdef");
    }

    #[test]
    fn a_session_outlives_the_process_that_wrote_it() {
        let (dir, sessions) = session();
        let first = sessions.open("s").unwrap();
        first.append(0, Bytes::from_static(b"0123456789")).unwrap();
        let epoch = first.attach(0).3;

        let reopened = Sessions::new(dir.path().to_owned());
        assert!(reopened.get("nobody").unwrap().is_none());

        let session = reopened.get("s").unwrap().unwrap();
        assert_eq!(session.attach(0).1, b"0123456789");
        assert_eq!(session.attach(0).3, epoch);
        session.append(10, Bytes::from_static(b"ab")).unwrap();

        let summaries = reopened.summaries().unwrap();
        assert_eq!((summaries[0].0.as_str(), summaries[0].1), ("s", 12));
    }

    #[test]
    fn an_id_has_to_look_like_one() {
        assert!(valid_id("nadir-01-20261011T143000"));

        assert!(!valid_id(""));
        assert!(!valid_id("../etc/passwd"));
        assert!(!valid_id("has space"));
        assert!(!valid_id(&"x".repeat(MAX_ID + 1)));
    }
}
