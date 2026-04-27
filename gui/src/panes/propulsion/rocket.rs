use core::System;

use egui::epaint::PathShape;
use egui::{Color32, CornerRadius, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};
use mavspec::rust::dialects::common::messages::BatteryStatus;
use rapid_dialect::rapid::enums::ValveId;
use rapid_dialect::rapid::messages::{PressureVessel, Valve};

use crate::colors::COLOR_INDICATOR_WARNING;
use crate::widgets::MeasurementIndicator;

const TANK_BULKHEAD_RATIO: f32 = 0.15;
const TANK_BULKHEAD_STEPS: usize = 32;
const N2_MAX_PRESSURE_BAR: f32 = 300.0;
const N2O_MAX_PRESSURE_BAR: f32 = 100.0;
const CC_MAX_PRESSURE_BAR: f32 = 70.0;

pub(super) const N2_COLOR: Color32 = Color32::from_rgb(0x54, 0xc3, 0x54);
pub(super) const N2O_COLOR: Color32 = Color32::from_rgb(0x4e, 0xa8, 0xe8);
pub(super) const FUEL_COLOR: Color32 = Color32::from_rgb(0xc4, 0x7a, 0x3a);
pub(super) const CC_COLOR: Color32 = Color32::from_rgb(0xe0, 0x55, 0x2c);
const NODE_COLOR: Color32 = Color32::from_rgb(0x3f, 0xb8, 0xa4);

pub(super) fn valve_state(system: &System, id: ValveId) -> Option<f32> {
    system
        .last_instance_message::<Valve>(i64::from(id.value()))
        .ok()
        .map(|v| v.state)
}

pub fn draw_hybrid(ui: &mut egui::Ui, system: &System, square: Rect) {
    let center_x = square.center().x;
    let n = square.height();

    let tank_w = 0.11 * n;

    let tank_h = 0.22 * n;
    let bulkhead_h = TANK_BULKHEAD_RATIO * tank_h;

    let top_tank_h = 0.155 * n;
    let top_tank_rect = Rect::from_min_size(
        pos2(center_x - tank_w / 2.0, square.top() + 0.17 * n),
        egui::vec2(tank_w, top_tank_h),
    );

    let tank_rect = Rect::from_min_size(
        pos2(center_x - tank_w / 2.0, top_tank_rect.bottom() + 0.18 * n),
        egui::vec2(tank_w, tank_h),
    );

    let reg_cy = top_tank_rect.bottom() + 0.045 * n;
    let reg_to_tank = tank_rect.top() - reg_cy;
    let junction_cy = reg_cy + reg_to_tank / 3.0;
    let valve_top_cy = reg_cy + reg_to_tank * 2.0 / 3.0;

    let cc_h = 0.12 * n;
    let cc_w = 0.10 * n;
    let throat_w = 0.04 * n;
    let exit_w = 0.085 * n;
    let fuel_port_half = 0.0286 * n;
    let nozzle_h = 0.045 * n;
    let cc_top = tank_rect.bottom() + 0.09 * n;
    let cc_bottom = cc_top + cc_h;
    let throat_y = cc_bottom + nozzle_h * 0.2;
    let exit_y = cc_bottom + nozzle_h;
    let cc_cy = (cc_top + cc_bottom) / 2.0;

    let valve_half = 0.022 * n;
    let valve_bot_cy = (tank_rect.bottom() + cc_top) / 2.0;

    if let Some(indicator) = super::battery_indicator(system, true) {
        let battery_half_w = tank_w * 0.6;
        let battery_rect = Rect::from_min_max(
            pos2(center_x - battery_half_w, square.top() + 0.02 * n),
            pos2(center_x + battery_half_w, top_tank_rect.top() - 0.03 * n),
        );
        ui.place(battery_rect, indicator);
    }

    let tank_left = center_x - tank_w / 2.0;
    let tank_right = center_x + tank_w / 2.0;
    let pressure_cx = (square.left() + tank_left) / 2.0;
    let temp_cx = (tank_right + square.right()) / 2.0;

    let pressurant = system.last_instance_message::<PressureVessel>(0).ok();
    let oxidizer = system.last_instance_message::<PressureVessel>(1).ok();
    let chamber = system.last_instance_message::<PressureVessel>(2).ok();

    let pressure_bar = |p: u16| f32::from(p) / 100.0;
    let temperature_c = |t: i16| f32::from(t) / 100.0;

    let n2_pressure_bar = pressurant.as_ref().map(|p| pressure_bar(p.pressure1));
    let n2o_pressure1_bar = oxidizer.as_ref().map(|p| pressure_bar(p.pressure1));
    let n2o_pressure2_bar = oxidizer.as_ref().map(|p| pressure_bar(p.pressure2));
    let cc_pressure_bar = chamber.as_ref().map(|p| pressure_bar(p.pressure1));

    let n2o_temp1 = oxidizer.as_ref().map(|p| temperature_c(p.temperature1));
    let n2o_temp2 = oxidizer.as_ref().map(|p| temperature_c(p.temperature2));

    let tank_fill_level = oxidizer
        .as_ref()
        .map_or(0.0, |p| f32::from(p.level) / 10000.0);

    for (cy, color, values) in [
        (top_tank_rect.center().y, N2_COLOR, vec![n2_pressure_bar]),
        (junction_cy, NODE_COLOR, vec![Some(45.0)]),
        (
            tank_rect.center().y,
            N2O_COLOR,
            vec![n2o_pressure1_bar, n2o_pressure2_bar],
        ),
        (cc_cy, CC_COLOR, vec![cc_pressure_bar]),
    ] {
        let indicator = MeasurementIndicator {
            values,
            unit: "bar",
            color,
            decimals: None,
        };
        let size = indicator.intrinsic_size(ui.ctx());
        ui.place(
            Rect::from_center_size(pos2(pressure_cx, cy), size),
            indicator,
        );
    }

    let indicator = MeasurementIndicator {
        values: vec![n2o_temp1, n2o_temp2],
        unit: "\u{00b0}C",
        color: Color32::WHITE,
        decimals: Some(0),
    };
    let size = indicator.intrinsic_size(ui.ctx());
    ui.place(
        Rect::from_center_size(pos2(temp_cx, tank_rect.center().y), size),
        indicator,
    );

    let battery_temp = system
        .last_instance_message::<BatteryStatus>(1)
        .ok()
        .and_then(|b| (b.temperature != i16::MAX).then(|| temperature_c(b.temperature)));

    let battery_cy = square.top() + 0.08 * n;
    let battery_temp_cx = temp_cx + 0.02 * n;
    let indicator = MeasurementIndicator {
        values: vec![battery_temp],
        unit: "\u{00b0}C",
        color: Color32::WHITE,
        decimals: Some(0),
    };
    let size = indicator.intrinsic_size(ui.ctx());
    ui.place(
        Rect::from_center_size(pos2(battery_temp_cx, battery_cy), size),
        indicator,
    );

    let stroke_col = ui.visuals().weak_text_color();
    let stroke = Stroke::new(1.5, stroke_col);
    let fill = ui.visuals().extreme_bg_color;
    let painter = ui.painter().clone();
    let hatch_stride = (0.012 * n).max(4.0);

    draw_capsule_tank(&painter, top_tank_rect, bulkhead_h, fill, stroke);
    draw_hatching(
        &painter,
        &capsule_polygon(top_tank_rect, bulkhead_h),
        N2_COLOR,
        pressure_coverage(n2_pressure_bar.unwrap_or(0.0), N2_MAX_PRESSURE_BAR),
        hatch_stride,
    );

    draw_capsule_tank(&painter, tank_rect, bulkhead_h, fill, stroke);
    draw_tank_fill(
        &painter,
        tank_rect,
        bulkhead_h,
        tank_fill_level,
        pressure_coverage(n2o_pressure1_bar.unwrap_or(0.0), N2O_MAX_PRESSURE_BAR),
        N2O_COLOR,
        stroke,
        hatch_stride,
    );

    let tank_fill_indicator = MeasurementIndicator {
        values: vec![Some(tank_fill_level * 100.0)],
        unit: "%",
        color: Color32::WHITE,
        decimals: Some(0),
    };
    let intrinsic = MeasurementIndicator {
        values: vec![Some(99.0)],
        unit: "%",
        color: Color32::WHITE,
        decimals: Some(0),
    }
    .intrinsic_size(ui.ctx());
    let pad = ui.ctx().style().spacing.button_padding;
    let size = egui::vec2(intrinsic.x - 2.0 * pad.x + 6.0, intrinsic.y);
    ui.place(
        Rect::from_center_size(tank_rect.center(), size),
        tank_fill_indicator,
    );
    let valve_fill_closed = Color32::BLACK;
    let valve_fill_open = COLOR_INDICATOR_WARNING;
    let valve_stroke_closed = Stroke::new(1.5, Color32::WHITE);
    let valve_stroke_open = Stroke::new(1.5, COLOR_INDICATOR_WARNING);
    let style_for = |state: Option<f32>| match state {
        Some(s) if s > 0.0 => (valve_stroke_open, valve_fill_open),
        _ => (valve_stroke_closed, valve_fill_closed),
    };
    let (stroke_pressurant_vent, fill_pressurant_vent) =
        style_for(valve_state(system, ValveId::PressurantVent));
    let (valve_stroke_pressurization, valve_fill_pressurization) =
        style_for(valve_state(system, ValveId::Pressurization));
    let (stroke_oxidizer_vent, fill_oxidizer_vent) =
        style_for(valve_state(system, ValveId::OxidizerVent));
    let (stroke_oxidizer_fill, fill_oxidizer_fill) =
        style_for(valve_state(system, ValveId::OxidizerFill));
    let (valve_stroke_main, valve_fill_main) = style_for(valve_state(system, ValveId::Main));

    draw_pressure_regulator(&painter, pos2(center_x, reg_cy), valve_half, stroke);
    draw_generic_valve(
        &painter,
        pos2(center_x, valve_top_cy),
        valve_half,
        valve_stroke_pressurization,
        valve_fill_pressurization,
    );
    interact_valve(ui, &painter, system, pos2(center_x, valve_top_cy), valve_half, ValveId::Pressurization, false);
    painter.line(
        vec![
            top_tank_rect.center_bottom(),
            pos2(center_x, reg_cy - valve_half),
        ],
        stroke,
    );
    let junction_r = valve_half * 0.45;
    painter.line(
        vec![
            pos2(center_x, reg_cy + valve_half),
            pos2(center_x, junction_cy - junction_r),
        ],
        stroke,
    );
    painter.line(
        vec![
            pos2(center_x, junction_cy + junction_r),
            pos2(center_x, valve_top_cy - valve_half),
        ],
        stroke,
    );
    painter.line(
        vec![
            pos2(center_x, valve_top_cy + valve_half),
            tank_rect.center_top(),
        ],
        stroke,
    );
    painter.circle_filled(pos2(center_x, junction_cy), junction_r, NODE_COLOR);

    let vent_end_x = square.right();
    let vent_valve_cx = (center_x + tank_w * 0.35 + vent_end_x) / 2.0;
    draw_generic_valve_horizontal(
        &painter,
        pos2(vent_valve_cx, junction_cy),
        valve_half,
        stroke_pressurant_vent,
        fill_pressurant_vent,
    );
    interact_valve(ui, &painter, system, pos2(vent_valve_cx, junction_cy), valve_half, ValveId::PressurantVent, true);
    painter.line(
        vec![
            pos2(center_x + junction_r, junction_cy),
            pos2(vent_valve_cx - valve_half, junction_cy),
        ],
        stroke,
    );
    painter.line(
        vec![
            pos2(vent_valve_cx + valve_half, junction_cy),
            pos2(vent_end_x, junction_cy),
        ],
        stroke,
    );

    let tank_vent_x = center_x + tank_w * 0.35;
    let tank_vent_y = tank_rect.top() - 0.025 * n;
    let tank_vent_start_y = {
        let half_w = tank_w / 2.0;
        let ratio = (tank_vent_x - center_x) / half_w;
        tank_rect.top() + bulkhead_h * (1.0 - (1.0 - ratio * ratio).sqrt())
    };
    let tank_vent_valve_cx = vent_valve_cx;
    painter.line(
        vec![
            pos2(tank_vent_x, tank_vent_start_y),
            pos2(tank_vent_x, tank_vent_y),
        ],
        stroke,
    );
    draw_generic_valve_horizontal(
        &painter,
        pos2(tank_vent_valve_cx, tank_vent_y),
        valve_half,
        stroke_oxidizer_vent,
        fill_oxidizer_vent,
    );
    interact_valve(ui, &painter, system, pos2(tank_vent_valve_cx, tank_vent_y), valve_half, ValveId::OxidizerVent, true);
    painter.line(
        vec![
            pos2(tank_vent_x, tank_vent_y),
            pos2(tank_vent_valve_cx - valve_half, tank_vent_y),
        ],
        stroke,
    );
    painter.line(
        vec![
            pos2(tank_vent_valve_cx + valve_half, tank_vent_y),
            pos2(vent_end_x, tank_vent_y),
        ],
        stroke,
    );

    let tank_vent_bot_x = center_x - tank_w * 0.35;
    let tank_vent_bot_y = tank_rect.bottom() + 0.025 * n;
    let tank_vent_bot_start_y = {
        let half_w = tank_w / 2.0;
        let ratio = (tank_vent_bot_x - center_x) / half_w;
        tank_rect.bottom() - bulkhead_h * (1.0 - (1.0 - ratio * ratio).sqrt())
    };
    let bot_vent_end_x = square.left();
    let bot_vent_valve_cx = 2.0 * center_x - vent_valve_cx;
    painter.line(
        vec![
            pos2(tank_vent_bot_x, tank_vent_bot_start_y),
            pos2(tank_vent_bot_x, tank_vent_bot_y),
        ],
        stroke,
    );
    draw_generic_valve_horizontal(
        &painter,
        pos2(bot_vent_valve_cx, tank_vent_bot_y),
        valve_half,
        stroke_oxidizer_fill,
        fill_oxidizer_fill,
    );
    interact_valve(ui, &painter, system, pos2(bot_vent_valve_cx, tank_vent_bot_y), valve_half, ValveId::OxidizerFill, true);
    painter.line(
        vec![
            pos2(tank_vent_bot_x, tank_vent_bot_y),
            pos2(bot_vent_valve_cx + valve_half, tank_vent_bot_y),
        ],
        stroke,
    );
    painter.line(
        vec![
            pos2(bot_vent_valve_cx - valve_half, tank_vent_bot_y),
            pos2(bot_vent_end_x, tank_vent_bot_y),
        ],
        stroke,
    );

    let top_tank_fill_x = center_x - tank_w * 0.35;
    let top_tank_fill_y = top_tank_rect.bottom() + 0.025 * n;
    let top_tank_fill_start_y = {
        let half_w = tank_w / 2.0;
        let ratio = (top_tank_fill_x - center_x) / half_w;
        top_tank_rect.bottom() - bulkhead_h * (1.0 - (1.0 - ratio * ratio).sqrt())
    };
    painter.line(
        vec![
            pos2(top_tank_fill_x, top_tank_fill_start_y),
            pos2(top_tank_fill_x, top_tank_fill_y),
        ],
        stroke,
    );
    painter.line(
        vec![
            pos2(top_tank_fill_x, top_tank_fill_y),
            pos2(square.left(), top_tank_fill_y),
        ],
        stroke,
    );

    draw_generic_valve(
        &painter,
        pos2(center_x, valve_bot_cy),
        valve_half,
        valve_stroke_main,
        valve_fill_main,
    );
    interact_valve(ui, &painter, system, pos2(center_x, valve_bot_cy), valve_half, ValveId::Main, false);
    painter.line(
        vec![
            tank_rect.center_bottom(),
            pos2(center_x, valve_bot_cy - valve_half),
        ],
        stroke,
    );
    painter.line(
        vec![
            pos2(center_x, valve_bot_cy + valve_half),
            pos2(center_x, cc_top),
        ],
        stroke,
    );

    let chamber_path = vec![
        pos2(center_x - cc_w / 2.0, cc_top),
        pos2(center_x + cc_w / 2.0, cc_top),
        pos2(center_x + cc_w / 2.0, cc_bottom),
        pos2(center_x + throat_w / 2.0, throat_y),
        pos2(center_x + exit_w / 2.0, exit_y),
        pos2(center_x - exit_w / 2.0, exit_y),
        pos2(center_x - throat_w / 2.0, throat_y),
        pos2(center_x - cc_w / 2.0, cc_bottom),
    ];

    let cc_interior = Rect::from_min_max(
        pos2(center_x - cc_w / 2.0, cc_top),
        pos2(center_x + cc_w / 2.0, cc_bottom),
    );
    let cc_polygon = vec![
        pos2(cc_interior.left(), cc_interior.top()),
        pos2(cc_interior.right(), cc_interior.top()),
        pos2(cc_interior.right(), cc_interior.bottom()),
        pos2(cc_interior.left(), cc_interior.bottom()),
    ];
    draw_hatching(
        &painter,
        &cc_polygon,
        CC_COLOR,
        pressure_coverage(cc_pressure_bar.unwrap_or(0.0), CC_MAX_PRESSURE_BAR),
        hatch_stride,
    );

    draw_fuel_grain(&painter, cc_interior, fuel_port_half, stroke, FUEL_COLOR);

    painter.add(Shape::Path(PathShape::closed_line(chamber_path, stroke)));
}

fn pressure_coverage(pressure_bar: f32, max_bar: f32) -> f32 {
    (pressure_bar / max_bar).clamp(0.0, 1.0)
}

fn capsule_polygon(rect: Rect, bulkhead_h: f32) -> Vec<Pos2> {
    let cx = rect.center().x;
    let half_w = rect.width() / 2.0;
    let top = rect.top();
    let bot = rect.bottom();

    let steps = TANK_BULKHEAD_STEPS * 2;
    let mut path: Vec<Pos2> = Vec::with_capacity((steps + 1) * 2);
    for i in 0..=steps {
        let r = std::f32::consts::PI * (i as f32) / (steps as f32);
        let x = cx - half_w * r.cos();
        let y = top + bulkhead_h * (1.0 - r.sin());
        path.push(pos2(x, y));
    }
    for i in 0..=steps {
        let r = std::f32::consts::PI * (i as f32) / (steps as f32);
        let x = cx + half_w * r.cos();
        let y = bot - bulkhead_h * (1.0 - r.sin());
        path.push(pos2(x, y));
    }
    path
}

fn tank_silhouette_half_width(rect: Rect, bulkhead_h: f32, y: f32) -> f32 {
    let half_w = rect.width() / 2.0;
    let top = rect.top();
    let bot = rect.bottom();
    let s = if y >= bot - bulkhead_h {
        1.0 - (bot - y) / bulkhead_h
    } else if y <= top + bulkhead_h {
        1.0 - (y - top) / bulkhead_h
    } else {
        return half_w;
    };
    let s = s.clamp(-1.0, 1.0);
    half_w * (1.0 - s * s).max(0.0).sqrt()
}

fn tank_fill_polygon(rect: Rect, bulkhead_h: f32, level: f32) -> Vec<Pos2> {
    let level = level.clamp(0.0, 1.0);
    let cx = rect.center().x;
    let half_w = rect.width() / 2.0;
    let top = rect.top();
    let bot = rect.bottom();
    let fill_y = bot - level * rect.height();

    let steps = TANK_BULKHEAD_STEPS * 2;
    let mut path: Vec<Pos2> = Vec::with_capacity(steps * 3 + 4);

    if fill_y >= bot - bulkhead_h {
        // Surface within the lower bulkhead: polygon is a circular segment.
        let s = (1.0 - (bot - fill_y) / bulkhead_h).clamp(-1.0, 1.0);
        let r_start = s.asin();
        let r_end = std::f32::consts::PI - r_start;
        for i in 0..=steps {
            let r = r_start + (r_end - r_start) * (i as f32) / (steps as f32);
            let x = cx + half_w * r.cos();
            let y = bot - bulkhead_h * (1.0 - r.sin());
            path.push(pos2(x, y));
        }
    } else if fill_y <= top + bulkhead_h {
        // Surface within the upper bulkhead: bottom dome + walls + partial top dome.
        for i in 0..=steps {
            let r = std::f32::consts::PI * (i as f32) / (steps as f32);
            let x = cx + half_w * r.cos();
            let y = bot - bulkhead_h * (1.0 - r.sin());
            path.push(pos2(x, y));
        }
        path.push(pos2(cx - half_w, top + bulkhead_h));
        let s_top = (1.0 - (fill_y - top) / bulkhead_h).clamp(-1.0, 1.0);
        let r_top = s_top.asin();
        for i in 0..=steps {
            let r = r_top * (i as f32) / (steps as f32);
            let x = cx - half_w * r.cos();
            let y = top + bulkhead_h * (1.0 - r.sin());
            path.push(pos2(x, y));
        }
        let r_right_start = std::f32::consts::PI - r_top;
        for i in 0..=steps {
            let r = r_right_start + r_top * (i as f32) / (steps as f32);
            let x = cx - half_w * r.cos();
            let y = top + bulkhead_h * (1.0 - r.sin());
            path.push(pos2(x, y));
        }
    } else {
        // Surface within the straight section.
        path.push(pos2(cx + half_w, fill_y));
        for i in 0..=steps {
            let r = std::f32::consts::PI * (i as f32) / (steps as f32);
            let x = cx + half_w * r.cos();
            let y = bot - bulkhead_h * (1.0 - r.sin());
            path.push(pos2(x, y));
        }
        path.push(pos2(cx - half_w, fill_y));
    }
    path
}

fn draw_capsule_tank(
    painter: &egui::Painter,
    rect: Rect,
    bulkhead_h: f32,
    fill: Color32,
    stroke: Stroke,
) {
    let path = capsule_polygon(rect, bulkhead_h);
    painter.add(Shape::Path(PathShape::convex_polygon(path, fill, stroke)));
}

fn draw_tank_fill(
    painter: &egui::Painter,
    rect: Rect,
    bulkhead_h: f32,
    level: f32,
    coverage: f32,
    color: Color32,
    stroke: Stroke,
    hatch_stride: f32,
) {
    let level = level.clamp(0.0, 1.0);
    if level <= 0.0 {
        return;
    }

    let polygon = tank_fill_polygon(rect, bulkhead_h, level);
    draw_hatching(painter, &polygon, color, coverage, hatch_stride);

    let cx = rect.center().x;
    let fill_y = rect.bottom() - level * rect.height();
    let surface_half_w = tank_silhouette_half_width(rect, bulkhead_h, fill_y);
    painter.line(
        vec![
            pos2(cx - surface_half_w, fill_y),
            pos2(cx + surface_half_w, fill_y),
        ],
        stroke,
    );
}

fn draw_hatching(
    painter: &egui::Painter,
    polygon: &[Pos2],
    color: Color32,
    coverage: f32,
    stride: f32,
) {
    let coverage = coverage.clamp(0.0, 1.0);
    if coverage <= 0.0 || polygon.is_empty() {
        return;
    }
    if coverage >= 1.0 {
        painter.add(Shape::Path(PathShape::convex_polygon(
            polygon.to_vec(),
            color,
            Stroke::NONE,
        )));
        return;
    }

    let line_width = coverage * stride;
    let perp = egui::vec2(
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    );
    let dir = egui::vec2(
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    );

    let sum: egui::Vec2 = polygon
        .iter()
        .fold(egui::Vec2::ZERO, |acc, v| acc + v.to_vec2());
    let center = (sum / polygon.len() as f32).to_pos2();

    let mut s_min = f32::INFINITY;
    let mut s_max = f32::NEG_INFINITY;
    let mut t_min = f32::INFINITY;
    let mut t_max = f32::NEG_INFINITY;
    for v in polygon {
        let off = *v - center;
        let s = off.dot(perp);
        let t = off.dot(dir);
        s_min = s_min.min(s);
        s_max = s_max.max(s);
        t_min = t_min.min(t);
        t_max = t_max.max(t);
    }

    let margin = stride.max(line_width);
    let t_lo = t_min - margin;
    let t_hi = t_max + margin;

    let span = s_max - s_min;
    let n_stripes = (span / stride).ceil().max(1.0) as i32;
    let s0 = (s_min + s_max - n_stripes as f32 * stride) / 2.0;

    let half_w = line_width / 2.0;
    for k in 0..=n_stripes {
        let s = s0 + k as f32 * stride;
        let base = center + s * perp;
        let stripe = [
            base + t_lo * dir - half_w * perp,
            base + t_hi * dir - half_w * perp,
            base + t_hi * dir + half_w * perp,
            base + t_lo * dir + half_w * perp,
        ];
        let clipped = clip_polygon_to_convex(&stripe, polygon);
        if clipped.len() >= 3 {
            painter.add(Shape::Path(PathShape::convex_polygon(
                clipped,
                color,
                Stroke::NONE,
            )));
        }
    }
}

fn clip_polygon_to_convex(subject: &[Pos2], clip_polygon: &[Pos2]) -> Vec<Pos2> {
    if clip_polygon.is_empty() {
        return subject.to_vec();
    }
    let sum: egui::Vec2 = clip_polygon
        .iter()
        .fold(egui::Vec2::ZERO, |acc, v| acc + v.to_vec2());
    let center = (sum / clip_polygon.len() as f32).to_pos2();

    let mut output: Vec<Pos2> = subject.to_vec();
    for i in 0..clip_polygon.len() {
        if output.is_empty() {
            break;
        }
        let input = std::mem::take(&mut output);

        let edge_a = clip_polygon[i];
        let edge_b = clip_polygon[(i + 1) % clip_polygon.len()];
        let edge = edge_b - edge_a;
        let mut normal = egui::vec2(-edge.y, edge.x);
        if normal.dot(center - edge_a) < 0.0 {
            normal = -normal;
        }

        for j in 0..input.len() {
            let s = input[j];
            let e = input[(j + 1) % input.len()];
            let ds = (s - edge_a).dot(normal);
            let de = (e - edge_a).dot(normal);
            let s_in = ds >= 0.0;
            let e_in = de >= 0.0;

            if e_in {
                if !s_in {
                    let t = ds / (ds - de);
                    output.push(s + t * (e - s));
                }
                output.push(e);
            } else if s_in {
                let t = ds / (ds - de);
                output.push(s + t * (e - s));
            }
        }
    }
    output
}

fn draw_fuel_grain(
    painter: &egui::Painter,
    chamber: Rect,
    port_half: f32,
    stroke: Stroke,
    color: Color32,
) {
    let inset_y = chamber.height() * 0.12;

    let cx = chamber.center().x;
    let top = chamber.top() + inset_y;
    let bot = chamber.bottom() - inset_y;

    let left = Rect::from_min_max(pos2(chamber.left(), top), pos2(cx - port_half, bot));
    let right = Rect::from_min_max(pos2(cx + port_half, top), pos2(chamber.right(), bot));

    for r in [left, right] {
        painter.rect(r, CornerRadius::ZERO, color, stroke, StrokeKind::Middle);
    }
}

fn draw_pressure_regulator(painter: &egui::Painter, center: Pos2, half: f32, stroke: Stroke) {
    let box_rect = Rect::from_center_size(center, egui::vec2(half * 1.8, half * 2.0));
    painter.rect(
        box_rect,
        CornerRadius::ZERO,
        Color32::TRANSPARENT,
        stroke,
        StrokeKind::Middle,
    );

    let arrow_start = pos2(center.x, box_rect.top() + half * 0.2);
    let arrow_end = pos2(center.x, box_rect.bottom() - half * 0.3);
    painter.line(vec![arrow_start, arrow_end], stroke);

    let head_w = half * 0.25;
    let head_h = half * 0.2;
    painter.add(Shape::convex_polygon(
        vec![
            arrow_end,
            pos2(arrow_end.x - head_h, arrow_end.y - head_w),
            pos2(arrow_end.x + head_h, arrow_end.y - head_w),
        ],
        stroke.color,
        Stroke::NONE,
    ));

    let d = half * 0.9;
    let e = half * 0.5;
    let path = vec![
        pos2(box_rect.left(), center.y),
        pos2(box_rect.left() - d, center.y),
        pos2(box_rect.left() - d, box_rect.bottom() + e),
        pos2(box_rect.center().x - e, box_rect.bottom() + e),
        pos2(box_rect.center().x, box_rect.bottom()),
    ];
    for shape in Shape::dashed_line(&path, stroke, half * 0.3, half * 0.2) {
        painter.add(shape);
    }
}

fn draw_generic_valve(
    painter: &egui::Painter,
    center: Pos2,
    half: f32,
    stroke: Stroke,
    fill: Color32,
) {
    let base_half = half * 0.65;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(center.x - base_half, center.y - half),
            pos2(center.x + base_half, center.y - half),
            center,
        ],
        fill,
        stroke,
    ));
    painter.add(Shape::convex_polygon(
        vec![
            pos2(center.x - base_half, center.y + half),
            pos2(center.x + base_half, center.y + half),
            center,
        ],
        fill,
        stroke,
    ));
}

fn interact_valve(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    system: &System,
    center: Pos2,
    half: f32,
    id: ValveId,
    horizontal: bool,
) {
    let size = if horizontal {
        egui::vec2(half * 2.0, half * 1.6)
    } else {
        egui::vec2(half * 1.6, half * 2.0)
    };
    let rect = Rect::from_center_size(center, size);
    let resp = ui.interact(rect, egui::Id::new(("valve", id.value())), egui::Sense::click());
    if resp.clicked() {
        let currently_open = matches!(valve_state(system, id), Some(s) if s > 0.0);
        system.do_set_valve(id, if currently_open { 0.0 } else { 1.0 });
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        painter.rect(
            rect.expand(2.0),
            CornerRadius::same(2),
            Color32::from_white_alpha(15),
            Stroke::new(1.0, Color32::from_white_alpha(60)),
            StrokeKind::Middle,
        );
    }
}

fn draw_generic_valve_horizontal(
    painter: &egui::Painter,
    center: Pos2,
    half: f32,
    stroke: Stroke,
    fill: Color32,
) {
    let base_half = half * 0.65;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(center.x - half, center.y - base_half),
            pos2(center.x - half, center.y + base_half),
            center,
        ],
        fill,
        stroke,
    ));
    painter.add(Shape::convex_polygon(
        vec![
            pos2(center.x + half, center.y - base_half),
            pos2(center.x + half, center.y + base_half),
            center,
        ],
        fill,
        stroke,
    ));
}
