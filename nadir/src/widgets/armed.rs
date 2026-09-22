use egui::{Color32, Frame, Margin, RichText};
use mavspec::rust::dialects::common::enums::MavModeFlag;

use crate::colors::{COLOR_INDICATOR_WARNING, dim, readable, text_on};

pub struct ArmedIndicator(pub MavModeFlag);

impl egui::Widget for ArmedIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (text, color) = if self.0.contains(MavModeFlag::SAFETY_ARMED) {
            ("ARMED", readable(COLOR_INDICATOR_WARNING, ui.visuals()))
        } else {
            ("DISARMED", ui.visuals().weak_text_color())
        };

        ui.label(RichText::new(text).color(color))
    }
}

/// Filled form of [`ArmedIndicator`], in the status pane's arm-button colors.
pub struct ArmedBadge {
    pub mode: MavModeFlag,
    pub faded: bool,
}

impl egui::Widget for ArmedBadge {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (text, fill, color) = if self.mode.contains(MavModeFlag::SAFETY_ARMED) {
            // Toward the background rather than by alpha, so `text_on` still answers for the
            // color actually drawn.
            let mut fill = readable(COLOR_INDICATOR_WARNING, ui.visuals());
            if self.faded {
                fill = fill.lerp_to_gamma(ui.visuals().window_fill(), 0.55);
            }
            ("ARMED", fill, text_on(fill))
        } else {
            let color = ui.visuals().weak_text_color();
            let color = if self.faded { dim(color, 0.5) } else { color };
            ("DISARMED", Color32::TRANSPARENT, color)
        };

        Frame::new()
            .fill(fill)
            .corner_radius(2.0)
            .inner_margin(Margin::symmetric(3, 0))
            .show(ui, |ui| super::small_text(ui, text, color))
            .response
    }
}
