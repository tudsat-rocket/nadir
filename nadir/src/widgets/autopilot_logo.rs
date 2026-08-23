use mavspec::rust::dialects::common::enums::{MavAutopilot, MavType};

pub struct AutopilotLogo(pub MavAutopilot, pub MavType);

impl egui::Widget for AutopilotLogo {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // The `_white`/`_dark` assets are the ones drawn *for* a dark background.
        let dark = ui.visuals().dark_mode;

        let image = match (self.0, self.1) {
            (MavAutopilot::Ardupilotmega, _) => {
                Some(egui::include_image!("../../assets/logos/ardupilot.png"))
            }
            (MavAutopilot::Px4, _) if dark => {
                Some(egui::include_image!("../../assets/logos/px4_white.svg"))
            }
            (MavAutopilot::Px4, _) => {
                Some(egui::include_image!("../../assets/logos/px4_black.svg"))
            }
            (MavAutopilot::Generic, MavType::Rocket) if dark => {
                Some(egui::include_image!("../../assets/logos/rapid_dark.png"))
            }
            (MavAutopilot::Generic, MavType::Rocket) => {
                Some(egui::include_image!("../../assets/logos/rapid_light.png"))
            }
            _ => None,
        };

        if let Some(logo) = image {
            let image = egui::Image::new(logo).maintain_aspect_ratio(true);
            ui.add(image)
        } else {
            ui.horizontal(|_ui| {}).response
        }
    }
}
