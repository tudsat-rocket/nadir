use core::{System, mode_name_string};

use mavspec::rust::dialects::common::enums::{MavModeFlag, MavModeProperty, MavStandardMode};
use mavspec::rust::dialects::common::messages::Heartbeat;

use eframe::egui;
use egui::{Button, Color32, Frame, Margin, RichText, Stroke, Vec2};

use crate::colors::{COLOR_INDICATOR_ADVANCED, COLOR_INDICATOR_AUTONOMY, COLOR_INDICATOR_WARNING};
use crate::panes::PaneUi;
use crate::widgets::{AlertLine, AlertTier};

pub struct StatusPane {}

impl StatusPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }
}

// Mode names arrive Debug-formatted in camel case ("PressFill"); stack the words on the button and
// shout them, aviation-style: "PRESS\nFILL".
fn mode_button_label(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if prev_lower && ch.is_uppercase() {
            out.push('\n');
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        out.push(ch.to_ascii_uppercase());
    }
    out
}

impl PaneUi for StatusPane {
    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        0.0
    }

    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        let Ok(Heartbeat {
            base_mode,
            custom_mode,
            ..
        }) = system.last_message::<Heartbeat>()
        else {
            return;
        };

        let s = 14.0;
        let button_h = s + 8.0;
        let armed = base_mode.contains(MavModeFlag::SAFETY_ARMED);

        ui.add_space(4.0);

        // Arm controls and cautions live on a subdued two-row strip across the top, "at eye level"
        // with the vitals columns. The arm buttons split across the rows so each pairs an arm
        // control with an alert tier: red flight-critical alarms (NACKs, RF uplink/downlink loss)
        // beside ARM, amber cautions beside DISARM. The mode grid (which shows the current mode via
        // its highlighted button) sits below so the mode buttons don't ride the window edge.
        let row_gap = 4.0;
        egui::TopBottomPanel::top(egui::Id::new("status_arm_caution_strip"))
            .resizable(false)
            .exact_height(2.0 * button_h + row_gap + 10.0)
            .frame(
                Frame::new()
                    .fill(ui.visuals().extreme_bg_color)
                    .inner_margin(Margin::symmetric(7, 5)),
            )
            .show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing.y = row_gap;
                let size = Vec2::new(90.0, button_h);

                ui.horizontal(|ui| {
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
                    ui.separator();
                    ui.add_space(2.0);
                    ui.add(AlertLine {
                        system: &system,
                        tier: AlertTier::Critical,
                    });
                });

                ui.horizontal(|ui| {
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
                    ui.add_space(2.0);
                    ui.add(AlertLine {
                        system: &system,
                        tier: AlertTier::Caution,
                    });
                });
            });

        ui.style_mut().spacing.item_spacing = Vec2::ZERO;

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

            // Roomier horizontal padding inside each mode button; the layout math below reads
            // button_padding.x, so widen it before those computations.
            ui.spacing_mut().button_padding.x = 10.0;

            ui.add_space(outer_pad);

            let side_pad = 8;
            let avail_h = ui.available_height() - outer_pad;
            let row_h_raw = (avail_h - (rows.saturating_sub(1) as f32) * spacing) / rows as f32;
            let row_h = row_h_raw.max(18.0);

            let labels: Vec<String> = modes
                .iter()
                .map(|mode_info| {
                    let name = if mode_info.standard_mode == MavStandardMode::NonStandard {
                        mode_name_string(&mode_info.mode_name)
                    } else {
                        format!("{:?}", mode_info.standard_mode)
                    };
                    mode_button_label(&name)
                })
                .collect();
            let max_lines = labels.iter().map(|l| l.lines().count()).max().unwrap_or(1);

            // Font grows with the row height (taller pane -> larger buttons), but is capped at the
            // largest size where all but the longest ~20% of names still fit their column. That
            // keeps most buttons at one uniform size; the few outliers get shrunk individually by
            // the per-button logic in the loop. Multi-line labels shrink the budget so all lines
            // fit the row height.
            let vertical_budget = if max_lines > 1 {
                (row_h * 0.64 / max_lines as f32).max(9.0)
            } else {
                (row_h * 0.5).max(10.0)
            };
            let button_pad_x = ui.spacing().button_padding.x;
            let grid_w = ui.available_width() - 2.0 * f32::from(side_pad);
            let col_w = (grid_w - (cols.saturating_sub(1) as f32) * spacing) / cols as f32;
            let inner_w = (col_w - button_pad_x * 2.0).max(1.0);
            let ref_size = 14.0;
            let mut fit_sizes: Vec<f32> = labels
                .iter()
                .map(|label| {
                    // Newlines survive layout_no_wrap; size().x is the widest line.
                    let w = ui.ctx().fonts(|fonts| {
                        fonts
                            .layout_no_wrap(
                                label.clone(),
                                egui::FontId::proportional(ref_size),
                                Color32::WHITE,
                            )
                            .size()
                            .x
                    });
                    if w > 0.0 {
                        // Leave a little extra horizontal padding around the name.
                        ref_size * inner_w / w * 0.88
                    } else {
                        ref_size
                    }
                })
                .collect();
            fit_sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let outlier_idx = (fit_sizes.len() as f32 * 0.2).floor() as usize;
            let width_cap = fit_sizes
                .get(outlier_idx)
                .copied()
                .unwrap_or(vertical_budget);
            let s_grid = width_cap.min(vertical_budget).max(10.0);

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
                                let name = labels[r * cols + c].clone();
                                let selected = custom_mode == mode_info.custom_mode;
                                let auto_mode =
                                    mode_info.properties.contains(MavModeProperty::AUTO_MODE);
                                let advanced =
                                    mode_info.properties.contains(MavModeProperty::ADVANCED);
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
                                    (s_grid * inner_w / text_w * 0.95).max(7.0)
                                } else {
                                    s_grid
                                };
                                // Cyan marks autonomous modes: as text color when unselected, as
                                // the fill when selected - so the color of the selected button
                                // answers "is the vehicle acting autonomously right now".
                                let text_color = match (selected, auto_mode) {
                                    (true, true) => Color32::BLACK,
                                    (true, false) => col_ui.visuals().selection.stroke.color,
                                    (false, true) => COLOR_INDICATOR_AUTONOMY,
                                    (false, false) => {
                                        col_ui.visuals().widgets.inactive.fg_stroke.color
                                    }
                                };
                                // The stacked lines should center on each other, which a Button's
                                // own label can't do - so the button renders empty and the centered
                                // galley is painted over it.
                                let mut job = egui::text::LayoutJob::default();
                                job.append(
                                    &name,
                                    0.0,
                                    egui::TextFormat {
                                        font_id: egui::FontId::proportional(s_btn),
                                        color: text_color,
                                        ..Default::default()
                                    },
                                );
                                job.halign = egui::Align::Center;
                                let galley = col_ui.fonts(|fonts| fonts.layout_job(job));

                                let button = if selected {
                                    let button = Button::new("").selected(true);
                                    if auto_mode {
                                        button.fill(COLOR_INDICATOR_AUTONOMY)
                                    } else {
                                        button
                                    }
                                } else {
                                    Button::new("")
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            col_ui.visuals().weak_text_color(),
                                        ))
                                };

                                let size = Vec2::new(col_ui.available_width(), row_h);
                                let mut resp = col_ui
                                    .add_sized(size, button)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                let galley_pos = egui::Pos2::new(
                                    resp.rect.center().x,
                                    resp.rect.center().y - galley.size().y / 2.0,
                                );
                                col_ui.painter().galley(galley_pos, galley, text_color);

                                // Advanced modes get a small amber corner tick: a "careful" marker
                                // in the caution color family that stays visible when the mode is
                                // selected.
                                if advanced {
                                    let r = resp.rect;
                                    col_ui.painter().add(egui::Shape::convex_polygon(
                                        vec![
                                            egui::pos2(r.max.x - 11.0, r.min.y + 3.0),
                                            egui::pos2(r.max.x - 3.0, r.min.y + 3.0),
                                            egui::pos2(r.max.x - 3.0, r.min.y + 11.0),
                                        ],
                                        COLOR_INDICATOR_ADVANCED,
                                        Stroke::NONE,
                                    ));
                                }
                                if auto_mode || advanced {
                                    let hover = match (auto_mode, advanced) {
                                        (true, true) => "Autonomous mode; advanced users only.",
                                        (true, false) => "Autonomous mode.",
                                        _ => "Advanced users only.",
                                    };
                                    resp = resp.on_hover_text(hover);
                                }

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
