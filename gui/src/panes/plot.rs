use core::{MessageInstance, format_message_label};

use eframe::egui;
use egui::Align;

use crate::{
    panes::{PaneUi, TreeBehavior},
    views::View,
    widgets::{Plot, PlotLine},
};

#[derive(Clone, PartialEq, Eq)]
struct ActiveField {
    message_name: String,
    instance: Option<MessageInstance>,
    field_name: String,
}

pub struct PlotPane {
    active_fields: Vec<ActiveField>,
}

macro_rules! draw_field_selector {
    ($ui:expr, $behavior:expr, $active:expr, $summary:expr, $dialect:expr, $common:expr) => {
        let dialect = $dialect;
        let common = $common;
        let is_common_dialect = dialect.name() == common.name();

        let mut message_names: Vec<_> = dialect.messages().into_iter().map(|m| m.name()).collect();
        message_names.sort();

        for message_name in message_names {
            if !is_common_dialect && common.get_message_by_name(&message_name).is_some() {
                continue;
            }

            let message = dialect.get_message_by_name(message_name).unwrap();
            let entries: Vec<_> = $summary
                .iter()
                .filter(|e: &&core::MessageSummary| e.name == message.name())
                .collect();
            if entries.is_empty() {
                continue;
            }

            for entry in entries {
                let header_label = format_message_label(&entry.name, entry.instance.as_ref());
                let default_open = $active.iter().any(|af: &ActiveField| {
                    af.message_name == entry.name && af.instance == entry.instance
                });
                egui::CollapsingHeader::new(header_label)
                    .id_salt((
                        entry.name.as_str(),
                        entry.instance.as_ref().map(|i| i.value),
                    ))
                    .default_open(default_open)
                    .show($ui, |ui| {
                        for field in message.fields() {
                            if entry
                                .instance
                                .as_ref()
                                .is_some_and(|i| i.field == field.name())
                            {
                                continue;
                            }
                            let label = if let Some(units) = field.units() {
                                format!("{} [{:?}]", field.name(), units)
                            } else {
                                field.name().to_owned()
                            };

                            let id = ActiveField {
                                message_name: entry.name.clone(),
                                instance: entry.instance.clone(),
                                field_name: field.name().to_owned(),
                            };
                            let selected = $active.contains(&id);
                            let button = egui::Button::new(label).selected(selected);

                            if ui
                                .add_sized(egui::Vec2::new(ui.available_width(), 20.0), button)
                                .clicked()
                            {
                                if selected {
                                    let pos = $active.iter().position(|i| i == &id).unwrap();
                                    $active.remove(pos);
                                } else {
                                    $active.push(id);
                                }
                            }
                        }
                    });
            }
        }
    };
}

impl PlotPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            active_fields: vec![
                ActiveField {
                    message_name: "ATTITUDE".to_owned(),
                    instance: None,
                    field_name: "rollspeed".to_owned(),
                },
                ActiveField {
                    message_name: "ATTITUDE".to_owned(),
                    instance: None,
                    field_name: "pitchspeed".to_owned(),
                },
                ActiveField {
                    message_name: "ATTITUDE".to_owned(),
                    instance: None,
                    field_name: "yawspeed".to_owned(),
                },
            ],
        }
    }
}

impl PaneUi for PlotPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System { system_id, .. } = behavior.active_view else {
            return;
        };

        let summary = behavior.source.db.message_summary(system_id, 1);

        ui.with_layout(
            egui::Layout::left_to_right(Align::TOP).with_cross_justify(true),
            |ui| {
                let mavspec_protocol = mavspec::definitions::protocol();
                let mavspec_common = mavspec_protocol.get_dialect_by_name("common").unwrap();
                let mavspec_ardupilot = mavspec_protocol
                    .get_dialect_by_name("ardupilotmega")
                    .unwrap();
                let rapid_protocol = rapid_dialect::definitions::protocol();
                let rapid_common = rapid_protocol
                    .get_dialect_by_canonical_name("common")
                    .unwrap();
                let rapid = rapid_protocol
                    .get_dialect_by_canonical_name("rapid")
                    .unwrap();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical(|ui| {
                        let width = f32::min(ui.available_width() * 0.4, 300.0);
                        ui.set_width(width);

                        draw_field_selector!(
                            ui,
                            behavior,
                            &mut self.active_fields,
                            summary,
                            mavspec_common,
                            mavspec_common
                        );
                        draw_field_selector!(
                            ui,
                            behavior,
                            &mut self.active_fields,
                            summary,
                            mavspec_ardupilot,
                            mavspec_common
                        );
                        draw_field_selector!(
                            ui,
                            behavior,
                            &mut self.active_fields,
                            summary,
                            rapid,
                            rapid_common
                        );
                    });
                });

                let lines = self
                    .active_fields
                    .iter()
                    .map(|af| PlotLine {
                        system_id,
                        component_id: 1,
                        message_name: af.message_name.clone(),
                        instance: af.instance.clone(),
                        field_name: af.field_name.clone(),
                        alias: None,
                        unit: None, // TODO
                        color: None,
                        scale: None,
                        sentinel: None,
                    })
                    .collect::<Vec<_>>();

                let plot = Plot::new(
                    &lines,
                    &behavior.source,
                    behavior.shared_plot_state,
                    (None, None),
                );
                ui.add_sized(ui.available_size(), plot);
            },
        );
    }
}
