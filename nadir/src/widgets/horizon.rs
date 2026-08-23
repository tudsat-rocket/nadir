use std::f32::consts::{FRAC_PI_2, TAU};

use nadir_core::System;

use egui::epaint::{Mesh, PathShape, TextShape, Vertex};
use egui::text::LayoutJob;
use egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke,
    TextFormat, TextureId, Vec2,
};
use mavspec::rust::dialects::common::messages::{Attitude, LocalPositionNed, VfrHud};

use crate::panes::{PositionSource, VelocityMode};

pub struct ArtificialHorizon {
    system: System,
    source: PositionSource,
    velocity_mode: VelocityMode,
}

const COLOR_GROUND: Color32 = Color32::from_rgb(0x7d, 0x52, 0x33);
const COLOR_SKY: Color32 = Color32::from_rgb(0x5b, 0x93, 0xc5);
const N: usize = 64;
// How far in front of the ball the camera sits, in ball radii. Lower is more perspective.
const CAMERA: f32 = 3.0;
// Half the roll range the dial around the top of the ball covers, in degrees.
const ROLL_LIMIT: i32 = 45;
const ROLL_STEP: usize = 15;

impl ArtificialHorizon {
    pub fn new(system: &System, source: PositionSource, velocity_mode: VelocityMode) -> Self {
        Self {
            system: system.clone(),
            source,
            velocity_mode,
        }
    }

    fn draw_ball(&self, painter: &mut egui::Painter, pitch: f32, center: Pos2, radius: f32) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        let original_rect = painter.clip_rect();
        // The top edge carries the roll dial, so only the sides and the bottom are clipped back.
        painter.set_clip_rect(Rect::from_x_y_ranges(
            center.x - 0.8 * radius..=center.x + 0.8 * radius,
            original_rect.top()..=center.y + 0.8 * radius,
        ));

        let rotation = nalgebra::Rotation3::new(nalgebra::Vector3::x() * pitch);
        let pole = rotation * nalgebra::Vector3::z();
        let ahead_is_sky = pole.y < 0.0;

        // The horizon cuts the visible cap in two, and only the half around the point facing us is
        // convex, so that is the one we lay on top as a polygon. With the horizon out of view there
        // is nothing to lay on top and the whole ball is one colour.
        let horizon = Self::visible_ring(&rotation, 0.0);
        let ahead = if ahead_is_sky {
            COLOR_SKY
        } else {
            COLOR_GROUND
        };
        let behind = if ahead_is_sky {
            COLOR_GROUND
        } else {
            COLOR_SKY
        };
        painter.circle_filled(
            center,
            radius,
            if horizon.len() >= 2 { behind } else { ahead },
        );

        if let [first, .., last] = horizon.as_slice() {
            let limb_radius = (1.0 - 1.0 / (CAMERA * CAMERA)).sqrt();
            let angle_of = |v: &nalgebra::Vector3<f32>| v.z.atan2(v.x);

            // Close the horizon along whichever way round the limb stays on our side of it.
            let sweep = {
                let delta = (angle_of(first) - angle_of(last)).rem_euclid(TAU);
                let middle = angle_of(last) + delta / 2.0;
                let on_limb = nalgebra::Vector3::new(
                    middle.cos() * limb_radius,
                    1.0 / CAMERA,
                    middle.sin() * limb_radius,
                );
                if (on_limb.dot(&pole) < 0.0) == ahead_is_sky {
                    delta
                } else {
                    delta - TAU
                }
            };

            // The samples stop one step short of the limb, so nudge the ends out to meet it.
            let to_limb = |v: &nalgebra::Vector3<f32>| {
                center + (Self::project(*v, center, radius) - center).normalized() * radius
            };

            let steps = (sweep.abs() / TAU * N as f32).ceil().max(1.0);
            let mut region = vec![to_limb(first)];
            region.extend(horizon.iter().map(|v| Self::project(*v, center, radius)));
            region.push(to_limb(last));
            region.extend((1..steps as usize).map(|i| {
                let a = angle_of(last) + sweep * (i as f32) / steps;
                center + Vec2::new(a.cos(), a.sin()) * radius
            }));

            painter.add(Shape::convex_polygon(region, ahead, Stroke::NONE));
        }

        // Between the full lines, a short strip either side of the meridian facing us. Its length
        // on screen is fixed, so the angle it spans has to come from the scale where it sits.
        for pitch_tick in (-85i32..90).step_by(10) {
            let r = (pitch_tick as f32).to_radians();
            let front = rotation * nalgebra::Vector3::new(0.0, r.cos(), -r.sin());
            if front.y * CAMERA <= 1.0 {
                continue;
            }

            let scale = (CAMERA * CAMERA - 1.0).sqrt() / (CAMERA - front.y);
            let half = (0.06 / (scale * r.cos())).min(FRAC_PI_2);

            let strip: Vec<_> = (0..=4)
                .map(|i| {
                    let a = FRAC_PI_2 + half * (i as f32 / 2.0 - 1.0);
                    rotation
                        * nalgebra::Vector3::new(a.cos() * r.cos(), a.sin() * r.cos(), -r.sin())
                })
                .filter(|v| v.y * CAMERA > 1.0)
                .map(|v| Self::project(v, center, radius))
                .collect();

            painter.add(Shape::Path(PathShape::line(
                strip,
                Stroke::new(0.5_f32, Color32::WHITE),
            )));
        }

        for pitch_tick in (-90i32..=90).step_by(10) {
            let r = (pitch_tick as f32).to_radians();

            let visible = Self::visible_ring(&rotation, r);
            let projected: Vec<_> = visible
                .iter()
                .map(|v| Self::project(*v, center, radius))
                .collect();

            if pitch_tick.abs() == 90 {
                if let Some(pos) = projected.first() {
                    painter.circle_filled(*pos, 1.0, Color32::WHITE);
                }
                continue;
            }

            let stroke = Stroke::new(
                match pitch_tick {
                    0 => 2.0_f32,
                    tick if tick % 20 == 0 => 1.0,
                    _ => 0.5,
                },
                Color32::WHITE,
            );

            painter.add(Shape::Path(if visible.len() == N {
                PathShape::closed_line(projected, stroke)
            } else {
                PathShape::line(projected, stroke)
            }));

            if pitch_tick % 20 == 0 && (20..=60).contains(&pitch_tick.abs()) {
                let plate = if pitch_tick > 0 {
                    COLOR_SKY
                } else {
                    COLOR_GROUND
                };

                for side in [-1.0_f32, 1.0] {
                    // A fixed longitude either side of the meridian facing us, so each label keeps
                    // its own spot on the ball whatever the ball is doing.
                    let a = FRAC_PI_2 - side * 30.0_f32.to_radians();
                    let anchor = rotation
                        * nalgebra::Vector3::new(a.cos() * r.cos(), a.sin() * r.cos(), -r.sin());

                    Self::draw_surface_label(
                        painter,
                        format!("{pitch_tick}"),
                        anchor,
                        pole,
                        plate,
                        center,
                        radius,
                    );
                }
            }
        }

        // restore our original clip rect
        painter.set_clip_rect(original_rect);
    }

    /// The run of the latitude circle at `latitude` radians that faces the camera, ordered so that
    /// it is contiguous.
    fn visible_ring(
        rotation: &nalgebra::Rotation3<f32>,
        latitude: f32,
    ) -> Vec<nalgebra::Vector3<f32>> {
        let ring: Vec<_> = (0..N)
            .map(|i| {
                let a = TAU * (i as f32) / (N as f32);
                rotation
                    * nalgebra::Vector3::new(
                        a.cos() * latitude.cos(),
                        a.sin() * latitude.cos(),
                        -latitude.sin(),
                    )
            })
            .collect();

        // Starting at the point furthest from the camera keeps the facing ones in one run instead
        // of wrapping the end of the vector.
        let back = ring
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
            .map_or(0, |(i, _)| i);

        ring.iter()
            .cycle()
            .skip(back)
            .take(N)
            .filter(|v| v.y * CAMERA > 1.0)
            .copied()
            .collect()
    }

    /// Projects a point on the unit ball, putting its limb exactly on `radius`.
    fn project(v: nalgebra::Vector3<f32>, center: Pos2, radius: f32) -> Pos2 {
        let scale = radius * (CAMERA * CAMERA - 1.0).sqrt() / (CAMERA - v.y);
        center + Vec2::new(v.x, v.z) * scale
    }

    /// Draws `text` as if it were painted onto the ball's surface at `anchor`, a unit vector in
    /// view space (camera on +y, screen axes +x right and +z down). `pole` is the ball's axis.
    fn draw_surface_label(
        painter: &egui::Painter,
        text: String,
        anchor: nalgebra::Vector3<f32>,
        pole: nalgebra::Vector3<f32>,
        plate: Color32,
        center: Pos2,
        radius: f32,
    ) {
        const PLATE_STEPS: usize = 3;

        // How much the glyphs end up squashed: the surface turned away from us, against the
        // perspective scale that being closer than the limb buys back. Negative behind the limb.
        // Squashed much past this, bilinear minification smears them, and wgpu has no mipmaps.
        let facing = (CAMERA * anchor.y - 1.0) / (1.0 + CAMERA * (CAMERA - 2.0 * anchor.y)).sqrt();
        if facing * (CAMERA * CAMERA - 1.0).sqrt() / (CAMERA - anchor.y) < 0.4 {
            return;
        }

        // Orthonormal and aligned with the latitude line through `anchor`, so the text lands on
        // the sphere without shear and stays parallel to the pitch line it belongs to.
        let mut right = anchor.cross(&pole).normalize();
        let mut down = right.cross(&anchor);

        // Past the ball's axis that tangent points the other way, which would set the label
        // upside down; half a turn keeps it reading left to right.
        if right.x < 0.0 {
            right = -right;
            down = -down;
        }

        let galley = painter.layout_no_wrap(text, FontId::monospace(10.0), Color32::WHITE);
        let origin = galley.rect.center().to_vec2();
        let project = |local: Pos2| {
            let offset = local - origin;
            let p = (anchor + (right * offset.x + down * offset.y) / radius).normalize();
            Self::project(p, center, radius)
        };

        // The plate keeps the pitch lines from crossing the glyphs. Its edges bow with the surface,
        // so they need more than the four corners.
        let plate_rect = galley.rect.expand2(Vec2::new(5.0, 2.0));
        let corners = [
            plate_rect.left_top(),
            plate_rect.right_top(),
            plate_rect.right_bottom(),
            plate_rect.left_bottom(),
            plate_rect.left_top(),
        ];
        let mut outline = Vec::with_capacity(4 * PLATE_STEPS);
        for edge in corners.windows(2) {
            for step in 0..PLATE_STEPS {
                outline.push(project(
                    edge[0].lerp(edge[1], step as f32 / PLATE_STEPS as f32),
                ));
            }
        }
        painter.add(Shape::convex_polygon(outline, plate, Stroke::NONE));

        let [tex_width, tex_height] = painter.fonts(egui::epaint::Fonts::font_image_size);
        let uv_scale = Vec2::new(1.0 / tex_width as f32, 1.0 / tex_height as f32);

        let mut mesh = Mesh::with_texture(TextureId::default());
        for row in &galley.rows {
            let base = mesh.vertices.len() as u32;
            mesh.indices
                .extend(row.visuals.mesh.indices.iter().map(|i| i + base));
            mesh.vertices
                .extend(row.visuals.mesh.vertices.iter().map(|v| Vertex {
                    pos: project(row.pos + v.pos.to_vec2()),
                    // Galley UVs are in texels; a `TextShape` would have normalized them for us.
                    uv: (v.uv.to_vec2() * uv_scale).to_pos2(),
                    color: v.color,
                }));
        }
        painter.add(mesh);
    }

    /// The dial around the top edge of the ball. The ball itself does not roll, so the pointer is
    /// what moves, banking with the aircraft symbol.
    fn draw_roll_dial(&self, painter: &egui::Painter, roll: f32, center: Pos2, radius: f32) {
        const BORDER: f32 = 2.0;
        const TICK: f32 = 4.0;
        const POINTER: f32 = 6.5;

        let limit = (ROLL_LIMIT as f32).to_radians();
        let at = |angle: f32, out: f32| center + Vec2::new(angle.sin(), -angle.cos()) * out;

        let arc: Vec<_> = (0..=N)
            .map(|i| at(limit * (2.0 * i as f32 / N as f32 - 1.0), radius))
            .collect();
        painter.add(Shape::Path(PathShape::line(
            arc,
            Stroke::new(BORDER, Color32::WHITE),
        )));

        // Everything below hangs off the outside of the border, which leaves the ball itself clear
        // but has only the sliver above it to fit into.
        let outside = radius + BORDER * 0.5;

        for tick in (-ROLL_LIMIT..=ROLL_LIMIT).step_by(ROLL_STEP) {
            let major = tick == 0 || tick.abs() == ROLL_LIMIT;
            let angle = (tick as f32).to_radians();
            painter.line(
                vec![
                    at(angle, outside),
                    at(angle, outside + if major { TICK } else { TICK * 0.6 }),
                ],
                Stroke::new(if major { 1.5_f32 } else { 1.0 }, Color32::WHITE),
            );
        }

        let clamped = roll.clamp(-limit, limit);
        let out = Vec2::new(clamped.sin(), -clamped.cos());
        let along = Vec2::new(clamped.cos(), clamped.sin());
        let apex = center + out * (outside + TICK + 1.5);
        painter.add(Shape::convex_polygon(
            vec![
                apex,
                apex + out * POINTER + along * POINTER * 0.6,
                apex + out * POINTER - along * POINTER * 0.6,
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));

        // Past the end of the dial the pointer stops, so a second arrow has to carry the direction.
        if roll.abs() > limit {
            let side = roll.signum();
            let base = apex + out * (POINTER * 0.5) + along * side * (POINTER * 0.6 + 3.0);
            painter.add(Shape::convex_polygon(
                vec![
                    base + along * side * 5.0,
                    base + out * 3.0,
                    base - out * 3.0,
                ],
                Color32::WHITE,
                Stroke::NONE,
            ));
        }
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
                Stroke::new(3.0_f32, Color32::RED),
            ));
            painter.add(Shape::line(
                projected,
                Stroke::new(2.0_f32, Color32::ORANGE),
            ));
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
                    a => (format!("{a}"), 12.0, Color32::WHITE),
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

                // `with_angle_and_anchor` only moves the pivot, leaving `pos` the unrotated
                // placement corner, so subtract the anchor to place the label centre itself.
                let anchor = Align2::CENTER_CENTER.pos_in_rect(&galley.rect).to_vec2();
                let label = center + Vec2::new(r.sin(), -r.cos()) * (radius - anchor.y);

                painter.add(Shape::Text(
                    TextShape::new(label - anchor, galley, Color32::TRANSPARENT)
                        .with_angle_and_anchor(r, Align2::CENTER_CENTER),
                ));
            } else {
                painter.line(
                    vec![center.lerp(tip, 1.0 - 0.05), tip],
                    Stroke::new(1.0_f32, Color32::WHITE),
                );
            }
        }

        let indicator_pos = center - Vec2::new(0.0, radius - 32.0);

        painter.rect(
            Rect::from_center_size(indicator_pos, Vec2::new(56.0, 21.0)),
            CornerRadius::ZERO.at_least(1),
            Color32::BLACK,
            Stroke::new(0.5_f32, Color32::WHITE),
            egui::StrokeKind::Outside,
        );

        painter.text(
            indicator_pos,
            Align2::CENTER_CENTER,
            format!(
                "{:.1}",
                (((heading.to_degrees() * 10.0).round() as i32 + 3600) % 3600) as f32 / 10.0
            ),
            FontId::monospace(14.0),
            Color32::WHITE,
        );
    }

    fn draw_side_dial(
        &self,
        painter: &mut egui::Painter,
        value: f32,
        side: f32,
        throttle: Option<f32>,
        allow_negative: bool,
    ) {
        const POINTS_PER_UNIT: f32 = 4.0;
        const COLOR_DIAL: Color32 = Color32::from_rgb(0xc0, 0xc0, 0xc0);

        let rect = painter.clip_rect();
        let center_side = rect.center() + Vec2::new(rect.width() * 0.5 * side, 0.0);

        let min_tick = if allow_negative {
            ((value.min(0.0) - 200.0) as i32) / 100 * 100
        } else {
            0
        };
        let max_tick = ((value + 200.0) as i32) / 100 * 100 + 100;

        let mut tick = min_tick - (min_tick.rem_euclid(2));
        while tick <= max_tick {
            let abs_tick = tick.unsigned_abs();
            let len = if abs_tick % 100 == 0 {
                18.0
            } else if abs_tick % 10 == 0 {
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
                Stroke::new(
                    if abs_tick % 50 == 0 { 2.0_f32 } else { 1.0_f32 },
                    Color32::WHITE,
                ),
            );

            if abs_tick % 50 == 0 {
                painter.text(
                    center_side - Vec2::new((len + 5.0) * side, y),
                    if side > 0.0 {
                        Align2::RIGHT_CENTER
                    } else {
                        Align2::LEFT_CENTER
                    },
                    format!("{tick}"),
                    FontId::monospace(12.0),
                    Color32::WHITE,
                );
            }
            tick += 2;
        }

        if let Some(throttle) = throttle {
            painter.line(
                vec![
                    center_side,
                    center_side - Vec2::new(0.0, throttle * rect.height() / 2.0),
                ],
                Stroke::new(12.0_f32, Color32::RED),
            );
        }

        painter.line(
            vec![
                center_side + Vec2::new(0.0, value * POINTS_PER_UNIT),
                center_side - Vec2::new(0.0, rect.height() / 2.0),
            ],
            Stroke::new(4.0_f32, COLOR_DIAL),
        );

        let box_left = if side > 0.0 { -87.0 } else { 80.0 };
        painter.add(Shape::convex_polygon(
            vec![
                center_side + Vec2::new(box_left, -12.0),
                center_side + Vec2::new(-20.0 * side, -12.0),
                center_side + Vec2::new(-8.0 * side, 0.0),
                center_side + Vec2::new(-20.0 * side, 12.0),
                center_side + Vec2::new(box_left, 12.0),
            ],
            Color32::BLACK,
            Stroke::new(1.0_f32, Color32::WHITE),
        ));

        let text_x = if side > 0.0 { -22.0 } else { 76.0 };

        painter.text(
            center_side + Vec2::new(text_x - 5.0, 0.0),
            Align2::RIGHT_CENTER,
            format!("{value:.0}."),
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
        vfr_hud: Option<&VfrHud>,
    ) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        let altitude = match self.source {
            PositionSource::LocalPositionNed => local_position.map_or(0.0, |v| -v.z),
            PositionSource::VfrHud => vfr_hud.map_or(0.0, |v| v.alt),
        };
        self.draw_side_dial(painter, altitude, 1.0, None, false);
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
            .map(|v| f32::from(v.throttle) / 100.0)
            .unwrap_or_default();
        let velocity = match self.velocity_mode {
            VelocityMode::Speed => match self.source {
                PositionSource::LocalPositionNed => local_position
                    .map_or(0.0, |v| (v.vx.powi(2) + v.vy.powi(2) + v.vz.powi(2)).sqrt()),
                PositionSource::VfrHud => vfr_hud.map_or(0.0, |v| v.airspeed),
            },
            VelocityMode::Climb => match self.source {
                PositionSource::LocalPositionNed => local_position.map_or(0.0, |v| -v.vz),
                PositionSource::VfrHud => vfr_hud.map_or(0.0, |v| v.climb),
            },
        };
        let allow_negative = self.velocity_mode == VelocityMode::Climb;
        self.draw_side_dial(painter, velocity, -1.0, Some(throttle), allow_negative);
    }
}

impl egui::Widget for ArtificialHorizon {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let attitude = self.system.last_message::<Attitude>().ok();
        let pitch = attitude.as_ref().map_or(0.0, |a| a.pitch);
        let roll = attitude.as_ref().map_or(0.0, |a| a.roll);
        let yaw = attitude.as_ref().map_or(0.0, |a| a.yaw);

        let local_position = self.system.last_message::<LocalPositionNed>().ok();
        let vfr_hud = self.system.last_message::<VfrHud>().ok();

        let (response, mut painter) = ui.allocate_painter(ui.available_size(), Sense::empty());

        // We fill the width of the UI with our little ball.
        let radius = f32::min(response.rect.width(), response.rect.height()) * 0.4;
        let center = response.rect.center();

        let rect = painter.clip_rect();

        // draw our static sky and ground backgrounds and our central line
        painter.rect_filled(
            Rect::from_two_pos(rect.left_top(), rect.right_center()),
            CornerRadius::ZERO,
            COLOR_SKY.gamma_multiply(0.5),
        );
        painter.rect_filled(
            Rect::from_two_pos(rect.left_bottom(), rect.right_center()),
            CornerRadius::ZERO,
            COLOR_GROUND.gamma_multiply(0.5),
        );
        painter.line(
            vec![rect.left_center(), rect.right_center()],
            Stroke::new(3.0_f32, Color32::WHITE),
        );

        // draw our horizon and roll indicators
        self.draw_ball(&mut painter, pitch, center, radius);
        self.draw_roll_dial(&painter, roll, center, radius);
        self.draw_roll_indicator(&mut painter, roll, center, radius);

        // draw some darkened backgrounds on the sides for our velocity and altitude dials
        painter.rect_filled(
            Rect::from_two_pos(rect.right_top(), rect.right_bottom() - Vec2::new(20.0, 0.0)),
            CornerRadius::ZERO,
            Color32::BLACK.gamma_multiply(0.2),
        );
        painter.rect_filled(
            Rect::from_two_pos(rect.left_top(), rect.left_bottom() + Vec2::new(20.0, 0.0)),
            CornerRadius::ZERO,
            Color32::BLACK.gamma_multiply(0.2),
        );

        let compass_center = rect.center_bottom() + Vec2::new(0.0, radius + 20.0 - 40.0);
        painter.circle_filled(compass_center, radius + 20.0 + 5.0, Color32::BLACK);
        self.draw_compass(&mut painter, yaw, compass_center, radius + 20.0);

        self.draw_altitude_dial(&mut painter, local_position.as_ref(), vfr_hud.as_ref());
        self.draw_velocity_dial(&mut painter, local_position.as_ref(), vfr_hud.as_ref());

        response
    }
}
