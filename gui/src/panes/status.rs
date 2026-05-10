use core::{System, mode_name_string};

use mavspec::rust::default_dialect::messages::BatteryStatus;
use mavspec::rust::dialects::common::enums::{MavModeFlag, MavModeProperty, MavStandardMode};
use mavspec::rust::dialects::common::messages::{Heartbeat, LocalPositionNed, SysStatus};

use eframe::egui;
use egui::{Align, Button, Color32, Frame, Layout, Margin, RichText, Separator, Stroke, Vec2};

use crate::colors::{COLOR_INDICATOR_AUTONOMY, COLOR_INDICATOR_WARNING};
use crate::panes::PaneUi;
use crate::widgets::{AutopilotLogo, MavStateIndicator, ModeDisplay};

pub struct StatusPane {}

impl StatusPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }
}

impl PaneUi for StatusPane {
    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        0.0
    }

    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        let local_position = system.last_message::<LocalPositionNed>().ok();

        let Ok(Heartbeat {
            type_: mav_type,
            autopilot,
            base_mode,
            custom_mode,
            system_status,
            mavlink_version: _,
        }) = system.last_message::<Heartbeat>()
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
                    ui.label(icon);
                    ui.monospace(format!("0x{:02x}", system.system_id));
                    ui.add_space(5.0);
                    ui.add(MavStateIndicator(system_status));
                    ui.add_space(5.0);
                    ui.label(
                        local_position
                            .as_ref()
                            .map(|lp| format!("🕑 {:.1}s", (lp.time_boot_ms as f32) / 1000.0))
                            .unwrap_or_default(),
                    );
                    ui.add_space(5.0);

                    // TODO: properly handle multiple batteries
                    let battery_remaining = system
                        .last_instance_message::<BatteryStatus>(1)
                        .ok()
                        .map(|b| b.battery_remaining)
                        .or_else(|| {
                            system
                                .last_message::<SysStatus>()
                                .ok()
                                .map(|s| s.battery_remaining)
                        });
                    if let Some(soc) = battery_remaining {
                        ui.label(
                            RichText::new(format!("🔋 {soc}%"))
                                .color(Color32::from_rgb(78, 154, 6)),
                        );
                    }

                    ui.add_space(5.0);
                    ui.label("💾 10%");
                    ui.add_space(5.0);
                    ui.label("📡 11/0.9");
                });
            });
        });

        ui.style_mut().spacing.item_spacing = Vec2::ZERO;

        ui.add(Separator::default().spacing(0.01));

        let s = 14.0;
        let button_h = s + 15.0;

        Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .outer_margin(0.0)
            .inner_margin(7.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                let armed = base_mode.contains(MavModeFlag::SAFETY_ARMED);

                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add(ModeDisplay::new(system.clone()).font_size(24.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let size = Vec2::new(100.0, button_h);

                        let arm_button = if armed {
                            Button::selectable(true, RichText::new("ARMED").size(s))
                                .fill(COLOR_INDICATOR_WARNING)
                        } else {
                            Button::selectable(false, RichText::new("ARM").size(s))
                        };

                        if armed {
                            ui.style_mut().visuals.override_text_color = Some(Color32::BLACK);
                        }
                        if ui.add_sized(size, arm_button).clicked() {
                            system.do_arm(true, false);
                        }
                        ui.style_mut().visuals.override_text_color = None;

                        ui.add_space(5.0);

                        let disarm_button = if armed {
                            Button::selectable(false, RichText::new("DISARM").size(s))
                        } else {
                            Button::selectable(true, RichText::new("DISARMED").size(s))
                        };

                        if ui.add_sized(size, disarm_button).clicked() {
                            system.do_arm(false, false);
                        }

                        ui.add_space(5.0);
                        ui.separator();
                        ui.add_space(5.0);
                        ui.weak("⎈ Mode");
                    });
                });
                ui.add_space(2.0);
            });

        ui.add(Separator::default().spacing(0.01));

        let modes: Vec<_> = system
            .available_modes()
            .unwrap_or_default()
            .into_iter()
            .filter(|m| !m.properties.contains(MavModeProperty::NOT_USER_SELECTABLE))
            .collect();
        if !modes.is_empty() {
            let count = modes.len();
            let max_cols = match count {
                0..=5 => count,
                6..=12 => 6,
                13..=20 => 7,
                21..=30 => 8,
                31..=50 => 10,
                _ => 12,
            };
            // Prefer fewer rows; on ties, minimise the empty trailing slots.
            let cols = (1..=max_cols)
                .min_by_key(|&c| {
                    let r = count.div_ceil(c);
                    let waste = r * c - count;
                    r * 2 + waste
                })
                .unwrap_or(1);
            let rows = count.div_ceil(cols);
            let spacing = 5.0;
            let outer_pad = 3.0;

            ui.add_space(outer_pad);

            let side_pad = 8;
            let avail_h = ui.available_height() - outer_pad;
            let row_h_raw = (avail_h - (rows.saturating_sub(1) as f32) * spacing) / rows as f32;
            let row_h = row_h_raw.max(18.0);
            let s_grid = (row_h - 14.0).clamp(10.0, s);

            Frame::new()
                .inner_margin(Margin::symmetric(side_pad, 0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = spacing;

                    for r in 0..rows {
                        if r != 0 {
                            ui.add_space(spacing);
                        }

                        let in_row = if r == rows - 1 {
                            count - r * cols
                        } else {
                            cols
                        };

                        ui.columns(in_row, |columns| {
                            for (c, col_ui) in columns.iter_mut().enumerate().take(in_row) {
                                let mode_info = &modes[r * cols + c];
                                let name =
                                    if mode_info.standard_mode == MavStandardMode::NonStandard {
                                        mode_name_string(&mode_info.mode_name)
                                    } else {
                                        format!("{:?}", mode_info.standard_mode)
                                    };
                                let selected = custom_mode == mode_info.custom_mode;
                                let auto_mode =
                                    mode_info.properties.contains(MavModeProperty::AUTO_MODE);
                                let advanced =
                                    mode_info.properties.contains(MavModeProperty::ADVANCED);
                                let accent = if auto_mode {
                                    Some(COLOR_INDICATOR_AUTONOMY)
                                } else if advanced {
                                    Some(COLOR_INDICATOR_WARNING)
                                } else {
                                    None
                                };
                                let inner_w = col_ui.available_width()
                                    - col_ui.spacing().button_padding.x * 2.0;
                                let text_w = col_ui.ctx().fonts(|fonts| {
                                    fonts
                                        .layout_no_wrap(
                                            name.clone(),
                                            egui::FontId::proportional(s_grid),
                                            Color32::WHITE,
                                        )
                                        .size()
                                        .x
                                });
                                let s_btn = if text_w > inner_w && text_w > 0.0 {
                                    (s_grid * inner_w / text_w * 0.95).max(8.0)
                                } else {
                                    s_grid
                                };
                                let text = RichText::new(name).size(s_btn);
                                let button = match (selected, accent) {
                                    (true, Some(c)) => Button::new(text).fill(c).selected(true),
                                    (true, None) => Button::new(text).selected(true),
                                    (false, Some(c)) => Button::new(text)
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::new(1.0, c)),
                                    (false, None) => {
                                        Button::new(text).fill(Color32::TRANSPARENT).stroke(
                                            Stroke::new(1.0, col_ui.visuals().weak_text_color()),
                                        )
                                    }
                                };
                                let button = button.wrap_mode(egui::TextWrapMode::Truncate);

                                if selected && advanced && !auto_mode {
                                    col_ui.style_mut().visuals.override_text_color =
                                        Some(Color32::BLACK);
                                }
                                let size = Vec2::new(col_ui.available_width(), row_h);
                                let resp = col_ui
                                    .add_sized(size, button)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                col_ui.style_mut().visuals.override_text_color = None;
                                if resp.clicked() {
                                    if mode_info.standard_mode == MavStandardMode::NonStandard {
                                        system.do_set_custom_mode(mode_info.custom_mode);
                                    } else {
                                        system.do_set_standard_mode(mode_info.standard_mode);
                                    }
                                }
                            }
                        });
                    }
                });

            ui.add_space(outer_pad);
        }
    }
}
