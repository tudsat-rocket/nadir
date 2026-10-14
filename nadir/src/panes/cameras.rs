use nadir_core::System;

use egui::{RichText, Vec2};
use mavspec::rust::dialects::common::messages::CameraInformation;

use crate::colors::{COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, readable};
use crate::panes::{MUTED_HINT, PaneUi};
use crate::widgets::Camera;

pub struct CamerasPane {}

impl CamerasPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    /// `camera` 0 addresses all cameras.
    fn button(ui: &mut egui::Ui, system: &System, camera: u8, recording: bool, label: &str) {
        let clicked = ui
            .add_enabled_ui(!system.muted(), |ui| ui.button(label))
            .inner
            .on_disabled_hover_text(MUTED_HINT)
            .clicked();
        if clicked {
            system.do_video_capture(camera, recording);
        }
    }

    fn name(info: &CameraInformation) -> Option<String> {
        let text = |bytes: &[u8]| {
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
        };
        let name = [text(&info.vendor_name), text(&info.model_name)]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        (!name.is_empty()).then_some(name)
    }

    fn details(info: &CameraInformation) -> Vec<String> {
        let mut details = Vec::new();
        if info.resolution_h != 0 && info.resolution_v != 0 {
            details.push(format!("{}x{}", info.resolution_h, info.resolution_v));
        }
        if !info.focal_length.is_nan() {
            details.push(format!("{} mm", info.focal_length));
        }
        if info.firmware_version != 0 {
            let [major, minor, patch, dev] = info.firmware_version.to_le_bytes();
            details.push(format!("fw {major}.{minor}.{patch}.{dev}"));
        }
        details
    }

    fn capabilities(info: &CameraInformation) -> String {
        let names: Vec<_> = info.flags.iter_names().map(|(name, _)| name).collect();
        if names.is_empty() {
            "No capabilities reported".to_owned()
        } else {
            names.join("\n")
        }
    }
}

impl PaneUi for CamerasPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        let cameras = Camera::all(&system);
        if cameras.is_empty() {
            ui.weak("No camera data");
            return;
        }

        ui.horizontal(|ui| {
            Self::button(ui, &system, 0, true, "⏺ Start all");
            Self::button(ui, &system, 0, false, "⏹ Stop all");
        });
        ui.separator();

        let red = readable(COLOR_INDICATOR_LIMITS, ui.visuals());
        let orange = readable(COLOR_INDICATOR_WARNING, ui.visuals());
        egui::Grid::new("cameras")
            .num_columns(5)
            .striped(true)
            .spacing(Vec2::new(10.0, 4.0))
            .show(ui, |ui| {
                for camera in cameras {
                    let (id, recording) = (camera.id, camera.recording);
                    let info = system
                        .last_instance_message::<CameraInformation>(id.into())
                        .ok();

                    ui.label(format!("Camera {id}"));
                    ui.horizontal(|ui| {
                        ui.add(Camera {
                            diameter: 10.0,
                            ..camera
                        });
                        match recording {
                            Some(true) => {
                                ui.label(RichText::new("REC").strong().color(red));
                            }
                            Some(false) => {
                                ui.weak("idle");
                            }
                            None => {
                                ui.label(RichText::new("unknown").color(orange));
                            }
                        }
                    });
                    match recording {
                        Some(true) => Self::button(ui, &system, id, false, "⏹ Stop"),
                        Some(false) => Self::button(ui, &system, id, true, "⏺ Start"),
                        None => {
                            ui.horizontal(|ui| {
                                Self::button(ui, &system, id, true, "⏺ Start");
                                Self::button(ui, &system, id, false, "⏹ Stop");
                            });
                        }
                    }
                    match &info {
                        Some(info) => {
                            ui.label(Self::name(info).unwrap_or_else(|| "--".to_owned()))
                                .on_hover_text(Self::capabilities(info));
                            ui.weak(Self::details(info).join(", "));
                        }
                        None => {
                            ui.weak("--");
                            ui.weak("");
                        }
                    }
                    ui.end_row();
                }
            });
    }
}
