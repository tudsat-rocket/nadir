use egui::RichText;
use mavspec::rust::dialects::common::enums::MavModeFlag;

use crate::colors::COLOR_INDICATOR_WARNING;

pub struct ArmedIndicator(pub MavModeFlag);

impl egui::Widget for ArmedIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (text, color) = if self.0.contains(MavModeFlag::SAFETY_ARMED) {
            ("ARMED", COLOR_INDICATOR_WARNING)
        } else {
            ("DISARMED", ui.visuals().weak_text_color())
        };

        ui.label(RichText::new(text).color(color))
    }
}
