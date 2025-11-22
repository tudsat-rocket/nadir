use std::f32::consts::PI;

use core::System;

use egui::epaint::PathShape;
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Shape, Stroke, Vec2};

pub struct FalseHorizon {
    system: System,
}

const COLOR_GROUND: Color32 = Color32::from_rgb(0x7d, 0x52, 0x33);
const COLOR_SKY: Color32 = Color32::from_rgb(0x5b, 0x93, 0xc5);
const N: usize = 1000;

impl FalseHorizon {
    pub fn new(system: &System) -> Self {
        Self {
            system: system.clone(),
        }
    }
}

impl egui::Widget for FalseHorizon {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let attitude = self.system.last_attitude().ok().flatten();
        let pitch = attitude.as_ref().map(|a| a.pitch).unwrap_or(0.0);
        let roll = attitude.as_ref().map(|a| a.roll).unwrap_or(0.0);

        let (response, mut painter) = ui.allocate_painter(ui.available_size(), Sense::empty());

        // We fill the width of the UI with our little ball.
        let radius = ui.available_width() * 0.5;
        let center = response.rect.center();

        let clip_rect = Rect::from_center_size(
            painter.clip_rect().center(),
            Vec2::new(1.6 * radius, 1.6 * radius),
        );
        painter.set_clip_rect(clip_rect);

        painter.circle_filled(
            center,
            radius,
            if pitch > 0.0 { COLOR_GROUND } else { COLOR_SKY },
        );

        painter.add(Shape::Path(PathShape::convex_polygon(
            (0..N)
                .into_iter()
                .map(|i| {
                    let a = PI * (i as f32) / (N as f32);
                    center + Vec2::new(a.cos(), a.sin() * pitch.signum() * -1.0) * radius
                })
                .chain((0..N).into_iter().map(|i| {
                    let a = PI - PI * (i as f32) / (N as f32);
                    center + Vec2::new(a.cos(), a.sin() * pitch.sin()) * radius
                }))
                .collect(),
            if pitch < 0.0 { COLOR_GROUND } else { COLOR_SKY },
            Stroke::NONE,
        )));

        for pitch_tick in (-80..=80).into_iter().step_by(10) {
            let r = (pitch_tick as f32).to_radians();

            let points3d = (0..N).into_iter().map(|i| {
                let a = 2.0 * PI * (i as f32) / (N as f32);
                nalgebra::Vector3::new(a.sin() * r.cos(), -a.cos() * r.cos(), -r.sin())
            });

            let rotation = nalgebra::Rotation3::new(nalgebra::Vector3::x() * pitch);
            let transformed = points3d.map(|p| rotation * p);

            let projected: Vec<_> = transformed
                .filter_map(|v| {
                    (v.y > 0.0 && v.x.abs() < 0.6).then_some(center + Vec2::new(v.x, v.z) * radius)
                })
                .collect();

            painter.add(Shape::Path(PathShape::line(
                projected.clone(),
                Stroke::new(
                    if pitch_tick == 0 {
                        2.0
                    } else if pitch_tick % 20 == 10 {
                        0.6
                    } else {
                        1.0
                    },
                    Color32::WHITE,
                ),
            )));

            if pitch_tick % 20 == 0 && pitch_tick != 0 && projected.len() > 0 {
                let center = projected[projected.len() / 2];

                painter.rect_filled(
                    Rect::from_center_size(center, Vec2::new(20.0, 10.0)),
                    CornerRadius::ZERO,
                    Color32::WHITE,
                );
                painter.text(
                    center,
                    Align2::CENTER_CENTER,
                    format!("{}", pitch_tick),
                    FontId::monospace(10.0),
                    Color32::BLACK,
                );
            }
        }

        painter.circle_filled(center, 2.0, Color32::RED);
        painter.circle_filled(center, 1.5, Color32::ORANGE);
        for mirror in [-1.0, 1.0] {
            let points = vec![
                nalgebra::Vector2::new(mirror * radius * 0.5, 0.0),
                nalgebra::Vector2::new(mirror * radius * 0.15, 0.0),
                nalgebra::Vector2::new(mirror * radius * 0.15, radius * 0.07),
            ];

            let projected: Vec<_> = points
                .into_iter()
                .map(|p| nalgebra::Rotation2::new(roll) * p)
                .map(|p| center + Vec2::new(p.x, p.y))
                .collect();

            painter.add(Shape::line(
                projected.clone(),
                Stroke::new(4.0, Color32::RED),
            ));
            painter.add(Shape::line(projected, Stroke::new(3.0, Color32::ORANGE)));
        }
        //})
        //.response
        response
    }
}
