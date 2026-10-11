use std::collections::HashMap;

use eframe::egui;
use egui::{
    Align, Button, CollapsingHeader, DragValue, Grid, Label, Layout, ProgressBar, RichText,
    ScrollArea, TextEdit, TextStyle, Vec2,
};

use nadir_core::{Composite, Param, ParamId, ParamMetadata, ParamProgress, ParamVal, System};

use crate::panes::{MUTED_HINT, PaneUi};
use crate::widgets::Hazard;

pub struct ParamsPane {
    pub search: String,
    pub hide_defaults: bool,
    pub unsaved_only: bool,
    /// Text typed into a composite's field, by base name, kept while it has focus.
    drafts: HashMap<ParamId, String>,
}

impl ParamsPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {
            search: String::new(),
            hide_defaults: false,
            unsaved_only: false,
            drafts: HashMap::new(),
        }
    }

    /// The row a param is shown in: a composite's base name if it is a complete one, else its own.
    fn row_of(params: &HashMap<ParamId, Param>, id: &str) -> ParamId {
        match Composite::of(id) {
            Some((composite, base, _))
                if Self::words(params, base, composite, |p| p.value).is_some() =>
            {
                base.to_string()
            }
            _ => id.to_string(),
        }
    }

    /// The params shown in a row, and the composite it is if any.
    fn members(params: &HashMap<ParamId, Param>, row: &str) -> (Vec<ParamId>, Option<Composite>) {
        if !params.contains_key(row)
            && let Some((composite, _, _)) = Composite::of(&Composite::member(row, 0))
        {
            let members: Vec<_> = (0..composite.width())
                .map(|slot| Composite::member(row, slot))
                .collect();
            if members.iter().all(|m| params.contains_key(m)) {
                return (members, Some(composite));
            }
        }
        (vec![row.to_string()], None)
    }

    /// A composite's words, if all of its members are there as u32s.
    fn words(
        params: &HashMap<ParamId, Param>,
        base: &str,
        composite: Composite,
        value: impl Fn(&Param) -> ParamVal,
    ) -> Option<Vec<u32>> {
        (0..composite.width())
            .map(
                |slot| match params.get(&Composite::member(base, slot)).map(&value) {
                    Some(ParamVal::Uint32(word)) => Some(word),
                    _ => None,
                },
            )
            .collect()
    }

    fn set_words(params: &mut HashMap<ParamId, Param>, base: &str, words: &[u32]) {
        for (slot, word) in words.iter().enumerate() {
            if let Some(param) = params.get_mut(&Composite::member(base, slot)) {
                param.value = ParamVal::Uint32(*word);
            }
        }
    }

    fn composite_editor(
        &mut self,
        ui: &mut egui::Ui,
        params: &mut HashMap<ParamId, Param>,
        base: &str,
        composite: Composite,
        size: Vec2,
    ) {
        let (Some(words), Some(downloaded)) = (
            Self::words(params, base, composite, |p| p.value),
            Self::words(params, base, composite, |p| p.downloaded_value),
        ) else {
            return;
        };

        let response = match composite {
            Composite::HopSeed => {
                let seed = Composite::seed_from_words([words[0], words[1]]);
                let mut text = self.drafts.get(base).cloned().unwrap_or_default();
                let hint = if seed == 0 {
                    "binding phrase".to_string()
                } else {
                    format!("phrase ({seed:016x})")
                };
                let response = ui.add_sized(size, TextEdit::singleline(&mut text).hint_text(hint));
                if response.changed() {
                    let words = if text.is_empty() {
                        downloaded
                    } else {
                        Composite::seed_words(Composite::hop_seed(&text)).to_vec()
                    };
                    Self::set_words(params, base, &words);
                    self.drafts.insert(base.to_string(), text);
                }
                response
            }
            Composite::Key => {
                let mut text = self.drafts.get(base).cloned().unwrap_or_else(|| {
                    if words.iter().all(|w| *w == 0) {
                        String::new()
                    } else {
                        Composite::key_hex([words[0], words[1], words[2], words[3]])
                    }
                });
                let invalid = !text.is_empty() && Composite::key_from_hex(&text).is_none();
                let mut edit = TextEdit::singleline(&mut text)
                    .hint_text("32 hex digits")
                    .font(TextStyle::Monospace);
                if invalid {
                    edit = edit.text_color(ui.visuals().error_fg_color);
                }
                let spacing = ui.spacing().item_spacing.x;
                let (response, randomize) = ui
                    .horizontal(|ui| {
                        let response =
                            ui.add_sized(Vec2::new(size.x - size.y - spacing, size.y), edit);
                        let randomize = ui.add_sized(Vec2::splat(size.y), Button::new("🔀"));
                        (response, randomize.clicked())
                    })
                    .inner;
                if response.changed() {
                    if text.is_empty() {
                        Self::set_words(params, base, &downloaded);
                    } else if let Some(words) = Composite::key_from_hex(&text) {
                        Self::set_words(params, base, &words);
                    }
                    self.drafts.insert(base.to_string(), text);
                }
                if randomize {
                    match (0..composite.width())
                        .map(|_| getrandom::u32())
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(words) => {
                            Self::set_words(params, base, &words);
                            self.drafts.remove(base);
                        }
                        Err(e) => tracing::error!("Failed to generate key: {e}"),
                    }
                }
                response
            }
        };

        if response.lost_focus() {
            self.drafts.remove(base);
        }
    }

    fn with_units(value: ParamVal, metadata: &ParamMetadata) -> String {
        match &metadata.units {
            Some(units) => format!("{value} {units}"),
            None => value.to_string(),
        }
    }

    /// `values` is false for composites, whose members' default and bounds are per word.
    fn metadata_tooltip(ui: &mut egui::Ui, metadata: &ParamMetadata, values: bool) {
        if !metadata.description.is_empty() {
            ui.label(&metadata.description);
        }
        if values {
            Grid::new("param_metadata").show(ui, |ui| {
                for (label, value) in [
                    ("Default", Some(metadata.default)),
                    ("Min", metadata.min_bound()),
                    ("Max", metadata.max_bound()),
                ] {
                    if let Some(value) = value {
                        ui.weak(label);
                        ui.monospace(Self::with_units(value, metadata));
                        ui.end_row();
                    }
                }
            });
        }
        if metadata.reboot_required {
            ui.weak(format!("{REBOOT_ICON} Takes effect on the next boot"));
        }
    }

    fn redownload_button(ui: &mut egui::Ui, system: &System) {
        let response = ui
            .add_enabled(!system.muted(), Button::new("⟳ Redownload"))
            .on_disabled_hover_text(MUTED_HINT);
        if response.clicked() {
            system.redownload_params();
        }
    }
}

const REBOOT_ICON: &str = "🔄";

const NAME_W: f32 = 150.0;
const VALUE_W: f32 = 140.0;
const BUTTON_W: f32 = 80.0;
const DESCRIPTION_MIN_W: f32 = 200.0;

impl PaneUi for ParamsPane {
    fn inset(&mut self, _ui: &mut egui::Ui) -> f32 {
        0.0
    }

    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        for style in [
            TextStyle::Button,
            TextStyle::Body,
            TextStyle::Monospace,
            TextStyle::Heading,
        ] {
            ui.style_mut().text_styles.get_mut(&style).unwrap().size = 12.0;
        }

        ui.spacing_mut().item_spacing.y = 2.0;

        let mut params = system.params.lock().unwrap();
        match &mut *params {
            ParamProgress::Unknown => {
                ui.centered_and_justified(|ui| ui.weak("Waiting for parameters"));
            }
            ParamProgress::Progress(i, count) => {
                ui.centered_and_justified(|ui| {
                    let pb = ProgressBar::new(*i as f32 / *count as f32).show_percentage();
                    ui.add_sized(
                        Vec2::new(f32::max(ui.available_width(), 70.0) - 50.0, 20.0),
                        pb,
                    );
                });
            }
            ParamProgress::Complete(params) => {
                let metadata = system.param_metadata.lock().unwrap();
                // A composite's members share one description, take its first.
                let metadata_of = |row: &str| {
                    metadata
                        .get(row)
                        .or_else(|| metadata.get(&Composite::member(row, 0)))
                };
                let set_open = ui
                    .horizontal(|ui| {
                        ui.add_space(5.0);
                        ui.set_height(25.0);
                        ui.weak("Filter");
                        ui.add(TextEdit::singleline(&mut self.search).desired_width(150.0));
                        ui.toggle_value(&mut self.hide_defaults, "Hide Defaults");
                        ui.toggle_value(&mut self.unsaved_only, "Unsaved Only");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(5.0);
                            Self::redownload_button(ui, &system);
                            let collapse = ui.button("⊟").on_hover_text("Collapse All").clicked();
                            let expand = ui.button("⊞").on_hover_text("Expand All").clicked();
                            (expand || collapse).then_some(expand)
                        })
                        .inner
                    })
                    .inner;

                ui.separator();

                ScrollArea::vertical().show(ui, |ui| {
                    let w = ui.available_width();
                    ui.set_width(w);

                    let search = self.search.to_lowercase();
                    let mut param_ids: Vec<_> = params
                        .keys()
                        .filter(|id| {
                            id.to_lowercase().contains(&search)
                                || metadata
                                    .get(*id)
                                    .is_some_and(|m| m.description.to_lowercase().contains(&search))
                        })
                        .filter(|id| {
                            let p = &params[*id];
                            let unsaved = p.value != p.downloaded_value;
                            let default = Composite::of(id).is_none()
                                && metadata.get(*id).is_some_and(|m| p.value == m.default);
                            (unsaved || !self.unsaved_only) && !(default && self.hide_defaults)
                        })
                        .map(|id| Self::row_of(params, id))
                        .collect();
                    param_ids.sort();
                    param_ids.dedup();

                    let param_id_chunks = param_ids.chunk_by(|a, b| {
                        let a_cat = a.split_once('_').map_or(a.as_str(), |s| s.0);
                        let b_cat = b.split_once('_').map_or(b.as_str(), |s| s.0);
                        a_cat == b_cat
                    });

                    let spacing = ui.spacing().item_spacing;
                    let fixed_w =
                        ui.spacing().indent + NAME_W + VALUE_W + 2.0 * BUTTON_W + 8.0 * spacing.x;
                    let (name_w, value_w, button_w, description_w) = if w
                        >= fixed_w + DESCRIPTION_MIN_W
                    {
                        (NAME_W, VALUE_W, BUTTON_W, Some(w - fixed_w))
                    } else {
                        let button_w = f32::max(w * 0.15, 50.0);
                        let col_w = f32::max(50.0, (w - 2.0 * button_w - 6.0 * spacing.x) / 2.0);
                        (col_w, col_w, button_w, None)
                    };
                    let col_h = 14.0;

                    for chunk in param_id_chunks {
                        let cat = chunk[0].split_once('_').map_or(chunk[0].as_str(), |s| s.0);
                        CollapsingHeader::new(cat)
                            .default_open(false)
                            .open(set_open)
                            .show(ui, |ui| {
                                Grid::new(ui.next_auto_id()).striped(true).show(ui, |ui| {
                                    for param_id in chunk {
                                        let (members, composite) = Self::members(params, param_id);
                                        let unchanged = members.iter().all(|m| {
                                            params
                                                .get(m)
                                                .is_none_or(|p| p.value == p.downloaded_value)
                                        });
                                        let meta = metadata_of(param_id);
                                        let non_default = composite.is_none()
                                            && meta.is_some_and(|m| {
                                                params[param_id].value != m.default
                                            });
                                        ui.vertical(|ui| {
                                            ui.set_width(name_w);
                                            let mut name = RichText::new(param_id).monospace();
                                            if !unchanged {
                                                name = name.strong();
                                            }
                                            if non_default {
                                                name =
                                                    name.color(ui.visuals().selection.stroke.color);
                                            }
                                            let response = ui
                                                .horizontal(|ui| {
                                                    let mut response = ui.label(name);
                                                    if meta.is_some_and(|m| m.reboot_required) {
                                                        response |= ui.weak(REBOOT_ICON);
                                                    }
                                                    response
                                                })
                                                .inner;
                                            if let Some(meta) = meta {
                                                response.on_hover_ui(|ui| {
                                                    Self::metadata_tooltip(
                                                        ui,
                                                        meta,
                                                        composite.is_none(),
                                                    );
                                                });
                                            }
                                        });
                                        let size = Vec2::new(value_w, col_h);
                                        if let Some(composite) = composite {
                                            self.composite_editor(
                                                ui, params, param_id, composite, size,
                                            );
                                        } else {
                                            let param = params.get_mut(param_id).unwrap();
                                            let mut drag = match param.value {
                                                ParamVal::Int8(ref mut i) => DragValue::new(i),
                                                ParamVal::Uint8(ref mut u) => DragValue::new(u),
                                                ParamVal::Int16(ref mut i) => DragValue::new(i),
                                                ParamVal::Uint16(ref mut u) => DragValue::new(u),
                                                ParamVal::Int32(ref mut i) => DragValue::new(i),
                                                ParamVal::Uint32(ref mut u) => DragValue::new(u),
                                                ParamVal::Int64(ref mut i) => DragValue::new(i),
                                                ParamVal::Uint64(ref mut u) => DragValue::new(u),
                                                ParamVal::Float32(ref mut f) => DragValue::new(f),
                                            };
                                            if let Some(meta) = meta {
                                                drag = drag
                                                    .range(
                                                        f64::from(meta.min.as_float())
                                                            ..=f64::from(meta.max.as_float()),
                                                    )
                                                    .clamp_existing_to_range(false);
                                                if let Some(units) = &meta.units {
                                                    drag = drag.suffix(format!(" {units}"));
                                                }
                                            }
                                            let response = ui.add_sized(size, drag);
                                            if let Some(meta) = meta {
                                                response.context_menu(|ui| {
                                                    let reset = Button::new(format!(
                                                        "⟲ Reset to default ({})",
                                                        Self::with_units(meta.default, meta)
                                                    ));
                                                    if ui.add_enabled(non_default, reset).clicked()
                                                    {
                                                        param.value = meta.default;
                                                        ui.close();
                                                    }
                                                });
                                            }
                                        }
                                        if let Some(description_w) = description_w {
                                            ui.vertical(|ui| {
                                                ui.set_width(description_w);
                                                if let Some(meta) = meta {
                                                    let first_line = meta
                                                        .description
                                                        .lines()
                                                        .next()
                                                        .unwrap_or("");
                                                    ui.add(
                                                        Label::new(
                                                            RichText::new(first_line).weak(),
                                                        )
                                                        .truncate(),
                                                    )
                                                    .on_hover_ui(|ui| {
                                                        Self::metadata_tooltip(
                                                            ui,
                                                            meta,
                                                            composite.is_none(),
                                                        );
                                                    });
                                                }
                                            });
                                        }

                                        ui.horizontal(|ui| {
                                            let size = Vec2::new(button_w, col_h);
                                            if unchanged {
                                                ui.add_space(2.0 * button_w + 2.0 * spacing.x);
                                            } else {
                                                if ui
                                                    .add_sized(size, Button::new("⟲ Discard"))
                                                    .clicked()
                                                {
                                                    for member in &members {
                                                        if let Some(param) = params.get_mut(member)
                                                        {
                                                            param.value = param.downloaded_value;
                                                        }
                                                    }
                                                }

                                                let save = ui
                                                    .add_enabled_ui(!system.muted(), |ui| {
                                                        ui.add_sized(size, Button::new("💾 Save"))
                                                    })
                                                    .inner
                                                    .on_disabled_hover_text(MUTED_HINT);
                                                Hazard::tick(ui, save.rect, &system);
                                                if Hazard::confirm(ui, &save, &system) {
                                                    // A hidden key echoes as zeros, so show it hidden
                                                    // again rather than edited forever.
                                                    let hidden = composite == Some(Composite::Key)
                                                        && members.iter().all(|m| {
                                                            params.get(m).is_some_and(|p| {
                                                                p.downloaded_value
                                                                    == ParamVal::Uint32(0)
                                                            })
                                                        });
                                                    for member in &members {
                                                        if let Some(param) = params.get_mut(member)
                                                        {
                                                            system.set_param(member, param.value);
                                                            if hidden {
                                                                param.value =
                                                                    param.downloaded_value;
                                                            }
                                                        }
                                                    }
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
            ParamProgress::Failed(e) => {
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(format!("Parameter download failed: {e}"));
                    Self::redownload_button(ui, &system);
                });
            }
        }
    }
}
