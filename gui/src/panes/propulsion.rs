use core::{MessageInstance, System};

use egui::{Button, Color32, DragValue, Frame, Image, Pos2, Rect, RichText, Vec2};
use mavspec::rust::dialects::common::enums::MavType;
use mavspec::rust::dialects::common::messages::{BatteryStatus, Heartbeat, SysStatus};
use mavspec::rust::dialects::minimal::enums::MavAutopilot;
use rapid_dialect::rapid::enums::ValveId;

use crate::colors::COLOR_INDICATOR_WARNING;
use crate::panes::{PaneUi, TreeBehavior};
use crate::views::View;
use crate::widgets::{BatteryIndicator, Plot, PlotLine};

mod arducopter;
mod arduplane;
mod px4;
mod rocket;

pub struct PropulsionPane {
    pressurization_throttle: f32,
    oxidizer_fill_throttle: f32,
    main_throttle: f32,
}

enum ValveControl<'a> {
    Pulse,
    Throttle(&'a mut f32),
}

// TODO: properly handle multiple batteries / different instance IDs
pub(super) fn battery_indicator(system: &System, compact: bool) -> Option<BatteryIndicator> {
    if let Ok(battery) = system.last_instance_message::<BatteryStatus>(1) {
        let voltage = battery
            .voltages
            .iter()
            .filter(|v| **v > 0 && **v < u16::MAX)
            .map(|v| f32::from(*v) / 1000.0)
            .next_back();

        Some(BatteryIndicator {
            id: battery.id,
            soc: f32::from(battery.battery_remaining) / 100.0,
            voltage,
            current: (battery.current_battery != -1)
                .then_some(f32::from(battery.current_battery) / 1000.0),
            consumed: (battery.current_consumed != -1).then_some(battery.current_consumed as f32),
            compact,
        })
    } else if let Ok(status) = system.last_message::<SysStatus>() {
        Some(BatteryIndicator {
            id: 0,
            soc: f32::from(status.battery_remaining) / 100.0,
            voltage: Some(f32::from(status.voltage_battery) / 1000.0),
            current: Some(f32::from(status.current_battery) / 100.0),
            consumed: None,
            compact,
        })
    } else {
        None
    }
}

impl PropulsionPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            pressurization_throttle: 0.0,
            oxidizer_fill_throttle: 0.0,
            main_throttle: 0.0,
        }
    }

    fn draw_battery(&mut self, ui: &mut egui::Ui, system: &System, pos: Pos2) {
        let battery_rect = Rect::from_center_size(pos, Vec2::new(60.0, 120.0));
        if let Some(indicator) = battery_indicator(system, false) {
            ui.place(battery_rect, indicator);
        }
    }

    fn draw_frame(&mut self, ui: &mut egui::Ui, system: &System, square: Rect) {
        let n = square.width();

        let Ok(heartbeat) = system.last_message::<Heartbeat>() else {
            return;
        };

        Frame::dark_canvas(ui.style()).show(ui, |ui| {
            ui.set_width(square.width());
            ui.set_height(square.height());

            // Frame::show shifts the inner ui by its inner_margin, so the outer
            // `square` no longer aligns with the visible content area. Rebind to
            // the inner ui's origin so absolute-coordinate painting matches the
            // frame border.
            let square = Rect::from_min_size(ui.max_rect().min, square.size());

            // TODO: extend support
            match (heartbeat.autopilot, heartbeat.type_) {
                (_, MavType::Rocket) => {
                    rocket::draw_hybrid(ui, system, square);
                }
                (MavAutopilot::Px4, _) => {
                    px4::draw_rotors(ui, system, square);
                    self.draw_battery(ui, system, square.center());
                }
                (MavAutopilot::Ardupilotmega, MavType::FixedWing) => {
                    let outline =
                        egui::include_image!("../../assets/vehicles/plane_twin_vtail_dark.svg");
                    ui.place(
                        square.shrink(n * 0.05),
                        Image::new(outline)
                            .maintain_aspect_ratio(true)
                            .max_width(n)
                            .tint(Color32::WHITE.gamma_multiply(0.5)),
                    );

                    arduplane::draw_servos(ui, system, square);
                    self.draw_battery(ui, system, square.center().lerp(square.center_top(), 0.5));
                }
                (MavAutopilot::Ardupilotmega, _) => {
                    arducopter::draw_rotors(ui, system, square);
                    self.draw_battery(ui, system, square.center());
                }
                _ => {}
            }
        });
    }
}

fn valve_row(
    ui: &mut egui::Ui,
    system: &System,
    label: &str,
    id: ValveId,
    control: ValveControl<'_>,
    button_size: Vec2,
) {
    let state = rocket::valve_state(system, id);

    ui.weak(label.to_uppercase());

    let close_active = state == Some(0.0);
    let close_text = if close_active { "CLOSED" } else { "CLOSE" };
    let close_btn = Button::selectable(close_active, RichText::new(close_text));
    if ui.add_sized(button_size, close_btn).clicked() {
        system.do_set_valve(id, 0.0);
    }

    match control {
        ValveControl::Pulse => {
            let _ = ui
                .add_sized(button_size, Button::new(RichText::new("PULSE")))
                .clicked();
        }
        ValveControl::Throttle(value) => {
            ui.allocate_ui_with_layout(
                button_size,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let dec = ui.small_button(RichText::new("\u{2193}"));
                    if dec.clicked() {
                        *value = (*value - 1.0).max(0.0);
                    }
                    let btn_w = dec.rect.width();
                    let spacing = ui.spacing().item_spacing.x;
                    let drag_w = (ui.available_width() - btn_w - spacing).max(0.0);
                    ui.add_sized(
                        Vec2::new(drag_w, button_size.y),
                        DragValue::new(value)
                            .speed(1.0)
                            .range(0.0..=100.0)
                            .suffix(" %"),
                    );
                    if ui.small_button(RichText::new("\u{2191}")).clicked() {
                        *value = (*value + 1.0).min(100.0);
                    }
                },
            );
        }
    }

    let open_active = matches!(state, Some(s) if s > 0.0);
    let open_btn = if open_active {
        Button::selectable(true, RichText::new("OPEN")).fill(COLOR_INDICATOR_WARNING)
    } else {
        Button::selectable(false, RichText::new("OPEN"))
    };
    if open_active {
        ui.style_mut().visuals.override_text_color = Some(Color32::BLACK);
    }
    if ui.add_sized(button_size, open_btn).clicked() {
        system.do_set_valve(id, 1.0);
    }
    ui.style_mut().visuals.override_text_color = None;

    ui.end_row();
}

fn valve_state_lines(system_id: u8) -> Vec<PlotLine> {
    [
        (ValveId::PressurantVent, "Pressurant Vent"),
        (ValveId::Pressurization, "Pressurization"),
        (ValveId::OxidizerVent, "Oxidizer Vent"),
        (ValveId::OxidizerFill, "Oxidizer Fill"),
        (ValveId::Main, "Main"),
    ]
    .into_iter()
    .map(|(id, alias)| PlotLine {
        system_id,
        component_id: 1,
        message_name: "VALVE".to_owned(),
        instance: Some(MessageInstance {
            field: "id".to_owned(),
            value: i64::from(id.value()),
        }),
        field_name: "state".to_owned(),
        alias: Some(alias.to_owned()),
        unit: None,
        color: None,
        scale: None,
    })
    .collect()
}

fn pressure_lines(system_id: u8) -> Vec<PlotLine> {
    let mut lines: Vec<PlotLine> = [
        (0, "Pressurant", rocket::N2_COLOR),
        (1, "Oxidizer", rocket::N2O_COLOR),
        (2, "Combustion", rocket::CC_COLOR),
    ]
    .into_iter()
    .map(|(id, alias, color)| PlotLine {
        system_id,
        component_id: 1,
        message_name: "PRESSURE_VESSEL".to_owned(),
        instance: Some(MessageInstance {
            field: "id".to_owned(),
            value: id,
        }),
        field_name: "pressure1".to_owned(),
        alias: Some(alias.to_owned()),
        unit: Some("bar".to_owned()),
        color: Some(color),
        // PRESSURE_VESSEL.pressure1 is in kPa; the diagram and rendering use bar.
        scale: Some(0.01),
    })
    .collect();

    lines.push(PlotLine {
        system_id,
        component_id: 1,
        message_name: "PRESSURE_VESSEL".to_owned(),
        instance: Some(MessageInstance {
            field: "id".to_owned(),
            value: 1,
        }),
        field_name: "level".to_owned(),
        alias: Some("Fill Level".to_owned()),
        unit: Some("%".to_owned()),
        color: Some(Color32::WHITE),
        // PRESSURE_VESSEL.level is 0..=10000; render as percent.
        scale: Some(0.01),
    });

    lines
}

impl PaneUi for PropulsionPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };
        let Some(system) = behavior.core.system(system_id) else {
            return;
        };
        let Ok(heartbeat) = system.last_message::<Heartbeat>() else {
            return;
        };

        let supported = heartbeat.autopilot == MavAutopilot::Px4
            || heartbeat.autopilot == MavAutopilot::Ardupilotmega
            || heartbeat.type_ == MavType::Rocket;
        if !supported {
            ui.centered_and_justified(|ui| {
                ui.weak("No propulsion information available.");
            });
            return;
        }

        let rect = ui.clip_rect();

        if heartbeat.type_ == MavType::Rocket {
            let h = rect.height();
            let w = h * 0.35;
            ui.horizontal_top(|ui| {
                let cursor = ui.cursor().min;
                let square = Rect::from_min_size(cursor, Vec2::new(w, h));
                self.draw_frame(ui, &system, square);

                ui.vertical(|ui| {
                    egui::TopBottomPanel::bottom(egui::Id::new((
                        "propulsion_valves_panel",
                        system_id,
                    )))
                    .resizable(false)
                    .show_separator_line(false)
                    .frame(egui::Frame::new())
                    .show_inside(ui, |ui| {
                        ui.separator();
                        ui.add_space(5.0);
                        ui.weak("🚰 Valves");
                        ui.add_space(5.0);

                        let total_w = ui.available_width();
                        let col_w = total_w / 4.0 - ui.spacing().item_spacing.x;
                        let button_size = Vec2::new(col_w, ui.spacing().interact_size.y);

                        egui::Grid::new("propulsion_valves")
                            .num_columns(4)
                            .min_col_width(col_w)
                            .striped(true)
                            .show(ui, |ui| {
                                valve_row(
                                    ui,
                                    &system,
                                    "Pressurant Vent",
                                    ValveId::PressurantVent,
                                    ValveControl::Pulse,
                                    button_size,
                                );
                                valve_row(
                                    ui,
                                    &system,
                                    "Pressurization",
                                    ValveId::Pressurization,
                                    ValveControl::Throttle(&mut self.pressurization_throttle),
                                    button_size,
                                );
                                valve_row(
                                    ui,
                                    &system,
                                    "Oxidizer Vent",
                                    ValveId::OxidizerVent,
                                    ValveControl::Pulse,
                                    button_size,
                                );
                                valve_row(
                                    ui,
                                    &system,
                                    "Oxidizer Fill",
                                    ValveId::OxidizerFill,
                                    ValveControl::Throttle(&mut self.oxidizer_fill_throttle),
                                    button_size,
                                );
                                valve_row(
                                    ui,
                                    &system,
                                    "Main",
                                    ValveId::Main,
                                    ValveControl::Throttle(&mut self.main_throttle),
                                    button_size,
                                );
                            });
                    });

                    let valve_states_h = ui.available_height() / 3.5;
                    egui::TopBottomPanel::bottom(egui::Id::new((
                        "propulsion_valve_states_panel",
                        system_id,
                    )))
                    .resizable(false)
                    .show_separator_line(false)
                    .frame(egui::Frame::new())
                    .exact_height(valve_states_h)
                    .show_inside(ui, |ui| {
                        let vs_lines = valve_state_lines(system_id);
                        let valve_states_plot = Plot::new(
                            &vs_lines,
                            &behavior.core,
                            behavior.shared_plot_state,
                            (Some(0.0), Some(3.0)),
                        );
                        ui.add_sized(
                            Vec2::new(ui.available_width(), ui.available_height()),
                            valve_states_plot,
                        );
                    });

                    let p_lines = pressure_lines(system_id);
                    let pressure_plot = Plot::new(
                        &p_lines,
                        &behavior.core,
                        behavior.shared_plot_state,
                        (Some(0.0), None),
                    );
                    ui.add_sized(
                        Vec2::new(ui.available_width(), ui.available_height()),
                        pressure_plot,
                    );
                });
            });
        } else {
            let n = f32::min(rect.width(), rect.height());
            let x_offset = (rect.width() - n).max(0.0) / 2.0;
            let square = Rect::from_min_size(
                egui::pos2(rect.left() + x_offset, rect.top()),
                Vec2::new(n, n),
            );
            ui.vertical_centered(|ui| {
                self.draw_frame(ui, &system, square);
            });
        }
    }
}
