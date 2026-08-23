use nadir_core::{ParamProgress, System};

use egui::{Grid, Margin, Rect, Stroke, Vec2};
use mavspec::rust::dialects::common::messages::ServoOutputRaw;

use crate::widgets::Dial;

pub fn draw_rotors(ui: &mut egui::Ui, system: &System, square: Rect) {
    let n = square.width();

    let Ok(servos) = system.last_message::<ServoOutputRaw>() else {
        return;
    };

    let params = system.params.lock().unwrap();
    let ParamProgress::Complete(ref params) = *params else {
        return;
    };

    let Some(frame_class) = params.get("FRAME_CLASS") else {
        return;
    };

    let Some(frame_type) = params.get("FRAME_TYPE") else {
        return;
    };

    let positions = match (
        frame_class.value.as_unsigned_int(),
        frame_type.value.as_unsigned_int(),
    ) {
        // Quad Plus & X
        (1, 0) => vec![
            Vec2::new(1.0, 0.0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, -1.0),
            Vec2::new(0.0, 1.0),
        ],
        (1, 1) => vec![
            Vec2::new(0.95, -0.95),
            Vec2::new(-0.95, 0.95),
            Vec2::new(-0.95, -0.95),
            Vec2::new(0.95, 0.95),
        ],
        _ => {
            return;
        }
    };

    // TODO
    let mins = [1000.0; 8];
    let maxs = [2000.0; 8];

    for (i, pos_normalized) in positions.iter().enumerate() {
        let pos = square.center() + *pos_normalized * 0.30 * n;

        let vector = pos - square.center();
        let normal = Vec2::new(vector.y, -vector.x).normalized();
        ui.painter().line(
            vec![square.center() + normal * 5.0, pos + normal * 5.0],
            Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
        );
        ui.painter().line(
            vec![square.center() - normal * 5.0, pos - normal * 5.0],
            Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
        );

        let all_servos = [
            servos.servo1_raw,
            servos.servo2_raw,
            servos.servo3_raw,
            servos.servo4_raw,
        ];

        let rect = Rect::from_center_size(pos, Vec2::new(120.0, 120.0));
        ui.place(rect, |ui: &mut egui::Ui| {
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(Margin::same(5))
                .show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ui.set_height(rect.height() - 10.0);

                        ui.add_space(10.0);
                        ui.add_sized(
                            Vec2::new(100.0, 50.0),
                            Dial {
                                value: f32::from(all_servos[i]),
                                min: mins[i],
                                max: maxs[i],
                                absolute_min: 1000.0,
                                absolute_max: 2000.0,
                                trim: None,
                            },
                        );

                        Grid::new(ui.next_auto_id())
                            .num_columns(2)
                            .min_col_width(10.0)
                            .show(ui, |ui| {
                                ui.weak("#");
                                ui.monospace(format!("{}", i + 1));
                                ui.end_row();

                                ui.weak("Dir");
                                // TODO
                                if i >= 2 {
                                    ui.label("🔃CW");
                                } else {
                                    ui.label("🔄CCW");
                                }
                                ui.end_row();
                            });
                    });
                })
                .response
        });
    }
}
