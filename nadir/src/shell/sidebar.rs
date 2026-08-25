use std::collections::BTreeMap;

use nadir_core::{Origin, Source};

use eframe::egui;
use egui::{Align, Layout, RichText};
use mavspec::rust::dialects::common::messages::Heartbeat;

use crate::views::{LIVE, SourceId, View};
use crate::widgets::{
    ArmedIndicator, AutopilotLogo, MavStateIndicator, ModeDisplay, Readout, TEXT_SIZE, soc_color,
    state_of_charge,
};

const WIDTH: f32 = 300.0;
/// Collapsed, the bar keeps just enough width for one icon per row.
const COLLAPSED_WIDTH: f32 = 37.0;

/// What the user asked of the sidebar this frame. At most one, since each comes from its own click.
pub enum SidebarAction {
    OpenLog,
    CloseLog(SourceId),
}

/// Left strip listing the known systems and the global navigation: which view is active, whether the
/// log panel is shown, and the collapse toggle.
#[derive(Default)]
pub struct Sidebar {
    collapsed: bool,
}

impl Sidebar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Collapses the bar, e.g. once a system connects and the tile tree needs the width.
    pub fn collapse(&mut self) {
        self.collapsed = true;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        live: &Source,
        logs: &BTreeMap<SourceId, Source>,
        active_view: &mut View,
        logs_shown: &mut bool,
    ) -> Option<SidebarAction> {
        let collapsed = self.collapsed;
        let mut action = None;

        egui::Panel::left("sidepanel")
            .resizable(false)
            .exact_size(if collapsed { COLLAPSED_WIDTH } else { WIDTH })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // No header while the live systems are the only thing here.
                if !logs.is_empty() && !collapsed {
                    ui.horizontal(|ui| {
                        ui.weak("🖧");
                        ui.weak("Live");
                    });
                }

                self.systems(ui, LIVE, live, active_view);

                for (id, source) in logs {
                    let Origin::Log(progress) = &source.origin else {
                        continue;
                    };

                    ui.separator();

                    if !collapsed {
                        ui.horizontal(|ui| {
                            ui.weak("📂");
                            ui.label(RichText::new(progress.name()).monospace().size(TEXT_SIZE));

                            ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                    if ui.small_button("✖").on_hover_text("Close log").clicked() {
                                        action = Some(SidebarAction::CloseLog(*id));
                                    }
                                })
                                .response
                            });
                        });

                        if progress.done() {
                            ui.weak(format!("{} records", progress.records()));
                        } else {
                            ui.add(egui::ProgressBar::new(progress.fraction()).show_percentage());
                        }
                    }

                    self.systems(ui, *id, source, active_view);
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.add_space(5.0);
                    if ui
                        .button(if collapsed { "➡" } else { "⬅  Collapse" })
                        .clicked()
                    {
                        self.collapsed = !collapsed;
                    }
                    ui.separator();

                    #[cfg(feature = "profiling")]
                    {
                        let mut profiling_on = puffin::are_scopes_on();
                        ui.selectable_value(
                            &mut profiling_on,
                            true,
                            if collapsed { "⏱" } else { "⏱ Profiling" },
                        );
                        puffin::set_scopes_on(profiling_on);
                    }

                    ui.toggle_value(
                        logs_shown,
                        if collapsed {
                            "📃"
                        } else {
                            "📃 Show Debug Logs"
                        },
                    );

                    ui.separator();

                    ui.selectable_value(
                        active_view,
                        View::Settings,
                        if collapsed {
                            "🔧"
                        } else {
                            "🔧 Preferences"
                        },
                    );

                    ui.selectable_value(
                        active_view,
                        View::Overview,
                        if collapsed { "🖧" } else { "🖧 Overview" },
                    );

                    if ui
                        .button(if collapsed { "📂" } else { "📂 Open Log" })
                        .on_hover_text("Open a telemetry log")
                        .clicked()
                    {
                        action = Some(SidebarAction::OpenLog);
                    }
                });
            });

        action
    }

    /// The systems of one source, live or recorded.
    fn systems(&self, ui: &mut egui::Ui, id: SourceId, source: &Source, active_view: &mut View) {
        let collapsed = self.collapsed;
        let recorded = !matches!(source.origin, Origin::Live);

        for (i, system_id) in source.known_system_ids().iter().enumerate() {
            if i != 0 && !collapsed {
                ui.separator();
            }

            let Some(system) = source.system(*system_id) else {
                continue;
            };

            let view = View::system(id, *system_id);

            if collapsed {
                // With only an icon per row, a recording would otherwise be indistinguishable from
                // the vehicle it was recorded from.
                let label = if recorded {
                    format!("📂{}", system.icon())
                } else {
                    system.icon().to_owned()
                };

                ui.selectable_value(active_view, view, label);
            } else if let Ok(heartbeat) = system.last_message::<Heartbeat>() {
                ui.horizontal(|ui| {
                    ui.monospace(format!("0x{system_id:02x}"));
                    ui.label(system.icon());

                    ui.add(ArmedIndicator(heartbeat.base_mode));

                    ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            ui.add_space(5.0);
                            ui.add(AutopilotLogo(heartbeat.autopilot, heartbeat.type_));
                        })
                        .response
                    });
                });

                ui.horizontal(|ui| {
                    ui.add(ModeDisplay::new(system.clone()));
                    ui.add_space(5.0);
                    ui.add(MavStateIndicator(heartbeat.system_status));
                });

                ui.horizontal(|ui| {
                    if let Some(soc) = state_of_charge(&system) {
                        let color = soc_color(f32::from(soc) / 100.0, ui.visuals());
                        ui.add(Readout {
                            value: f32::from(soc),
                            decimals: 0,
                            prefix: "🔋 ",
                            unit: Some("%"),
                            font: egui::TextStyle::Monospace.resolve(ui.style()),
                            color,
                            ..Default::default()
                        });
                    } else {
                        ui.weak("🔋 --");
                    }

                    ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            ui.selectable_value(active_view, view, "Select ➡");

                            // A recording has no channels, and so no rate to show.
                            if !recorded {
                                let total_data_rate = system
                                    .channels()
                                    .iter_mut()
                                    .map(|(_, s)| s.received_data_rate())
                                    .sum::<f32>()
                                    / 1024.0;
                                ui.label("KiB/s");
                                ui.add(Readout {
                                    value: total_data_rate,
                                    decimals: 2,
                                    width_chars: 5,
                                    font: egui::TextStyle::Monospace.resolve(ui.style()),
                                    color: ui.visuals().text_color(),
                                    ..Default::default()
                                });
                                ui.weak("⏬");
                            }
                        })
                        .response
                    });
                });
            } else {
                ui.monospace(format!("0x{system_id:02x}"));
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    ui.selectable_value(active_view, view, "Select ➡");
                });
            }
        }
    }
}
