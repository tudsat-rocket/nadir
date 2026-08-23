use nadir_core::{ParamProgress, System};

use egui::{Grid, Margin, Rect, Vec2};
use mavspec::rust::dialects::common::messages::ServoOutputRaw;

use crate::widgets::Dial;

#[allow(clippy::similar_names)]
pub fn draw_servos(ui: &mut egui::Ui, system: &System, square: Rect) {
    let Ok(servos) = system.last_message::<ServoOutputRaw>() else {
        return;
    };

    let params = system.params.lock().unwrap();
    let ParamProgress::Complete(ref params) = *params else {
        return;
    };

    let mins: Vec<_> = (1..=6)
        .map(|i| params.get(&format!("SERVO{i}_MIN")).map(|p| p.value))
        .collect();
    let maxs: Vec<_> = (1..=6)
        .map(|i| params.get(&format!("SERVO{i}_MAX")).map(|p| p.value))
        .collect();
    let trims: Vec<_> = (1..=6)
        .map(|i| params.get(&format!("SERVO{i}_TRIM")).map(|p| p.value))
        .collect();

    let motor_size = Vec2::new(130.0, 100.0);
    let servo_size = Vec2::new(190.0, 60.0);

    let motor1_rect = Rect::from_two_pos(
        square.shrink(20.0).left_top(),
        square.shrink(20.0).left_top() + motor_size,
    );

    let aileron_l_rect = Rect::from_two_pos(
        square.shrink(20.0).left_center(),
        square.shrink(20.0).left_center() + servo_size,
    );

    let rudder_l_rect = Rect::from_two_pos(
        aileron_l_rect.left_bottom() + Vec2::new(0.0, 30.0),
        aileron_l_rect.left_bottom() + Vec2::new(0.0, 30.0) + servo_size,
    );

    let rudder_r_rect = Rect::from_two_pos(
        square.shrink(20.0).right_center() + Vec2::new(-servo_size.x, servo_size.y + 30.0),
        square.shrink(20.0).right_center()
            + Vec2::new(-servo_size.x, servo_size.y + 30.0)
            + servo_size,
    );

    let all_servos = [
        servos.servo1_raw,
        servos.servo2_raw,
        servos.servo3_raw,
        servos.servo4_raw,
        servos.servo5_raw,
        servos.servo6_raw,
        servos.servo7_raw,
        servos.servo8_raw,
    ];

    ui.place(motor1_rect, |ui: &mut egui::Ui| {
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(Margin::same(5))
            .show(ui, |ui| {
                ui.add(Dial {
                    value: f32::from(servos.servo3_raw),
                    min: mins[2].map_or(1000.0, nadir_core::ParamVal::as_float),
                    max: maxs[2].map_or(2000.0, nadir_core::ParamVal::as_float),
                    absolute_min: 1000.0,
                    absolute_max: 2000.0,
                    trim: None,
                });

                Grid::new(ui.next_auto_id())
                    .num_columns(2)
                    .min_col_width(10.0)
                    .show(ui, |ui| {
                        ui.weak("#");
                        ui.monospace("3");
                        ui.end_row();

                        ui.weak("Fn");
                        ui.label("Throttle");
                        ui.end_row();
                    });
            })
            .response
    });

    for (i, rect, function) in [
        (0, aileron_l_rect, "Aileron"),
        (1, rudder_l_rect, "Elevator"),
        (3, rudder_r_rect, "Rudder"),
    ] {
        ui.place(rect, |ui: &mut egui::Ui| {
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(Margin::same(5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.add_space(5.0);
                            ui.add_sized(
                                Vec2::new(80.0, 50.0),
                                Dial {
                                    value: f32::from(all_servos[i]),
                                    min: mins[i].map_or(1000.0, nadir_core::ParamVal::as_float),
                                    max: maxs[i].map_or(2000.0, nadir_core::ParamVal::as_float),
                                    absolute_min: 1000.0,
                                    absolute_max: 2000.0,
                                    trim: trims[i].map(nadir_core::ParamVal::as_float),
                                },
                            );
                        });

                        ui.vertical(|ui| {
                            Grid::new(ui.next_auto_id())
                                .num_columns(2)
                                .min_col_width(10.0)
                                .show(ui, |ui| {
                                    ui.weak("#");
                                    ui.monospace(format!("{}", i + 1));
                                    ui.end_row();

                                    ui.weak("Fn");
                                    ui.label(function);
                                    ui.end_row();

                                    ui.weak("Rev");
                                    ui.label("No"); // TODO
                                    ui.end_row();
                                });
                        });
                    });
                })
                .response
        });
    }
}
