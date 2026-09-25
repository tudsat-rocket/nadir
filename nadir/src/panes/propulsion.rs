use nadir_core::{MessageInstance, System};

use egui::{Color32, Image, Pos2, Rect, Vec2};
use mavspec::rust::dialects::common::enums::MavType;
use mavspec::rust::dialects::common::messages::{BatteryStatus, Heartbeat, SysStatus};
use mavspec::rust::dialects::minimal::enums::MavAutopilot;
use rapid_dialect::rapid::enums::ValveId;

use crate::colors::{schematic_frame, schematic_ink};
use crate::panes::{PaneUi, TreeBehavior};
use crate::views::View;
use crate::widgets::{BatteryIndicator, Plot, PlotLine};

mod arducopter;
mod arduplane;
mod px4;
mod rocket;
mod valves;

// Firmware bound on pulse length (mission::valves::MAX_PULSE_DURATION).
pub(super) const MAX_PULSE_DURATION_SECS: f32 = 30.0;

// How long a commanded-vs-actual mismatch must persist before the cue starts
// blinking, so normal valve travel doesn't flash the UI.
const VALVE_MISMATCH_DEBOUNCE_SECS: f64 = 0.5;

// Share of the propulsion column the valve grid may claim before the plots above
// it start to suffer.
const VALVES_HEIGHT_SHARE: f32 = 0.6;

// Commanded position within this of fully closed/open latches the CLOSE/OPEN button.
pub(super) const VALVE_LATCH_EPS: f32 = 0.02;

pub(super) const VALVE_COUNT: usize = 9;

// Solenoid valves are binary; servo valves additionally accept a proportional
// set-position, making the servo control a strict superset of the solenoid one.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum ValveKind {
    Solenoid,
    Servo,
}

pub(super) struct Valve {
    pub id: ValveId,
    pub label: &'static str,
    pub kind: ValveKind,
    // Identifies the valve wherever it appears: schematic glyph, knob heading,
    // state plot. Pastel so a warm hue does not read as a warning.
    pub color: Color32,
}

// Single source of truth for the rocket's valves. Ordered roughly by use over a
// flight; a valve's position in this table indexes the per-valve pane state and
// blink flags.
pub(super) const VALVES: [Valve; VALVE_COUNT] = [
    Valve {
        id: ValveId::OxidizerFill,
        label: "Oxidizer Fill",
        kind: ValveKind::Servo,
        color: Color32::from_rgb(125, 170, 245),
    },
    Valve {
        id: ValveId::PressurantVent,
        label: "Pressurant Vent",
        kind: ValveKind::Servo,
        color: Color32::from_rgb(118, 181, 135),
    },
    Valve {
        id: ValveId::OxidizerVent,
        label: "Oxidizer Vent",
        kind: ValveKind::Solenoid,
        color: Color32::from_rgb(106, 180, 189),
    },
    Valve {
        id: ValveId::Pressurization,
        label: "Pressurization",
        kind: ValveKind::Servo,
        color: Color32::from_rgb(194, 165, 103),
    },
    Valve {
        id: ValveId::Main,
        label: "Main",
        kind: ValveKind::Servo,
        color: Color32::from_rgb(235, 145, 145),
    },
    Valve {
        id: ValveId::ExternalPressurantFill,
        label: "Ext Press. Fill",
        kind: ValveKind::Servo,
        color: Color32::from_rgb(178, 159, 227),
    },
    Valve {
        id: ValveId::ExternalOxidizerFill,
        label: "Ext Oxidizer Fill",
        kind: ValveKind::Servo,
        color: Color32::from_rgb(212, 148, 189),
    },
    Valve {
        id: ValveId::ExternalPressurantVent,
        label: "Ext Press. Vent",
        kind: ValveKind::Solenoid,
        color: Color32::from_rgb(169, 173, 111),
    },
    Valve {
        id: ValveId::ExternalOxidizerVent,
        label: "Ext Oxidizer Vent",
        kind: ValveKind::Solenoid,
        color: Color32::from_rgb(194, 161, 134),
    },
];

/// For the contrast tests in [`crate::colors`], which cannot reach into `rocket`.
#[cfg(test)]
pub(crate) fn fluid_colors() -> [(&'static str, Color32); 7] {
    rocket::fluid_colors()
}

/// For the contrast tests in [`crate::colors`].
#[cfg(test)]
pub(crate) fn valve_colors() -> impl Iterator<Item = (&'static str, Color32)> {
    VALVES.iter().map(|v| (v.label, v.color))
}

impl Valve {
    pub(super) fn index(id: ValveId) -> usize {
        VALVES.iter().position(|v| v.id == id).unwrap_or(0)
    }

    pub(super) fn color(id: ValveId) -> Color32 {
        VALVES[Self::index(id)].color
    }
}

// What a click on a valve in the graphical overview does. Every valve honors
// Pulse now; the mode just picks pulse-open vs toggle for the whole schematic.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum ValveInteractionMode {
    Pulse,
    Toggle,
}

pub struct PropulsionPane {
    pulse_secs: f32,
    pending_target: [Option<f32>; VALVE_COUNT],
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
            pulse_secs: 1.0,
            pending_target: [None; VALVE_COUNT],
            valve_mismatch_since: [None; VALVE_COUNT],
            valve_mode: ValveInteractionMode::Pulse,
        }
    }

    // Per-valve blink flags, computed once per frame and shared by the list and
    // the schematic so the two surfaces stay consistent.
    fn update_valve_blink(&mut self, system: &System, now: f64) -> [bool; VALVE_COUNT] {
        let mut flags = [false; VALVE_COUNT];
        for (i, valve) in VALVES.iter().enumerate() {
            let mismatch =
                rocket::valve_reading(system, valve.id).is_some_and(rocket::valve_mismatch);
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

        schematic_frame(ui.style()).show(ui, |ui| {
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
                        &mut self.pulse_secs,
                        valve_blink,
                    );
                }
                (MavAutopilot::Px4, _) => {
                    px4::draw_rotors(ui, system, square);
                    self.draw_battery(ui, system, square.center());
                }
                (MavAutopilot::Ardupilotmega, MavType::FixedWing) => {
                    // Two copies: a tint only multiplies, so it cannot turn white strokes black.
                    let outline = if ui.visuals().dark_mode {
                        egui::include_image!("../../assets/vehicles/plane_twin_vtail_dark.svg")
                    } else {
                        egui::include_image!("../../assets/vehicles/plane_twin_vtail_light.svg")
                    };
                    ui.place(
                        square.shrink(n * 0.05),
                        Image::new(outline)
                            .maintain_aspect_ratio(true)
                            .max_width(n)
                            .tint(schematic_ink(ui.visuals()).gamma_multiply(0.5)),
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

fn valve_state_lines(system_id: u8) -> Vec<PlotLine> {
    VALVES
        .iter()
        .map(|valve| PlotLine {
            system_id,
            component_id: 1,
            message_name: "VALVE".to_owned(),
            instance: Some(MessageInstance {
                field: "id".to_owned(),
                value: i64::from(valve.id.value()),
            }),
            field_name: "state".to_owned(),
            alias: Some(valve.label.to_owned()),
            unit: None,
            color: Some(valve.color),
            scale: None,
            sentinel: None,
        })
        .collect()
}

/// Colours are left undarkened: `Plot` runs every line through `readable` itself.
fn pressure_lines(system_id: u8) -> Vec<PlotLine> {
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
        color: Some(color),
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
                    // Measured before the panel resolves: inside it, available height
                    // is already the panel's own.
                    let budget = ui.available_height() * VALVES_HEIGHT_SHARE;
                    // egui's scroll bar floats over the content rather than allocating
                    // width (so `ScrollStyle::allocated_width` is 0), and a button half
                    // hidden under it is not clickable text.
                    let scroll = &ui.spacing().scroll;
                    let bar = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
                    let plan = valves::Plan::best(Vec2::new(ui.available_width() - bar, budget));
                    egui::Panel::bottom(egui::Id::new(("propulsion_valves_panel", system_id)))
                        .resizable(false)
                        .show_separator_line(false)
                        .frame(egui::Frame::new())
                        .exact_size(plan.panel_height(budget))
                        .show(ui, |ui| {
                            if system.muted() {
                                ui.disable();
                            }

                            valves::grid(ui, &system, &mut self.pending_target, blink, &plan);
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
                    .show(ui, |ui| {
                        let vs_lines = valve_state_lines(system_id);
                        // The knob headings carry the same colors, so a legend here
                        // would only repeat them over the traces.
                        let valve_states_plot = Plot::new(
                            &vs_lines,
                            &behavior.source,
                            behavior.shared_plot_state,
                            (Some(0.0), Some(3.0)),
                        )
                        .without_legend();
                        ui.add_sized(
                            Vec2::new(ui.available_width(), ui.available_height()),
                            valve_states_plot,
                        );
                    });

                    let p_lines = pressure_lines(system_id);
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
