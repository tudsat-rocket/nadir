use core::System;

use egui::RichText;
use mavspec::rust::dialects::common::{enums::MavModeFlag, messages::Heartbeat};

pub struct ModeDisplay {
    system: System,
}

impl ModeDisplay {
    pub fn new(system: System) -> Self {
        Self { system }
    }
}

impl egui::Widget for ModeDisplay {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let Ok(heartbeat) = self.system.last_message::<Heartbeat>() else {
            return ui.label("");
        };

        let rt = if let Some(name) = self.system.current_mode_name() {
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

        ui.label(rt)
    }
}
