use eframe::egui;
use egui::DragValue;
use nadir_core::System;

use crate::panes::PaneUi;
use crate::widgets::Hazard;

pub struct PayloadPane {
    winch_step: f32,
}

impl PayloadPane {
    const INSTANCES: [u8; 2] = [1, 2];

    pub fn new(_ctx: &egui::Context) -> Self {
        Self { winch_step: 1.0 }
    }

    fn button(ui: &mut egui::Ui, system: &System, label: &str) -> bool {
        let response = ui.button(label);
        Hazard::tick(ui, response.rect, system);
        Hazard::confirm(ui, &response, system)
    }
}

impl PaneUi for PayloadPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        if system.muted() {
            ui.disable();
        }

        ui.horizontal(|ui| {
            ui.label("Winch step");
            ui.add(
                DragValue::new(&mut self.winch_step)
                    .speed(0.1)
                    .range(0.0..=f32::MAX),
            );
        });

        egui::Grid::new("payload").striped(true).show(ui, |ui| {
            for instance in Self::INSTANCES {
                ui.label(format!("QD {instance}"));
                if Self::button(ui, &system, "Grip") {
                    system.do_gripper(instance, true);
                }
                if Self::button(ui, &system, "Release") {
                    system.do_gripper(instance, false);
                }
                if Self::button(ui, &system, "Winch -") {
                    system.do_winch_relative(instance, -self.winch_step);
                }
                if Self::button(ui, &system, "Winch +") {
                    system.do_winch_relative(instance, self.winch_step);
                }
                ui.end_row();
            }
        });
    }
}
