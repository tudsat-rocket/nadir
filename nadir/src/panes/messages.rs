use nadir_core::{MessageInstance, MessageSummary, System, format_message_label};

use chrono::{DateTime, Local, Utc};
use eframe::egui;
use egui::{Align, Layout, RichText, TextStyle};
use egui_extras::{Column, TableBuilder};

use crate::panes::PaneUi;
use crate::widgets::Readout;

#[derive(Clone, PartialEq, Eq)]
struct SelectedMessage {
    name: String,
    instance: Option<MessageInstance>,
}

pub struct MessagesPane {
    /// When set, the bottom split shows the debug view for this message.
    selected_message: Option<SelectedMessage>,
}

impl MessagesPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            selected_message: None,
        }
    }
}

impl PaneUi for MessagesPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        let now = system.now();
        for style in [TextStyle::Button, TextStyle::Body, TextStyle::Monospace] {
            ui.style_mut().text_styles.get_mut(&style).unwrap().size = 12.0;
        }

        let summary = system.db.message_summary(system.system_id, 0x01);

        let has_detail = self.selected_message.is_some();
        let total = ui.available_rect_before_wrap();

        let table_rect = if has_detail {
            egui::Rect::from_min_max(total.min, egui::pos2(total.max.x, total.center().y))
        } else {
            total
        };

        let mut table_ui = ui.new_child(egui::UiBuilder::new().max_rect(table_rect));
        self.table_ui(&mut table_ui, &summary, now);

        if has_detail {
            let detail_rect =
                egui::Rect::from_min_max(egui::pos2(total.min.x, total.center().y), total.max);
            ui.painter()
                .rect_filled(detail_rect, 0.0, ui.visuals().panel_fill);
            let mut detail_ui = ui.new_child(egui::UiBuilder::new().max_rect(detail_rect));
            detail_ui.separator();
            self.detail_ui(&mut detail_ui, &system);
        }

        // Advance the parent layout past the space we used.
        ui.allocate_rect(total, egui::Sense::hover());
    }
}

impl MessagesPane {
    fn table_ui(&mut self, ui: &mut egui::Ui, summary: &[MessageSummary], now: DateTime<Utc>) {
        let h = ui.available_height();

        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .max_scroll_height(h)
            .auto_shrink(false)
            .column(Column::auto().resizable(true))
            .column(Column::remainder())
            .column(Column::auto().resizable(true))
            .column(Column::auto().resizable(true))
            .header(14.0, |mut header| {
                header.col(|ui| {
                    ui.weak("Last Received");
                });
                header.col(|ui| {
                    ui.weak("Message");
                });
                header.col(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.weak("Count");
                    });
                });
                header.col(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.weak("Hz");
                    });
                });
            })
            .body(|mut body| {
                for entry in summary {
                    body.row(14.0, |mut row| {
                        row.col(|ui| {
                            let elapsed = now - entry.last;
                            let elapsed_log = elapsed.as_seconds_f32().log2();
                            let color = ui.visuals().strong_text_color().lerp_to_gamma(
                                ui.visuals().weak_text_color(),
                                ((elapsed_log + 4.0) / 5.0).clamp(0.0, 1.0),
                            );
                            ui.colored_label(
                                color,
                                entry
                                    .last
                                    .with_timezone(&Local)
                                    .format("%H:%M:%S")
                                    .to_string(),
                            );
                        });
                        row.col(|ui| {
                            let label_text =
                                format_message_label(&entry.name, entry.instance.as_ref());
                            let label = ui.add(
                                egui::Label::new(RichText::new(&label_text).small().monospace())
                                    .sense(egui::Sense::click()),
                            );
                            if label.clicked() {
                                let is_selected = self.selected_message.as_ref().is_some_and(|s| {
                                    s.name == entry.name && s.instance == entry.instance
                                });
                                self.selected_message = if is_selected {
                                    None
                                } else {
                                    Some(SelectedMessage {
                                        name: entry.name.clone(),
                                        instance: entry.instance.clone(),
                                    })
                                };
                            }
                        });
                        row.col(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.monospace(format!("{}", entry.count));
                            });
                        });
                        row.col(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add(Readout {
                                    value: entry.freq_hz,
                                    font: TextStyle::Monospace.resolve(ui.style()),
                                    color: ui.visuals().text_color(),
                                    ..Default::default()
                                });
                            });
                        });
                    });
                }
            });
    }

    fn detail_ui(&mut self, ui: &mut egui::Ui, system: &System) {
        let Some(selected) = self.selected_message.clone() else {
            return;
        };

        let title = format_message_label(&selected.name, selected.instance.as_ref());

        ui.horizontal(|ui| {
            ui.strong(&title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("x").clicked() {
                    self.selected_message = None;
                }
            });
        });

        ui.separator();

        let instance_arg = selected
            .instance
            .as_ref()
            .map(|i| (i.field.as_str(), i.value));
        match system.db.last_message_debug_by_name(
            &selected.name,
            system.system_id,
            0x01,
            instance_arg,
        ) {
            Ok(Some(mut debug)) => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_sized(
                        ui.available_size(),
                        egui::TextEdit::multiline(&mut debug)
                            .font(TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
            }
            Ok(None) => {
                ui.weak("Unknown message type");
            }
            Err(_) => {
                ui.weak("No data available");
            }
        }
    }
}
