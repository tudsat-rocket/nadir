use eframe::egui;

use crate::panes::TreeBehavior;

pub struct MessagesPane {}

impl MessagesPane {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        ui.label("messages");
        // TODO
    }
}
