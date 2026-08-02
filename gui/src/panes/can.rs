use core::System;
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;

use chrono::{DateTime, Local, Utc};
use eframe::egui;
use egui::{Button, DragValue, TextEdit, Vec2};
use egui_extras::{Column, TableBuilder};
use mavspec::rust::dialects::common::messages::CanFrame;

use crate::panes::PaneUi;

const ROW_HEIGHT: f32 = 20.0;

struct IdStats {
    count: u64,
    last_at: DateTime<Utc>,
    frame: CanFrame,
}

/// One system's frames, folded out of the store once and kept, so that a repaint only ever reads
/// the rows that arrived since the last one.
#[derive(Default)]
struct SystemState {
    last_seen: Option<DateTime<Utc>>,
    /// Stored message count this was last brought up to date with.
    counted: usize,
    /// Sorted by CAN id, as the grouped table draws them.
    ids: Vec<(u32, IdStats)>,
    trace: VecDeque<(DateTime<Utc>, CanFrame)>,
    /// Frames dropped off the front of `trace`, so the table can say what it leaves out.
    dropped: usize,
}

impl SystemState {
    /// How far the trace scrolls back: about a hundred seconds of a saturated 125 kbit/s bus.
    const TRACE_CAPACITY: usize = 100_000;

    /// Frames folded in per repaint, bounding a pane that is far behind: opened onto a session
    /// that has been running for hours, or onto a log still loading.
    const POLL_LIMIT: usize = 100_000;

    /// Folds in newly stored frames, returning whether more are still waiting.
    fn poll(&mut self, system: &System) -> bool {
        let total = system.message_count::<CanFrame>();
        if total == self.counted {
            return false;
        }
        self.counted = total;

        let new = system
            .messages_since::<CanFrame>(self.last_seen, Some(Self::POLL_LIMIT))
            .unwrap_or_default();
        let saturated = new.len() == Self::POLL_LIMIT;

        for (received_at, frame) in new {
            match self.ids.binary_search_by_key(&frame.id, |(id, _)| *id) {
                Ok(i) => {
                    let entry = &mut self.ids[i].1;
                    entry.count += 1;
                    entry.last_at = received_at;
                    entry.frame = frame.clone();
                }
                Err(i) => self.ids.insert(
                    i,
                    (
                        frame.id,
                        IdStats {
                            count: 1,
                            last_at: received_at,
                            frame: frame.clone(),
                        },
                    ),
                ),
            }

            if self.trace.len() == Self::TRACE_CAPACITY {
                self.trace.pop_front();
                self.dropped += 1;
            }
            self.trace.push_back((received_at, frame));
            self.last_seen = Some(received_at);
        }

        saturated
    }
}

pub struct CanProbePane {
    can_forwarding_enabled: bool,
    group_by_id: bool,
    id_to_send: u32,
    hex_to_send: String,
    /// Keyed by system, since the pane is redrawn for whichever system the active view names.
    state: HashMap<u8, SystemState>,
}

impl CanProbePane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            can_forwarding_enabled: false,
            group_by_id: false,
            id_to_send: 0x1ff,
            hex_to_send: String::new(),
            state: HashMap::new(),
        }
    }

    fn controls_ui(&mut self, ui: &mut egui::Ui, system: &System) {
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
            let edit_w = ui.available_width() - button_w - 2.0 * ui.style().spacing.item_spacing.x;
            ui.add_sized(
                Vec2::new(edit_w, h),
                TextEdit::singleline(&mut self.hex_to_send),
            );
            self.hex_to_send = self.hex_to_send.to_lowercase();

            if ui
                .add_sized(Vec2::new(button_w, h), Button::new("Send ➡"))
                .clicked()
            {
                self.send(system);
            }
        });
    }

    fn send(&self, system: &System) {
        if self.hex_to_send.len() > 16
            || !self.hex_to_send.len().is_multiple_of(2)
            || !self.hex_to_send.chars().all(|c| c.is_ascii_hexdigit())
        {
            tracing::warn!("hex string not a valid can message body");
            return;
        }

        let data: Vec<u8> = self
            .hex_to_send
            .as_bytes()
            .chunks(2)
            .map(|pair| {
                let hi = (pair[0] as char).to_digit(16).unwrap_or_default();
                let lo = (pair[1] as char).to_digit(16).unwrap_or_default();
                ((hi << 4) | lo) as u8
            })
            .collect();
        let mut buffer = [0x00; 8];
        buffer[..data.len()].copy_from_slice(&data);
        tracing::info!(
            "sending can message: id: {:x}, body: {:?}",
            &self.id_to_send,
            &data
        );
        system.send_message(&CanFrame {
            target_system: system.system_id,
            target_component: 0x01,
            bus: 1,
            id: self.id_to_send,
            len: data.len() as u8,
            data: buffer,
        });
    }

    fn format_time(at: DateTime<Utc>) -> String {
        at.with_timezone(&Local).format("%H:%M:%S%.3f").to_string()
    }

    // `len` is whatever the sender put in the message; slicing `data` with it would panic.
    fn format_data(frame: &CanFrame) -> String {
        let mut hex = String::with_capacity(24);
        for byte in &frame.data[..(frame.len as usize).min(frame.data.len())] {
            let _ = write!(hex, "{byte:02x} ");
        }
        hex
    }
}

impl PaneUi for CanProbePane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        let now = system.now();

        self.controls_ui(ui, &system);
        ui.separator();

        let state = self.state.entry(system.system_id).or_default();
        if state.poll(&system) {
            ui.ctx().request_repaint();
        }

        if state.dropped > 0 && !self.group_by_id {
            ui.weak(format!(
                "Showing the last {} of {} frames",
                state.trace.len(),
                state.dropped + state.trace.len()
            ));
        }

        let h = ui.available_height();
        let table = TableBuilder::new(ui)
            .striped(true)
            .max_scroll_height(h)
            .stick_to_bottom(!self.group_by_id)
            .auto_shrink(false);

        if self.group_by_id {
            table
                .column(Column::auto().at_least(120.0).resizable(true))
                .column(Column::auto().at_least(80.0).resizable(true))
                .column(Column::auto().at_least(80.0).resizable(true))
                .column(Column::remainder())
                .header(ROW_HEIGHT, |mut header| {
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
                .body(|body| {
                    body.rows(ROW_HEIGHT, state.ids.len(), |mut row| {
                        let (id, entry) = &state.ids[row.index()];
                        row.col(|ui| {
                            let elapsed_log = (now - entry.last_at).as_seconds_f32().log2();
                            let color = ui.visuals().text_color().lerp_to_gamma(
                                ui.visuals().weak_text_color(),
                                elapsed_log.clamp(0.0, 1.0),
                            );
                            ui.colored_label(color, Self::format_time(entry.last_at));
                        });
                        row.col(|ui| {
                            ui.label(format!("{}", entry.count));
                        });
                        row.col(|ui| {
                            ui.monospace(format!("{id:03x}"));
                        });
                        row.col(|ui| {
                            ui.monospace(Self::format_data(&entry.frame));
                        });
                    });
                });
        } else {
            table
                .column(Column::auto().at_least(120.0).resizable(true))
                .column(Column::auto().at_least(80.0).resizable(true))
                .column(Column::remainder())
                .header(ROW_HEIGHT, |mut header| {
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
                .body(|body| {
                    body.rows(ROW_HEIGHT, state.trace.len(), |mut row| {
                        let (received_at, frame) = &state.trace[row.index()];
                        row.col(|ui| {
                            ui.label(Self::format_time(*received_at));
                        });
                        row.col(|ui| {
                            ui.monospace(format!("{:03x}", frame.id));
                        });
                        row.col(|ui| {
                            ui.monospace(Self::format_data(frame));
                        });
                    });
                });
        }
    }
}
