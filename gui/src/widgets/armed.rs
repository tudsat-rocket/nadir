use egui::{Color32, RichText};
use mavspec::rust::dialects::common::enums::MavModeFlag;

pub struct ArmedIndicator(pub MavModeFlag);

impl egui::Widget for ArmedIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (text, color) = if self.0.contains(MavModeFlag::SAFETY_ARMED) {
            ("ARMED", Color32::from_rgb(204, 0, 0))
        } else {
            ("DISARMED", ui.visuals().weak_text_color())
        };

        ui.label(RichText::new(text).color(color))
    }
}
