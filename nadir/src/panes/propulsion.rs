use nadir_core::{MessageInstance, System};

use egui::{
    Align2, Button, Color32, CornerRadius, DragValue, FontId, Frame, Image, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, Vec2, pos2,
};
use mavspec::rust::dialects::common::enums::MavType;
use mavspec::rust::dialects::common::messages::{BatteryStatus, Heartbeat, SysStatus};
use mavspec::rust::dialects::minimal::enums::MavAutopilot;
use rapid_dialect::rapid::enums::ValveId;

use crate::colors::{COLOR_INDICATOR_GOOD, COLOR_INDICATOR_WARNING, instrument_visuals, readable};
use crate::panes::{PaneUi, TreeBehavior};
use crate::views::View;
use crate::widgets::{BatteryIndicator, Plot, PlotLine, Readout};

mod arducopter;
mod arduplane;
mod px4;
mod rocket;

// Firmware bound on pulse length (mission::valves::MAX_PULSE_DURATION).
const MAX_PULSE_DURATION_SECS: f32 = 30.0;

// How long a commanded-vs-actual mismatch must persist before the cue starts
// blinking, so normal valve travel doesn't flash the UI.
const VALVE_MISMATCH_DEBOUNCE_SECS: f64 = 0.5;

// Commanded position within this of fully closed/open latches the CLOSE/OPEN button.
const VALVE_LATCH_EPS: f32 = 0.02;

const VALVE_COUNT: usize = 9;

// Solenoid valves are binary; servo valves additionally accept a proportional
// set-position, making the servo control a strict superset of the solenoid one.
#[derive(Copy, Clone, PartialEq, Eq)]
enum ValveKind {
    Solenoid,
    Servo,
}

// Single source of truth for the rocket's valves: identity, label, capability.
// A valve's position in this table indexes the per-valve pane state and blink flags.
const VALVES: [(ValveId, &str, ValveKind); VALVE_COUNT] = [
    (ValveId::PressurantVent, "Pressurant Vent", ValveKind::Servo),
    (ValveId::Pressurization, "Pressurization", ValveKind::Servo),
    (ValveId::OxidizerVent, "Oxidizer Vent", ValveKind::Solenoid),
    (ValveId::OxidizerFill, "Oxidizer Fill", ValveKind::Servo),
    (ValveId::Main, "Main", ValveKind::Servo),
    (
        ValveId::ExternalPressurantFill,
        "Ext Pressurant Fill",
        ValveKind::Servo,
    ),
    (
        ValveId::ExternalOxidizerFill,
        "Ext Oxidizer Fill",
        ValveKind::Servo,
    ),
    (
        ValveId::ExternalPressurantVent,
        "Ext Pressurant Vent",
        ValveKind::Solenoid,
    ),
    (
        ValveId::ExternalOxidizerVent,
        "Ext Oxidizer Vent",
        ValveKind::Solenoid,
    ),
];

fn valve_index(id: ValveId) -> usize {
    VALVES.iter().position(|(v, _, _)| *v == id).unwrap_or(0)
}

// What a click on a valve in the graphical overview does. Every valve honors
// Pulse now; the mode just picks pulse-open vs toggle for the whole schematic.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum ValveInteractionMode {
    Pulse,
    Toggle,
}

pub struct PropulsionPane {
    pulse_secs: [f32; VALVE_COUNT],
    valve_mismatch_since: [Option<f64>; VALVE_COUNT],
    valve_mode: ValveInteractionMode,
}

// Blink once a mismatch has persisted past the debounce window; clears on agreement.
fn debounce_blink(since: &mut Option<f64>, mismatch: bool, now: f64) -> bool {
    if mismatch {
        let start = *since.get_or_insert(now);
        now - start > VALVE_MISMATCH_DEBOUNCE_SECS
    } else {
        *since = None;
        false
    }
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
                .then_some(f32::from(battery.current_battery) / 100.0),
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
            pulse_secs: [1.0; VALVE_COUNT],
            valve_mismatch_since: [None; VALVE_COUNT],
            valve_mode: ValveInteractionMode::Pulse,
        }
    }

    // Per-valve blink flags, computed once per frame and shared by the list and
    // the schematic so the two surfaces stay consistent.
    fn update_valve_blink(&mut self, system: &System, now: f64) -> [bool; VALVE_COUNT] {
        let mut flags = [false; VALVE_COUNT];
        for (i, (id, _, _)) in VALVES.iter().enumerate() {
            let mismatch = rocket::valve_reading(system, *id).is_some_and(rocket::valve_mismatch);
            flags[i] = debounce_blink(&mut self.valve_mismatch_since[i], mismatch, now);
        }
        flags
    }

    fn draw_battery(&mut self, ui: &mut egui::Ui, system: &System, pos: Pos2) {
        let battery_rect = Rect::from_center_size(pos, Vec2::new(60.0, 120.0));
        if let Some(indicator) = battery_indicator(system, false) {
            ui.place(battery_rect, indicator);
        }
    }

    fn draw_frame(
        &mut self,
        ui: &mut egui::Ui,
        system: &System,
        square: Rect,
        valve_blink: [bool; VALVE_COUNT],
    ) {
        let n = square.width();

        let Ok(heartbeat) = system.last_message::<Heartbeat>() else {
            return;
        };

        Frame::dark_canvas(ui.style()).show(ui, |ui| {
            instrument_visuals(ui);
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
                    rocket::draw_hybrid(
                        ui,
                        system,
                        square,
                        &mut self.valve_mode,
                        self.pulse_secs,
                        valve_blink,
                    );
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

// A horizontal position bar: fill = reported state, caret = commanded (intended)
// position. Servo valves are draggable to command a proportional position, and
// the whole bar's border blinks on a debounced mismatch. Returns the new target
// (0.0..=1.0) when a servo drag completes.
fn valve_bar(
    ui: &mut egui::Ui,
    size: Vec2,
    reading: Option<rocket::ValveReading>,
    servo: bool,
    blink: bool,
    time: f64,
) -> Option<f32> {
    let sense = if servo {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter().clone();
    let rounding = CornerRadius::same(3);
    let visuals = ui.visuals();

    painter.rect_filled(rect, rounding, visuals.extreme_bg_color);

    let state = reading.and_then(|r| r.state).map(|s| s.clamp(0.0, 1.0));
    if let Some(s) = state
        && s > 0.0
    {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width() * s, rect.height()));
        painter.rect_filled(
            fill_rect,
            rounding,
            readable(COLOR_INDICATOR_WARNING, visuals).gamma_multiply(0.8),
        );
    }

    if let Some(c) = reading.and_then(|r| r.commanded).map(|c| c.clamp(0.0, 1.0)) {
        let x = rect.left() + rect.width() * c;
        painter.line_segment(
            [pos2(x, rect.top() + 1.0), pos2(x, rect.bottom() - 1.0)],
            Stroke::new(2.0_f32, visuals.strong_text_color()),
        );
    }

    let mut target = None;
    if servo {
        if let Some(p) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.drag_stopped())
        {
            let t = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let x = rect.left() + rect.width() * t;
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(2.0_f32, readable(COLOR_INDICATOR_GOOD, ui.visuals())),
            );
            if resp.drag_stopped() {
                target = Some(t);
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    let font = FontId::monospace(11.0);
    match state {
        Some(s) => {
            Readout {
                value: s * 100.0,
                decimals: 0,
                unit: Some("%"),
                font,
                color: visuals.text_color(),
                ..Default::default()
            }
            .paint(&painter, rect.center(), Align2::CENTER_CENTER);
        }
        None => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "--",
                font,
                visuals.text_color(),
            );
        }
    }

    let border = if blink && crate::colors::blink_on(time) {
        Stroke::new(2.0_f32, readable(COLOR_INDICATOR_WARNING, visuals))
    } else {
        Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color)
    };
    painter.rect(
        rect,
        rounding,
        Color32::TRANSPARENT,
        border,
        StrokeKind::Inside,
    );

    target
}

fn valve_row(
    ui: &mut egui::Ui,
    system: &System,
    index: usize,
    pulse_secs: &mut f32,
    blink: bool,
    button_size: Vec2,
) {
    let (id, label, kind) = VALVES[index];
    let reading = rocket::valve_reading(system, id);
    let commanded = reading.and_then(|r| r.commanded);
    let time = ui.input(|i| i.time);

    ui.weak(label.to_uppercase());

    let close_active = matches!(commanded, Some(c) if c <= VALVE_LATCH_EPS);
    let close_text = if close_active { "CLOSED" } else { "CLOSE" };
    let close_btn = Button::selectable(close_active, RichText::new(close_text));
    if ui.add_sized(button_size, close_btn).clicked() {
        system.do_set_valve(id, 0.0);
    }

    let servo = kind == ValveKind::Servo;
    if let Some(target) = valve_bar(ui, button_size, reading, servo, blink, time) {
        system.do_set_valve(id, target);
    }

    let open_active = matches!(commanded, Some(c) if c >= 1.0 - VALVE_LATCH_EPS);
    let open_btn = if open_active {
        Button::selectable(true, RichText::new("OPEN"))
            .fill(readable(COLOR_INDICATOR_WARNING, ui.visuals()))
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

    ui.allocate_ui_with_layout(
        button_size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let spacing = ui.spacing().item_spacing.x;
            let drag_w = (button_size.x * 0.4).max(0.0);
            let btn_w = (button_size.x - drag_w - spacing).max(0.0);
            if ui
                .add_sized(
                    Vec2::new(btn_w, button_size.y),
                    Button::new(RichText::new("PULSE")),
                )
                .clicked()
            {
                system.do_pulse_valve(id, *pulse_secs);
            }
            ui.add_sized(
                Vec2::new(drag_w, button_size.y),
                DragValue::new(pulse_secs)
                    .speed(0.1)
                    .range(0.0..=MAX_PULSE_DURATION_SECS)
                    .suffix(" s"),
            );
        },
    );

    ui.end_row();
}

fn valve_state_lines(system_id: u8) -> Vec<PlotLine> {
    [
        (ValveId::PressurantVent, "Pressurant Vent"),
        (ValveId::Pressurization, "Pressurization"),
        (ValveId::OxidizerVent, "Oxidizer Vent"),
        (ValveId::OxidizerFill, "Oxidizer Fill"),
        (ValveId::Main, "Main"),
        (ValveId::ExternalPressurantFill, "Ext Pressurant Fill"),
        (ValveId::ExternalOxidizerFill, "Ext Oxidizer Fill"),
        (ValveId::ExternalPressurantVent, "Ext Pressurant Vent"),
        (ValveId::ExternalOxidizerVent, "Ext Oxidizer Vent"),
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
        sentinel: None,
    })
    .collect()
}

fn pressure_lines(system_id: u8, visuals: &egui::Visuals) -> Vec<PlotLine> {
    [
        (0, "Pressurant", rocket::N2_COLOR),
        (1, "Oxidizer", rocket::N2O_COLOR),
        (2, "Combustion", rocket::CC_COLOR),
        (3, "Reg. Pressurant", rocket::NODE_COLOR),
        (4, "Ext. Pressurant", rocket::EXT_N2_COLOR),
        (5, "Ext. Oxidizer", rocket::EXT_N2O_COLOR),
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
        color: Some(readable(color, visuals)),
        // PRESSURE_VESSEL.pressure1 is in kPa; the diagram and rendering use bar.
        scale: Some(0.01),
        // Firmware reports an unavailable sensor as u16::MAX.
        sentinel: Some(f64::from(u16::MAX)),
    })
    .collect()
}

impl PaneUi for PropulsionPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System { system_id, .. } = behavior.active_view else {
            return;
        };
        let Some(system) = behavior.source.system(system_id) else {
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
            // Wider than the flight plant alone: the left slice is a ground-support
            // lane for the external tanks and fill valves (see rocket::draw_hybrid).
            let w = h * 0.438;
            ui.horizontal_top(|ui| {
                let now = ui.input(|i| i.time);
                let blink = self.update_valve_blink(&system, now);
                let cursor = ui.cursor().min;
                let square = Rect::from_min_size(cursor, Vec2::new(w, h));
                self.draw_frame(ui, &system, square, blink);

                ui.vertical(|ui| {
                    egui::Panel::bottom(egui::Id::new(("propulsion_valves_panel", system_id)))
                        .resizable(false)
                        .show_separator_line(false)
                        .frame(egui::Frame::new())
                        .show_inside(ui, |ui| {
                            ui.separator();
                            ui.add_space(5.0);
                            ui.weak("🚰 Valves");
                            ui.add_space(5.0);

                            let button_size = Vec2::new(80.0, ui.spacing().interact_size.y);

                            egui::Grid::new("propulsion_valves")
                                .striped(true)
                                .show(ui, |ui| {
                                    for (i, (pulse, blink)) in
                                        self.pulse_secs.iter_mut().zip(blink).enumerate()
                                    {
                                        valve_row(ui, &system, i, pulse, blink, button_size);
                                    }
                                });
                        });

                    let valve_states_h = ui.available_height() / 3.5;
                    egui::Panel::bottom(egui::Id::new((
                        "propulsion_valve_states_panel",
                        system_id,
                    )))
                    .resizable(false)
                    .show_separator_line(false)
                    .frame(egui::Frame::new())
                    .exact_size(valve_states_h)
                    .show_inside(ui, |ui| {
                        let vs_lines = valve_state_lines(system_id);
                        let valve_states_plot = Plot::new(
                            &vs_lines,
                            &behavior.source,
                            behavior.shared_plot_state,
                            (Some(0.0), Some(3.0)),
                        );
                        ui.add_sized(
                            Vec2::new(ui.available_width(), ui.available_height()),
                            valve_states_plot,
                        );
                    });

                    let p_lines = pressure_lines(system_id, ui.visuals());
                    let pressure_plot = Plot::new(
                        &p_lines,
                        &behavior.source,
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
                self.draw_frame(ui, &system, square, [false; VALVE_COUNT]);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rocket::{ValveReading, valve_mismatch};
    use super::{VALVE_MISMATCH_DEBOUNCE_SECS, debounce_blink};

    #[test]
    fn mismatch_flags_both_directions() {
        assert!(valve_mismatch(ValveReading {
            commanded: Some(1.0),
            state: Some(0.0)
        }));
        assert!(valve_mismatch(ValveReading {
            commanded: Some(0.0),
            state: Some(1.0)
        }));
        // Within the deadband: normal travel / agreement, not a mismatch.
        assert!(!valve_mismatch(ValveReading {
            commanded: Some(1.0),
            state: Some(0.95)
        }));
        assert!(!valve_mismatch(ValveReading {
            commanded: Some(0.5),
            state: Some(0.55)
        }));
        // Unknown (NaN -> None) reported state is a fault for now.
        assert!(valve_mismatch(ValveReading {
            commanded: Some(1.0),
            state: None
        }));
        assert!(valve_mismatch(ValveReading {
            commanded: None,
            state: None
        }));
        // Known state with no command to compare against: not a mismatch.
        assert!(!valve_mismatch(ValveReading {
            commanded: None,
            state: Some(0.0)
        }));
    }

    #[test]
    fn debounce_waits_then_blinks_and_clears() {
        let mut since = None;
        let d = VALVE_MISMATCH_DEBOUNCE_SECS;

        // First frame of a mismatch: armed but not yet blinking.
        assert!(!debounce_blink(&mut since, true, 100.0));
        // Still within the window.
        assert!(!debounce_blink(&mut since, true, 100.0 + d - 0.01));
        // Past the window: blink.
        assert!(debounce_blink(&mut since, true, 100.0 + d + 0.01));
        // Agreement clears the timer and stops the blink.
        assert!(!debounce_blink(&mut since, false, 200.0));
        assert_eq!(since, None);
        // A fresh mismatch restarts the debounce.
        assert!(!debounce_blink(&mut since, true, 300.0));
    }
}
