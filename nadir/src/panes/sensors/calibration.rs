use std::time::Duration;

use chrono::{DateTime, Utc};
use eframe::egui;
use egui::{Button, Grid, ProgressBar};
use mavspec::rust::dialects::common::messages::{ScaledImu, ScaledImu2, ScaledImu3};

use nadir_core::{ParamProgress, ParamVal, System};

use super::G;
use crate::panes::MUTED_HINT;
use crate::widgets::Hazard;

const STATIC_DURATION: Duration = Duration::from_secs(10);
const MIN_SAMPLES: usize = 20;
const AXES: [&str; 3] = ["X", "Y", "Z"];
/// Below the full-rotation span (twice the field) anywhere on Earth, so only catches a
/// capture that has barely moved.
const MIN_MAG_SPAN: f32 = 40.0;
const WRITE_RETRY_SECS: f64 = 1.0;
const WRITE_ATTEMPTS: u8 = 5;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Static,
    Mag,
}

/// A `SCALED_IMU*` message in the units of zenith's `CAL_*` params: m/s^2, deg/s and uT.
#[derive(Clone, Copy)]
struct Sample {
    acc: [f32; 3],
    gyr: [f32; 3],
    mag: [f32; 3],
}

struct Capture {
    kind: Kind,
    start: DateTime<Utc>,
    /// Telemetry is already offset by these, so they must not change while capturing.
    snapshot: Vec<(String, f32)>,
}

/// One vector param, e.g. `CAL_IMU1_ACC`, standing for its `_X/_Y/_Z` members.
struct Row {
    prefix: String,
    current: [f32; 3],
    proposed: [f32; 3],
}

pub struct Calibration {
    capture: Option<Capture>,
    proposal: Vec<Row>,
    /// Values sent but not yet echoed back. zenith drops a burst of `PARAM_SET` beyond the
    /// first few, so these are resent until confirmed.
    writing: Vec<(String, f32)>,
    written_at: f64,
    attempts: u8,
    message: Option<String>,
}

impl Kind {
    fn params(self) -> Vec<String> {
        match self {
            Self::Static => (1..=3)
                .flat_map(|imu| {
                    ["GYR", "ACC"].into_iter().flat_map(move |sensor| {
                        AXES.map(|axis| format!("CAL_IMU{imu}_{sensor}_{axis}"))
                    })
                })
                .collect(),
            Self::Mag => AXES.map(|axis| format!("CAL_MAG_{axis}")).to_vec(),
        }
    }
}

impl Sample {
    fn between(system: &System, start: DateTime<Utc>, end: DateTime<Utc>, imu: usize) -> Vec<Self> {
        macro_rules! convert {
            ($msg:ty) => {
                system
                    .messages_since::<$msg>(Some(start), None)
                    .into_iter()
                    .take_while(|(t, _)| *t <= end)
                    .map(|(_, m)| Self {
                        acc: [m.xacc, m.yacc, m.zacc].map(|v| f32::from(v) * G / 1000.0),
                        gyr: [m.xgyro, m.ygyro, m.zgyro]
                            .map(|v| (f32::from(v) / 1000.0).to_degrees()),
                        mag: [m.xmag, m.ymag, m.zmag].map(|v| f32::from(v) / 10.0),
                    })
                    .collect()
            };
        }

        match imu {
            1 => convert!(ScaledImu),
            2 => convert!(ScaledImu2),
            _ => convert!(ScaledImu3),
        }
    }

    fn mean_std(values: impl Iterator<Item = [f32; 3]> + Clone) -> ([f32; 3], [f32; 3]) {
        let n = values.clone().count().max(1) as f32;
        let mean = values
            .clone()
            .fold([0.0; 3], |acc, v| [0, 1, 2].map(|i| acc[i] + v[i] / n));
        let var = values.fold([0.0; 3], |acc, v| {
            [0, 1, 2].map(|i| acc[i] + (v[i] - mean[i]).powi(2) / n)
        });
        (mean, var.map(f32::sqrt))
    }

    /// Gyro and accel offsets for a vehicle at rest, nose (+Z) up.
    fn static_offsets(samples: &[Self]) -> ([f32; 3], [f32; 3]) {
        let (gyr, _) = Self::mean_std(samples.iter().map(|s| s.gyr));
        let (acc, _) = Self::mean_std(samples.iter().map(|s| s.acc));
        (gyr, [acc[0], acc[1], acc[2] - G])
    }

    fn mag_extremes(samples: &[Self]) -> ([f32; 3], [f32; 3]) {
        samples.iter().fold(
            ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
            |(min, max), s| {
                (
                    [0, 1, 2].map(|i| min[i].min(s.mag[i])),
                    [0, 1, 2].map(|i| max[i].max(s.mag[i])),
                )
            },
        )
    }
}

impl Calibration {
    pub fn new() -> Self {
        Self {
            capture: None,
            proposal: Vec::new(),
            writing: Vec::new(),
            written_at: 0.0,
            attempts: 0,
            message: None,
        }
    }

    fn write_ui(&mut self, ui: &mut egui::Ui, system: &System) {
        let names: Vec<String> = self.writing.iter().map(|(n, _)| n.clone()).collect();
        if let Some(current) = Self::current(system, &names) {
            self.writing.retain(|w| !current.contains(w));
        }
        if self.writing.is_empty() {
            return;
        }

        let now = ui.input(|i| i.time);
        if now - self.written_at >= WRITE_RETRY_SECS {
            if self.attempts == WRITE_ATTEMPTS {
                let names: Vec<&str> = self.writing.iter().map(|(n, _)| n.as_str()).collect();
                self.message = Some(format!("Not confirmed: {}", names.join(", ")));
                self.writing.clear();
                return;
            }
            for (name, value) in &self.writing {
                system.set_param(name, ParamVal::Float32(*value));
            }
            self.written_at = now;
            self.attempts += 1;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        ui.label(format!("Writing {} params", self.writing.len()));
    }

    fn current(system: &System, names: &[String]) -> Option<Vec<(String, f32)>> {
        let params = system.params.lock().unwrap();
        let ParamProgress::Complete(params) = &*params else {
            return None;
        };
        names
            .iter()
            .map(|name| Some((name.clone(), params.get(name)?.downloaded_value.as_float())))
            .collect()
    }

    fn finish(&mut self, system: &System, end: DateTime<Utc>) {
        let Some(capture) = self.capture.take() else {
            return;
        };
        let names: Vec<String> = capture.snapshot.iter().map(|(n, _)| n.clone()).collect();
        if Self::current(system, &names).as_ref() != Some(&capture.snapshot) {
            self.message = Some("CAL params changed during capture, rerun".to_owned());
            return;
        }

        let current = |name: &str| {
            capture
                .snapshot
                .iter()
                .find_map(|(n, v)| (n == name).then_some(*v))
                .unwrap_or_default()
        };
        let mut rows = Vec::new();
        let mut skipped = Vec::new();
        let imus = match capture.kind {
            Kind::Static => 1..=3,
            Kind::Mag => 1..=1,
        };
        for imu in imus {
            let samples = Sample::between(system, capture.start, end, imu);
            if samples.len() < MIN_SAMPLES {
                skipped.push(format!("IMU{imu}"));
                continue;
            }
            let groups = match capture.kind {
                Kind::Static => {
                    let (gyr, acc) = Sample::static_offsets(&samples);
                    vec![
                        (format!("CAL_IMU{imu}_GYR"), gyr),
                        (format!("CAL_IMU{imu}_ACC"), acc),
                    ]
                }
                Kind::Mag => {
                    let (min, max) = Sample::mag_extremes(&samples);
                    vec![(
                        "CAL_MAG".to_owned(),
                        [0, 1, 2].map(|i| f32::midpoint(min[i], max[i])),
                    )]
                }
            };
            for (prefix, delta) in groups {
                let current = AXES.map(|axis| current(&format!("{prefix}_{axis}")));
                rows.push(Row {
                    proposed: [0, 1, 2].map(|i| current[i] + delta[i]),
                    prefix,
                    current,
                });
            }
        }

        self.message = (!skipped.is_empty()).then(|| {
            format!(
                "Too few samples from {}; use a USB or Ethernet link",
                skipped.join(", ")
            )
        });
        self.proposal = rows;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, system: &System) {
        self.write_ui(ui, system);
        let Some(capture) = &self.capture else {
            self.idle_ui(ui, system);
            return;
        };
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        let (kind, start) = (capture.kind, capture.start);

        match kind {
            Kind::Static => {
                ui.label(
                    "Keep the vehicle vertical, nose up, and completely still until the capture \
                     completes.",
                );
                let elapsed = (Utc::now() - start).to_std().unwrap_or_default();
                ui.add(
                    ProgressBar::new(elapsed.as_secs_f32() / STATIC_DURATION.as_secs_f32())
                        .show_percentage(),
                );
                Grid::new("calibration_static")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("");
                        ui.strong("Samples");
                        ui.strong("Gyro std [deg/s]");
                        ui.strong("Accel std [m/s²]");
                        ui.end_row();
                        for imu in 1..=3 {
                            let samples = Sample::between(system, start, Utc::now(), imu);
                            let (_, gyr) = Sample::mean_std(samples.iter().map(|s| s.gyr));
                            let (_, acc) = Sample::mean_std(samples.iter().map(|s| s.acc));
                            ui.label(format!("IMU{imu}"));
                            ui.label(samples.len().to_string());
                            ui.monospace(format!("{:.3} {:.3} {:.3}", gyr[0], gyr[1], gyr[2]));
                            ui.monospace(format!("{:.3} {:.3} {:.3}", acc[0], acc[1], acc[2]));
                            ui.end_row();
                        }
                    });
                if elapsed >= STATIC_DURATION {
                    self.finish(system, start + STATIC_DURATION);
                    return;
                }
            }
            Kind::Mag => {
                ui.label(
                    "Slowly rotate the vehicle through every orientation, all axes in both \
                     directions, away from metal and magnets. Press Done once each span has \
                     stopped growing.",
                );
                let samples = Sample::between(system, start, Utc::now(), 1);
                let (min, max) = Sample::mag_extremes(&samples);
                Grid::new("calibration_mag").striped(true).show(ui, |ui| {
                    ui.strong("Axis");
                    ui.strong("Min [uT]");
                    ui.strong("Max [uT]");
                    ui.strong("Span [uT]");
                    ui.end_row();
                    for (i, axis) in AXES.iter().enumerate() {
                        ui.label(*axis);
                        if samples.is_empty() {
                            ui.label("-");
                            ui.label("-");
                            ui.label("-");
                        } else {
                            ui.monospace(format!("{:.1}", min[i]));
                            ui.monospace(format!("{:.1}", max[i]));
                            ui.monospace(format!("{:.1}", max[i] - min[i]));
                        }
                        ui.end_row();
                    }
                });
                let covered =
                    !samples.is_empty() && (0..3).all(|i| max[i] - min[i] >= MIN_MAG_SPAN);
                let done = ui
                    .add_enabled(covered, Button::new("✔ Done"))
                    .on_disabled_hover_text(format!(
                        "Every axis needs a span of at least {MIN_MAG_SPAN} uT"
                    ));
                if done.clicked() {
                    self.finish(system, Utc::now());
                    return;
                }
            }
        }

        if ui.button("✖ Cancel").clicked() {
            self.capture = None;
        }
    }

    fn idle_ui(&mut self, ui: &mut egui::Ui, system: &System) {
        let mut start = |kind: Kind, ui: &mut egui::Ui, label: &str| {
            let snapshot = Self::current(system, &kind.params());
            let button = ui
                .add_enabled(snapshot.is_some(), Button::new(label))
                .on_disabled_hover_text("Waiting for parameters");
            if button.clicked() {
                self.capture = snapshot.map(|snapshot| Capture {
                    kind,
                    start: Utc::now(),
                    snapshot,
                });
                self.proposal.clear();
                self.message = None;
            }
        };

        ui.label(
            "1. Gyro and accelerometer: stand the vehicle vertical, nose up, keep it still, \
             then start a 10 s capture.",
        );
        start(Kind::Static, ui, "▶ Capture gyro + accel");
        ui.label("2. Magnetometer: rotate the vehicle through every orientation.");
        start(Kind::Mag, ui, "▶ Capture magnetometer");

        if let Some(message) = &self.message {
            ui.colored_label(ui.visuals().warn_fg_color, message);
        }
        if self.proposal.is_empty() {
            return;
        }

        ui.separator();
        Grid::new("calibration_proposal")
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Param");
                ui.strong("Current");
                ui.strong("Proposed");
                ui.strong("Change");
                ui.end_row();
                for row in &self.proposal {
                    let fmt = |v: [f32; 3]| format!("{:+.3} {:+.3} {:+.3}", v[0], v[1], v[2]);
                    ui.monospace(&row.prefix);
                    ui.monospace(fmt(row.current));
                    ui.monospace(fmt(row.proposed));
                    ui.monospace(fmt([0, 1, 2].map(|i| row.proposed[i] - row.current[i])));
                    ui.end_row();
                }
            });

        ui.horizontal(|ui| {
            let apply = ui
                .add_enabled(!system.muted(), Button::new("💾 Apply"))
                .on_disabled_hover_text(MUTED_HINT);
            Hazard::tick(ui, apply.rect, system);
            if Hazard::confirm(ui, &apply, system) {
                self.writing = self
                    .proposal
                    .drain(..)
                    .flat_map(|row| {
                        AXES.iter()
                            .zip(row.proposed)
                            .map(move |(axis, v)| (format!("{}_{axis}", row.prefix), v))
                    })
                    .collect();
                self.written_at = f64::NEG_INFINITY;
                self.attempts = 0;
            }
            if ui.button("⟲ Discard").clicked() {
                self.proposal.clear();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(acc: [f32; 3], gyr: [f32; 3], mag: [f32; 3]) -> Sample {
        Sample { acc, gyr, mag }
    }

    #[test]
    fn static_offsets_remove_gravity_on_z() {
        let samples = [
            sample([0.1, -0.2, G + 0.3], [1.0, 2.0, 3.0], [0.0; 3]),
            sample([0.3, -0.4, G + 0.5], [3.0, 4.0, 5.0], [0.0; 3]),
        ];
        let (gyr, acc) = Sample::static_offsets(&samples);
        assert_eq!(gyr, [2.0, 3.0, 4.0]);
        for (a, e) in acc.iter().zip([0.2, -0.3, 0.4]) {
            assert!((a - e).abs() < 1e-5, "{acc:?}");
        }
    }

    #[test]
    fn mag_extremes_per_axis() {
        let samples = [
            sample([0.0; 3], [0.0; 3], [10.0, -5.0, 0.0]),
            sample([0.0; 3], [0.0; 3], [-30.0, 25.0, 4.0]),
        ];
        assert_eq!(
            Sample::mag_extremes(&samples),
            ([-30.0, -5.0, 0.0], [10.0, 25.0, 4.0])
        );
    }
}
