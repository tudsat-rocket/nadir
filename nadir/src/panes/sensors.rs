use eframe::egui;
use mavspec::rust::dialects::common::enums::MavType;
use mavspec::rust::dialects::common::messages::Heartbeat;
use nadir_core::mav::SystemId;

use crate::{
    panes::{PaneUi, TreeBehavior},
    views::View,
    widgets::{Plot, PlotLine},
};

mod calibration;

use calibration::Calibration;

/// zenith's `mavlink.rs` converts with the standard gravity, not the local one.
const G: f32 = 9.80665;

pub struct SensorsPane {
    calibration: Calibration,
}

impl SensorsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            calibration: Calibration::new(),
        }
    }

    fn sensor_plot_lines(
        system_id: SystemId,
        msg_prefix: &str,
        field_suffix: &str,
        unit: &str,
        scale: f64,
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
                    unit: Some(unit.to_owned()),
                    color: None,
                    scale: Some(scale),
                    sentinel: None,
                });
            }
        }

        lines
    }
}

impl PaneUi for SensorsPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System { system_id, .. } = behavior.active_view else {
            return;
        };

        if let Some(system) = behavior.source.system(system_id)
            && system
                .last_message::<Heartbeat>()
                .is_ok_and(|hb| hb.type_ == MavType::Rocket)
        {
            egui::CollapsingHeader::new("Calibration")
                .default_open(false)
                .show(ui, |ui| self.calibration.show(ui, &system));
        }

        let size = egui::Vec2::new(ui.available_width(), ui.available_height() * 0.25);

        let acc_lines = Self::sensor_plot_lines(
            system_id,
            "SCALED_IMU",
            "acc",
            "m/s^2",
            f64::from(G) / 1000.0,
        );
        let acc_plot = Plot::new(
            &acc_lines,
            &behavior.source,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, acc_plot);

        let gyro_lines = Self::sensor_plot_lines(
            system_id,
            "SCALED_IMU",
            "gyro",
            "\u{00b0}/s",
            1e-3_f64.to_degrees(),
        );
        let gyro_plot = Plot::new(
            &gyro_lines,
            &behavior.source,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, gyro_plot);

        let mag_lines = Self::sensor_plot_lines(system_id, "SCALED_IMU", "mag", "\u{00b5}T", 0.1);
        let mag_plot = Plot::new(
            &mag_lines,
            &behavior.source,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(size, mag_plot);

        let pres_lines = vec![
            PlotLine {
                system_id,
                component_id: 1,
                message_name: "SCALED_PRESSURE".to_owned(),
                instance: None,
                field_name: "press_abs".to_owned(),
                alias: None,
                unit: Some("hPa".to_owned()),
                color: None,
                scale: None,
                sentinel: None,
            },
            PlotLine {
                system_id,
                component_id: 1,
                message_name: "SCALED_PRESSURE2".to_owned(),
                instance: None,
                field_name: "press_abs".to_owned(),
                alias: None,
                unit: Some("hPa".to_owned()),
                color: None,
                scale: None,
                sentinel: None,
            },
            PlotLine {
                system_id,
                component_id: 1,
                message_name: "SCALED_PRESSURE3".to_owned(),
                instance: None,
                field_name: "press_abs".to_owned(),
                alias: None,
                unit: Some("hPa".to_owned()),
                color: None,
                scale: None,
                sentinel: None,
            },
        ];
        let pres_plot = Plot::new(
            &pres_lines,
            &behavior.source,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(ui.available_size(), pres_plot);
    }
}
