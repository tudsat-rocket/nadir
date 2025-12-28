use core::System;

use eframe::egui;

use crate::panes::PaneUi;

pub struct MessagesPane {}

impl MessagesPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }
}

impl PaneUi for MessagesPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, _system: System) {
        ui.centered_and_justified(|ui| {
            ui.weak("To be implemented: display all received message types");
        });
    }
}
