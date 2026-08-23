use nadir_core::System;

use egui::{Align, Color32, CornerRadius, Frame, Layout, Sense, Vec2};
use mavspec::rust::dialects::common::messages::{BatteryStatus, SysStatus};

use crate::colors::{
    COLOR_INDICATOR_GOOD, COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, readable,
};
use crate::widgets::Readout;

/// The pack's state of charge in percent, from `BATTERY_STATUS` if the system sends it and
/// `SYS_STATUS` otherwise. Both report `-1` for a vehicle that does not measure it.
// TODO: properly handle multiple batteries (same as the propulsion pane and the status bar)
pub(crate) fn state_of_charge(system: &System) -> Option<i8> {
    let remaining = match system.last_instance_message::<BatteryStatus>(1) {
        Ok(battery) => battery.battery_remaining,
        Err(_) => system.last_message::<SysStatus>().ok()?.battery_remaining,
    };

    (remaining >= 0).then_some(remaining)
}

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

pub struct BatteryIndicator {
    pub id: u8,
    pub soc: f32,
    pub voltage: Option<f32>,
    pub current: Option<f32>,
    pub consumed: Option<f32>,
    pub compact: bool,
}

impl BatteryIndicator {
    /// One value row, monospace so the digits stack in a column.
    fn row(
        ui: &egui::Ui,
        value: f32,
        decimals: usize,
        unit: &'static str,
        color: Color32,
    ) -> Readout {
        Readout {
            value,
            decimals,
            unit: Some(unit),
            font: egui::TextStyle::Monospace.resolve(ui.style()),
            color,
            ..Default::default()
        }
    }
}

impl egui::Widget for BatteryIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let color = soc_color(self.soc, ui.visuals());

        let s = ui.available_size();
        Frame::dark_canvas(ui.style())
            .show(ui, |ui| {
                ui.set_width(s.x);
                ui.set_height(s.y);

                ui.horizontal_top(|ui| {
                    let bar_size = Vec2::new(8.0, ui.available_height());
                    let (response, painter) = ui.allocate_painter(bar_size, Sense::empty());

                    painter.rect_filled(
                        response.rect,
                        CornerRadius::ZERO,
                        ui.visuals().window_fill(),
                    );

                    let mut fill_rect = response.rect;
                    fill_rect.set_top(fill_rect.bottom() - self.soc * fill_rect.height());
                    painter.rect_filled(fill_rect, CornerRadius::ZERO, color);

                    ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
                        if !self.compact {
                            ui.weak(format!("#{}", self.id));
                            ui.add_space(5.0);
                        }

                        ui.add(Self::row(ui, self.soc * 100.0, 0, "%", color));

                        if let Some(u) = self.voltage {
                            ui.add(Self::row(ui, u, 1, "V", color));
                        }

                        if let Some(i) = self.current {
                            const I_MIN: f32 = 0.1;
                            const I_MAX: f32 = 10.0;
                            let i_log = (f32::max(i / I_MAX, I_MIN).log2() - I_MIN.log2())
                                / (-I_MIN.log2());

                            let color = ui.visuals().weak_text_color().lerp_to_gamma(
                                ui.visuals().strong_text_color(),
                                f32::min(i_log, 1.0),
                            );
                            ui.add(Self::row(ui, i, 1, "A", color));
                        }

                        if !self.compact
                            && let Some(cap) = self.consumed
                        {
                            ui.add_space(5.0);
                            ui.monospace(format!("{cap:.0}"));
                            ui.weak("mAh");
                        }
                    });
                });
            })
            .response
    }
}
