use eframe::egui;

use crate::panes::TreeBehavior;

pub struct StatusPane {}

impl StatusPane {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        ui.label("status");
    }
}
