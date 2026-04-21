use crate::panes::{PaneUi, PositionSource, TreeBehavior};
use crate::views::View;
use crate::widgets::{Plot, PlotLine};

use eframe::egui;
use egui::Color32;

fn ned_vz(system_id: u8) -> PlotLine {
    PlotLine {
        system_id,
        component_id: 1,
        message_name: "LOCAL_POSITION_NED".to_owned(),
        field_name: "vz".to_owned(),
        alias: Some("LOCAL_POSITION_NED.vz (-NED)".to_owned()),
        unit: None,
        color: Some(Color32::from_rgb(0xd7, 0x99, 0x21)),
        scale: Some(-1.0),
    }
}

fn altitude_lines(source: PositionSource, system_id: u8) -> Vec<PlotLine> {
    match source {
        PositionSource::LocalPositionNed => vec![PlotLine {
            system_id,
            component_id: 1,
            message_name: "LOCAL_POSITION_NED".to_owned(),
            field_name: "z".to_owned(),
            alias: Some("LOCAL_POSITION_NED.z (-NED)".to_owned()),
            unit: None,
            color: Some(Color32::from_rgb(0x45, 0x85, 0x88)),
            scale: Some(-1.0),
        }],
        PositionSource::VfrHud => vec![PlotLine {
            system_id,
            component_id: 1,
            message_name: "VFR_HUD".to_owned(),
            field_name: "alt".to_owned(),
            alias: Some("VFR_HUD.alt".to_owned()),
            unit: None,
            color: Some(Color32::from_rgb(0x45, 0x85, 0x88)),
            scale: None,
        }],
    }
}

fn velocity_lines(source: PositionSource, system_id: u8) -> Vec<PlotLine> {
    match source {
        PositionSource::LocalPositionNed => vec![ned_vz(system_id)],
        PositionSource::VfrHud => vec![PlotLine {
            system_id,
            component_id: 1,
            message_name: "VFR_HUD".to_owned(),
            field_name: "climb".to_owned(),
            alias: Some("VFR_HUD.climb".to_owned()),
            unit: None,
            color: Some(Color32::from_rgb(0xd7, 0x99, 0x21)),
            scale: None,
        }],
    }
}

pub struct StateEstimatorPane {}

impl StateEstimatorPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }
}

impl PaneUi for StateEstimatorPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        ui.horizontal(|ui| {
            for src in PositionSource::ALL {
                let has_data = src.has_data(&behavior.core, system_id);
                ui.add_enabled(
                    has_data,
                    egui::Button::selectable(*behavior.position_source == src, src.label()),
                )
                .clicked()
                .then(|| *behavior.position_source = src);
            }
        });

        let source = *behavior.position_source;

        let alt_size = egui::Vec2::new(ui.available_width(), ui.available_height() * 0.57);

        let altitude_plot = Plot::new(
            altitude_lines(source, system_id),
            &behavior.core,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(alt_size, altitude_plot);

        let vel_size = egui::Vec2::new(ui.available_width(), ui.available_height());

        let velocity_plot = Plot::new(
            velocity_lines(source, system_id),
            &behavior.core,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(vel_size, velocity_plot);
    }
}
