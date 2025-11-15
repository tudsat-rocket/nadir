use eframe::egui;

use crate::{panes::TreeBehavior, views::View};

pub struct CanProbePane {
    can_forwarding_enabled: bool,
}

impl CanProbePane {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            can_forwarding_enabled: false,
        }
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        ui.set_width(ui.available_width());

        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.add_space(5.0);

            if ui
                .checkbox(&mut self.can_forwarding_enabled, "Enable CAN Forwarding")
                .clicked()
            {
                system.request_can_forwarding(self.can_forwarding_enabled);
            }
        });

        ui.separator();

        // TODO
        for frame in behavior
            .core
            .db
            .can_frames_for_system((0x01, 0x01))
            .unwrap()
        {
            ui.label(format!("{:02x?}", &frame.data[..(frame.len as usize)]));
        }
    }
}
