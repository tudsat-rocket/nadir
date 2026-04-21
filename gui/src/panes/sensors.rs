use eframe::egui;
use egui::Align;
use maviola::protocol::SystemId;

use crate::{
    panes::{PaneUi, TreeBehavior},
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
            msg_prefix.to_string(),
            format!("{msg_prefix}2"),
            format!("{msg_prefix}3"),
        ] {
            for field in [
                format!("x{field_suffix}"),
                format!("y{field_suffix}"),
                format!("z{field_suffix}"),
            ] {
                lines.push(PlotLine {
                    system_id,
                    component_id: 1,
                    message_name: msg.clone(),
                    instance: None,
                    field_name: field,
                    alias: None,
                    unit: None,
                    color: None,
                    scale: None,
                });
            }
        }

        lines
    }
}

impl PaneUi for SensorsPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let size = egui::Vec2::new(ui.available_width(), ui.available_height() * 0.25);

        let acc_lines = Self::sensor_plot_lines(system_id, "SCALED_IMU", "acc");
        let acc_plot = Plot::new(
            &acc_lines,
            &behavior.core,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, acc_plot);

        let gyro_lines = Self::sensor_plot_lines(system_id, "SCALED_IMU", "gyro");
        let gyro_plot = Plot::new(
            &gyro_lines,
            &behavior.core,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, gyro_plot);

        let mag_lines = Self::sensor_plot_lines(system_id, "SCALED_IMU", "mag");
        let mag_plot = Plot::new(
            &mag_lines,
            &behavior.core,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, mag_plot);

        ui.with_layout(
            egui::Layout::left_to_right(Align::TOP).with_cross_justify(true),
            |ui| {
                let size = egui::Vec2::new(ui.available_width() * 0.5, ui.available_height());
                let temp_lines = vec![
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_IMU".to_owned(),
                        instance: None,
                        field_name: "temperature".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_IMU2".to_owned(),
                        instance: None,
                        field_name: "temperature".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_IMU3".to_owned(),
                        instance: None,
                        field_name: "temperature".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_PRESSURE".to_owned(),
                        instance: None,
                        field_name: "temperature".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_PRESSURE2".to_owned(),
                        instance: None,
                        field_name: "temperature".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_PRESSURE3".to_owned(),
                        instance: None,
                        field_name: "temperature".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                ];
                let temp_plot = Plot::new(
                    &temp_lines,
                    &behavior.core,
                    behavior.shared_plot_state,
                    (None, None),
                );
                ui.add_sized(size, temp_plot);

                let pres_lines = vec![
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_PRESSURE".to_owned(),
                        instance: None,
                        field_name: "press_abs".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_PRESSURE2".to_owned(),
                        instance: None,
                        field_name: "press_abs".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                    PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: "SCALED_PRESSURE3".to_owned(),
                        instance: None,
                        field_name: "press_abs".to_owned(),
                        alias: None,
                        unit: None,
                        color: None,
                        scale: None,
                    },
                ];
                let pres_plot = Plot::new(
                    &pres_lines,
                    &behavior.core,
                    behavior.shared_plot_state,
                    (None, None),
                );
                ui.add_sized(size, pres_plot);
            },
        );
    }
}
