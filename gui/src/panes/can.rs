use chrono::{Local, Utc};
use eframe::egui;
use egui::{Button, DragValue, TextEdit, Vec2};
use egui_extras::{Column, TableBuilder};
use mavspec::rust::dialects::common::messages::CanFrame;

use crate::{panes::TreeBehavior, views::View};

pub struct CanProbePane {
    can_forwarding_enabled: bool,
    group_by_id: bool,
    id_to_send: u32,
    hex_to_send: String,
}

impl CanProbePane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            can_forwarding_enabled: false,
            group_by_id: false,
            id_to_send: 0x1ff,
            hex_to_send: String::new(),
        }
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let h = ui.available_height();
        ui.set_width(ui.available_width());
        ui.set_height(ui.available_height());

        ui.horizontal(|ui| {
            ui.add_space(5.0);
            ui.set_height(h);

            ui.vertical(|ui| {
                ui.add_space(5.0);
                ui.set_height(h);

                ui.horizontal(|ui| {
                    let enabled = self.can_forwarding_enabled;
                    ui.checkbox(&mut self.can_forwarding_enabled, "Enable CAN Forwarding");
                    if enabled != self.can_forwarding_enabled {
                        system.request_can_forwarding(self.can_forwarding_enabled);
                    }
                    ui.checkbox(&mut self.group_by_id, "Group Messages by ID");
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.weak("ID");
                    ui.add(DragValue::new(&mut self.id_to_send).hexadecimal(3, true, false));
                    ui.weak("Data");
                    let h = ui.available_height();
                    let button_w = 80.0;
                    let edit_w =
                        ui.available_width() - button_w - 2.0 * ui.style().spacing.item_spacing.x;
                    ui.add_sized(
                        Vec2::new(edit_w, h),
                        TextEdit::singleline(&mut self.hex_to_send),
                    );
                    self.hex_to_send = self.hex_to_send.to_lowercase();

                    if ui
                        .add_sized(Vec2::new(button_w, h), Button::new("Send ➡"))
                        .clicked()
                    {
                        let data: Vec<u8> = vec![0x00, 0x01];
                        let mut buffer = [0x00; 8];
                        buffer[..data.len()].copy_from_slice(&data);
                        system.send_message(&CanFrame {
                            target_system: system_id,
                            target_component: 0x01,
                            bus: 1,
                            id: self.id_to_send,
                            len: data.len() as u8,
                            data: buffer,
                        });
                    }
                });

                ui.separator();

                let mut frames = behavior
                    .core
                    .db
                    .can_frames_for_system((system_id, 0x01))
                    .unwrap();

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .max_scroll_height(h)
                    .stick_to_bottom(true)
                    .auto_shrink(false);

                if self.group_by_id {
                    frames.sort_by_key(|(_t, f)| f.id);
                    let chunks = frames.chunk_by(|(_t1, f1), (_t2, f2)| f1.id == f2.id);

                    table
                        .column(Column::auto().at_least(120.0).resizable(true))
                        .column(Column::auto().at_least(80.0).resizable(true))
                        .column(Column::auto().at_least(80.0).resizable(true))
                        .column(Column::remainder())
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.weak("Last");
                            });
                            header.col(|ui| {
                                ui.weak("Count");
                            });
                            header.col(|ui| {
                                ui.weak("ID");
                            });
                            header.col(|ui| {
                                ui.weak("Data");
                            });
                        })
                        .body(|mut body| {
                            for chunk in chunks {
                                let last = chunk.last().unwrap();
                                let elapsed = Utc::now() - last.0;
                                let elapsed_log = elapsed.as_seconds_f32().log2();
                                body.row(20.0, |mut row| {
                                    row.col(|ui| {
                                        let color = ui.visuals().text_color().lerp_to_gamma(
                                            ui.visuals().weak_text_color(),
                                            f32::min(elapsed_log, 1.0),
                                        );
                                        ui.colored_label(
                                            color,
                                            last.0
                                                .with_timezone(&Local)
                                                .format("%H:%M:%S%.3f")
                                                .to_string(),
                                        );
                                    });
                                    row.col(|ui| {
                                        ui.label(format!("{}", chunk.len()));
                                    });
                                    row.col(|ui| {
                                        ui.monospace(format!("{:03x}", last.1.id));
                                    });
                                    row.col(|ui| {
                                        let data = &last.1.data[..(last.1.len as usize)];
                                        let hex: String =
                                            data.iter().map(|b| format!("{b:02x} ")).collect();
                                        ui.monospace(hex);
                                    });
                                });
                            }
                        });
                } else {
                    table
                        .column(Column::auto().at_least(120.0).resizable(true))
                        .column(Column::auto().at_least(80.0).resizable(true))
                        .column(Column::remainder())
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.weak("Received");
                            });
                            header.col(|ui| {
                                ui.weak("ID");
                            });
                            header.col(|ui| {
                                ui.weak("Data");
                            });
                        })
                        .body(|mut body| {
                            for (received_at, frame) in frames {
                                body.row(20.0, |mut row| {
                                    row.col(|ui| {
                                        ui.label(
                                            received_at
                                                .with_timezone(&Local)
                                                .format("%H:%M:%S%.3f")
                                                .to_string(),
                                        );
                                    });
                                    row.col(|ui| {
                                        ui.monospace(format!("{:03x}", frame.id));
                                    });
                                    row.col(|ui| {
                                        let data = &frame.data[..(frame.len as usize)];
                                        let hex: String =
                                            data.iter().map(|b| format!("{b:02x} ")).collect();
                                        ui.monospace(hex);
                                    });
                                });
                            }
                        });
                }
            });
        });
    }
}
