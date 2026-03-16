use core::System;

use egui::{CornerRadius, Frame, Vec2};

use crate::{panes::PaneUi, widgets::ArtificialHorizon};

pub struct HorizonPane {}

impl HorizonPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }
}

impl PaneUi for HorizonPane {
    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        0.0
    }

    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        Frame::dark_canvas(ui.style())
            .corner_radius(CornerRadius::ZERO)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(ui.available_height());
                    ui.add_sized(
                        Vec2::new(ui.available_width(), ui.available_height()),
                        ArtificialHorizon::new(&system),
                    );
                });
            });
    }
}
