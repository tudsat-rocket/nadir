//! The view shown when no system is selected: what the ground station is currently attached to, and
//! what it has recorded before.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use web_time::Instant;

use nadir_core::{Link, LinkId, Origin, Source, tlog};

use eframe::egui;

use crate::views::SourceId;
use crate::widgets::column_header;

const RECENT_LIMIT: usize = 10;

/// The session's own recording is created lazily, on the first frame received, so the listing taken
/// when the view was built does not have it yet.
const RESCAN_INTERVAL: Duration = Duration::from_secs(2);

pub struct Overview {
    recent: Vec<tlog::LogFile>,
    scanned_at: Instant,
}

impl Overview {
    pub fn new() -> Self {
        Self {
            recent: tlog::recent(RECENT_LIMIT),
            scanned_at: Instant::now(),
        }
    }

    pub fn refresh(&mut self) {
        self.recent = tlog::recent(RECENT_LIMIT);
        self.scanned_at = Instant::now();
    }

    /// Returns the log the user picked. `App` owns the sources, so the view reports rather than acts.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        links: &[Link],
        logs: &BTreeMap<SourceId, Source>,
    ) -> Option<PathBuf> {
        if self.scanned_at.elapsed() >= RESCAN_INTERVAL {
            self.refresh();
        }

        let mut picked = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(10.0);
            ui.indent("overview", |ui| {
                self.links(ui, links);
                ui.add_space(15.0);
                picked = self.recent_logs(ui, logs);
            });
        });

        picked
    }

    fn links(&self, ui: &mut egui::Ui, links: &[Link]) {
        column_header(ui, "🖧 LINKS");

        for link in links {
            let mut stats = link.stats.clone();
            let info_string = match &link.id {
                LinkId::UdpServer(addr) => format!("udp:{addr}"),
                LinkId::TcpClient(addr) => format!("tcp:{addr}"),
                LinkId::SerialPort(port) => format!("serial:{port}"),
            };

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.weak("🖧");
                ui.label(info_string);
            });

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.weak("⏬");
                ui.monospace(format!("{:>3.0}", stats.received_packet_rate()));
                ui.label("pkt/s ");
                ui.monospace(format!("{:>5.2}", stats.received_data_rate() / 1024.0));
                ui.label("KiB/s");
            });
        }
    }

    fn recent_logs(&self, ui: &mut egui::Ui, logs: &BTreeMap<SourceId, Source>) -> Option<PathBuf> {
        column_header(ui, "🕑 RECENT TELEMETRY LOGS");

        let mut picked = None;

        if self.recent.is_empty() {
            ui.weak("Nothing recorded yet.");
            return picked;
        }

        let open: Vec<&PathBuf> = logs
            .values()
            .filter_map(|source| match &source.origin {
                Origin::Log(progress) => Some(&progress.path),
                Origin::Live => None,
            })
            .collect();

        for log in &self.recent {
            let system_id = log
                .system_id
                .map_or_else(|| "--".to_owned(), |id| format!("0x{id:02x}"));

            let label = format!(
                "{}  {system_id}  {:>6.2} MiB",
                log.recorded_at.format("%Y-%m-%d %H:%M:%S"),
                log.bytes as f64 / (1024.0 * 1024.0),
            );

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.monospace(label)
                    .on_hover_text(log.path.display().to_string());

                if ui
                    .add_enabled(!open.contains(&&log.path), egui::Button::new("Open"))
                    .on_disabled_hover_text("Already open")
                    .clicked()
                {
                    picked = Some(log.path.clone());
                }
            });
        }

        picked
    }
}
