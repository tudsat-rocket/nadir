use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use super::partial_log::{PartialLogCompleteness, PartialLogSlow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogDlCommand {
    FetchLogs,
    DownloadLog(u16),
    SaveLog { log_id: u16, path: PathBuf },
    Pause,
}
#[derive(Debug, Clone)]
pub struct LogMeta {
    pub mav_log_id: u16,
    pub log_created_at: chrono::DateTime<chrono::Utc>,
    /// size estimate in bytes as reported by vehicle
    pub size: u32,
    pub latest_error_msg: Option<String>,
    pub info_fetched_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Default, Clone)]
pub struct LogState {
    pub data: Arc<Mutex<PartialLogSlow>>,
    pub dl_start: Option<chrono::DateTime<chrono::Utc>>,
    pub dl_end: Option<chrono::DateTime<chrono::Utc>>,
    pub last_save: Option<chrono::DateTime<chrono::Utc>>,
    pub data_file: Option<PathBuf>,
    pub meta_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct FlightLogUiState {
    pub dl_state: GlobLogDownloadState,
    pub items: HashMap<u16, LogItem>,
}

#[derive(Debug, Clone)]
pub struct LogItem {
    pub meta: LogMeta,
    pub state: LogState,
}

#[derive(Debug, Clone)]
pub enum GlobLogDownloadState {
    // number of already fetched logs
    Fetching(u64),
    // error message
    Idle(Option<String>),
    // id of downloading
    Downloading(u16),
}

impl Default for GlobLogDownloadState {
    fn default() -> Self {
        Self::Idle(None)
    }
}
