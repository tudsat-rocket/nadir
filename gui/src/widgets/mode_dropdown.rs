use core::System;

use mavspec::rust::dialects::common::enums::{MavModeProperty, MavStandardMode};

pub struct ModeDropdown {
    system: System,
}

impl ModeDropdown {
    pub fn new(system: &System) -> Self {
        Self {
            system: system.clone(),
        }
    }
}

impl egui::Widget for ModeDropdown {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            let Some(modes) = self.system.available_modes() else {
                return;
            };

            let mut selected = None;

            egui::ComboBox::from_label("")
                .selected_text(self.system.current_mode_name().unwrap_or_default())
                .show_ui(ui, |ui| {
                    for mode_info in modes {
                        if mode_info
                            .properties
                            .contains(MavModeProperty::NOT_USER_SELECTABLE)
                        {
                            continue;
                        }

                        let name = if mode_info.standard_mode == MavStandardMode::NonStandard {
                            String::from_utf8_lossy(&mode_info.mode_name).to_string()
                        } else {
                            format!("{:?}", mode_info.standard_mode)
                        };

                        ui.selectable_value(&mut selected, Some(mode_info), name);
                    }
                });

            if let Some(selected_mode) = selected {
                if selected_mode.standard_mode == MavStandardMode::NonStandard {
                    self.system.do_set_custom_mode(selected_mode.custom_mode);
                } else {
                    self.system
                        .do_set_standard_mode(selected_mode.standard_mode);
                }
            }
        })
        .response
    }
}
