use std::time::Duration;

use nadir_core::System;

use egui::{Sense, Stroke, Vec2};
use mavspec::rust::dialects::common::messages::CameraCaptureStatus;

use crate::colors::{COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, readable};

/// One autopilot-attached camera, drawn as a recording dot: blinking red while recording, a hollow
/// ring while idle, solid orange while its state is unknown.
pub struct Camera {
    /// `CAMERA_CAPTURE_STATUS.camera_device_id`, 1-based.
    pub id: u8,
    pub recording: Option<bool>,
    pub diameter: f32,
}

impl Camera {
    pub fn all(system: &System) -> Vec<Self> {
        system
            .last_instance_messages::<CameraCaptureStatus>()
            .iter()
            .map(|status| Self {
                id: status.camera_device_id,
                // zenith reports an output the bus has no reading for as `u8::MAX`.
                recording: match status.video_status {
                    0 => Some(false),
                    1 => Some(true),
                    _ => None,
                },
                diameter: 8.0,
            })
            .collect()
    }
}

impl egui::Widget for Camera {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.diameter), Sense::hover());
        let radius = self.diameter / 2.0 - 0.5;
        let painter = ui.painter();

        match self.recording {
            Some(true) => {
                // Telemetry can arrive slower than the blink, so it cannot be relied on to repaint.
                ui.ctx().request_repaint_after(Duration::from_millis(100));
                let red = readable(COLOR_INDICATOR_LIMITS, ui.visuals());
                // Slower than the shared alert blink, like a camera's own tally light.
                if (ui.input(|i| i.time) * 0.5).fract() < 0.5 {
                    painter.circle_filled(rect.center(), radius, red);
                } else {
                    painter.circle_stroke(rect.center(), radius, Stroke::new(1.0, red));
                }
            }
            Some(false) => {
                let weak = ui.visuals().weak_text_color();
                painter.circle_stroke(rect.center(), radius, Stroke::new(1.0, weak));
            }
            None => {
                let orange = readable(COLOR_INDICATOR_WARNING, ui.visuals());
                painter.circle_filled(rect.center(), radius, orange);
            }
        }

        response
    }
}
