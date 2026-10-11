use std::collections::BTreeMap;

use nadir_core::settings::{LayoutRef, SharingSettings};
use nadir_core::{Origin, Settings, Source};

use eframe::egui;
use egui::{Align, Button, Color32, Layout, RichText};
use mavspec::rust::dialects::common::messages::Heartbeat;

use crate::colors::{COLOR_INDICATOR_LIMITS, readable};
use crate::views::{LAYOUT_TEMPLATES, LIVE, SourceId, View};
use crate::widgets::{
    ArmedIndicator, AutopilotLogo, MavStateIndicator, ModeDisplay, Readout, TEXT_SIZE, soc_color,
    state_of_charge,
};
#[cfg(not(target_arch = "wasm32"))]
mod sessions;
mod version;
#[cfg(not(target_arch = "wasm32"))]
use sessions::Sessions;
use version::Version;

const WIDTH: f32 = 300.0;
/// Collapsed, the bar keeps just enough width for one icon per row.
const COLLAPSED_WIDTH: f32 = 37.0;

/// What the user asked of the sidebar this frame. At most one, since each comes from its own click.
pub enum SidebarAction {
    OpenLog,
    CloseLog(SourceId),
    RestoreLayout(LayoutRef),
    #[cfg(not(target_arch = "wasm32"))]
    JoinSession(nadir_core::SessionAddr),
}

/// Left strip listing the known systems and the global navigation: which view is active, whether the
/// log panel is shown, and the collapse toggle.
pub struct Sidebar {
    collapsed: bool,
    version: Version,
    #[cfg(not(target_arch = "wasm32"))]
    sessions: Sessions,
}

impl Sidebar {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            collapsed: false,
            version: Version::check(ctx),
            #[cfg(not(target_arch = "wasm32"))]
            sessions: Sessions::default(),
        }
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
        settings: &Settings,
    ) -> Option<SidebarAction> {
        let sharing = &settings.sharing;
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

                self.systems(ui, LIVE, live, active_view, sharing);

                for (id, source) in logs {
                    ui.separator();

                    if !collapsed {
                        let (icon, name, hover) = match &source.origin {
                            Origin::Log(progress) => ("📂", progress.name(), "Close log"),
                            Origin::Stream(progress) => {
                                ("📡", progress.session.clone(), "Leave session")
                            }
                            Origin::Live => continue,
                        };

                        ui.horizontal(|ui| {
                            ui.weak(icon);
                            ui.label(RichText::new(name).monospace().size(TEXT_SIZE));

                            ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                    if ui.small_button("✖").on_hover_text(hover).clicked() {
                                        action = Some(SidebarAction::CloseLog(*id));
                                    }
                                })
                                .response
                            });
                        });

                        match &source.origin {
                            Origin::Log(progress) if progress.done() => {
                                ui.weak(format!("{} records", progress.records()));
                            }
                            Origin::Log(progress) => {
                                ui.add(
                                    egui::ProgressBar::new(progress.fraction()).show_percentage(),
                                );
                            }
                            Origin::Stream(progress) => {
                                let state = if progress.connected() {
                                    "connected"
                                } else {
                                    "disconnected"
                                };
                                ui.weak(format!("{} records, {state}", progress.records()));
                            }
                            Origin::Live => {}
                        }
                    }

                    self.systems(ui, *id, source, active_view, sharing);
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.add_space(5.0);
                    if ui
                        .button(if collapsed { "➡" } else { "⬅  Collapse" })
                        .clicked()
                    {
                        self.collapsed = !collapsed;
                    }
                    self.version.ui(ui, collapsed);
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

                    let muted = live.mute_new_systems()
                        && live
                            .known_system_ids()
                            .iter()
                            .filter_map(|id| live.system(*id))
                            .all(|system| system.muted());

                    if mute_toggle(ui, muted, if collapsed { "⛔" } else { "⛔ Mute Uplink" })
                        .on_hover_text("Transmit nothing to any system, incl. future ones")
                        .clicked()
                    {
                        live.set_muted(!muted);
                    }

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

                    ui.menu_button(if collapsed { "⊞" } else { "⊞ Layout" }, |ui| {
                        let templates = LAYOUT_TEMPLATES
                            .iter()
                            .map(|(template, label)| (LayoutRef::Template(*template), *label));
                        let saved = settings.layouts.iter().map(|saved| {
                            (LayoutRef::Saved(saved.name.clone()), saved.name.as_str())
                        });

                        for (layout, label) in templates.chain(saved) {
                            if ui.button(label).clicked() {
                                action = Some(SidebarAction::RestoreLayout(layout));
                            }
                        }
                    });

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

                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(addr) = self.sessions.ui(ui, collapsed, sharing.server.as_deref()) {
                        action = Some(SidebarAction::JoinSession(addr));
                    }
                });
            });

        action
    }

    /// The systems of one source, live or recorded.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    fn systems(
        &self,
        ui: &mut egui::Ui,
        id: SourceId,
        source: &Source,
        active_view: &mut View,
        sharing: &SharingSettings,
    ) {
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
                let label = match source.origin {
                    Origin::Live => system.icon().to_owned(),
                    Origin::Log(_) => format!("📂{}", system.icon()),
                    Origin::Stream(_) => format!("📡{}", system.icon()),
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
                        let color = soc_color(soc / 100.0, ui.visuals());
                        ui.add(Readout {
                            value: soc,
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

                            if !recorded {
                                let muted = system.muted();
                                if mute_toggle(ui, muted, "⛔")
                                    .on_hover_text("Transmit nothing to this system")
                                    .clicked()
                                {
                                    system.set_muted(!muted);
                                }

                                #[cfg(not(target_arch = "wasm32"))]
                                share_toggle(ui, &system, sharing);

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

/// Starts or stops publishing a system to the sharing server, as a new session each time.
#[cfg(not(target_arch = "wasm32"))]
fn share_toggle(ui: &mut egui::Ui, system: &nadir_core::System, sharing: &SharingSettings) {
    let shared = system.sharing();

    let Some(server) = &sharing.server else {
        ui.add_enabled(false, Button::selectable(false, "📡"))
            .on_disabled_hover_text("Set a sharing server in Preferences");
        return;
    };

    let hover = match &shared {
        Some((addr, status)) => format!(
            "Sharing as {}\n{}, {:.1} KiB sent, {:.1} KiB pending, {} dropped",
            addr.id,
            if status.connected() {
                "connected"
            } else {
                "not connected"
            },
            status.acked_bytes() as f64 / 1024.0,
            status.pending_bytes() as f64 / 1024.0,
            status.dropped(),
        ),
        None => format!("Share this system's telemetry through {server}"),
    };

    if ui
        .add(Button::selectable(shared.is_some(), "📡"))
        .on_hover_text(hover)
        .clicked()
    {
        system.set_sharing(shared.is_none().then(|| {
            nadir_core::SessionAddr::new(server.clone(), &sharing.station, system.system_id)
        }));
    }
}

fn mute_toggle(ui: &mut egui::Ui, muted: bool, label: &str) -> egui::Response {
    let mut button = Button::selectable(muted, label);
    if muted {
        button = button.fill(readable(COLOR_INDICATOR_LIMITS, ui.visuals()));
        ui.style_mut().visuals.override_text_color = Some(Color32::WHITE);
    }

    let response = ui.add(button);
    ui.style_mut().visuals.override_text_color = None;

    response
}
