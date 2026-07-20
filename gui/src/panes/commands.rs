use core::System;
use std::f32;

use chrono::{DateTime, Local, Utc};
use convert_case::{Case, Casing as _};

use eframe::egui;
use egui::{Align, DragValue, Layout, RichText, TextStyle};
use egui_extras::{Column, TableBuilder};
// rapid's MAV_CMD/MAV_FRAME enums are supersets of common's (they inherit common's entries
// and add rapid-specific ones, e.g. the valve commands), so decoding/sending through these
// types covers both common and rapid-dialect commands rather than only common's.
// TODO: properly handle an arbitrary number of supersets of common
use rapid_dialect::rapid::{
    enums::{MavCmd, MavFrame},
    messages::{CommandInt, CommandLong},
};

use crate::panes::PaneUi;

#[derive(PartialEq)]
enum CommandType {
    Long,
    Int,
}

pub struct CommandsPane {
    cmd_type: CommandType,
    cmd: MavCmd,
    frame: MavFrame,
    param1: Option<f32>,
    param2: Option<f32>,
    param3: Option<f32>,
    param4: Option<f32>,
    param5: Option<u32>,
    param6: Option<u32>,
    param7: Option<f32>,
}

impl CommandsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            cmd_type: CommandType::Long,
            cmd: MavCmd::PreflightRebootShutdown,
            frame: MavFrame::GlobalRelativeAltInt,
            param1: Some(1.0),
            param2: None,
            param3: None,
            param4: Some(1.0),
            param5: None,
            param6: Some(0),
            param7: None,
        }
    }

    fn send(&self, system: &System) {
        match self.cmd_type {
            CommandType::Long => {
                let cmd = CommandLong {
                    target_system: system.system_id,
                    target_component: 0x01,
                    command: self.cmd,
                    confirmation: 0,
                    param1: self.param1.unwrap_or(f32::NAN),
                    param2: self.param2.unwrap_or(f32::NAN),
                    param3: self.param3.unwrap_or(f32::NAN),
                    param4: self.param4.unwrap_or(f32::NAN),
                    param5: self.param5.map_or(f32::NAN, f32::from_bits),
                    param6: self.param6.map_or(f32::NAN, f32::from_bits),
                    param7: self.param7.unwrap_or(f32::NAN),
                };
                system.send_message(&cmd);
            }
            CommandType::Int => {
                let cmd = CommandInt {
                    target_system: system.system_id,
                    target_component: 0x01,
                    frame: self.frame,
                    command: self.cmd,
                    current: 0,
                    autocontinue: 0,
                    param1: self.param1.unwrap_or(f32::NAN),
                    param2: self.param2.unwrap_or(f32::NAN),
                    param3: self.param3.unwrap_or(f32::NAN),
                    param4: self.param4.unwrap_or(f32::NAN),
                    x: self.param5.map_or(i32::MAX, |p| p as i32),
                    y: self.param6.map_or(i32::MAX, |p| p as i32),
                    z: self.param7.unwrap_or(f32::NAN),
                };
                system.send_message(&cmd);
            }
        }
    }
}

enum Command {
    Long(CommandLong),
    Int(CommandInt),
}

impl Command {
    fn cmd(&self) -> MavCmd {
        match self {
            Self::Long(inner) => inner.command,
            Self::Int(inner) => inner.command,
        }
    }
}

fn format_cmd(cmd: MavCmd) -> String {
    let uppercamel = format!("{cmd:?}");
    uppercamel.to_case(Case::Constant)
}

fn format_frame(frame: MavFrame) -> String {
    let uppercamel = format!("{frame:?}");
    uppercamel.to_case(Case::Constant)
}

impl PaneUi for CommandsPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        for style in [TextStyle::Button, TextStyle::Body, TextStyle::Monospace] {
            ui.style_mut().text_styles.get_mut(&style).unwrap().size = 12.0;
        }

        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.cmd_type, CommandType::Long, "LONG");
            ui.selectable_value(&mut self.cmd_type, CommandType::Int, "INT");

            egui::ComboBox::from_label("")
                .selected_text(format_cmd(self.cmd))
                .show_ui(ui, |ui| {
                    let mut entries: Vec<_> = MavCmd::entries().collect();
                    entries.sort_by_key(|e| format!("{e:?}"));

                    for value in entries {
                        ui.selectable_value(&mut self.cmd, value, format_cmd(value));
                    }
                });

            ui.add_enabled_ui(self.cmd_type == CommandType::Int, |ui| {
                egui::ComboBox::from_label("")
                    .selected_text(format_frame(self.frame))
                    .show_ui(ui, |ui| {
                        let mut entries: Vec<_> = MavFrame::entries().collect();
                        entries.sort_by_key(|e| format!("{e:?}"));

                        for value in entries {
                            ui.selectable_value(&mut self.frame, value, format_frame(value));
                        }
                    });
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Send ➡").clicked() {
                    self.send(&system);
                }
            });
        });

        ui.horizontal(|ui| {
            for (name, p) in [
                ("1", &mut self.param1),
                ("2", &mut self.param2),
                ("3", &mut self.param3),
                ("4", &mut self.param4),
            ] {
                ui.weak(name);

                let mut enabled = p.is_some();
                ui.checkbox(&mut enabled, "");
                if let Some(v) = p {
                    ui.add(DragValue::new(v));
                }

                if p.is_some() && !enabled {
                    *p = None;
                } else if p.is_none() && enabled {
                    *p = Some(0.0);
                }
            }

            for (name, p) in [("5", &mut self.param5), ("6", &mut self.param6)] {
                ui.weak(name);

                let mut enabled = p.is_some();
                ui.checkbox(&mut enabled, "");

                if let Some(v) = p {
                    if self.cmd_type == CommandType::Int {
                        let mut i = *v as i32;
                        ui.add(DragValue::new(&mut i));
                        *v = i as u32;
                    } else {
                        let mut f = f32::from_bits(*v);
                        ui.add(DragValue::new(&mut f));
                        *v = f.to_bits();
                    }
                }

                if p.is_some() && !enabled {
                    *p = None;
                } else if p.is_none() && enabled {
                    *p = Some(0);
                }
            }

            ui.weak("7");
            let mut enabled = self.param7.is_some();
            ui.checkbox(&mut enabled, "");
            if let Some(mut v) = self.param7 {
                ui.add(DragValue::new(&mut v));
            }

            if self.param7.is_some() && !enabled {
                self.param7 = None;
            } else if self.param7.is_none() && enabled {
                self.param7 = Some(0.0);
            }
        });

        ui.separator();

        let command_longs = system.all_messages::<CommandLong>().unwrap_or_default();
        let command_ints = system.all_messages::<CommandInt>().unwrap_or_default();

        let mut commands: Vec<(DateTime<Utc>, Command)> = command_longs
            .into_iter()
            .map(|(t, c)| (t, Command::Long(c)))
            .chain(command_ints.into_iter().map(|(t, c)| (t, Command::Int(c))))
            .collect();

        commands.sort_by_key(|(t, _c)| t.timestamp_micros());

        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::remainder())
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .header(14.0, |mut header| {
                header.col(|ui| {
                    ui.weak("Sent At");
                });
                header.col(|ui| {
                    ui.weak("T/Fr.");
                });
                header.col(|ui| {
                    ui.weak("CMD");
                });
                header.col(|ui| {
                    ui.weak("P1");
                });
                header.col(|ui| {
                    ui.weak("P2");
                });
                header.col(|ui| {
                    ui.weak("P3");
                });
                header.col(|ui| {
                    ui.weak("P4");
                });
                header.col(|ui| {
                    ui.weak("P5/x");
                });
                header.col(|ui| {
                    ui.weak("P6/y");
                });
                header.col(|ui| {
                    ui.weak("P7/z");
                });
                header.col(|ui| {
                    ui.weak("Result");
                });
                header.col(|ui| {
                    ui.weak("At");
                });
            })
            .body(|mut body| {
                for (t, cmd) in commands.into_iter().rev() {
                    body.row(14.0, |mut row| {
                        row.col(|ui| {
                            let elapsed = Utc::now() - t;
                            let elapsed_log = elapsed.as_seconds_f32().log2();
                            let color = ui.visuals().strong_text_color().lerp_to_gamma(
                                ui.visuals().weak_text_color(),
                                ((elapsed_log + 4.0) / 5.0).clamp(0.0, 1.0),
                            );
                            ui.colored_label(
                                color,
                                t.with_timezone(&Local).format("%H:%M:%S").to_string(),
                            );
                        });

                        match &cmd {
                            Command::Long(_c) => {
                                row.col(|ui| {
                                    ui.label("L");
                                });
                            }
                            Command::Int(c) => {
                                row.col(|ui| {
                                    ui.label(format!(
                                        "I:{}",
                                        match c.frame {
                                            MavFrame::Global => "G",
                                            MavFrame::LocalNed => "LN",
                                            MavFrame::Mission => "MI",
                                            MavFrame::GlobalRelativeAlt => "GR",
                                            MavFrame::LocalEnu => "LE",
                                            MavFrame::GlobalInt => "GI",
                                            MavFrame::GlobalRelativeAltInt => "GRI",
                                            MavFrame::LocalOffsetNed => "LON",
                                            MavFrame::BodyNed => "BN",
                                            MavFrame::BodyOffsetNed => "BON",
                                            MavFrame::GlobalTerrainAlt => "GT",
                                            MavFrame::GlobalTerrainAltInt => "GTI",
                                            MavFrame::BodyFrd => "BF",
                                            MavFrame::LocalFrd => "LFR",
                                            MavFrame::LocalFlu => "LFL",
                                            _ => "?",
                                        }
                                    ));
                                });
                            }
                        }

                        row.col(|ui| {
                            ui.monospace(RichText::new(format_cmd(cmd.cmd())).small());
                        });

                        match cmd {
                            Command::Long(c) => {
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param1));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param2));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param3));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param4));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param5));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param6));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param7));
                                });
                            }
                            Command::Int(c) => {
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param1));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param2));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param3));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.param4));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.x));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.y));
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{}", c.z));
                                });
                            }
                        }

                        // TODO: acknowledgment status
                        row.col(|_ui| {
                            //ui.label("Failed");
                        });
                        row.col(|_ui| {
                            //ui.label("16:10:33");
                        });
                    });
                }
            });
    }
}
