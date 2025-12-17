use mavspec::rust::dialects::ardupilotmega::enums::PlaneMode;
use mavspec::rust::dialects::common::enums::{MavAutopilot, MavModeFlag, MavStandardMode, MavType};
use mavspec::rust::dialects::common::messages::Heartbeat;

use eframe::egui;
use egui::{Align, Button, Color32, Frame, Layout, RichText, Separator, Vec2};

use crate::colors::COLOR_INDICATOR_WARNING;
use crate::widgets::{AutopilotLogo, MavStateIndicator, ModeDisplay, ModeDropdown};
use crate::{panes::TreeBehavior, views::View};

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
                    let s = 14.0;
                    let button_h = s + 15.0;

                    ui.vertical(|ui| {
                        let armed = base_mode.contains(MavModeFlag::SAFETY_ARMED);
                        let size = Vec2::new(100.0, button_h);

                        let arm_button = if armed {
                            Button::selectable(true, RichText::new("ARMED").size(s))
                                .fill(COLOR_INDICATOR_WARNING)
                        } else {
                            Button::selectable(false, RichText::new("ARM").size(s))
                        };

                        let disarm_button = if !armed {
                            Button::selectable(true, RichText::new("DISARMED").size(s))
                        } else {
                            Button::selectable(false, RichText::new("DISARM").size(s))
                        };

                        if armed {
                            ui.style_mut().visuals.override_text_color = Some(Color32::BLACK);
                        }

                        if ui.add_sized(size, arm_button).clicked() {
                            system.do_arm(true, false);
                        }

                        ui.style_mut().visuals.override_text_color = None;

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
                        let size = Vec2::new(w, button_h);

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
                                if i != 0 {
                                    ui.add_space(5.0);
                                }

                                ui.horizontal(|ui| {
                                    for cm in row {
                                        let Some(mode_info) = system.custom_mode_info(*cm) else {
                                            continue;
                                        };

                                        let name = String::from_utf8_lossy(&mode_info.mode_name);
                                        let button = Button::selectable(
                                            custom_mode == *cm,
                                            RichText::new(name).size(s),
                                        );

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
    }
}
