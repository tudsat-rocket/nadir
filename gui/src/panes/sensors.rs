use eframe::egui;
use egui::Align;
use maviola::protocol::SystemId;

use crate::{
    panes::TreeBehavior,
    views::View,
    widgets::{Plot, PlotLine},
};

pub struct SensorsPane {}

impl SensorsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    fn sensor_plot_lines(
        system_id: SystemId,
        msg_prefix: &str,
        field_suffix: &str,
    ) -> Vec<PlotLine> {
        let mut lines = Vec::new();
        for msg in [
            format!("{msg_prefix}"),
            format!("{msg_prefix}2"),
            format!("{msg_prefix}3"),
        ] {
            for field in [
                format!("x{field_suffix}"),
                format!("y{field_suffix}"),
                format!("z{field_suffix}"),
            ] {
                lines.push(PlotLine {
                    system_id: system_id,
                    component_id: 1,
                    message_name: msg.clone(),
                    field_name: field,
                    alias: None,
                    unit: None,
                    color: None,
                });
            }
        }

        lines
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let size = egui::Vec2::new(ui.available_width(), ui.available_height() * 0.25);

        let acc_plot = Plot::new(
            Self::sensor_plot_lines(system_id, "SCALED_IMU", "acc"),
            &behavior.core,
            &mut behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, acc_plot);

        let gyro_plot = Plot::new(
            Self::sensor_plot_lines(system_id, "SCALED_IMU", "gyro"),
            &behavior.core,
            &mut behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, gyro_plot);

        let mag_plot = Plot::new(
            Self::sensor_plot_lines(system_id, "SCALED_IMU", "mag"),
            &behavior.core,
            &mut behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, mag_plot);

        ui.with_layout(
            egui::Layout::left_to_right(Align::TOP).with_cross_justify(true),
            |ui| {
                let size = egui::Vec2::new(ui.available_width() * 0.5, ui.available_height());

                let temp_plot = Plot::new(
                    vec![
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_IMU".to_owned(),
                            field_name: "temperature".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_IMU2".to_owned(),
                            field_name: "temperature".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_IMU3".to_owned(),
                            field_name: "temperature".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_PRESSURE".to_owned(),
                            field_name: "temperature".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_PRESSURE2".to_owned(),
                            field_name: "temperature".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_PRESSURE3".to_owned(),
                            field_name: "temperature".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                    ],
                    &behavior.core,
                    &mut behavior.shared_plot_state,
                    (None, None),
                );
                ui.add_sized(size, temp_plot);

                let pres_plot = Plot::new(
                    vec![
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_PRESSURE".to_owned(),
                            field_name: "press_abs".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_PRESSURE2".to_owned(),
                            field_name: "press_abs".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                        PlotLine {
                            system_id: system_id,
                            component_id: 1,
                            message_name: "SCALED_PRESSURE3".to_owned(),
                            field_name: "press_abs".to_owned(),
                            alias: None,
                            unit: None,
                            color: None,
                        },
                    ],
                    &behavior.core,
                    &mut behavior.shared_plot_state,
                    (None, None),
                );
                ui.add_sized(size, pres_plot);
            },
        );
    }
}
