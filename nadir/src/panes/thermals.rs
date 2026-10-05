use eframe::egui;
use nadir_core::MessageInstance;

use crate::{
    panes::{PaneUi, TreeBehavior},
    views::View,
    widgets::{Plot, PlotLine},
};

const CDEG: f64 = 0.01;

const SENSORS: &[Sensor] = &[
    Sensor::new("MCU_STATUS", "FC MCU", &["MCU_temperature"], CDEG, None),
    Sensor::new("SCALED_IMU", "IMU 1", &["temperature"], CDEG, Some(0.0)),
    Sensor::new("SCALED_IMU2", "IMU 2", &["temperature"], CDEG, Some(0.0)),
    Sensor::new("SCALED_IMU3", "IMU 3", &["temperature"], CDEG, Some(0.0)),
    Sensor::new("RAW_IMU", "Raw IMU", &["temperature"], CDEG, Some(0.0)),
    Sensor::new("HIGHRES_IMU", "HighRes IMU", &["temperature"], 1.0, None),
    Sensor::new("SCALED_PRESSURE", "Baro 1", &["temperature"], CDEG, None),
    Sensor::new("SCALED_PRESSURE2", "Baro 2", &["temperature"], CDEG, None),
    Sensor::new("SCALED_PRESSURE3", "Baro 3", &["temperature"], CDEG, None),
    Sensor::new(
        "BATTERY_STATUS",
        "Battery",
        &["temperature"],
        CDEG,
        Some(i16::MAX as f64),
    ),
    Sensor::new(
        "VALVE",
        "Valve",
        &["temperature"],
        CDEG,
        Some(i16::MAX as f64),
    ),
    Sensor::new(
        "PRESSURE_VESSEL",
        "Vessel",
        &["temperature1", "temperature2"],
        CDEG,
        Some(i16::MAX as f64),
    ),
    Sensor::new("ESC_INFO", "ESC", ESC_FIELDS, CDEG, Some(i16::MAX as f64)),
    // Slots without an ESC report 0.
    Sensor::new("ESC_TELEMETRY_1_TO_4", "ESC", ESC_FIELDS, 1.0, Some(0.0)).first_esc(1),
    Sensor::new("ESC_TELEMETRY_5_TO_8", "ESC", ESC_FIELDS, 1.0, Some(0.0)).first_esc(5),
    Sensor::new(
        "ONBOARD_COMPUTER_STATUS",
        "Companion",
        &["temperature_board"],
        1.0,
        Some(i8::MAX as f64),
    ),
];

const ESC_FIELDS: &[&str] = &[
    "temperature[0]",
    "temperature[1]",
    "temperature[2]",
    "temperature[3]",
];

/// A message carrying temperatures, in whatever unit `scale` converts to degC.
struct Sensor {
    message: &'static str,
    label: &'static str,
    fields: &'static [&'static str],
    scale: f64,
    sentinel: Option<f64>,
    /// Number of the ESC in element 0, offset by the instance value (`ESC_INFO.index`).
    first_esc: i64,
}

impl Sensor {
    const fn new(
        message: &'static str,
        label: &'static str,
        fields: &'static [&'static str],
        scale: f64,
        sentinel: Option<f64>,
    ) -> Self {
        Self {
            message,
            label,
            fields,
            scale,
            sentinel,
            first_esc: 1,
        }
    }

    const fn first_esc(mut self, first_esc: i64) -> Self {
        self.first_esc = first_esc;
        self
    }

    fn alias(&self, index: usize, instance: Option<&MessageInstance>) -> String {
        if self.fields == ESC_FIELDS {
            let first = self.first_esc + instance.map_or(0, |i| i.value);
            return format!("{} {}", self.label, first + index as i64);
        }

        let instance = instance
            .map(|i| format!(" {}", i.value))
            .unwrap_or_default();
        if self.fields.len() > 1 {
            format!("{}{instance} T{}", self.label, index + 1)
        } else {
            format!("{}{instance}", self.label)
        }
    }
}

pub struct ThermalsPane {}

impl ThermalsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }
}

impl PaneUi for ThermalsPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System { system_id, .. } = behavior.active_view else {
            return;
        };

        let summary = behavior.source.db.message_summary(system_id);
        let lines: Vec<PlotLine> = SENSORS
            .iter()
            .flat_map(|sensor| {
                summary
                    .iter()
                    .filter(|row| row.name == sensor.message)
                    .flat_map(move |row| {
                        sensor
                            .fields
                            .iter()
                            .enumerate()
                            .map(move |(i, field)| PlotLine {
                                system_id,
                                component_id: row.component_id,
                                message_name: sensor.message.to_owned(),
                                instance: row.instance.clone(),
                                field_name: (*field).to_owned(),
                                alias: Some(sensor.alias(i, row.instance.as_ref())),
                                unit: Some("\u{00b0}C".to_owned()),
                                color: None,
                                scale: Some(sensor.scale),
                                sentinel: sensor.sentinel,
                            })
                    })
            })
            .collect();

        let plot = Plot::new(
            &lines,
            &behavior.source,
            behavior.shared_plot_state,
            (None, None),
        );
        ui.add_sized(ui.available_size(), plot);
    }
}
