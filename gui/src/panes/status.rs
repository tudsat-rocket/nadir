use mavspec::rust::default_dialect::messages::AvailableModes;
use mavspec::rust::dialects::ardupilotmega::enums::PlaneMode;
use mavspec::rust::dialects::common::enums::{
    MavAutopilot, MavModeFlag, MavModeProperty, MavStandardMode, MavState, MavType,
};
use mavspec::rust::dialects::common::messages::Heartbeat;

use eframe::egui;
use egui::{Align, Button, Color32, Frame, Grid, Layout, Rect, RichText, Separator, Stroke, Vec2};

use crate::widgets::ModeDropdown;
use crate::{panes::TreeBehavior, views::View, widgets};

pub struct StatusPane {}

impl StatusPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let local_position = system.last_local_position_ned().ok().flatten();

        let Some(Heartbeat {
            type_: mav_type,
            autopilot,
            base_mode,
            custom_mode,
            system_status,
            mavlink_version: _,
        }) = system.last_heartbeat().unwrap_or_default()
        else {
            return;
        };

        let icon = system.icon();

        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.add_space(5.0);

            ui.vertical(|ui| {
                ui.place(ui.available_rect_before_wrap(), |ui: &mut egui::Ui| {
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        ui.add_space(5.0);
                        ui.add(AutopilotLogo(autopilot, mav_type));
                    })
                    .response
                });

                ui.horizontal(|ui| {
                    ui.weak("System");
                    ui.monospace(format!("0x{system_id:02}"));
                    ui.add_space(5.0);
                    ui.add(MavStateIndicator(system_status));
                    ui.add_space(5.0);
                    ui.label(format!("{icon} {mav_type:?}"));
                    ui.add_space(5.0);
                    ui.label(
                        local_position
                            .as_ref()
                            .map(|lp| format!("🕑 {:.1}s", (lp.time_boot_ms as f32) / 1000.0))
                            .unwrap_or_default(),
                    )
                });
            });
        });

        ui.style_mut().spacing.item_spacing = Vec2::ZERO;

        ui.add(Separator::default().spacing(0.01));

        Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .outer_margin(0.0)
            .inner_margin(7.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                let mut right_matter_rect = ui.available_rect_before_wrap();
                //right_matter_rect.set_left(right_matter_rect.right() - 350.0);
                right_matter_rect.set_left(right_matter_rect.right() - 210.0);
                right_matter_rect.set_bottom(right_matter_rect.top() + 65.0);

                ui.place(right_matter_rect, |ui: &mut egui::Ui| {
                    ui.horizontal_top(|ui| {
                        ui.set_height(ui.available_height());

                        ui.separator();

                        ui.vertical(|ui| {
                            ui.weak("♥ Vitals");
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("🔋 98%, 12.5V")
                                        .color(Color32::from_rgb(78, 154, 6)),
                                );
                                ui.separator();
                                ui.label(
                                    RichText::new("97%, 12.4V")
                                        .color(Color32::from_rgb(78, 154, 6)),
                                );
                            });
                            ui.label("💾 10%, 3.2/32.0 MiB");
                            ui.label("📡 11 sats, HDOP 0.9");
                        });
                    })
                    .response
                });

                ui.vertical(|ui| {
                    ui.weak("⎈ Mode");
                    ui.add_space(5.0);
                    ui.add(ModeDisplay::new(system.clone()).font_size(24.0));
                    ui.add_space(20.0);
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let armed = base_mode.contains(MavModeFlag::SAFETY_ARMED);
                        let size = Vec2::new(75.0, 24.0);

                        let arm_button = if armed {
                            Button::selectable(true, "ARMED").fill(Color32::from_rgb(204, 0, 0))
                        } else {
                            Button::selectable(false, "ARM")
                        };

                        let disarm_button = if !armed {
                            Button::selectable(true, "DISARMED")
                        } else {
                            Button::selectable(false, "DISARM")
                        };

                        if ui.add_sized(size, arm_button).clicked() {
                            system.do_arm(true, false);
                        }

                        ui.add_space(5.0);

                        if ui.add_sized(size, disarm_button).clicked() {
                            system.do_arm(false, false);
                        }
                    });

                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(5.0);

                    ui.vertical(|ui| {
                        let w = ui.available_width() / 5.0;
                        let size = Vec2::new(w, 24.0);

                        let favourites = match (autopilot, mav_type) {
                            (MavAutopilot::Ardupilotmega, MavType::FixedWing) => Some((
                                vec![
                                    PlaneMode::Manual as u32,
                                    PlaneMode::Stabilize as u32,
                                    PlaneMode::Loiter as u32,
                                    PlaneMode::Guided as u32,
                                    PlaneMode::Auto as u32,
                                ],
                                vec![
                                    PlaneMode::Takeoff as u32,
                                    PlaneMode::Rtl as u32,
                                    PlaneMode::Autoland as u32,
                                ],
                            )),
                            // TODO: dialect
                            (MavAutopilot::Generic, MavType::Rocket) => {
                                Some((vec![0, 1, 2, 3, 4], vec![5, 6, 7, 8, 9]))
                            }
                            _ => None,
                        };

                        if let Some((row1, row2)) = favourites {
                            for (i, row) in [&row1, &row2].into_iter().enumerate() {
                                ui.horizontal(|ui| {
                                    for cm in row {
                                        let Some(mode_info) = system.custom_mode_info(*cm) else {
                                            continue;
                                        };

                                        let name = String::from_utf8_lossy(&mode_info.mode_name);
                                        let button = Button::selectable(custom_mode == *cm, name);

                                        if ui.add_sized(size, button).clicked() {
                                            if mode_info.standard_mode
                                                == MavStandardMode::NonStandard
                                            {
                                                system.do_set_custom_mode(*cm);
                                            } else {
                                                system
                                                    .do_set_standard_mode(mode_info.standard_mode);
                                            }
                                        }
                                    }

                                    if let Some(modes) = system.available_modes()
                                        && modes.len() > (row1.len() + row2.len())
                                        && i == 1
                                    {
                                        ui.add(ModeDropdown::new(&system));
                                    }
                                });
                            }
                        } else {
                            ui.add(ModeDropdown::new(&system));
                        }
                    });
                });
            });

        ui.add(Separator::default().spacing(0.01));

        let fs = f32::max(14.0, f32::min(52.0, ui.available_height() / 6.0));

        egui::Grid::new("hero_gauges")
            .min_row_height(ui.available_height() / 2.0)
            .min_col_width(ui.available_width() / 3.0)
            .max_col_width(ui.available_width() / 3.0)
            .show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.weak(RichText::new("Alt. AGL [m]").size(10.0));
                    if let Some(z) = local_position.as_ref().map(|lp| lp.z * -1.0) {
                        ui.strong(RichText::new(format!("{:.1}", z)).monospace().size(fs));
                    } else {
                        ui.weak(RichText::new("N/A").monospace().size(fs));
                    }
                });

                ui.vertical_centered_justified(|ui| {
                    ui.weak(RichText::new("Downrange [m]").size(10.0));
                    ui.label(RichText::new("1235").monospace().size(fs));
                });

                ui.vertical_centered_justified(|ui| {
                    ui.weak(RichText::new("Heading [°]").size(10.0));
                    ui.label(RichText::new("167").monospace().size(fs));
                });

                ui.end_row();

                ui.vertical_centered_justified(|ui| {
                    ui.weak(RichText::new("Vario [m/s]").size(10.0));
                    if let Some(vz) = local_position.as_ref().map(|lp| lp.vz * -1.0) {
                        ui.strong(RichText::new(format!("{:.1}", vz)).monospace().size(fs));
                    } else {
                        ui.weak(RichText::new("N/A").monospace().size(fs));
                    }
                });

                ui.vertical_centered_justified(|ui| {
                    ui.weak(RichText::new("Groundspeed [m/s]").size(10.0));
                    ui.label(RichText::new(" 058").monospace().size(fs));
                });

                ui.vertical_centered_justified(|ui| {
                    ui.weak(RichText::new("Bearing [°]").size(10.0));
                    ui.label(RichText::new("146").monospace().size(fs));
                });
            });
    }
}
