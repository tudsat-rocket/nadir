use mavinspect::protocol::{Dialect, MavType};
use nadir_core::{MessageInstance, MessageSummary, format_message_label};

use eframe::egui;
use egui::{Align, Button, CollapsingHeader, DragValue, Layout, RichText, ScrollArea, TextEdit};

use crate::{
    panes::{PaneUi, TreeBehavior},
    views::View,
    widgets::{Plot, PlotLine, SharedPlotState, YBounds},
};

#[derive(Clone, PartialEq, Eq)]
struct ActiveField {
    message_name: String,
    instance: Option<MessageInstance>,
    field_name: String,
    unit: Option<String>,
}

struct Field {
    name: String,
    unit: Option<String>,
    array_len: Option<usize>,
}

struct MessageFields {
    name: String,
    instance: Option<MessageInstance>,
    label: String,
    fields: Vec<Field>,
}

pub struct PlotPane {
    active_fields: Vec<ActiveField>,
    search: String,
    selector_shown: bool,
    legend_shown: bool,
    y_bounds: YBounds,
}

impl Field {
    fn label(&self) -> String {
        match &self.unit {
            Some(unit) => format!("{} [{unit}]", self.name),
            None => self.name.clone(),
        }
    }
}

impl MessageFields {
    fn collect(
        out: &mut Vec<Self>,
        summary: &[MessageSummary],
        dialect: &Dialect,
        common: &Dialect,
    ) {
        let is_common_dialect = dialect.name() == common.name();

        let mut messages: Vec<_> = dialect.messages().into_iter().collect();
        messages.sort_by_key(|m| m.name());

        for message in messages {
            if !is_common_dialect && common.get_message_by_name(message.name()).is_some() {
                continue;
            }

            for entry in summary.iter().filter(|e| e.name == message.name()) {
                let fields = message
                    .fields()
                    .iter()
                    .filter(|f| entry.instance.as_ref().is_none_or(|i| i.field != f.name()))
                    .filter_map(|field| {
                        let array_len = match field.r#type() {
                            // A char array is a string: no entry of it is a number to plot.
                            MavType::Array(base, _) if **base == MavType::Char => return None,
                            MavType::Array(_, len) => Some(*len),
                            _ => None,
                        };
                        Some(Field {
                            name: field.name().to_owned(),
                            unit: field.units().map(ToString::to_string),
                            array_len,
                        })
                    })
                    .collect();

                out.push(Self {
                    name: entry.name.clone(),
                    instance: entry.instance.clone(),
                    label: format_message_label(&entry.name, entry.instance.as_ref()),
                    fields,
                });
            }
        }
    }

    fn active_field(&self, field_name: String, unit: Option<String>) -> ActiveField {
        ActiveField {
            message_name: self.name.clone(),
            instance: self.instance.clone(),
            field_name,
            unit,
        }
    }
}

impl PlotPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        let attitude_rate = |field_name: &str| ActiveField {
            message_name: "ATTITUDE".to_owned(),
            instance: None,
            field_name: field_name.to_owned(),
            unit: Some("rad/s".to_owned()),
        };

        Self {
            active_fields: vec![
                attitude_rate("rollspeed"),
                attitude_rate("pitchspeed"),
                attitude_rate("yawspeed"),
            ],
            search: String::new(),
            selector_shown: true,
            legend_shown: true,
            y_bounds: YBounds::default(),
        }
    }

    fn controls_ui(&mut self, ui: &mut egui::Ui, shared: &mut SharedPlotState) {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut self.selector_shown, "Fields");
            ui.toggle_value(&mut self.legend_shown, "Legend");
            ui.toggle_value(&mut shared.attached_to_edge, "Follow");

            ui.separator();
            ui.weak("Window");
            let speed = shared.view_width * 0.01;
            ui.add_enabled(
                shared.attached_to_edge,
                DragValue::new(&mut shared.view_width)
                    .range(0.1..=f64::INFINITY)
                    .speed(speed)
                    .suffix(" s"),
            );

            ui.separator();
            ui.weak("Y");
            let y = &mut self.y_bounds;
            let span = (y.max - y.min).abs();
            // Three significant digits of the span, however large or small it is.
            let decimals = (2.0 - span.log10().floor()).clamp(0.0, 12.0) as usize;
            let drag = |value| {
                DragValue::new(value)
                    .speed(span * 0.005)
                    .fixed_decimals(decimals)
            };
            let min = ui.add(drag(&mut y.min));
            ui.weak("to");
            let max = ui.add(drag(&mut y.max));
            if min.changed() || max.changed() {
                y.locked = true;
            }
            let mut auto = !y.locked;
            if ui.toggle_value(&mut auto, "Auto").changed() {
                y.locked = !auto;
            }
        });
    }

    fn selector_ui(&mut self, ui: &mut egui::Ui, messages: &[MessageFields]) {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let clear = ui.add_enabled(!self.active_fields.is_empty(), Button::new("Clear"));
                if clear.clicked() {
                    self.active_fields.clear();
                }
                ui.add(
                    TextEdit::singleline(&mut self.search)
                        .hint_text("Filter")
                        .desired_width(f32::INFINITY),
                );
            });
        });

        let query = self.search.trim().to_lowercase();
        ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width());
            for message in messages {
                self.message_ui(ui, message, &query);
            }
        });
    }

    fn message_ui(&mut self, ui: &mut egui::Ui, message: &MessageFields, query: &str) {
        let message_matches = message.label.to_lowercase().contains(query);
        let fields: Vec<_> = message
            .fields
            .iter()
            .filter(|f| message_matches || f.name.to_lowercase().contains(query))
            .collect();
        if fields.is_empty() {
            return;
        }

        let selected = self
            .active_fields
            .iter()
            .filter(|af| af.message_name == message.name && af.instance == message.instance)
            .count();
        let header = match selected {
            0 => message.label.clone(),
            n => format!("{} ({n})", message.label),
        };

        // Searching gets its own open/closed state, so the headers it opens are closed again once
        // the search is cleared.
        let searching = !query.is_empty();
        CollapsingHeader::new(header)
            .id_salt((
                message.name.as_str(),
                message.instance.as_ref().map(|i| i.value),
                searching,
            ))
            .default_open(searching || selected > 0)
            .show(ui, |ui| {
                for field in fields {
                    match field.array_len {
                        None => self.field_button(
                            ui,
                            field.label(),
                            message.active_field(field.name.clone(), field.unit.clone()),
                        ),
                        Some(len) => self.array_ui(ui, message, field, len),
                    }
                }
            });
    }

    fn array_ui(&mut self, ui: &mut egui::Ui, message: &MessageFields, field: &Field, len: usize) {
        let elements: Vec<_> = (0..len)
            .map(|i| message.active_field(format!("{}[{i}]", field.name), field.unit.clone()))
            .collect();
        let selected = elements
            .iter()
            .filter(|e| self.active_fields.contains(e))
            .count();
        let header = match selected {
            0 => field.label(),
            n => format!("{} ({n}/{len})", field.label()),
        };

        CollapsingHeader::new(header)
            .id_salt((
                message.name.as_str(),
                message.instance.as_ref().map(|i| i.value),
                field.name.as_str(),
            ))
            .show(ui, |ui| {
                let all = selected == len;
                let text = RichText::new(if all { "Deselect all" } else { "Select all" }).weak();
                if ui
                    .add_sized([ui.available_width(), 20.0], Button::new(text))
                    .clicked()
                {
                    if all {
                        self.active_fields.retain(|af| !elements.contains(af));
                    } else {
                        for element in &elements {
                            if !self.active_fields.contains(element) {
                                self.active_fields.push(element.clone());
                            }
                        }
                    }
                }

                for element in elements {
                    self.field_button(ui, element.field_name.clone(), element);
                }
            });
    }

    fn field_button(&mut self, ui: &mut egui::Ui, label: String, field: ActiveField) {
        let position = self.active_fields.iter().position(|af| af == &field);
        let button = Button::new(label).selected(position.is_some());
        if ui.add_sized([ui.available_width(), 20.0], button).clicked() {
            match position {
                Some(position) => {
                    self.active_fields.remove(position);
                }
                None => self.active_fields.push(field),
            }
        }
    }
}

impl PaneUi for PlotPane {
    fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System { system_id, .. } = behavior.active_view else {
            return;
        };

        self.controls_ui(ui, behavior.shared_plot_state);

        // The pane plots one component's series at a time, and that is component 1.
        let summary: Vec<_> = behavior
            .source
            .db
            .message_summary(system_id)
            .into_iter()
            .filter(|entry| entry.component_id == 1)
            .collect();

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

        let mut messages = Vec::new();
        MessageFields::collect(&mut messages, &summary, mavspec_common, mavspec_common);
        MessageFields::collect(&mut messages, &summary, mavspec_ardupilot, mavspec_common);
        MessageFields::collect(&mut messages, &summary, rapid, rapid_common);

        let mut selector_shown = self.selector_shown;
        egui::Panel::left(ui.id().with("fields"))
            .resizable(true)
            .default_size(250.0)
            .show_collapsible(ui, &mut selector_shown, |ui| {
                self.selector_ui(ui, &messages);
            });
        self.selector_shown = selector_shown;

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
                unit: af.unit.clone(),
                color: None,
                scale: None,
                sentinel: None,
            })
            .collect::<Vec<_>>();

        let mut plot = Plot::new(
            &lines,
            &behavior.source,
            behavior.shared_plot_state,
            (None, None),
        )
        .y_bounds(&mut self.y_bounds);
        if !self.legend_shown {
            plot = plot.without_legend();
        }
        ui.add_sized(ui.available_size(), plot);
    }
}
