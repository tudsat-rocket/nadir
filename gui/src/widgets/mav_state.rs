use egui::{Color32, RichText};
use mavspec::rust::dialects::common::enums::MavState;

pub struct MavStateIndicator(pub MavState);

impl egui::Widget for MavStateIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let color = match self.0 {
            MavState::Boot | MavState::Uninit | MavState::Calibrating => ui.visuals().text_color(),
            MavState::Standby => Color32::from_rgb(114, 159, 207),
            MavState::Active => Color32::from_rgb(78, 154, 6),
            MavState::FlightTermination => Color32::from_rgb(196, 160, 0),
            MavState::Critical | MavState::Emergency | MavState::Poweroff => {
                Color32::from_rgb(204, 0, 0)
            }
        };

        ui.label(RichText::new(format!("{:?}", self.0).to_uppercase()).color(color))
    }
}
