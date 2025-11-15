use mavspec::rust::dialects::common::{enums::MavState, messages::Heartbeat};

use eframe::egui;
use egui::{Color32, RichText};

use crate::{panes::TreeBehavior, views::View};

pub struct StatusPane {}

impl StatusPane {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let Some(Heartbeat {
            type_: mav_type,
            autopilot,
            base_mode,
            custom_mode,
            system_status,
            mavlink_version: _,
        }) = system.last_heartbeat().unwrap_or_default()
        else {
            return;
        };

        let icon = system.icon();

        let h = ui.available_height();

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.add_space(5.0);
            //ui.set_height(h / 2.0);

            ui.vertical(|ui| {
                ui.weak("System");

                ui.monospace(format!("＃ 0x{system_id:02}"));
                ui.label(format!("{icon} {mav_type:?}"));
                ui.label(format!("⛓ {:?}", autopilot));
            });

            ui.separator();

            ui.vertical(|ui| {
                ui.weak("Mode");

                let color = match system_status {
                    MavState::Boot | MavState::Uninit | MavState::Calibrating => {
                        ui.visuals().text_color()
                    }
                    MavState::Standby => Color32::from_rgb(114, 159, 207),
                    MavState::Active => Color32::from_rgb(78, 154, 6),
                    MavState::FlightTermination => Color32::from_rgb(196, 160, 0),
                    MavState::Critical | MavState::Emergency | MavState::Poweroff => {
                        Color32::from_rgb(204, 0, 0)
                    }
                };

                let custom_modes = system.custom_modes();

                if let Some(name) = custom_modes
                    .map(|map| map.get(&custom_mode).cloned())
                    .flatten()
                {
                    ui.label(RichText::new(name).strong().size(24.0));
                } else {
                    ui.label(
                        RichText::new(format!("0x{custom_mode:02}"))
                            .strong()
                            .monospace()
                            .size(24.0),
                    );
                }

                ui.label(RichText::new(format!("{:?}", system_status).to_uppercase()).color(color));

                // TODO: ardupilot doesn't do mode labels. test with PX4, or just use mode display
                // and state?
                //ui.monospace(format!("{base_mode:?}"));
                //let flag_labels: Vec<_> = [
                //    (MavModeFlag::CUSTOM_MODE_ENABLED, "CUSTOM"),
                //    (MavModeFlag::TEST_ENABLED, "TEST"),
                //    (MavModeFlag::AUTO_ENABLED, "AUTO"),
                //    (MavModeFlag::GUIDED_ENABLED, "GUIDED"),
                //    (MavModeFlag::STABILIZE_ENABLED, "STABILIZED"),
                //    (MavModeFlag::HIL_ENABLED, "HIL"),
                //    (MavModeFlag::MANUAL_INPUT_ENABLED, "MANUAL"),
                //    (MavModeFlag::SAFETY_ARMED, "ARMED"),
                //]
                //.into_iter()
                //.filter_map(|(flag, label)| base_mode.contains(flag).then_some(label))
                //.collect();
                //ui.label(flag_labels.join(" | "));
            });
        });

        ui.separator();
    }
}
