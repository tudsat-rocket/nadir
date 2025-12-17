use eframe::egui;

use crate::panes::TreeBehavior;

pub struct MessagesPane {}

impl MessagesPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, _behavior: &mut TreeBehavior) {
        ui.centered_and_justified(|ui| {
            ui.weak("To be implemented: display all received message types");
        });
    }
}
