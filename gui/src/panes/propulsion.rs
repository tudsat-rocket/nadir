use core::{ParamProgress, System};

use egui::{Button, Color32, DragValue, Frame, Grid, Image, Margin, Pos2, Rect, Stroke, Vec2};
use mavspec::rust::dialects::common::enums::MavType;
use mavspec::rust::dialects::common::messages::{
    BatteryStatus, Heartbeat, ServoOutputRaw, SysStatus,
};
use mavspec::rust::dialects::minimal::enums::MavAutopilot;

use crate::panes::PaneUi;
use crate::widgets::{BatteryIndicator, Dial};

pub struct PropulsionPane {
    pub motor_id: u32,
    pub motor_test_throttle: f32,
    pub servo_id: u32,
    pub servo_pulse_width: u32,
    pub servo_cycles: u32,
    pub servo_cycle_time: u32,
}

impl PropulsionPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            motor_id: 1,
            motor_test_throttle: 10.0,
            servo_id: 1,
            servo_pulse_width: 1500,
            servo_cycles: 3,
            servo_cycle_time: 500,
        }
    }

    #[allow(clippy::similar_names)]
    fn draw_arduplane_servos(&mut self, ui: &mut egui::Ui, system: &System, square: Rect) {
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
                        min: mins[2].unwrap_or(1000.0),
                        max: maxs[2].unwrap_or(2000.0),
                        absolute_min: 1000.0,
                        absolute_max: 2000.0,
                        trim: None,
                    });

                    egui::Grid::new(ui.next_auto_id())
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
                                        min: mins[i].unwrap_or(1000.0),
                                        max: maxs[i].unwrap_or(2000.0),
                                        absolute_min: 1000.0,
                                        absolute_max: 2000.0,
                                        trim: trims[i],
                                    },
                                );
                            });

                            ui.vertical(|ui| {
                                egui::Grid::new(ui.next_auto_id())
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

    fn draw_px4_rotors(&mut self, ui: &mut egui::Ui, system: &System, square: Rect) {
        let n = square.width();

        let Ok(servos) = system.last_message::<ServoOutputRaw>() else {
            return;
        };

        let params = system.params.lock().unwrap();
        let ParamProgress::Complete(ref params) = *params else {
            return;
        };

        let Some(count_param) = params.get("CA_ROTOR_COUNT") else {
            return;
        };

        let count = i32::from_be_bytes(count_param.value.to_be_bytes());

        let Some(px): Option<Vec<f32>> = (0..count)
            .map(|i| params.get(&format!("CA_ROTOR{i}_PX")).map(|p| p.value))
            .collect()
        else {
            return;
        };

        let Some(py): Option<Vec<f32>> = (0..count)
            .map(|i| params.get(&format!("CA_ROTOR{i}_PY")).map(|p| p.value))
            .collect()
        else {
            return;
        };

        // TODO
        let mins = [1000.0; 8];
        let maxs = [2000.0; 8];

        let max = px
            .iter()
            .chain(py.iter())
            .map(|f| f.abs())
            .fold(0.0, f32::max);

        for (i, (x, y)) in px.iter().zip(py.iter()).enumerate() {
            let pos = square.center() + Vec2::new(*y, *x * -1.0) * 0.35 * n / max;
            let rect = Rect::from_center_size(pos, Vec2::new(120.0, 120.0));
            // TODO: exact motor mapping, more motors

            let vector = pos - square.center();
            let normal = Vec2::new(vector.y, -vector.x).normalized();
            ui.painter().line(
                vec![square.center() + normal * 5.0, pos + normal * 5.0],
                Stroke::new(1.0, ui.visuals().weak_text_color()),
            );
            ui.painter().line(
                vec![square.center() - normal * 5.0, pos - normal * 5.0],
                Stroke::new(1.0, ui.visuals().weak_text_color()),
            );

            let all_servos = [
                servos.servo1_raw,
                servos.servo2_raw,
                servos.servo3_raw,
                servos.servo4_raw,
            ];

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

                            egui::Grid::new(ui.next_auto_id())
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

    fn draw_arducopter_rotors(&mut self, ui: &mut egui::Ui, system: &System, square: Rect) {
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

        let positions = match (frame_class.value, frame_type.value) {
            // Quad Plus & X
            (1.0, 0.0) => vec![
                Vec2::new(1.0, 0.0),
                Vec2::new(-1.0, 0.0),
                Vec2::new(0.0, -1.0),
                Vec2::new(0.0, 1.0),
            ],
            (1.0, 1.0) => vec![
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
                Stroke::new(1.0, ui.visuals().weak_text_color()),
            );
            ui.painter().line(
                vec![square.center() - normal * 5.0, pos - normal * 5.0],
                Stroke::new(1.0, ui.visuals().weak_text_color()),
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

                            egui::Grid::new(ui.next_auto_id())
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

    fn draw_battery(&mut self, ui: &mut egui::Ui, system: &System, pos: Pos2) {
        let battery_rect = Rect::from_center_size(pos, Vec2::new(60.0, 120.0));

        // TODO: properly handle multiple batteries / different instance IDs
        if let Ok(battery) = system.last_message::<BatteryStatus>() {
            let voltage = battery
                .voltages
                .iter()
                .filter(|v| **v > 0 && **v < u16::MAX)
                .map(|v| f32::from(*v) / 1000.0)
                .next_back();

            ui.place(
                battery_rect,
                BatteryIndicator {
                    id: battery.id,
                    soc: f32::from(battery.battery_remaining) / 100.0,
                    voltage,
                    current: (battery.current_battery != -1)
                        .then_some(f32::from(battery.current_battery) / 1000.0),
                    consumed: (battery.current_consumed != -1)
                        .then_some(battery.current_consumed as f32),
                },
            );
        } else if let Ok(status) = system.last_message::<SysStatus>() {
            ui.place(
                battery_rect,
                BatteryIndicator {
                    id: 0,
                    soc: f32::from(status.battery_remaining) / 100.0,
                    voltage: Some(f32::from(status.voltage_battery) / 1000.0),
                    current: Some(f32::from(status.current_battery) / 100.0),
                    consumed: None,
                },
            );
        }
    }

    fn draw_frame(&mut self, ui: &mut egui::Ui, system: &System, square: Rect) {
        let n = square.width();

        let Ok(heartbeat) = system.last_message::<Heartbeat>() else {
            return;
        };

        Frame::dark_canvas(ui.style()).show(ui, |ui| {
            ui.set_width(n);
            ui.set_height(n);

            // TODO: extend support
            match (heartbeat.autopilot, heartbeat.type_) {
                (MavAutopilot::Px4, _) => {
                    self.draw_px4_rotors(ui, system, square);
                    self.draw_battery(ui, system, square.center());
                }
                (MavAutopilot::Ardupilotmega, MavType::FixedWing) => {
                    let outline =
                        egui::include_image!("../../assets/vehicles/plane_twin_vtail_dark.svg");
                    ui.place(
                        square.shrink(n * 0.05),
                        Image::new(outline)
                            .maintain_aspect_ratio(true)
                            .max_width(n)
                            .tint(Color32::WHITE.gamma_multiply(0.5)),
                    );

                    self.draw_arduplane_servos(ui, system, square);
                    self.draw_battery(ui, system, square.center().lerp(square.center_top(), 0.5));
                }
                (MavAutopilot::Ardupilotmega, _) => {
                    self.draw_arducopter_rotors(ui, system, square);
                    self.draw_battery(ui, system, square.center());
                }
                _ => {}
            }
        });
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui, _system: &System, _square: Rect) {
        let first_col_width = 20.0;
        let c = f32::min(
            f32::max(ui.available_width() - first_col_width, first_col_width),
            80.0,
        );

        ui.vertical(|ui| {
            ui.weak("⚙ Motor Test");
            ui.add_space(5.0);

            Grid::new(ui.next_auto_id())
                .num_columns(2)
                .min_col_width(first_col_width)
                .spacing(Vec2::new(0.0, ui.spacing().item_spacing.y))
                .show(ui, |ui| {
                    ui.weak("＃");
                    ui.add_sized(Vec2::new(c, 20.0), DragValue::new(&mut self.motor_id));
                    ui.end_row();

                    ui.weak("⏩");
                    ui.add_sized(
                        Vec2::new(c, 20.0),
                        DragValue::new(&mut self.motor_test_throttle).suffix("%"),
                    );
                    ui.end_row();

                    ui.horizontal(|_ui| {});
                    ui.add_sized(Vec2::new(c, 20.0), Button::new("Run"));
                    ui.end_row();
                });
        });

        ui.add_space(5.0);
        ui.separator();
        ui.add_space(5.0);

        ui.vertical(|ui| {
            ui.weak("⟳ Set Servo");
            ui.add_space(5.0);

            Grid::new(ui.next_auto_id())
                .num_columns(2)
                .min_col_width(first_col_width)
                .spacing(Vec2::new(0.0, ui.spacing().item_spacing.y))
                .show(ui, |ui| {
                    ui.weak("＃");
                    ui.add_sized(Vec2::new(c, 20.0), DragValue::new(&mut self.servo_id));
                    ui.end_row();

                    ui.weak("🏁");
                    ui.add_sized(
                        Vec2::new(c, 20.0),
                        DragValue::new(&mut self.servo_pulse_width).suffix("µs"),
                    );
                    ui.end_row();

                    ui.horizontal(|_ui| {});
                    ui.add_sized(Vec2::new(c, 20.0), Button::new("Set"));
                    ui.end_row();
                });
        });

        ui.add_space(5.0);
        ui.separator();
        ui.add_space(5.0);

        ui.vertical(|ui| {
            ui.weak("🔃 Wiggle Servo");
            ui.add_space(5.0);

            Grid::new(ui.next_auto_id())
                .num_columns(2)
                .min_col_width(first_col_width)
                .spacing(Vec2::new(0.0, ui.spacing().item_spacing.y))
                .show(ui, |ui| {
                    ui.weak("❌");
                    ui.add_sized(Vec2::new(c, 20.0), DragValue::new(&mut self.servo_cycles));
                    ui.end_row();

                    ui.weak("⏱");
                    ui.add_sized(
                        Vec2::new(c, 20.0),
                        DragValue::new(&mut self.servo_cycle_time).suffix("ms"),
                    );
                    ui.end_row();

                    ui.horizontal(|_ui| {});
                    ui.add_sized(Vec2::new(c, 20.0), Button::new("Wiggle"));
                    ui.end_row();
                });
        });
    }
}

impl PaneUi for PropulsionPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        let Ok(heartbeat) = system.last_message::<Heartbeat>() else {
            return;
        };

        // TODO: extend support to other types, firmwares, code organization
        if heartbeat.autopilot != MavAutopilot::Px4
            && heartbeat.autopilot != MavAutopilot::Ardupilotmega
        {
            ui.centered_and_justified(|ui| {
                ui.weak("No propulsion information available.");
            });
            return;
        }

        let rect = ui.clip_rect();
        let n = f32::min(rect.width(), rect.height());

        if rect.width() > rect.height() {
            let square = Rect::from_two_pos(rect.left_top(), rect.left_top() + Vec2::splat(n));

            ui.horizontal(|ui| {
                self.draw_frame(ui, &system, square);

                ui.vertical(|ui| {
                    ui.set_width(f32::max(10.0, ui.available_width()));
                    self.draw_controls(ui, &system, square);
                });
            });
        } else {
            let mut square = rect.shrink2(Vec2::new((rect.width() - n) / 2.0, 0.0));
            square.set_top(rect.top());
            square.set_bottom(rect.top() + n);

            ui.vertical_centered(|ui| {
                self.draw_frame(ui, &system, square);

                ui.horizontal(|ui| {
                    ui.set_height(f32::max(10.0, ui.available_height()));
                    self.draw_controls(ui, &system, square);
                });
            });
        }
    }
}
