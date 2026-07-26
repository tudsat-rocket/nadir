use core::Core;

use eframe::egui;
use egui::{Align, Color32, Layout, RichText};
use mavspec::rust::dialects::common::messages::Heartbeat;

use crate::views::View;
use crate::widgets::{ArmedIndicator, AutopilotLogo, MavStateIndicator, ModeDisplay};

const WIDTH: f32 = 300.0;
/// Collapsed, the bar keeps just enough width for one icon per row.
const COLLAPSED_WIDTH: f32 = 37.0;

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
        ctx: &egui::Context,
        core: &Core,
        active_view: &mut View,
        logs_shown: &mut bool,
    ) {
        let collapsed = self.collapsed;

        egui::SidePanel::left("sidepanel")
            .resizable(false)
            .exact_width(if collapsed { COLLAPSED_WIDTH } else { WIDTH })
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());

                for (i, system_id) in core.live.known_system_ids().iter().enumerate() {
                    if i != 0 && !collapsed {
                        ui.separator();
                    }

                    let Some(system) = core.live.system(*system_id) else {
                        continue;
                    };

                    if collapsed {
                        ui.selectable_value(active_view, View::System(*system_id), system.icon());
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
                            // TODO: hardcoded, unlike the status bar's consumables column.
                            ui.label(RichText::new("🔋 98%").color(Color32::from_rgb(78, 154, 6)));
                            ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                    ui.selectable_value(
                                        active_view,
                                        View::System(*system_id),
                                        "Select ➡",
                                    );

                                    let total_data_rate = system
                                        .channels()
                                        .iter_mut()
                                        .map(|(_, s)| s.received_data_rate())
                                        .sum::<f32>()
                                        / 1024.0;
                                    ui.label("KiB/s");
                                    ui.monospace(format!("{total_data_rate:>5.2}"));
                                    ui.weak("⏬");
                                })
                                .response
                            });
                        });
                    } else {
                        ui.monospace(format!("0x{system_id:02x}"));
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            ui.selectable_value(active_view, View::System(*system_id), "Select ➡");
                        });
                    }
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
                });
            });
    }
}
