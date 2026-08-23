use crate::time::timeout;
use chrono::Utc;
use std::sync::{Arc, Mutex};
use tracing::warn;

use crate::mav::ComponentId;
use mavspec::rust::dialects::common::messages::LogRequestEnd;
use mavspec::rust::dialects::common::{Common, messages::LogRequestData};

#[allow(clippy::wildcard_imports)]
use super::types::*;
use crate::System;

/// Helper function that listens to message broadcast and returns the first message matching the
/// predicate.
async fn wait_for_message(
    message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
    predicate: impl Fn(&Common) -> bool,
) -> Common {
    loop {
        if let Ok(common) = message_rx.recv().await
            && predicate(&common)
        {
            return common;
        }
    }
}

/// Tries to download the log upto `log_size`, injesting data into `partial_log`.
/// Exits with `Ok` when log is downloaded or with `Err` when too many timeouts and stalls have
/// occured.
#[tracing::instrument(name = "DL Log", skip_all, fields())]
async fn download_log(
    system: &System,
    component_id: ComponentId,
    log_id: u16,
    log_size: u32,
    partial_log: &Arc<Mutex<PartialLogSlow>>,
    message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
) -> Result<(), ()> {
    // after too many failures, terminate
    let mut not_grown_for = 0;
    let mut last_size = 0;
    loop {
        let (top, next_chunk) = {
            let partial = partial_log.lock().unwrap();
            (
                partial.data().len() as u32,
                partial.missing_chunks().first().cloned(),
            )
        };

        // craft next chunk request
        let (offset, count) = match next_chunk {
            None => {
                if top >= log_size {
                    return Ok(());
                }
                (top, log_size - top)
            }
            Some(range) => (range.start, (range.end - range.start)),
        };

        let _ = download_log_chunk(
            system,
            component_id,
            log_id,
            message_rx,
            partial_log,
            offset,
            count,
        )
        .await;

        let info = partial_log.lock().unwrap().get_completeness();
        tracing::info!("status after dl_log_chunk: {:?}", info);

        if info.num_valid_byes() <= last_size {
            not_grown_for += 1;
            if not_grown_for >= 3 {
                return Err(());
            }
        } else {
            not_grown_for = 0;
            last_size = info.num_valid_byes();
        }
    }
}

/// Initiates download of a range of log data and injests any incoming chunks.   
/// Does not issue a re-download for missing parts of the requested data range.
/// Returns `Ok()` if vehicle is believed to have finished sending the log data range.
/// Returns `Err()` after timeouting waiting for a message to be sent by the vehicle.
#[tracing::instrument(name = "Chunk", skip_all, fields(offset, count))]
async fn download_log_chunk(
    system: &System,
    component_id: ComponentId,
    log_id: u16,
    message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
    partial_log: &Arc<Mutex<PartialLogSlow>>,
    offset: u32,
    count: u32,
) -> Result<(), ()> {
    // NOTE: timeout may be to gracious for high packet-loss systems, with a large amounts of small
    // chunks to download -> todo: start downloading chunks in parallel
    const ACK_RETRIES_ALLOWED: i32 = 3;
    tracing::info!("Downloading chunk {{ofs: {}, count: {}}}", offset, count);
    let mut ack_retries_allowed = ACK_RETRIES_ALLOWED;
    // In the mavlink logs protocol there does not exist an ACK, but if we receive a correct
    // LogData message we can assume our request was received.
    let mut ack_received = false;
    let req_count = count;
    system.send_message(&LogRequestData {
        target_system: system.system_id,
        target_component: component_id,
        id: log_id,
        ofs: offset,
        count: req_count,
    });

    loop {
        let result = timeout(
            std::time::Duration::from_secs(3),
            wait_for_message(message_rx, |m| matches!(m, Common::LogData(_))),
        )
        .await;
        let log_data = match result {
            Ok(Common::LogData(log_data)) => log_data,
            Ok(_) => continue, // impossible, because earlier filter
            Err(_e) => {
                tracing::info!("Timeout waiting for LOG_DATA message");
                if ack_retries_allowed <= 0 {
                    tracing::info!(
                        "Maximun retries for log chunk {{ofs: {}, count: {}}} download reached, {} unsuccessful attempts",
                        offset,
                        req_count,
                        ACK_RETRIES_ALLOWED
                    );
                    system.send_message(&LogRequestEnd {
                        target_system: system.system_id,
                        target_component: component_id,
                    });
                    return Err(());
                }

                if !ack_received {
                    tracing::info!(
                        "Timeout waiting for LOG_DATA message, resending inital LogRequestData Message"
                    );
                    system.send_message(&LogRequestData {
                        target_system: system.system_id,
                        target_component: component_id,
                        id: log_id,
                        ofs: offset,
                        count: req_count,
                    });
                }
                ack_retries_allowed -= 1;
                continue;
            }
        };
        ack_received = true;

        if log_data.id != log_id {
            // if there is a download ongoing for other log_id, stop it (should never happen, really)
            system.send_message(&LogRequestEnd {
                target_system: system.system_id,
                target_component: component_id,
            });
            tracing::error!(
                "received log data with unexpected log id, this might be because of heavy packet loss"
            );
            return Err(());
        }
        if log_data.count == 0 {
            // end of message stream
            tracing::info!("end of log {} data stream", log_id);
            return Ok(());
        }

        partial_log.lock().unwrap().ingest(
            log_data.ofs,
            &log_data.data[..(log_data.count as usize).min(90)],
        );

        if log_data.ofs + u32::from(log_data.count) >= offset + req_count {
            // Finished downloading chunk, although some parts may be missing
            return Ok(());
        }
    }
}

/// Downloads a log either until it is canceled via [`DlLogCommand::Pause`] or
/// unitl too many timeouts and stalls were reached.
/// Updates ui state.
pub async fn download_log_until_cancel(
    system: &System,
    component_id: ComponentId,
    log_id: u16,
    message_rx: &mut tokio::sync::broadcast::Receiver<Common>,
    command_rx: &mut tokio::sync::mpsc::Receiver<LogDlCommand>,
) {
    let (data_store, log_size) = {
        // First, update the item's state
        let mut ui_state = system.logs.lock().unwrap();
        if let Some(this_item) = ui_state.items.get_mut(&log_id) {
            if this_item.state.dl_start.is_none() {
                this_item.state.dl_start = Some(Utc::now());
            }

            let data = this_item.state.data.clone();
            let log_size = this_item.meta.size;

            ui_state.dl_state = GlobLogDownloadState::Downloading(log_id);

            (data, log_size)
        } else {
            tracing::error!("bug: ui tried to download log item, which does not exist");
            return;
        }
    };

    loop {
        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    Some(LogDlCommand::Pause) => {
                        // stop polling download function, cancel by dropping
                        {
                            let mut logs = system.logs.lock().unwrap();
                            if let Some(log) = logs.items.get_mut(&log_id) {
                                log.state.dl_end = Some(Utc::now());
                            }
                            logs.dl_state = GlobLogDownloadState::Idle(None);

                        }

                        system.send_message(&LogRequestEnd {
                            target_system: system.system_id,
                            target_component: component_id,
                        });
                        system.send_message(&LogRequestEnd {
                            target_system: system.system_id,
                            target_component: component_id,
                        });
                        system.send_message(&LogRequestEnd {
                            target_system: system.system_id,
                            target_component: component_id,
                        });
                        return;
                    }
                    Some(LogDlCommand::FetchLogs) => {
                        warn!("Fetching list of logs is not supported while downloading a log");
                    }
                    Some(LogDlCommand::SaveLog{..}) => {
                        warn!("Saving a log to file is not supported while downloading a log");
                    }
                    Some(LogDlCommand::DownloadLog(id)) => {
                        if log_id == id {
                            warn!("Can't start another download on the same log");
                        } else {
                            warn!("Another log with id {} is already downloading", log_id);
                        }
                    }
                    None => (),
                }
            }
            _ = download_log(system, component_id, log_id, log_size, &data_store, message_rx) => {
                tracing::info!("Finished downloading log");
                system.logs.lock().unwrap().dl_state = GlobLogDownloadState::Idle(None);
                return ;
            }
        }
    }
}
