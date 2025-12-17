use std::f32::consts::PI;

use core::System;

use egui::epaint::{PathShape, TextShape};
use egui::text::LayoutJob;
use egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Galley, Pos2, Rect, Sense, Shape,
    Stroke, TextFormat, Vec2,
};
use mavspec::rust::dialects::common::messages::{LocalPositionNed, VfrHud};

pub struct ArtificialHorizon {
    system: System,
}

const COLOR_GROUND: Color32 = Color32::from_rgb(0x7d, 0x52, 0x33);
const COLOR_SKY: Color32 = Color32::from_rgb(0x5b, 0x93, 0xc5);
const N: usize = 1000;

impl ArtificialHorizon {
    pub fn new(system: &System) -> Self {
        Self {
            system: system.clone(),
        }
    }

    fn draw_ball(&self, painter: &mut egui::Painter, pitch: f32, center: Pos2, radius: f32) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        let original_rect = painter.clip_rect();
        let ball_clip_rect = Rect::from_center_size(
            painter.clip_rect().center(),
            Vec2::new(1.6 * radius, 1.6 * radius),
        );
        painter.set_clip_rect(ball_clip_rect);

        // Our ball will almost always have a convex and a concave half. We want convex polygons,
        // so render the concave part first as a circle.
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

        // project our pitch ticks into 2d and paint them
        for pitch_tick in (-90i32..=90).into_iter().step_by(5) {
            let r = (pitch_tick as f32).to_radians();

            let points3d = (0..N).into_iter().map(|i| {
                let a = PI * (i as f32) / (N as f32);
                nalgebra::Vector3::new(a.cos() * r.cos(), a.sin() * r.cos(), -r.sin())
            });

            let rotation = nalgebra::Rotation3::new(nalgebra::Vector3::x() * pitch);
            let transformed = points3d.map(|p| rotation * p);

            let projected: Vec<_> = transformed
                .filter_map(|v| {
                    (v.y > 0.0
                        && (pitch_tick == 0
                            || (v.x.abs() < 0.4
                                && (pitch_tick.abs() % 20 != 10 || v.x.abs() < 0.1)
                                && (pitch_tick.abs() % 10 != 5 || v.x.abs() < 0.05))))
                        .then_some(center + Vec2::new(v.x, v.z) * radius)
                })
                .collect();

            if let Some(pos) = projected.first()
                && pitch_tick.abs() == 90
            {
                painter.circle_filled(*pos, 1.0, Color32::WHITE);
            }

            painter.add(Shape::Path(PathShape::line(
                projected.clone(),
                Stroke::new(
                    if pitch_tick == 0 {
                        3.0
                    } else if pitch_tick % 20 == 10 || pitch_tick % 10 == 5 {
                        0.75
                    } else {
                        2.0
                    },
                    Color32::WHITE,
                ),
            )));

            if pitch_tick % 20 == 0 && pitch_tick != 0 && projected.len() > 0 {
                for center in [projected[0], projected[projected.len() - 1]] {
                    painter.rect_filled(
                        Rect::from_center_size(center, Vec2::new(30.0, 20.0)),
                        CornerRadius::ZERO,
                        if pitch_tick > 0 {
                            COLOR_SKY
                        } else {
                            COLOR_GROUND
                        },
                    );
                    painter.text(
                        center,
                        Align2::CENTER_CENTER,
                        format!("{}", pitch_tick),
                        FontId::monospace(10.0),
                        //Color32::BLACK,
                        Color32::WHITE,
                    );
                }
            }
        }

        // restore our original clip rect
        painter.set_clip_rect(original_rect);
    }

    fn draw_roll_indicator(
        &self,
        painter: &mut egui::Painter,
        roll: f32,
        center: Pos2,
        radius: f32,
    ) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

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
    }

    fn draw_compass(&self, painter: &mut egui::Painter, heading: f32, center: Pos2, radius: f32) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        for a in (0..360).step_by(10) {
            let r = (a as f32).to_radians() - heading;
            let tip = center + Vec2::new(r.sin(), -r.cos()) * radius;

            if a % 30 == 0 {
                let (text, size, color) = match a {
                    0 => ("N".to_owned(), 20.0, Color32::ORANGE),
                    90 => ("E".to_owned(), 20.0, Color32::ORANGE),
                    180 => ("S".to_owned(), 20.0, Color32::ORANGE),
                    270 => ("W".to_owned(), 20.0, Color32::ORANGE),
                    a => (format!("{}", a), 12.0, Color32::WHITE),
                };

                let mut job = LayoutJob::default();
                job.append(
                    &text,
                    0.0,
                    TextFormat {
                        font_id: FontId::new(size, FontFamily::Monospace),
                        color,
                        ..Default::default()
                    },
                );
                job.halign = Align::Center;

                let galley = painter.layout_job(job);

                painter.add(Shape::Text(
                    TextShape::new(tip, galley, Color32::TRANSPARENT)
                        .with_angle_and_anchor(r, Align2::CENTER_CENTER),
                ));
            } else {
                painter.line(
                    vec![center.lerp(tip, 1.0 - 0.05), tip],
                    Stroke::new(2.0, Color32::WHITE),
                );
            }
        }

        let indicator_pos = center - Vec2::new(0.0, radius - 40.0);

        painter.rect(
            Rect::from_center_size(indicator_pos, Vec2::new(48.0, 25.0)),
            CornerRadius::ZERO.at_least(1),
            Color32::BLACK,
            Stroke::new(1.0, Color32::WHITE),
            egui::StrokeKind::Outside,
        );

        painter.text(
            indicator_pos,
            Align2::CENTER_CENTER,
            format!("{:.0}", (heading.to_degrees().round() as i32 + 360) % 360),
            FontId::monospace(16.0),
            Color32::WHITE,
        );
    }

    fn draw_side_dial(
        &self,
        painter: &mut egui::Painter,
        value: f32,
        side: f32,
        throttle: Option<f32>,
    ) {
        const POINTS_PER_UNIT: f32 = 4.0;
        const COLOR_DIAL: Color32 = Color32::from_rgb(0xc0, 0xc0, 0xc0);

        let rect = painter.clip_rect();
        let center_side = rect.center() + Vec2::new(rect.width() * 0.5 * side, 0.0);

        for tick in (0..10_000).step_by(2) {
            let len = if tick % 100 == 0 {
                18.0
            } else if tick % 10 == 0 {
                14.0
            } else {
                10.0
            };

            let y = (tick as f32 - value) * POINTS_PER_UNIT;
            painter.line(
                vec![
                    center_side - Vec2::new(0.0, y),
                    center_side - Vec2::new(len * side, y),
                ],
                Stroke::new(if tick % 50 == 0 { 3.0 } else { 1.5 }, Color32::WHITE),
            );

            if tick % 50 == 0 {
                painter.text(
                    center_side - Vec2::new((len + 5.0) * side, y),
                    if side > 0.0 {
                        Align2::RIGHT_CENTER
                    } else {
                        Align2::LEFT_CENTER
                    },
                    format!("{:0}", tick),
                    FontId::monospace(12.0),
                    Color32::WHITE,
                );
            }
        }

        if let Some(throttle) = throttle {
            painter.line(
                vec![
                    center_side,
                    center_side - Vec2::new(0.0, throttle * rect.height() / 2.0),
                ],
                Stroke::new(12.0, Color32::RED),
            );
        }

        painter.line(
            vec![
                center_side + Vec2::new(0.0, value * POINTS_PER_UNIT),
                center_side - Vec2::new(0.0, rect.height() / 2.0),
            ],
            Stroke::new(5.0, COLOR_DIAL),
        );

        painter.add(Shape::convex_polygon(
            vec![
                center_side + Vec2::new(-80.0 * side, -12.0),
                center_side + Vec2::new(-20.0 * side, -12.0),
                center_side + Vec2::new(-8.0 * side, 0.0),
                center_side + Vec2::new(-20.0 * side, 12.0),
                center_side + Vec2::new(-80.0 * side, 12.0),
            ],
            Color32::BLACK,
            Stroke::new(2.0, Color32::WHITE),
        ));

        let text_x = if side > 0.0 { -22.0 } else { 76.0 };

        painter.text(
            center_side + Vec2::new(text_x - 5.0, 0.0),
            Align2::RIGHT_CENTER,
            format!("{:.0}.", value),
            FontId::monospace(18.0),
            Color32::WHITE,
        );
        painter.text(
            center_side + Vec2::new(text_x, 0.0),
            Align2::RIGHT_CENTER,
            format!("{:.1}", value % 1.0)
                .chars()
                .last()
                .iter()
                .collect::<String>(),
            FontId::monospace(18.0),
            Color32::WHITE,
        );
    }

    fn draw_altitude_dial(
        &self,
        painter: &mut egui::Painter,
        local_position: Option<&LocalPositionNed>,
    ) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        let altitude = local_position.as_ref().map(|v| v.z * -1.0).unwrap_or(0.0);
        let climb = local_position.as_ref().map(|v| v.vz * -1.0).unwrap_or(0.0);
        self.draw_side_dial(painter, altitude, 1.0, None);
    }

    fn draw_velocity_dial(
        &self,
        painter: &mut egui::Painter,
        local_position: Option<&LocalPositionNed>,
        vfr_hud: Option<&VfrHud>,
    ) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        let throttle = vfr_hud
            .map(|v| v.throttle as f32 / 100.0)
            .unwrap_or_default();
        let velocity = local_position
            .map(|v| (v.vx.powi(2) + v.vy.powi(2) + v.vz.powi(2)).sqrt())
            .unwrap_or(0.0);
        self.draw_side_dial(painter, velocity, -1.0, Some(throttle));
    }
}

impl egui::Widget for ArtificialHorizon {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let attitude = self.system.last_attitude().ok().flatten();
        let pitch = attitude.as_ref().map(|a| a.pitch).unwrap_or(0.0);
        let roll = attitude.as_ref().map(|a| a.roll).unwrap_or(0.0);
        let yaw = attitude.as_ref().map(|a| a.yaw).unwrap_or(0.0);

        let local_position = self.system.last_local_position_ned().ok().flatten();
        let vfr_hud = self.system.last_vfr_hud().ok().flatten();

        let (response, mut painter) = ui.allocate_painter(ui.available_size(), Sense::empty());

        // We fill the width of the UI with our little ball.
        let radius = f32::min(response.rect.width(), response.rect.height()) * 0.4;
        let center = response.rect.center();

        let rect = painter.clip_rect();

        // draw our static sky and ground backgrounds and our central line
        painter.rect_filled(
            Rect::from_two_pos(rect.left_top(), rect.right_center()),
            CornerRadius::ZERO,
            COLOR_SKY.gamma_multiply(0.9),
        );
        painter.rect_filled(
            Rect::from_two_pos(rect.left_bottom(), rect.right_center()),
            CornerRadius::ZERO,
            COLOR_GROUND.gamma_multiply(0.9),
        );
        painter.line(
            vec![rect.left_center(), rect.right_center()],
            Stroke::new(3.0, Color32::WHITE),
        );

        // draw our horizon and roll indicators
        self.draw_ball(&mut painter, pitch, center, radius);
        self.draw_roll_indicator(&mut painter, roll, center, radius);

        // draw some darkened backgrounds on the sides for our velocity and altitude dials
        painter.rect_filled(
            Rect::from_two_pos(rect.right_top(), rect.right_bottom() - Vec2::new(50.0, 0.0)),
            CornerRadius::ZERO,
            Color32::BLACK.gamma_multiply(1.0),
        );
        painter.rect_filled(
            Rect::from_two_pos(rect.left_top(), rect.left_bottom() + Vec2::new(50.0, 0.0)),
            CornerRadius::ZERO,
            Color32::BLACK.gamma_multiply(1.0),
        );

        painter.rect_filled(
            Rect::from_two_pos(
                rect.left_bottom(),
                rect.right_bottom() - Vec2::new(0.0, 80.0),
            ),
            CornerRadius::ZERO,
            Color32::BLACK.gamma_multiply(1.0),
        );

        self.draw_compass(
            &mut painter,
            yaw,
            rect.center_bottom() + Vec2::new(0.0, radius - 60.0),
            radius,
        );

        self.draw_altitude_dial(&mut painter, local_position.as_ref());
        self.draw_velocity_dial(&mut painter, local_position.as_ref(), vfr_hud.as_ref());

        response
    }
}
