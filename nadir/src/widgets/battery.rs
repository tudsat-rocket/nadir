use nadir_core::System;

use egui::{Align, Color32, CornerRadius, FontId, Layout, Margin, Sense, Vec2};
use mavspec::rust::dialects::common::messages::{BatteryStatus, SysStatus};

use crate::colors::{
    COLOR_INDICATOR_GOOD, COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, readable,
    schematic_frame,
};
use crate::widgets::{MeasurementIndicator, Readout};

/// The lowest state of charge in percent over all packs, from `BATTERY_STATUS` if the system sends
/// it and `SYS_STATUS` otherwise.
pub(crate) fn state_of_charge(system: &System) -> Option<f32> {
    BatteryReading::all(system)
        .into_iter()
        .filter_map(|(_, reading)| reading.soc)
        .min_by(f32::total_cmp)
        .map(|soc| soc * 100.0)
}

/// Size of the compact form's rows below the charge, relative to it.
const SMALL: f32 = 0.75;
const COMPACT_BAR_W: f32 = 8.0;
const COMPACT_BAR_GAP: f32 = 3.0;

/// Green above 60% charge, amber down to 20%, red below.
pub(crate) fn soc_color(fraction: f32, visuals: &egui::Visuals) -> Color32 {
    let color = if fraction > 0.6 {
        COLOR_INDICATOR_GOOD
    } else if fraction > 0.2 {
        COLOR_INDICATOR_WARNING
    } else {
        COLOR_INDICATOR_LIMITS
    };

    readable(color, visuals)
}

/// One pack's values, with the "not measured" sentinels of either message mapped to `None`.
#[derive(Clone, Copy)]
pub struct BatteryReading {
    /// Fraction, not percent.
    pub soc: Option<f32>,
    pub voltage: Option<f32>,
    pub current: Option<f32>,
    pub consumed: Option<f32>,
    pub temperature: Option<f32>,
}

impl BatteryReading {
    /// Every pack the system reports, by id, or `SYS_STATUS` as id 0 if it reports none.
    pub fn all(system: &System) -> Vec<(u8, Self)> {
        let batteries: Vec<_> = system
            .last_instance_messages::<BatteryStatus>()
            .iter()
            .map(|b| (b.id, Self::from_status(b)))
            .collect();

        if batteries.is_empty() {
            system
                .last_message::<SysStatus>()
                .map(|s| vec![(0, Self::from_sys_status(&s))])
                .unwrap_or_default()
        } else {
            batteries
        }
    }

    fn from_status(b: &BatteryStatus) -> Self {
        Self {
            soc: (b.battery_remaining >= 0).then(|| f32::from(b.battery_remaining) / 100.0),
            // The last populated cell-sum entry.
            voltage: b
                .voltages
                .iter()
                .filter(|v| **v > 0 && **v < u16::MAX)
                .map(|v| f32::from(*v) / 1000.0)
                .next_back(),
            current: (b.current_battery != -1).then(|| f32::from(b.current_battery) / 100.0),
            consumed: (b.current_consumed != -1).then_some(b.current_consumed as f32),
            temperature: (b.temperature != i16::MAX).then(|| f32::from(b.temperature) / 100.0),
        }
    }

    fn from_sys_status(s: &SysStatus) -> Self {
        Self {
            soc: (s.battery_remaining >= 0).then(|| f32::from(s.battery_remaining) / 100.0),
            voltage: (s.voltage_battery != u16::MAX).then(|| f32::from(s.voltage_battery) / 1000.0),
            current: (s.current_battery != -1).then(|| f32::from(s.current_battery) / 100.0),
            consumed: None,
            temperature: None,
        }
    }
}

pub struct BatteryIndicator {
    pub id: u8,
    pub reading: BatteryReading,
    /// Drops the id and consumed charge, shrinks the values below the charge, and adds temperature.
    /// Holds how far the text may grow past the schematic's other readouts at their base size.
    pub compact: Option<f32>,
}

impl BatteryIndicator {
    /// One value row, monospace so the digits stack in a column.
    fn row(value: f32, decimals: usize, unit: &'static str, color: Color32, size: f32) -> Readout {
        Readout {
            value,
            decimals,
            unit: Some(unit),
            font: FontId::monospace(size),
            color,
            ..Default::default()
        }
    }

    /// The bar runs flush against the border on three sides.
    fn frame(style: &egui::Style) -> egui::Frame {
        let mut frame = schematic_frame(style);
        frame.inner_margin = Margin {
            left: 0,
            top: 0,
            bottom: 0,
            ..frame.inner_margin
        };
        frame
    }

    /// Scale and inner size of the compact form in at most `max_inner`: four rows tall, wide
    /// enough for the widest current, and never larger than the schematic's other readouts.
    fn compact_layout(ui: &egui::Ui, max_inner: Vec2, max_scale: f32) -> (f32, Vec2) {
        let ctx = ui.ctx();
        let base = egui::TextStyle::Monospace.resolve(ui.style()).size;
        let widest = Self::row(-999.0, 0, "mA", Color32::PLACEHOLDER, base * SMALL).size(ctx);
        let charge = Self::row(100.0, 0, "%", Color32::PLACEHOLDER, base).size(ctx);
        let natural = Vec2::new(
            widest.x + COMPACT_BAR_W + COMPACT_BAR_GAP,
            charge.y + 3.0 * widest.y,
        );

        let scale = (max_inner / natural)
            .min_elem()
            .min(MeasurementIndicator::value_font().size * max_scale / base);
        (scale, natural * scale)
    }

    /// Outer size of the compact form placed in at most `max`.
    pub fn compact_size(ui: &egui::Ui, max: Vec2, max_scale: f32) -> Vec2 {
        let margin = Self::frame(ui.style()).total_margin().sum();
        Self::compact_layout(ui, max - margin, max_scale).1 + margin
    }
}

impl egui::Widget for BatteryIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Only ever drawn inside the propulsion schematic, so it follows that canvas.
        let reading = self.reading;
        let soc = reading.soc.unwrap_or(0.0);
        let color = reading.soc.map_or(ui.visuals().text_color(), |soc| {
            soc_color(soc, ui.visuals())
        });

        let frame = Self::frame(ui.style());
        let s = ui.available_size() - frame.total_margin().sum();
        frame
            .show(ui, |ui| {
                ui.set_width(s.x);
                ui.set_height(s.y);

                let base = egui::TextStyle::Monospace.resolve(ui.style()).size;
                let (size, small, bar_w) = if let Some(max_scale) = self.compact {
                    let (scale, _) = Self::compact_layout(ui, s, max_scale);
                    ui.spacing_mut().item_spacing = Vec2::new(COMPACT_BAR_GAP * scale, 0.0);
                    (base * scale, base * scale * SMALL, COMPACT_BAR_W * scale)
                } else {
                    (base, base, 8.0)
                };

                ui.horizontal_top(|ui| {
                    let bar_size = Vec2::new(bar_w, ui.available_height());
                    let (response, painter) = ui.allocate_painter(bar_size, Sense::empty());

                    painter.rect_filled(
                        response.rect,
                        CornerRadius::ZERO,
                        ui.visuals().window_fill(),
                    );

                    let mut fill_rect = response.rect;
                    fill_rect.set_top(fill_rect.bottom() - soc * fill_rect.height());
                    painter.rect_filled(fill_rect, CornerRadius::ZERO, color);

                    ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
                        if self.compact.is_none() {
                            ui.weak(format!("#{}", self.id));
                            ui.add_space(5.0);
                        }

                        if let Some(soc) = reading.soc {
                            ui.add(Self::row(soc * 100.0, 0, "%", color, size));
                        }

                        if let Some(u) = reading.voltage {
                            ui.add(Self::row(u, 1, "V", color, small));
                        }

                        if let Some(i) = reading.current {
                            const I_MIN: f32 = 0.1;
                            const I_MAX: f32 = 10.0;
                            let i_log = (f32::max(i / I_MAX, I_MIN).log2() - I_MIN.log2())
                                / (-I_MIN.log2());

                            let color = ui.visuals().weak_text_color().lerp_to_gamma(
                                ui.visuals().strong_text_color(),
                                f32::min(i_log, 1.0),
                            );
                            // Sub-amp avionics draws would otherwise all read as "0.xA".
                            ui.add(if self.compact.is_some() && i.abs() < 1.0 {
                                Self::row(i * 1000.0, 0, "mA", color, small)
                            } else {
                                Self::row(i, 1, "A", color, small)
                            });
                        }

                        if self.compact.is_some()
                            && let Some(t) = reading.temperature
                        {
                            let color = ui.visuals().text_color();
                            ui.add(Self::row(t, 0, "\u{00b0}C", color, small));
                        }

                        if self.compact.is_none()
                            && let Some(cap) = reading.consumed
                        {
                            ui.add_space(5.0);
                            ui.monospace(format!("{cap:.0}"));
                            ui.weak("mAh");
                        }
                    });
                });
            })
            .response
            .on_hover_text(format!("Battery #{}", self.id))
    }
}
