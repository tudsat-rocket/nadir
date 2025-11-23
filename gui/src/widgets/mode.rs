use core::System;

use egui::RichText;
use mavspec::rust::dialects::common::enums::MavModeFlag;

pub struct ModeDisplay {
    system: System,
    font_size: Option<f32>,
}

impl ModeDisplay {
    pub fn new(system: System) -> Self {
        Self {
            system,
            font_size: None,
        }
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = Some(size);
        self
    }
}

impl egui::Widget for ModeDisplay {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let Some(heartbeat) = self.system.last_heartbeat().ok().flatten() else {
            return ui.label("");
        };

        let mut rt = if let Some(name) = self.system.current_mode_name() {
            RichText::new(name).strong()
        } else if heartbeat
            .base_mode
            .contains(MavModeFlag::CUSTOM_MODE_ENABLED)
        {
            RichText::new(format!("0x{:02}", heartbeat.custom_mode))
                .strong()
                .monospace()
        } else {
            RichText::new("")
            // TODO
        };

        if let Some(size) = self.font_size {
            rt = rt.size(size);
        }

        ui.label(rt)
    }
}
