use egui::{CornerRadius, Frame, Sense, Vec2};

use crate::colors::{COLOR_INDICATOR_GOOD, COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING};

pub struct BatteryIndicator {
    pub id: u8,
    pub soc: f32,
    pub voltage: Option<f32>,
    pub current: Option<f32>,
    pub consumed: Option<f32>,
}

impl egui::Widget for BatteryIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let color = if self.soc > 0.6 {
            COLOR_INDICATOR_GOOD
        } else if self.soc > 0.2 {
            COLOR_INDICATOR_WARNING
        } else {
            COLOR_INDICATOR_LIMITS
        };

        let s = ui.available_size();
        Frame::dark_canvas(ui.style())
            .show(ui, |ui| {
                ui.set_width(s.x);
                ui.set_height(s.y);

                ui.horizontal_top(|ui| {
                    let bar_size = Vec2::new(15.0, ui.available_height());
                    let (response, painter) = ui.allocate_painter(bar_size, Sense::empty());

                    painter.rect_filled(
                        response.rect,
                        CornerRadius::ZERO,
                        ui.visuals().window_fill(),
                    );

                    let mut fill_rect = response.rect;
                    fill_rect.set_top(fill_rect.bottom() - self.soc * fill_rect.height());
                    painter.rect_filled(fill_rect, CornerRadius::ZERO, color);

                    egui::Grid::new(ui.next_auto_id())
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.weak("#");
                            ui.label(format!("{}", self.id));
                            ui.end_row();

                            ui.weak("SoC");
                            ui.colored_label(color, format!("{:>3.0}%", self.soc * 100.0));
                            ui.end_row();

                            if let Some(u) = self.voltage {
                                ui.weak("Vlt.");
                                ui.colored_label(color, format!("{u:.1}V"));
                                ui.end_row();
                            }

                            if let Some(i) = self.current {
                                ui.weak("Cur.");
                                ui.colored_label(color, format!("{i:.1}A"));
                                ui.end_row();
                            }

                            if let Some(cap) = self.consumed {
                                ui.weak("Con.");
                                ui.label(format!("{cap:.0}"));
                                ui.end_row();
                                ui.weak("");
                                ui.weak("mAh");
                                ui.end_row();
                            }
                        });
                });
            })
            .response
    }
}
