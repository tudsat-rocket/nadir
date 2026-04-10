use std::collections::HashMap;

use chrono::Utc;
use maviola::protocol::ComponentId;
use mavspec::rust::dialects::common::{
    Common,
    messages::{LogEntry, LogRequestList},
};
use tokio::sync::{
    broadcast::{self},
    mpsc,
};
use tracing::warn;

use super::super::{Gatherable, gather};
#[allow(clippy::wildcard_imports)]
use super::types::*;
use crate::System;

// TODO: check if this accepts 1 based indexing, as per mav spec
impl Gatherable for LogEntry {
    type InitialRequest = LogRequestList;
    type SpecificRequest = LogRequestList;

    fn index(&self) -> usize {
        self.id as usize
    }
    fn count(&self) -> usize {
        self.num_logs as usize
    }

    fn initial_request(system_id: u8, component_id: u8) -> Self::InitialRequest {
        LogRequestList {
            target_system: system_id,
            target_component: component_id,
            start: 0,
            end: 0xffff,
        }
    }
    fn specific_request(system_id: u8, component_id: u8, index: usize) -> Self::SpecificRequest {
        LogRequestList {
            target_system: system_id,
            target_component: component_id,
            start: index as u16,
            end: index as u16 + 1,
        }
    }
    fn unpack(msg: mavspec::rust::dialects::Common) -> Option<Self> {
        match msg {
            Common::LogEntry(entry) => Some(entry),
            _ => None,
        }
    }
}
// fetches list of logs and manages, updates state for gui
pub async fn fetch_logs(
    system: System,
    component_id: ComponentId,
    message_rx: &mut broadcast::Receiver<Common>,
    command_rx: &mut mpsc::Receiver<LogDlCommand>,
) {
    (system.logs.lock().unwrap()).dl_state = GlobLogDownloadState::Fetching(0);
    loop {
        let cloned_sys = system.clone();
        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    Some(LogDlCommand::Pause) => {
                    // NOTE: check cancel safety
                        // stop polling gather function, cancel by dropping
                        {
                            let mut logs = system.logs.lock().unwrap();
                            logs.dl_state = GlobLogDownloadState::Idle(None);

                        }
                        return ;
                    }
                    Some(LogDlCommand::FetchLogs) => {
                        warn!("Already fetching list of Logs.");
                    }
                    Some(LogDlCommand::SaveLog{..}) => {
                        warn!("Saving a log to file is not supported while fetching list of logs.");
                    }
                    Some(LogDlCommand::DownloadLog(_)) => {
                            warn!("Wait for fetching a list of logs to finish, before downloading a log.");
                    }
                    None => (),
                }
            }
            result = gather::<LogEntry>(&system, component_id, message_rx, Some(Box::new(move |received, _total| {state_to_feching_with_progress(&cloned_sys,received)} ))) => {
                (system.logs.lock().unwrap()).dl_state = GlobLogDownloadState::Idle(None);
                if let Ok(new_logs) = result {
                    let new_logs: Vec<LogItem> = new_logs
                        .iter()
                        .map(|l| LogItem {
                            meta: LogMeta {
                                mav_log_id: l.id,
                                log_created_at: chrono::DateTime::from_timestamp_secs(
                                    l.time_utc.into(),
                                )
                                .unwrap_or_default(),
                                info_fetched_at: Utc::now(),
                                size: l.size,
                                latest_error_msg: None,
                            },
                            state: LogState::default(),
                        })
                        .collect();

                    // TODO: actual error reporting
                    let info_msg: Option<String> = if new_logs.is_empty() {
                        Some("No logs available".to_string())
                    } else {
                        None
                    };
                    {
                    // NOTE: overwrite logic is sus, because making it correct would require user
                    // interaction
                        let mut logs = system.logs.lock().unwrap();
                        logs.dl_state = GlobLogDownloadState::Idle(info_msg);
                        merge_logs(new_logs, &mut logs.items);
                    }
                    return;
                }
                (system.logs.lock().unwrap()).dl_state = GlobLogDownloadState::Idle(Some("Error fetching log entries".to_string()));
                return;
            }
        }
    }
}

// Merges every log of new_logs into old_log by matching the log_id.
// Skipps merging a log if creation time or size indicate that the log has been exchanged in
// vehicle.
fn merge_logs(new_logs: Vec<LogItem>, old_logs: &mut HashMap<u16, LogItem>) {
    for new_log in new_logs {
        if let Some(ref mut old_log) = old_logs.get_mut(&new_log.meta.mav_log_id) {
            if old_log.meta.log_created_at == new_log.meta.log_created_at
                && old_log.meta.size <= new_log.meta.size
            {
                // overwrite log
                old_log.meta = new_log.meta.clone();
            } else {
                tracing::warn!(
                    "cound not integrate newly fetched log (id: {}), this will cause issues",
                    new_log.meta.mav_log_id
                );
            }
        } else {
            old_logs.insert(new_log.meta.mav_log_id, new_log);
        }
    }
}

fn state_to_feching_with_progress(system: &System, current: usize) {
    (system.logs.lock().unwrap()).dl_state = GlobLogDownloadState::Fetching(current as u64);
}
