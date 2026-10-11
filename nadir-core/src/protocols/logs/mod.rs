use std::path::PathBuf;

use tokio::sync::{broadcast, mpsc};

use crate::System;
use crate::mav::ComponentId;
use mavspec::rust::dialects::common::Common;
use mavspec::rust::dialects::common::messages::LogErase;

pub mod download;
pub mod fetch_list;
pub mod partial_log;
pub mod types;
pub use types::*;

use download::download_log_until_cancel;
use fetch_list::fetch_logs;

// NOTE: Do we need to distinguish mav components here?
/// Runs a backround task fetches the list of logs and downloads single logs.
/// Only one action may run at a time.
pub async fn run_log_worker(
    system: System,
    component_id: ComponentId,
    mut message_rx: broadcast::Receiver<Common>,
    mut command_rx: mpsc::Receiver<LogDlCommand>,
) {
    loop {
        let Some(cmd) = command_rx.recv().await else {
            continue;
        };
        match cmd {
            LogDlCommand::SaveLog { log_id, path } => save_log_to_file(&system, log_id, &path),
            LogDlCommand::EraseAll => {
                system.send_message(&LogErase {
                    target_system: system.system_id,
                    target_component: component_id,
                });
                system.logs.lock().unwrap().items.clear();
                tracing::info!("Erasing all logs on system {}", system.system_id);
                // LOG_ERASE is not acknowledged, so let the vehicle confirm the list is empty.
                fetch_logs(
                    system.clone(),
                    component_id,
                    &mut message_rx,
                    &mut command_rx,
                )
                .await;
            }
            LogDlCommand::FetchLogs => {
                fetch_logs(
                    system.clone(),
                    component_id,
                    &mut message_rx,
                    &mut command_rx,
                )
                .await;
            }
            LogDlCommand::DownloadLog(id) => {
                download_log_until_cancel(
                    &system,
                    component_id,
                    id,
                    &mut message_rx,
                    &mut command_rx,
                )
                .await;
            }
            LogDlCommand::Pause => (), // there is nothing currently running
        }
    }
}

fn save_log_to_file(system: &System, log_id: u16, path: &PathBuf) {
    let contents = {
        let logs = system.logs.lock().unwrap();
        let Some(item) = logs.items.get(&log_id) else {
            return;
        };
        item.state.data.clone()
    };

    match std::fs::write(path, contents.lock().unwrap().data()) {
        Ok(()) => {
            let mut logs = system.logs.lock().unwrap();
            let Some(ref mut item) = logs.items.get_mut(&log_id) else {
                return;
            };
            item.state.data_file = Some(path.clone());
            item.state.last_save = Some(chrono::Utc::now());
            tracing::info!("Saved log {log_id} to file {path:?}");
        }
        Err(e) => {
            tracing::error!("Saving log {log_id} to {path:?} failed: {e}");
            // A transfer keeps its own state; the error only takes over an idle toolbar.
            let mut logs = system.logs.lock().unwrap();
            if matches!(logs.dl_state, GlobLogDownloadState::Idle(_)) {
                logs.dl_state =
                    GlobLogDownloadState::Idle(format!("Error saving to file: {e}").into());
            }
        }
    }
}
