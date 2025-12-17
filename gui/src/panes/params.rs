use eframe::egui;
use egui::{Button, CollapsingHeader, DragValue, Grid, ProgressBar, RichText, ScrollArea, Vec2};

use core::ParamProgress;

use crate::{panes::TreeBehavior, views::View};

pub struct ParamsPane {}

impl ParamsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let mut params = system.params.lock().unwrap();
        match &mut *params {
            ParamProgress::Unknown => {
                ui.label("");
            }
            ParamProgress::Progress(i, count) => {
                let pb = ProgressBar::new(*i as f32 / *count as f32).show_percentage();
                ui.add(pb);
            }
            ParamProgress::Complete(params) => {
                ScrollArea::vertical().show(ui, |ui| {
                    let w = ui.available_width();
                    ui.set_width(w);

                    let mut param_ids: Vec<_> = params.keys().cloned().collect();
                    param_ids.sort();

                    let param_id_chunks = param_ids.chunk_by(|a, b| {
                        let a_cat = a.split_once('_').map_or(a.as_str(), |s| s.0);
                        let b_cat = b.split_once('_').map_or(b.as_str(), |s| s.0);
                        a_cat == b_cat
                    });

                    for chunk in param_id_chunks {
                        let cat = chunk[0].split_once('_').map_or(chunk[0].as_str(), |s| s.0);
                        CollapsingHeader::new(cat)
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                Grid::new(ui.next_auto_id()).striped(true).show(ui, |ui| {
                                    ui.set_width(ui.available_width());

                                    let button_w = 100.0;
                                    let spacing = ui.spacing().item_spacing;
                                    let col_w = f32::max(
                                        200.0,
                                        (w - 2.0 * button_w - 6.0 * spacing.x) / 2.0,
                                    );
                                    let col_h = 20.0;

                                    for param_id in chunk {
                                        let param = params.get_mut(param_id).unwrap();
                                        ui.vertical(|ui| {
                                            ui.set_width(col_w);
                                            if param.value == param.downloaded_value {
                                                ui.monospace(param_id);
                                            } else {
                                                ui.monospace(RichText::new(param_id).strong());
                                            }
                                        });
                                        ui.add_sized(
                                            Vec2::new(col_w, col_h),
                                            DragValue::new(&mut param.value),
                                        );
                                        ui.horizontal(|ui| {
                                            let size = Vec2::new(button_w, col_h);
                                            if param.value == param.downloaded_value {
                                                ui.add_space(2.0 * button_w + 2.0 * spacing.x);
                                            } else {
                                                if ui
                                                    .add_sized(size, Button::new("⟲ Discard"))
                                                    .clicked()
                                                {
                                                    param.value = param.downloaded_value;
                                                }

                                                if ui
                                                    .add_sized(size, Button::new("💾 Save"))
                                                    .clicked()
                                                {
                                                    system.set_param(
                                                        param_id,
                                                        param.param_type,
                                                        param.value,
                                                    );
                                                }
                                            }
                                        });

                                        ui.end_row();
                                    }
                                });
                            });
                    }
                });
            }
            ParamProgress::Failed(_e) => {
                ui.label("failed");
            }
        }
    }
}
