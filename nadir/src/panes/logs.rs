use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, Utc};
use eframe::egui;
use egui::{Align, Button, Layout, ProgressBar, RichText, TextStyle};
use egui_extras::{Column, TableBuilder};
use mavspec::rust::dialects::common::enums::StorageStatus;
use mavspec::rust::dialects::common::messages::StorageInformation;
use tokio::sync::mpsc;

use nadir_core::{
    FlightLogUiState, GlobLogDownloadState, LogDlCommand, LogItem, PartialLogCompleteness, System,
};

use crate::colors::{COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, readable, text_on};
use crate::panes::{MUTED_HINT, PaneUi};

const ROW_HEIGHT: f32 = 20.0;

const BUSY_HINT: &str = "Wait for the current transfer to finish";

/// For downloading and saving flight logs from vehicle.
pub struct LogsPane {
    state: FlightLogUiState,
    /// Keyed by log id
    completeness_cache: HashMap<u16, PartialLogCompleteness>,
    /// System and time of the first click on the erase button, which a second click confirms.
    erase_armed: Option<(u8, f64)>,
}

impl LogsPane {
    const ERASE_CONFIRM_SECS: f64 = 3.0;
    /// Below this share of free space the storage readouts turn to a warning.
    pub const STORAGE_WARN_FREE: f32 = 0.1;

    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            state: FlightLogUiState::default(),
            completeness_cache: HashMap::new(),
            erase_armed: None,
        }
    }

    /// The vehicle's log storage, from the `STORAGE_INFORMATION` it sends unprompted. Nothing until
    /// one has arrived.
    fn storage_ui(ui: &mut egui::Ui, system: &System) {
        let Ok(info) = system.last_message::<StorageInformation>() else {
            return;
        };

        let (total, used, available) = (
            info.total_capacity,
            info.used_capacity,
            info.available_capacity,
        );
        let text = match info.status {
            StorageStatus::NotSupported => {
                ui.weak("Storage unavailable");
                return;
            }
            StorageStatus::Unformatted => {
                ui.weak("Storage unformatted");
                return;
            }
            StorageStatus::Empty => format!("Storage empty, {total:.2} MiB available"),
            StorageStatus::Ready => {
                format!("{used:.2} MiB used, {available:.2} MiB available of {total:.2} MiB")
            }
        };

        let fraction = if total > 0.0 {
            (used / total).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut bar = ProgressBar::new(fraction).text(text);
        if 1.0 - fraction < Self::STORAGE_WARN_FREE {
            bar = bar.fill(readable(COLOR_INDICATOR_WARNING, ui.visuals()));
        }
        ui.add(bar);
    }

    fn toolbar_ui(&mut self, ui: &mut egui::Ui, system: &System) {
        let muted = system.muted();
        let idle = matches!(self.state.dl_state, GlobLogDownloadState::Idle(_));
        let hint = if muted { MUTED_HINT } else { BUSY_HINT };

        ui.horizontal(|ui| {
            ui.set_height(25.0);
            match &self.state.dl_state {
                GlobLogDownloadState::Fetching(n) => {
                    ui.spinner();
                    ui.weak(format!("Fetching list ({n})"));
                }
                GlobLogDownloadState::Downloading(id) => {
                    ui.spinner();
                    ui.weak(format!("Downloading log_{id:04}"));
                }
                GlobLogDownloadState::Idle(Some(err)) => {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                }
                GlobLogDownloadState::Idle(None) if !self.state.items.is_empty() => {
                    ui.weak(format!("{} logs", self.state.items.len()));
                }
                GlobLogDownloadState::Idle(None) => {}
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.erase_button(ui, system, idle && !muted, hint);
                let refresh = ui
                    .add_enabled(idle && !muted, Button::new("⟳ Refresh"))
                    .on_disabled_hover_text(hint);
                if refresh.clicked() {
                    Self::send(&system.log_cmd_tx, LogDlCommand::FetchLogs);
                }
            });
        });
    }

    fn erase_button(&mut self, ui: &mut egui::Ui, system: &System, enabled: bool, hint: &str) {
        let now = ui.input(|i| i.time);
        let armed = enabled
            && self.erase_armed.is_some_and(|(system_id, at)| {
                system_id == system.system_id && now - at < Self::ERASE_CONFIRM_SECS
            });

        let button = if armed {
            let fill = readable(COLOR_INDICATOR_LIMITS, ui.visuals());
            Button::new(RichText::new("🗑 Confirm erase").color(text_on(fill))).fill(fill)
        } else {
            Button::new("🗑 Erase all")
        };
        let response = ui
            .add_enabled(enabled, button)
            .on_hover_text("Erases every log on the vehicle")
            .on_disabled_hover_text(hint);

        if response.clicked() {
            if armed {
                Self::send(&system.log_cmd_tx, LogDlCommand::EraseAll);
                self.erase_armed = None;
            } else {
                self.erase_armed = Some((system.system_id, now));
                ui.ctx()
                    .request_repaint_after_secs(Self::ERASE_CONFIRM_SECS as f32);
            }
        } else if !armed || response.clicked_elsewhere() {
            self.erase_armed = None;
        }
    }

    fn table_ui(&self, ui: &mut egui::Ui, system: &System) {
        let muted = system.muted();
        let downloading = match self.state.dl_state {
            GlobLogDownloadState::Downloading(id) => Some(id),
            _ => None,
        };
        let mut log_ids: Vec<u16> = self.state.items.keys().copied().collect();
        log_ids.sort_unstable();

        let h = ui.available_height();
        TableBuilder::new(ui)
            .striped(true)
            .max_scroll_height(h)
            .auto_shrink(false)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::auto().at_least(60.0))
            .column(Column::auto().at_least(110.0).resizable(true))
            .column(Column::auto().at_least(60.0).resizable(true))
            .column(Column::remainder().at_least(80.0).clip(true))
            .column(Column::auto())
            .header(ROW_HEIGHT, |mut header| {
                header.col(|ui| {
                    ui.weak("Log");
                });
                header.col(|ui| {
                    ui.weak("Created");
                });
                header.col(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.weak("Size");
                    });
                });
                header.col(|ui| {
                    ui.weak("Progress");
                });
                header.col(|_| {});
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, log_ids.len(), |mut row| {
                    let item = &self.state.items[&log_ids[row.index()]];
                    let log_id = item.meta.mav_log_id;
                    let this_downloading = downloading == Some(log_id);
                    let started = item.state.dl_start.is_some();
                    let (downloaded_bytes, complete) = match self.completeness_cache.get(&log_id) {
                        Some(PartialLogCompleteness::Contiguous { size }) => {
                            (*size, *size >= item.meta.size)
                        }
                        Some(PartialLogCompleteness::NonContigous {
                            downloaded_bytes, ..
                        }) => (*downloaded_bytes, false),
                        None => (0, false),
                    };

                    row.col(|ui| {
                        ui.monospace(format!("log_{log_id:04}"));
                    });
                    row.col(|ui| {
                        let created: DateTime<Local> = item.meta.log_created_at.into();
                        ui.label(created.format("%Y-%m-%d %H:%M").to_string());
                    });
                    row.col(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(Self::fmt_bytes(u64::from(item.meta.size)));
                        });
                    });
                    row.col(|ui| {
                        let progress = format!(
                            "{} / {}",
                            Self::fmt_bytes(u64::from(downloaded_bytes)),
                            Self::fmt_bytes(u64::from(item.meta.size)),
                        );
                        let frac =
                            (downloaded_bytes as f32 / item.meta.size as f32).clamp(0.0, 1.0);
                        if this_downloading {
                            let speed = item
                                .state
                                .dl_start
                                .map(|t| Self::speed(u64::from(downloaded_bytes), t))
                                .unwrap_or_default();
                            ui.add(
                                ProgressBar::new(frac)
                                    .text(format!("{progress}   {speed}"))
                                    .animate(true),
                            );
                        } else if let Some(err) = &item.meta.latest_error_msg {
                            ui.colored_label(
                                readable(COLOR_INDICATOR_WARNING, ui.visuals()),
                                format!("⚠ {err}"),
                            );
                        } else if complete {
                            ui.add(ProgressBar::new(1.0).text("Complete"));
                        } else if started {
                            ui.add(ProgressBar::new(frac).text(format!("Paused, {progress}")));
                        }
                    });
                    row.col(|ui| {
                        if this_downloading {
                            if ui.button("⏸ Pause").clicked() {
                                Self::send(&system.log_cmd_tx, LogDlCommand::Pause);
                            }
                            return;
                        }
                        if !complete {
                            let label = if started {
                                "▶ Resume"
                            } else {
                                "⬇ Download"
                            };
                            let hint = if muted { MUTED_HINT } else { BUSY_HINT };
                            let download = ui
                                .add_enabled(downloading.is_none() && !muted, Button::new(label))
                                .on_disabled_hover_text(hint);
                            if download.clicked() {
                                Self::send(&system.log_cmd_tx, LogDlCommand::DownloadLog(log_id));
                            }
                        }
                        if started {
                            Self::save_button(ui, item, &system.log_cmd_tx);
                        }
                    });
                });
            });
    }

    fn save_button(
        ui: &mut egui::Ui,
        item: &LogItem,
        cmd_tx: &Arc<Mutex<mpsc::Sender<LogDlCommand>>>,
    ) {
        let log_id = item.meta.mav_log_id;

        let mut response = ui.button("💾 Save");
        if let Some(path) = &item.state.data_file {
            response = response.on_hover_text(format!("Saved to {}", path.display()));
        }
        if !response.clicked() {
            return;
        }

        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            let _ = (log_id, cmd_tx);
            tracing::warn!("Saving a flight log needs a desktop build");
        }
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            // Every click asks, starting from the last save: the log may have grown since.
            let (directory, name) = match &item.state.data_file {
                Some(previous) => (
                    previous.parent().map(std::path::Path::to_path_buf),
                    previous
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned()),
                ),
                None => (std::env::current_dir().ok(), None),
            };
            let name = name.unwrap_or_else(|| {
                format!(
                    "{}_flightlog_{:04}.bin",
                    Utc::now().format("%Y%m%d_%H%M%S"),
                    log_id,
                )
            });
            let mut dialog = rfd::FileDialog::new().set_file_name(name);
            if let Some(directory) = directory {
                dialog = dialog.set_directory(directory);
            }
            if let Some(path) = dialog.save_file() {
                Self::send(cmd_tx, LogDlCommand::SaveLog { log_id, path });
            }
        }
    }

    fn fmt_bytes(b: u64) -> String {
        if b < 1024 {
            format!("{b} B")
        } else if b < 1024 * 1024 {
            format!("{:.1} KiB", b as f64 / 1024.0)
        } else {
            format!("{:.2} MiB", b as f64 / (1024.0 * 1024.0))
        }
    }

    fn speed(bytes: u64, dl_start: DateTime<Utc>) -> String {
        let secs = (Utc::now() - dl_start).num_milliseconds().max(1) as f64 / 1000.0;
        format!("{}/s", Self::fmt_bytes((bytes as f64 / secs) as u64))
    }

    fn send(tx: &Arc<Mutex<mpsc::Sender<LogDlCommand>>>, cmd: LogDlCommand) {
        if let Ok(sender) = tx.try_lock() {
            if let Err(e) = sender.try_send(cmd) {
                tracing::error!("command not sent: {e}");
            }
        } else {
            tracing::error!("command not sent: mutex locked");
        }
    }
}

impl PaneUi for LogsPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        for style in [TextStyle::Button, TextStyle::Body, TextStyle::Monospace] {
            ui.style_mut().text_styles.get_mut(&style).unwrap().size = 12.0;
        }
        ui.spacing_mut().item_spacing.y = 2.0;

        if let Some(new_state) = system.logs.try_lock().ok().map(|g| g.clone()) {
            self.state = new_state;
        }

        // On a missed lock the previous value stays, so progress never drops back to zero.
        for item in self.state.items.values() {
            if let Ok(log) = item.state.data.try_lock() {
                self.completeness_cache
                    .insert(item.meta.mav_log_id, log.get_completeness());
            }
        }

        Self::storage_ui(ui, &system);
        self.toolbar_ui(ui, &system);
        ui.separator();

        if self.state.items.is_empty() {
            if !matches!(self.state.dl_state, GlobLogDownloadState::Fetching(_)) {
                ui.centered_and_justified(|ui| ui.weak("No logs listed"));
            }
            return;
        }

        self.table_ui(ui, &system);
    }
}
