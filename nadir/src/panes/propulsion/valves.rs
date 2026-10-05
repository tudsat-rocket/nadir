use std::f32::consts::PI;

use egui::{
    Align2, Button, Color32, CornerRadius, DragValue, FontId, Label, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, UiBuilder, Vec2,
};
use mavspec::rust::dialects::common::messages::ServoOutputRaw;
use nadir_core::System;
use rapid_dialect::rapid::enums::ValveId;

use crate::colors::{
    COLOR_INDICATOR_WARNING, blink_on, gauge_fill, gauge_track, high_contrast, readable, text_on,
};
use crate::widgets::Hazard;

use super::rocket::{self, ValveReading};
use super::{SERVO_COUNT, SERVOS, VALVE_COUNT, VALVE_LATCH_EPS, VALVES, Valve, ValveKind};

// Every valve, plus one cell holding all servos.
const CELL_COUNT: usize = VALVE_COUNT + 1;

const GAP: f32 = 3.0;
const CELL_GAP: Vec2 = Vec2::new(12.0, 4.0);
const NAME_FONT: f32 = 9.0;
const NAME_H: f32 = 16.0;
const BTN_FONT: f32 = 10.0;
const PULSE_FONT: f32 = 9.0;

// (minimum button width, label)
const CLOSE_LABELS: [(f32, &str); 3] = [(32.0, "CLOSE"), (24.0, "CLS"), (0.0, "C")];
const CLOSE_W: f32 = 34.0;
const CLOSE_H: f32 = 14.0;

const OPEN_H: f32 = 18.0;
const SERVO_DRAG_W: f32 = 44.0;
const PULSE_W: f32 = 22.0;
const PULSE_H: f32 = 18.0;
const OUTLINE_PAD: f32 = 4.0;
const GROUP_W: f32 = 3.0 * PULSE_W + 2.0 * GAP + 2.0 * OUTLINE_PAD;
const GROUP_H: f32 = OPEN_H + GAP + PULSE_H + 2.0 * OUTLINE_PAD;

const GAUGE_MIN: f32 = 36.0;
const GAUGE_MAX: f32 = 52.0;
const RING_RATIO: f32 = 0.1;
const GAUGE_FONT_RATIO: f32 = 0.22;
const GAUGE_FONT_MIN: f32 = 8.0;
// Hub width as a share of the knob.
const GAUGE_VALUE_RATIO: f32 = 0.72;

// The knob sweeps 270 deg with the gap at the bottom, starting lower-left.
const GAUGE_START: f32 = 0.75 * PI;
const GAUGE_SWEEP: f32 = 1.5 * PI;

const STATE_FONT_RATIO: f32 = 0.62;
const STATE_FONT_MIN: f32 = 7.0;

const WIDE_CHROME: f32 = NAME_H + GAP;
const TALL_CHROME: f32 = NAME_H + 2.0 * GAP + GROUP_H;
const WIDE_KNOB_MIN: f32 = GROUP_H;

const WIDE_MIN_W: f32 = GROUP_H + GAP + GROUP_W;
const TALL_MIN_W: f32 = GROUP_W;

// Knob grid layout, buttons beside the knob (wide) or below it (tall).
#[derive(Copy, Clone)]
pub(super) struct Plan {
    wide: bool,
    cols: usize,
    cell: Vec2,
    gauge: f32,
    height: f32,
}

impl Plan {
    fn new(wide: bool, avail: Vec2) -> Self {
        let gap = CELL_GAP;
        let (min_w, chrome, knob_min) = if wide {
            (WIDE_MIN_W, WIDE_CHROME, WIDE_KNOB_MIN)
        } else {
            (TALL_MIN_W, TALL_CHROME, GAUGE_MIN)
        };

        let fit = (((avail.x + gap.x) / (min_w + gap.x)).floor() as usize).clamp(1, CELL_COUNT);
        // Spread the cells evenly over the rows they already need.
        let cols = CELL_COUNT.div_ceil(CELL_COUNT.div_ceil(fit));
        let rows = CELL_COUNT.div_ceil(cols);

        let spare = (avail.y - (rows - 1) as f32 * gap.y).max(0.0) / rows as f32;
        let gauge = (spare - chrome).clamp(GAUGE_MIN, GAUGE_MAX).max(knob_min);
        let cell = Vec2::new(
            ((avail.x - (cols - 1) as f32 * gap.x) / cols as f32)
                .max(min_w)
                .min(avail.x),
            chrome + gauge,
        );

        Self {
            wide,
            cols,
            cell,
            gauge,
            height: rows as f32 * cell.y + (rows - 1) as f32 * gap.y,
        }
    }

    // Biggest knob that fits; failing that, the lowest grid.
    pub(super) fn best(avail: Vec2) -> Self {
        let (wide, tall) = (Self::new(true, avail), Self::new(false, avail));
        let pick = |take_wide: bool| if take_wide { wide } else { tall };
        match (wide.height <= avail.y, tall.height <= avail.y) {
            (true, true) => pick(wide.gauge >= tall.gauge),
            (true, false) => wide,
            (false, true) => tall,
            (false, false) => pick(wide.height <= tall.height),
        }
    }

    // A bottom panel sizes itself to its content, and a scroll area reports its
    // collapsed minimum, so the panel has to be told how tall the grid will be.
    pub(super) fn height(&self, budget: f32) -> f32 {
        self.height.min(budget)
    }
}

pub(super) fn grid(
    ui: &mut egui::Ui,
    system: &System,
    pending: &mut [Option<f32>; VALVE_COUNT + SERVO_COUNT],
    blink: [bool; VALVE_COUNT],
    pulse_durations: [f32; 3],
    plan: &Plan,
) {
    egui::ScrollArea::vertical()
        .max_height(ui.available_height())
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("propulsion_valves")
                .num_columns(plan.cols)
                .spacing(CELL_GAP)
                .show(ui, |ui| {
                    let (valves, servo_pending) = pending.split_at_mut(VALVE_COUNT);
                    for (i, blink) in blink.into_iter().enumerate() {
                        knob(ui, system, i, &mut valves[i], blink, pulse_durations, plan);
                        if (i + 1).is_multiple_of(plan.cols) {
                            ui.end_row();
                        }
                    }
                    servos(ui, system, servo_pending, plan);
                    if !CELL_COUNT.is_multiple_of(plan.cols) {
                        ui.end_row();
                    }
                });
        });
}

fn knob(
    ui: &mut egui::Ui,
    system: &System,
    index: usize,
    pending: &mut Option<f32>,
    blink: bool,
    pulse_durations: [f32; 3],
    plan: &Plan,
) {
    let Valve {
        id,
        label,
        kind,
        color,
    } = VALVES[index];
    let reading = rocket::valve_reading(system, id);
    let commanded = reading.and_then(|r| r.commanded);
    let time = ui.input(|i| i.time);

    // Placed by hand: egui's nested layouts add item spacing the plan cannot see.
    let (rect, _) = ui.allocate_exact_size(plan.cell, Sense::hover());
    let ui = &mut ui.new_child(UiBuilder::new().max_rect(rect));

    let close_w = CLOSE_W.min(rect.width() / 2.0);
    let close = Rect::from_min_size(
        Pos2::new(
            rect.right() - close_w,
            rect.top() + (NAME_H - CLOSE_H) / 2.0,
        ),
        Vec2::new(close_w, CLOSE_H),
    );
    name(
        ui,
        label,
        color,
        Rect::from_min_max(rect.min, close.left_bottom()),
    );
    close_button(ui, system, id, commanded, close);

    let body = rect.top() + NAME_H + GAP;
    let (dial, group) = if plan.wide {
        (
            Rect::from_min_size(Pos2::new(rect.left(), body), Vec2::splat(plan.gauge)),
            Rect::from_min_size(
                Pos2::new(rect.right() - GROUP_W, body),
                Vec2::new(GROUP_W, GROUP_H),
            ),
        )
    } else {
        (
            Rect::from_min_size(
                Pos2::new(rect.center().x - plan.gauge / 2.0, body),
                Vec2::splat(plan.gauge),
            ),
            Rect::from_min_size(
                Pos2::new(rect.center().x - GROUP_W / 2.0, body + plan.gauge + GAP),
                Vec2::new(GROUP_W, GROUP_H),
            ),
        )
    };

    let font = FontId::monospace((plan.gauge * GAUGE_FONT_RATIO).max(GAUGE_FONT_MIN));
    gauge(ui, dial, reading, blink, time);
    if kind == ValveKind::Servo {
        target_drag(ui, dial, font, system, commanded, pending, |v| {
            system.do_set_valve(id, v);
        });
    } else {
        reported(ui, dial, &font, reading);
    }

    open_group(ui, system, id, commanded, pulse_durations, group);
}

fn name(ui: &mut egui::Ui, label: &str, color: Color32, rect: Rect) {
    let color = readable(color, ui.visuals());
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                Label::new(
                    RichText::new(label.to_uppercase())
                        .size(NAME_FONT)
                        .color(color),
                )
                .truncate()
                .selectable(false),
            );
        },
    );
}

fn close_button(
    ui: &mut egui::Ui,
    system: &System,
    id: ValveId,
    commanded: Option<f32>,
    rect: Rect,
) {
    let (_, text) = CLOSE_LABELS
        .into_iter()
        .find(|(needs, _)| rect.width() >= *needs)
        .unwrap_or(CLOSE_LABELS[2]);
    let latched = matches!(commanded, Some(c) if c <= VALVE_LATCH_EPS);
    let close = label_button(text, BTN_FONT, rect.size()).selected(latched);

    let close = ui.put(rect, close);
    if Hazard::confirm(ui, &close, system) {
        system.do_set_valve(id, 0.0);
    }
}

fn open_group(
    ui: &mut egui::Ui,
    system: &System,
    id: ValveId,
    commanded: Option<f32>,
    pulse_durations: [f32; 3],
    rect: Rect,
) {
    ui.painter().add(Shape::rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
        StrokeKind::Inside,
    ));

    let inner = rect.shrink(OUTLINE_PAD);
    let open_rect = Rect::from_min_size(inner.min, Vec2::new(inner.width(), OPEN_H));
    let active = matches!(commanded, Some(c) if c >= 1.0 - VALVE_LATCH_EPS);
    let mut open = label_button("OPEN", BTN_FONT, open_rect.size()).selected(active);
    if active {
        let fill = readable(COLOR_INDICATOR_WARNING, ui.visuals());
        open = open.fill(fill);
        ui.style_mut().visuals.override_text_color = Some(text_on(fill));
    }
    let open = ui.put(open_rect, open);
    if Hazard::confirm(ui, &open, system) {
        system.do_set_valve(id, 1.0);
    }
    ui.style_mut().visuals.override_text_color = None;

    let size = Vec2::new((inner.width() - 2.0 * GAP) / 3.0, PULSE_H);
    for (i, secs) in pulse_durations.into_iter().enumerate() {
        let min = Pos2::new(
            inner.left() + i as f32 * (size.x + GAP),
            open_rect.bottom() + GAP,
        );
        let text = format!("{secs}s");
        let button = label_button(&text, PULSE_FONT, size);
        let button = ui.put(Rect::from_min_size(min, size), button);
        if Hazard::confirm(ui, &button, system) {
            system.do_pulse_valve(id, secs);
        }
    }
}

// `Ui::put` hands a widget its rect as a wrap width and otherwise leaves it at
// its natural size, which for a button is at least `interact_size` tall. Both
// have to be overridden for a button to land exactly where the plan put it.
fn label_button(text: &str, font: f32, size: Vec2) -> Button<'_> {
    Button::new(RichText::new(text).size(font))
        .wrap_mode(egui::TextWrapMode::Extend)
        .small()
        .min_size(size)
}

// The edit is held in `pending` so telemetry cannot overwrite it mid-drag, and
// sent once, on release.
fn target_drag(
    ui: &mut egui::Ui,
    knob: Rect,
    font: FontId,
    system: &System,
    commanded: Option<f32>,
    pending: &mut Option<f32>,
    send: impl FnOnce(f32),
) {
    let mut value = pending.unwrap_or(commanded.unwrap_or(0.0) * 100.0);
    let resp = ui
        .scope(|ui| {
            // Unframed until hovered, except in high contrast.
            if !high_contrast() {
                let widgets = &mut ui.visuals_mut().widgets;
                widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                widgets.inactive.bg_stroke = Stroke::NONE;
            }
            ui.style_mut().override_font_id = Some(font);

            let drag = DragValue::new(&mut value)
                .speed(1.0)
                .range(0.0..=100.0)
                .suffix("%");
            ui.add_enabled_ui(system.hot(), |ui| ui.put(hub(knob), drag))
                .inner
                .on_disabled_hover_text("Set HOT to drag")
        })
        .inner;

    commit(&resp, value, commanded, pending, send);
}

fn commit(
    resp: &egui::Response,
    value: f32,
    commanded: Option<f32>,
    pending: &mut Option<f32>,
    send: impl FnOnce(f32),
) {
    if resp.drag_stopped() || resp.lost_focus() {
        // A click that opens the text entry and leaves it alone is not a command.
        if commanded.is_none_or(|c| (c * 100.0 - value).abs() >= 0.5) {
            send(value / 100.0);
        }
        *pending = None;
    } else if resp.dragged() || resp.has_focus() {
        *pending = Some(value);
    } else {
        *pending = None;
    }
}

fn servos(ui: &mut egui::Ui, system: &System, pending: &mut [Option<f32>], plan: &Plan) {
    let (rect, _) = ui.allocate_exact_size(plan.cell, Sense::hover());
    let ui = &mut ui.new_child(UiBuilder::new().max_rect(rect));
    let raw = system.last_message::<ServoOutputRaw>().ok();
    let row_h = rect.height() / SERVO_COUNT as f32;

    for (i, (label, pending)) in SERVOS.into_iter().zip(pending).enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + i as f32 * row_h),
            Vec2::new(rect.width(), row_h),
        );
        let drag_w = SERVO_DRAG_W.min(row.width() / 2.0);
        let drag_rect = Rect::from_center_size(
            Pos2::new(row.right() - drag_w / 2.0, row.center().y),
            Vec2::new(drag_w, (row_h - GAP).min(OPEN_H)),
        );
        name(
            ui,
            label,
            ui.visuals().text_color(),
            Rect::from_min_max(row.min, Pos2::new(drag_rect.left() - GAP, row.bottom())),
        );

        let commanded = raw.as_ref().and_then(|raw| servo_commanded(raw, i));
        let editing = pending.is_some();
        let mut value = pending.unwrap_or(commanded.unwrap_or(0.0) * 100.0);
        let resp = ui
            .scope(|ui| {
                ui.spacing_mut().interact_size.y = drag_rect.height();
                ui.style_mut().override_font_id = Some(FontId::monospace(BTN_FONT));
                let drag = DragValue::new(&mut value).speed(1.0).range(0.0..=100.0);
                let drag = if commanded.is_none() && !editing {
                    drag.custom_formatter(|_, _| "--".to_owned())
                } else {
                    drag.suffix("%")
                };
                ui.add_enabled_ui(system.hot(), |ui| ui.put(drag_rect, drag))
                    .inner
                    .on_disabled_hover_text("Set HOT to drag")
            })
            .inner;

        let instance = u8::try_from(i + 1).unwrap_or(u8::MAX);
        commit(&resp, value, commanded, pending, |v| {
            system.do_set_servo(instance, servo_pwm(v));
        });
    }
}

// 0 marks a channel the firmware has never driven.
fn servo_commanded(raw: &ServoOutputRaw, index: usize) -> Option<f32> {
    let pwm = [
        raw.servo1_raw,
        raw.servo2_raw,
        raw.servo3_raw,
        raw.servo4_raw,
    ][index];
    (pwm != 0).then(|| (f32::from(pwm) - 1000.0) / 1000.0)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=1000 first"
)]
fn servo_pwm(position: f32) -> u16 {
    1000 + (position.clamp(0.0, 1.0) * 1000.0).round() as u16
}

fn reported(ui: &egui::Ui, knob: Rect, font: &FontId, reading: Option<ValveReading>) {
    let hub = hub(knob);
    let color = ui.visuals().text_color();
    let font = FontId::proportional((font.size * STATE_FONT_RATIO).max(STATE_FONT_MIN));

    let text = match reading.and_then(|r| r.state) {
        None => "--",
        Some(s) if s >= 0.5 => "OPEN",
        Some(_) => {
            let width = ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap("CLOSED".to_owned(), font.clone(), color)
                    .size()
                    .x
            });
            if width <= hub_text_width(knob) {
                "CLOSED"
            } else {
                "CLS"
            }
        }
    };

    ui.painter()
        .text(hub.center(), Align2::CENTER_CENTER, text, font, color);
}

fn hub(knob: Rect) -> Rect {
    Rect::from_center_size(
        knob.center(),
        Vec2::new(knob.width() * GAUGE_VALUE_RATIO, CLOSE_H),
    )
}

// The hub box runs wider than the ring's opening at the height the text sits at,
// so a word measured against the box alone still crosses the ring.
fn hub_text_width(knob: Rect) -> f32 {
    knob.width() * 0.56
}

fn gauge(ui: &egui::Ui, rect: Rect, reading: Option<ValveReading>, blink: bool, time: f64) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let center = rect.center();
    let width = rect.size().min_elem() * RING_RATIO;
    let radius = rect.size().min_elem() / 2.0 - width;

    painter.add(Shape::line(
        arc(center, radius, GAUGE_SWEEP),
        Stroke::new(width, gauge_track(visuals)),
    ));

    let state = reading.and_then(|r| r.state).map(|s| s.clamp(0.0, 1.0));
    if let Some(s) = state
        && s > 0.0
    {
        painter.add(Shape::line(
            arc(center, radius, GAUGE_SWEEP * s),
            Stroke::new(width, gauge_fill(visuals)),
        ));
    }

    if let Some(c) = reading.and_then(|r| r.commanded).map(|c| c.clamp(0.0, 1.0)) {
        let dir = Vec2::angled(GAUGE_START + GAUGE_SWEEP * c);
        painter.line_segment(
            [
                center + dir * (radius - width / 2.0),
                center + dir * (radius + width / 2.0),
            ],
            Stroke::new(2.0_f32, visuals.strong_text_color()),
        );
    }

    if blink && blink_on(time) {
        painter.circle_stroke(
            center,
            radius + width,
            Stroke::new(2.0_f32, readable(COLOR_INDICATOR_WARNING, visuals)),
        );
    }
}

fn arc(center: Pos2, radius: f32, sweep: f32) -> Vec<Pos2> {
    const STEPS: usize = 32;
    (0..=STEPS)
        .map(|i| {
            let angle = GAUGE_START + sweep * i as f32 / STEPS as f32;
            center + Vec2::angled(angle) * radius
        })
        .collect()
}
