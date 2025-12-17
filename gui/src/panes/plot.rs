use eframe::egui;
use egui::Align;

use crate::{
    panes::TreeBehavior,
    views::View,
    widgets::{Plot, PlotLine},
};

pub struct PlotPane {
    active_fields: Vec<(String, String)>,
}

macro_rules! draw_field_selector {
    ($ui:expr, $behavior:expr, $active:expr, $dialect:expr) => {
        let mut message_names: Vec<_> = $dialect.messages().into_iter().map(|m| m.name()).collect();
        message_names.sort();

        for message_name in message_names {
            let message = $dialect.get_message_by_name(message_name).unwrap();

            let crate::views::View::System(system_id) = $behavior.active_view else {
                continue;
            };

            let count = $behavior
                .core
                .db
                .common_count_by_name_for_system(message.name(), (system_id, 1))
                .unwrap_or(0);
            if count == 0 {
                continue;
            }

            let default_open = $active.iter().any(|(m, _f)| m == message.name());
            egui::CollapsingHeader::new(message.name())
                .default_open(default_open)
                .show($ui, |ui| {
                    for field in message.fields() {
                        let label = if let Some(units) = field.units() {
                            format!("{} [{:?}]", field.name(), units)
                        } else {
                            field.name().to_owned()
                        };

                        let id = (message.name().to_owned(), field.name().to_owned());
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
    };
}

impl PlotPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            active_fields: vec![
                ("ATTITUDE".to_owned(), "rollspeed".to_owned()),
                ("ATTITUDE".to_owned(), "pitchspeed".to_owned()),
                ("ATTITUDE".to_owned(), "yawspeed".to_owned()),
            ],
        }
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let protocol = mavspec::definitions::protocol();
        let dialect = protocol.get_dialect_by_name("common").unwrap();

        let View::System(system_id) = behavior.active_view else {
            return;
        };

        ui.with_layout(
            egui::Layout::left_to_right(Align::TOP).with_cross_justify(true),
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical(|ui| {
                        let width = f32::min(ui.available_width() * 0.4, 300.0);
                        ui.set_width(width);

                        draw_field_selector!(ui, behavior, &mut self.active_fields, dialect);
                    });
                });

                let lines = self
                    .active_fields
                    .iter()
                    .map(|(m, f)| PlotLine {
                        system_id: system_id,
                        component_id: 1,
                        message_name: m.clone(),
                        field_name: f.clone(),
                        alias: None,
                        unit: None, // TODO
                        color: None,
                    })
                    .collect();

                let plot = Plot::new(
                    lines,
                    &behavior.core,
                    behavior.shared_plot_state,
                    (None, None),
                );
                ui.add_sized(ui.available_size(), plot);
            },
        );
    }
}
