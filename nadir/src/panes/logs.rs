use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use tokio::sync::mpsc;

use eframe::egui;
use egui::{Color32, ProgressBar, RichText};

use nadir_core::System;
use nadir_core::{
    FlightLogUiState, GlobLogDownloadState, LogDlCommand, LogItem, PartialLogCompleteness,
};

use crate::colors::readable;
use crate::panes::PaneUi;

/// For downloading and saving flight logs from vehicle.
pub struct LogsPane {
    state: FlightLogUiState,
    /// Keyed by log id
    completeness_cache: HashMap<u16, PartialLogCompleteness>,
}

impl LogsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            state: FlightLogUiState::default(),
            completeness_cache: HashMap::new(),
        }
    }
}

impl PaneUi for LogsPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        if let Some(new_state) = system.logs.try_lock().ok().map(|g| g.clone()) {
            self.state = new_state;
        }

        // Opportunistically refresh the completeness cache for every item.
        // try_lock() is non-blocking; on failure we simply keep the old cached value.
        for item in self.state.items.values() {
            if let Ok(log) = item.state.data.try_lock() {
                let c = log.get_completeness();
                self.completeness_cache.insert(item.meta.mav_log_id, c);
            }
        }

        let is_fetching = matches!(self.state.dl_state, GlobLogDownloadState::Fetching(_));
        let is_idle = matches!(self.state.dl_state, GlobLogDownloadState::Idle(_));
        let downloading_id: Option<u16> = match self.state.dl_state {
            GlobLogDownloadState::Downloading(id) => Some(id),
            _ => None,
        };
        let is_downloading = downloading_id.is_some();

        // ── Header row ──────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.heading("Active flight logs");
            ui.add_space(6.0);

            match &self.state.dl_state {
                GlobLogDownloadState::Fetching(n) => {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("Fetching… ({n})"))
                            .color(readable(Color32::from_rgb(56, 138, 221), ui.visuals())),
                    );
                }
                GlobLogDownloadState::Idle(None) if !self.state.items.is_empty() => {
                    ui.label(
                        RichText::new(format!("✔  {} log(s)", self.state.items.len()))
                            .color(readable(Color32::from_rgb(60, 180, 100), ui.visuals())),
                    );
                }
                GlobLogDownloadState::Idle(Some(err)) => {
                    ui.label(
                        RichText::new(format!("✖  {err}"))
                            .color(readable(Color32::from_rgb(210, 70, 60), ui.visuals())),
                    );
                }
                GlobLogDownloadState::Downloading(id) => {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("Downloading log_{id:04}…"))
                            .color(readable(Color32::from_rgb(56, 138, 221), ui.visuals())),
                    );
                }
                GlobLogDownloadState::Idle(_) => (),
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(is_idle, |ui| {
                    if ui.button("↺  Refresh list").clicked() {
                        send(&system.log_cmd_tx, LogDlCommand::FetchLogs);
                    }
                });
            });
        });

        ui.separator();

        // ── Empty state ──────────────────────────────────────────────────────
        if self.state.items.is_empty() && !is_fetching {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No logs — press Refresh to fetch the list from the vehicle.")
                        .color(ui.visuals().weak_text_color()),
                );
            });
            return;
        }

        // ── Log list ─────────────────────────────────────────────────────────
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut log_ids: Vec<u16> = self.state.items.keys().copied().collect();
                log_ids.sort_unstable();

                for log_id in log_ids {
                    let Some(item) = self.state.items.get_mut(&log_id) else {
                        continue;
                    };
                    let this_is_downloading = downloading_id == Some(item.meta.mav_log_id);
                    let log_completeness = self.completeness_cache.get(&item.meta.mav_log_id);
                    show_log_row(
                        ui,
                        item,
                        &system.log_cmd_tx,
                        is_downloading,
                        this_is_downloading,
                        log_completeness.unwrap_or(&PartialLogCompleteness::default()),
                    );
                    ui.separator();
                }
            });
    }
}

// ── Per-row rendering ────────────────────────────────────────────────────────

fn show_log_row(
    ui: &mut egui::Ui,
    item: &LogItem,
    cmd_tx: &Arc<Mutex<mpsc::Sender<LogDlCommand>>>,
    any_downloading: bool,
    this_is_downloading: bool,
    log_completenes: &PartialLogCompleteness,
) {
    // let state = &item.state;

    // Use cached downloaded_bytes — never falls back to zero on a missed lock.
    let downloaded_bytes = match log_completenes {
        PartialLogCompleteness::Contiguous { size } => size,
        PartialLogCompleteness::NonContigous {
            downloaded_bytes, ..
        } => downloaded_bytes,
    };
    let frac = (*downloaded_bytes as f32 / item.meta.size as f32).clamp(0.0, 1.0);

    let is_paused = !this_is_downloading && item.state.dl_start.is_some();
    // let is_finished = state.dl_start.is_some() && state.dl_end.is_some();
    let not_started = item.state.dl_start.is_none();

    // ── Name / meta line ────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("log_{:04}", item.meta.mav_log_id)).strong());

        ui.label(
            RichText::new(
                item.meta
                    .log_created_at
                    .format("%Y-%m-%d  %H:%M UTC")
                    .to_string(),
            )
            .color(ui.visuals().weak_text_color())
            .size(12.0),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(fmt_bytes(u64::from(item.meta.size)))
                    .color(ui.visuals().weak_text_color())
                    .size(12.0),
            );
        });
    });

    // ── State-specific controls ──────────────────────────────────────────────

    if this_is_downloading {
        let speed_str = item
            .state
            .dl_start
            .map(|t| calc_speed(u64::from(*downloaded_bytes), t))
            .unwrap_or_default();

        ui.add(
            ProgressBar::new(frac)
                .text(format!(
                    "{}  /  {}{}",
                    fmt_bytes(u64::from(*downloaded_bytes)),
                    fmt_bytes(u64::from(item.meta.size)),
                    if speed_str.is_empty() {
                        String::new()
                    } else {
                        format!("   ·   {speed_str}")
                    },
                ))
                .animate(true),
        );
        ui.horizontal(|ui| {
            if ui.button("⏸  Pause").clicked() {
                send(cmd_tx, LogDlCommand::Pause);
            }
        });
    } else if not_started {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!any_downloading, |ui| {
                if ui.button("⬇  Download").clicked() {
                    send(cmd_tx, LogDlCommand::DownloadLog(item.meta.mav_log_id));
                }
            });
            if any_downloading {
                ui.label(
                    RichText::new("another download is active")
                        .color(ui.visuals().weak_text_color())
                        .size(11.0),
                );
            }
        });
    } else if is_paused {
        ui.horizontal(|ui| {
            ui.add(ProgressBar::new(frac).text(format!(
                "Paused — {}  /  {}",
                fmt_bytes(u64::from(*downloaded_bytes)),
                fmt_bytes(u64::from(item.meta.size)),
            )));
        });

        ui.horizontal(|ui| {
            ui.add_enabled_ui(!any_downloading, |ui| {
                if ui.button("▶  Resume").clicked() {
                    send(cmd_tx, LogDlCommand::DownloadLog(item.meta.mav_log_id));
                }
            });
            show_save_button(ui, item, cmd_tx);

            if let Some(path) = &item.state.data_file {
                ui.label(
                    RichText::new(path.display().to_string())
                        .color(ui.visuals().weak_text_color())
                        .size(11.0),
                );
            }

            if any_downloading {
                ui.label(
                    RichText::new("another download is active")
                        .color(ui.visuals().weak_text_color())
                        .size(11.0),
                );
            }
        });
    }

    // ── Per-item error ───────────────────────────────────────────────────────
    if let Some(err) = &item.meta.latest_error_msg {
        ui.label(
            RichText::new(format!("⚠  {err}"))
                .color(readable(Color32::from_rgb(200, 150, 30), ui.visuals()))
                .size(11.0),
        );
    }
}
// NOTE: make sure button only appears when no download in action
fn show_save_button(
    ui: &mut egui::Ui,
    item: &LogItem,
    cmd_tx: &Arc<Mutex<mpsc::Sender<LogDlCommand>>>,
) {
    let log_id = item.meta.mav_log_id;

    if ui.button("💾  Save").clicked() {
        let path = match &item.state.data_file {
            Some(p) => p.clone(),
            #[cfg(any(target_arch = "wasm32", target_os = "android"))]
            None => {
                tracing::warn!("Saving a flight log needs a desktop build");
                return;
            }
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            None => {
                let default_name = format!(
                    "{}_flightlog_{:04}.bin",
                    chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                    log_id,
                );
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

                // Open a native save dialog (e.g. rfd::FileDialog)
                match rfd::FileDialog::new()
                    .set_file_name(default_name)
                    .set_directory(cwd)
                    .save_file()
                {
                    Some(p) => {
                        tracing::info!("Got fiel save path from user: {}", p.display());
                        p
                    }
                    None => return, // user cancelled
                }
            }
        };
        send(
            cmd_tx,
            LogDlCommand::SaveLog {
                log_id,
                path: path.clone(),
            },
        );
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn fmt_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KiB", b as f64 / 1024.0)
    } else {
        format!("{:.2} MiB", b as f64 / (1024.0 * 1024.0))
    }
}

fn calc_speed(bytes: u64, dl_start: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (chrono::Utc::now() - dl_start).num_milliseconds().max(1) as f64 / 1000.0;
    format!("{}/s", fmt_bytes((bytes as f64 / secs) as u64))
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
