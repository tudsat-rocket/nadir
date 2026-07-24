use egui::{Align2, Area, Color32, CornerRadius, Frame, Id, Order, Stroke, Vec2};

use crate::panes::{PaneUi, PositionSource, TreeBehavior};
use crate::views::View;
use crate::widgets::ArtificialHorizon;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum VelocityMode {
    Speed,
    #[default]
    Climb,
}

pub struct HorizonPane {
    velocity_mode: VelocityMode,
}

impl HorizonPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            velocity_mode: VelocityMode::default(),
        }
    }
}

impl PaneUi for HorizonPane {
    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        0.0
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let pane_rect = ui.available_rect_before_wrap();

        Frame::dark_canvas(ui.style())
            .corner_radius(CornerRadius::ZERO)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(ui.available_height());
                    ui.add_sized(
                        Vec2::new(ui.available_width(), ui.available_height()),
                        ArtificialHorizon::new(
                            &system,
                            *behavior.position_source,
                            self.velocity_mode,
                        ),
                    );
                });
            });

        // Keep the toggles available at status-bar widths; they only disappear when the horizon is
        // genuinely too narrow to fit them.
        if pane_rect.width() >= 220.0 {
            let button_size = Vec2::new(52.0, 22.0);

            Area::new(Id::new("horizon_velocity_toggle"))
                .order(Order::Foreground)
                .pivot(Align2::LEFT_BOTTOM)
                .fixed_pos(pane_rect.left_bottom() + Vec2::new(34.0, -14.0))
                .show(ui.ctx(), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (mode, label) in [
                        (VelocityMode::Speed, "SPEED"),
                        (VelocityMode::Climb, "CLIMB"),
                    ] {
                        let selected = self.velocity_mode == mode;
                        let stroke = if selected {
                            Stroke::new(1.0, Color32::WHITE)
                        } else {
                            Stroke::new(0.5, Color32::from_gray(120))
                        };
                        let button = egui::Button::new(egui::RichText::new(label).size(12.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(stroke)
                            .corner_radius(CornerRadius::same(3))
                            .selected(false);
                        ui.add_sized(button_size, button)
                            .clicked()
                            .then(|| self.velocity_mode = mode);
                    }
                });

            Area::new(Id::new("horizon_source_toggle"))
                .order(Order::Foreground)
                .pivot(Align2::RIGHT_BOTTOM)
                .fixed_pos(pane_rect.right_bottom() - Vec2::new(34.0, 14.0))
                .show(ui.ctx(), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (src, label) in [
                        (PositionSource::LocalPositionNed, "LOCAL"),
                        (PositionSource::VfrHud, "MSL"),
                    ] {
                        let has_data = src.has_data(&behavior.core, system_id);
                        let selected = *behavior.position_source == src;
                        let stroke = if selected {
                            Stroke::new(1.0, Color32::WHITE)
                        } else {
                            Stroke::new(0.5, Color32::from_gray(120))
                        };
                        let button = egui::Button::new(egui::RichText::new(label).size(12.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(stroke)
                            .corner_radius(CornerRadius::same(3))
                            .selected(false);
                        ui.add_enabled_ui(has_data, |ui| {
                            ui.add_sized(button_size, button)
                                .clicked()
                                .then(|| *behavior.position_source = src);
                        });
                    }
                });
        }
    }
}
