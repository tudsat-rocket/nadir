use nadir_core::System;

use eframe::egui;
use egui::{Align, Color32, FontId, Layout, RichText, Vec2};
use mavspec::rust::dialects::common::messages::{
    BatteryStatus, GpsRawInt, Heartbeat, LocalPositionNed, SysStatus,
};

use crate::colors::{
    COLOR_INDICATOR_GOOD, COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, readable,
};
use crate::widgets::{
    AutopilotLogo, Readout, TEXT_SIZE, column_header, link_quality, small_text, soc_color,
};

/// Same dim-to-strong ramp as the battery indicator widget: current only lights up as it climbs, so
/// an idle bus does not read as an event. Uses the magnitude, since firmware that reports discharge
/// as negative would otherwise never brighten.
fn current_color(ui: &egui::Ui, amps: f32) -> Color32 {
    const I_MIN: f32 = 0.1;
    const I_MAX: f32 = 10.0;

    let ramp = (f32::max(amps.abs() / I_MAX, I_MIN).log2() - I_MIN.log2()) / (-I_MIN.log2());
    ui.visuals()
        .weak_text_color()
        .lerp_to_gamma(ui.visuals().strong_text_color(), f32::min(ramp, 1.0))
}

/// One piece of a value cell. The rows mix prose ("9 sat", the "--" placeholder) with numbers that
/// want the readout's tucked decimals and small units, so they cannot all be one string.
enum Segment {
    Text(String, Color32),
    Value(Readout),
}

impl Segment {
    fn value(value: f32, decimals: usize, unit: Option<&'static str>, color: Color32) -> Self {
        Self::Value(Readout {
            value,
            decimals,
            unit,
            font: FontId::monospace(TEXT_SIZE),
            color,
            ..Default::default()
        })
    }

    fn show(self, ui: &mut egui::Ui) {
        match self {
            Self::Text(text, color) => small_text(ui, &text, color),
            Self::Value(readout) => {
                ui.add(readout);
            }
        }
    }
}

/// Right-hand columns of the status bar: the consumables and RF numbers a pilot watches
/// continuously, plus the component inventory. In compact mode (narrow windows) only the
/// consumables column survives.
pub struct Vitals<'a> {
    pub system: &'a System,
    pub compact: bool,
}

/// Width the consumables rows need before their widest value (battery: charge, voltage and current,
/// e.g. "87%, 12.2V, -200mA" beside its label) starts truncating.
const CONSUMABLES_MIN_WIDTH: f32 = 225.0;
/// Share of the zone the consumables column takes. Its rows are label-plus-value and of known
/// length, while the component list has to hold board names, so the larger share goes there.
const CONSUMABLES_SHARE: f32 = 0.45;
const SEPARATOR_WIDTH: f32 = 13.0;

impl Vitals<'_> {
    /// Width the two-column layout needs before the consumables values start truncating; below this,
    /// callers should ask for the compact form.
    pub const FULL_MIN_WIDTH: f32 = CONSUMABLES_MIN_WIDTH / CONSUMABLES_SHARE + SEPARATOR_WIDTH;

    fn consumables_column(&self, ui: &mut egui::Ui) {
        let system = self.system;
        let weak = ui.visuals().weak_text_color();
        let nodata = weak.gamma_multiply(0.5);
        let normal = ui.visuals().text_color();
        let good = readable(COLOR_INDICATOR_GOOD, ui.visuals());
        let warning = readable(COLOR_INDICATOR_WARNING, ui.visuals());
        let limits = readable(COLOR_INDICATOR_LIMITS, ui.visuals());

        // Identity header: icon + system id on the left, firmware logo on the right (both used to
        // live on the status pane's removed header line).
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} 0x{:02x}", system.icon(), system.system_id))
                    .monospace()
                    .size(TEXT_SIZE),
            );
            if let Ok(hb) = system.last_message::<Heartbeat>() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_sized(Vec2::new(60.0, 12.0), AutopilotLogo(hb.autopilot, hb.type_));
                });
            }
        });

        // TODO: properly handle multiple batteries (same as StatusPane)
        // Both messages report current in cA, with -1 for "not measured".
        let (battery, voltage, current) = if let Ok(b) =
            system.last_instance_message::<BatteryStatus>(1)
        {
            // Same reading as the propulsion pane's indicator: the last populated cell-sum
            // entry.
            let voltage = b
                .voltages
                .iter()
                .filter(|v| **v > 0 && **v < u16::MAX)
                .map(|v| f32::from(*v) / 1000.0)
                .next_back();
            let current = (b.current_battery != -1).then(|| f32::from(b.current_battery) / 100.0);
            (Some(b.battery_remaining), voltage, current)
        } else if let Ok(s) = system.last_message::<SysStatus>() {
            (
                Some(s.battery_remaining),
                (s.voltage_battery != u16::MAX).then(|| f32::from(s.voltage_battery) / 1000.0),
                (s.current_battery != -1).then(|| f32::from(s.current_battery) / 100.0),
            )
        } else {
            (None, None, None)
        };
        // Charge and voltage share the state-of-charge color; the current is a separate segment so
        // it can carry the battery widget's brightness ramp instead.
        let battery_cell = {
            let soc = battery.filter(|soc| *soc >= 0);
            // A pack reporting only volts stays neutral, with nothing to color it by.
            let charge_color = soc.map_or(normal, |soc| {
                soc_color(f32::from(soc) / 100.0, ui.visuals())
            });

            let mut cell = Vec::new();
            if let Some(soc) = soc {
                cell.push(Segment::value(f32::from(soc), 0, Some("%"), charge_color));
            }
            if let Some(u) = voltage {
                if !cell.is_empty() {
                    cell.push(Segment::Text(", ".into(), charge_color));
                }
                cell.push(Segment::value(u, 1, Some("V"), charge_color));
            }
            if let Some(i) = current {
                let color = current_color(ui, i);
                if !cell.is_empty() {
                    cell.push(Segment::Text(", ".into(), color));
                }
                // Sub-amp draws are the norm on an avionics bus, where "0.0A" would hide the
                // reading.
                cell.push(if i.abs() < 1.0 {
                    Segment::value(i * 1000.0, 0, Some("mA"), color)
                } else {
                    Segment::value(i, 1, Some("A"), color)
                });
            }
            if cell.is_empty() {
                cell.push(Segment::Text("--".into(), nodata));
            }
            cell
        };

        let gps = system.last_message::<GpsRawInt>().ok();
        let gps_cell = match gps {
            Some(g) if g.satellites_visible != u8::MAX => {
                let color = if g.satellites_visible >= 6 {
                    good
                } else {
                    warning
                };
                let mut cell = vec![Segment::Text(
                    format!("{} sat ", g.satellites_visible),
                    color,
                )];
                if g.eph != u16::MAX {
                    cell.push(Segment::value(f32::from(g.eph) / 100.0, 1, None, color));
                }
                cell
            }
            _ => vec![Segment::Text("--".into(), nodata)],
        };

        // Both directions get their own row: on narrow windows the Links pane is dropped from the
        // status bar entirely, and these two numbers are all that is left of it.
        let (downlink, uplink) = link_quality(system);
        let link_cell = |lq: Option<f32>| match lq {
            Some(lq) => {
                let color = if lq > 0.9 {
                    good
                } else if lq > 0.5 {
                    warning
                } else {
                    limits
                };
                vec![Segment::value(100.0 * lq, 0, Some("%"), color)]
            }
            None => vec![Segment::Text("--".into(), nodata)],
        };

        let uptime_cell = match system.last_message::<LocalPositionNed>() {
            Ok(lp) => vec![Segment::value(
                lp.time_boot_ms as f32 / 1000.0,
                1,
                Some("s"),
                normal,
            )],
            Err(_) => vec![Segment::Text("--".into(), nodata)],
        };

        // Same presentation as the preflight pane's status checks: a striped two-column grid, weak
        // labels left, colored values right.
        egui::Grid::new("bar_consumables")
            .num_columns(2)
            .striped(true)
            .spacing(Vec2::new(10.0, 2.0))
            .min_col_width(0.0)
            .show(ui, |ui| {
                for (label, segments) in [
                    ("🕑 Uptime", uptime_cell),
                    ("🔋 Battery", battery_cell),
                    ("💾 Flash", vec![Segment::Text("--".into(), nodata)]),
                    ("📡 GPS", gps_cell),
                    ("📶 Downlink", link_cell(downlink)),
                    ("📶 Uplink", link_cell(uplink)),
                ] {
                    // Extend, not Truncate: inside a grid cell a truncating label shrinks to the
                    // column width it is itself defining, which collapses to an ellipsis.
                    ui.add(
                        egui::Label::new(RichText::new(label).size(TEXT_SIZE).weak())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    // Pad the value cell out to the right edge so the striped row backgrounds span
                    // the whole zone instead of ending where the text does. The label column keeps
                    // its natural width, so the values stay aligned across rows.
                    ui.horizontal(|ui| {
                        // Segments carry their own separators, so they butt up against each other.
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for segment in segments {
                            segment.show(ui);
                        }
                        ui.add_space(ui.available_width().max(0.0));
                    });
                    ui.end_row();
                }
            });
    }

    fn components_column(&self, ui: &mut egui::Ui) {
        let weak = ui.visuals().weak_text_color();

        column_header(ui, "COMPONENTS");

        // TODO: the component/PCB inventory is not plumbed through core yet. Everything the GUI
        // reaches for is hardcoded to component 0x01, so there is nothing to enumerate.
        small_text(ui, "no component data", weak.gamma_multiply(0.5));
    }
}

impl egui::Widget for Vitals<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 2.0);
            ui.add_space(4.0);

            if self.compact {
                self.consumables_column(ui);
            } else {
                ui.horizontal(|ui| {
                    let usable = ui.available_width() - SEPARATOR_WIDTH;
                    let consumables_w = usable * CONSUMABLES_SHARE;

                    ui.vertical(|ui| {
                        ui.set_width(consumables_w);
                        self.consumables_column(ui);
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(usable - consumables_w);
                        self.components_column(ui);
                    });
                });
            }
        })
        .response
    }
}
