use egui::{CornerRadius, Frame, Vec2};
use maviola::core::io::ChannelDetails;

use crate::{panes::TreeBehavior, views::View, widgets::ArtificialHorizon};

pub struct HorizonPane {}

impl HorizonPane {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

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
