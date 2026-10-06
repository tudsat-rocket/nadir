use nadir_core::System;

use egui::epaint::{PathShape, PathStroke};
use egui::{
    Align2, Area, Button, Color32, CornerRadius, FontId, Id, Order, Pos2, Rangef, Rect, RichText,
    Shape, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use rapid_dialect::rapid::enums::{PressureVesselFlag, ValveFlag, ValveId};
use rapid_dialect::rapid::messages::{PressureVessel, Valve};

use super::ValveInteractionMode;
use crate::colors::{
    COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, blink_on, dim, readable, schematic_ink,
    schematic_line, schematic_void, schematic_wash,
};
use crate::widgets::{BatteryIndicator, Hazard, MeasurementIndicator, Readout};

const TANK_BULKHEAD_RATIO: f32 = 0.15;
const TANK_BULKHEAD_STEPS: usize = 32;
// Narrower hatch lines are stroked rather than filled as polygons. epaint anti-aliases a fill by
// pulling its inner ring in by half a feather width per side, which leaves a sliver this thin with
// a sub-pixel core that the rasterizer samples into a hard staircase; a stroke has a dedicated
// thin-line path (a feather-wide ridge with scaled opacity) that stays smooth at any width.
const MIN_FILLED_HATCH_WIDTH: f32 = 2.0;
// Floor on a hatch line's width, in the same units. A stroke below a pixel is drawn by scaling its
// opacity, so this is where the line stops fading rather than where it stops thinning.
const MIN_HATCH_WIDTH: f32 = 0.35;
// A valve's bow-tie is `half` long along its axis and this much of `half` across it.
const VALVE_GLYPH_BASE_RATIO: f32 = 0.65;
const LABEL_GAP: f32 = 3.0;

const N2_MAX_PRESSURE_BAR: f32 = 300.0;
const N2O_MAX_PRESSURE_BAR: f32 = 100.0;
const CC_MAX_PRESSURE_BAR: f32 = 70.0;

// Opacity applied to a ground-support tank's valves and plumbing when the tank
// isn't reporting, muting controls that currently can't do anything.
//
// Fading ink toward white gives up luminance faster than fading it toward black gains it.
fn gse_muted_opacity(visuals: &egui::Visuals) -> f32 {
    if visuals.dark_mode { 0.3 } else { 0.45 }
}

pub(super) const N2_COLOR: Color32 = Color32::from_rgb(0x54, 0xc3, 0x54);
pub(super) const N2O_COLOR: Color32 = Color32::from_rgb(0x4e, 0xa8, 0xe8);
pub(super) const FUEL_COLOR: Color32 = Color32::from_rgb(0xc4, 0x7a, 0x3a);
pub(super) const CC_COLOR: Color32 = Color32::from_rgb(0xe0, 0x55, 0x2c);
// The regulated-pressurant node (post-regulator volume) reuses the junction hue.
pub(super) const NODE_COLOR: Color32 = Color32::from_rgb(0x3f, 0xb8, 0xa4);
// Ground-support tanks: darker variants of the onboard fluid colors so the
// external volumes read as related-but-distinct in the schematic and plots.
pub(super) const EXT_N2_COLOR: Color32 = Color32::from_rgb(0x3c, 0x8c, 0x3c);
pub(super) const EXT_N2O_COLOR: Color32 = Color32::from_rgb(0x38, 0x79, 0xa7);

/// For the contrast tests in [`crate::colors`].
#[cfg(test)]
pub(super) fn fluid_colors() -> [(&'static str, Color32); 7] {
    [
        ("pressurant", N2_COLOR),
        ("oxidizer", N2O_COLOR),
        ("fuel", FUEL_COLOR),
        ("combustion chamber", CC_COLOR),
        ("regulated pressurant", NODE_COLOR),
        ("external pressurant", EXT_N2_COLOR),
        ("external oxidizer", EXT_N2O_COLOR),
    ]
}

// A valve's reported position (`state`) alongside the vehicle-reported intended
// position (`commanded`). Both are 0.0 (closed) ..= 1.0 (open); the firmware sends
// NaN for an unknown value, which we carry as `None`.
#[derive(Copy, Clone)]
pub(super) struct ValveReading {
    pub state: Option<f32>,
    pub commanded: Option<f32>,
}

// Positions further apart than this are treated as a real mismatch rather than
// normal actuator travel. Fractional, so it covers binary and servo valves alike.
pub(super) const VALVE_MISMATCH_DEADBAND: f32 = 0.1;

pub(super) fn valve_reading(system: &System, id: ValveId) -> Option<ValveReading> {
    system
        .last_instance_message::<Valve>(i64::from(id.value()))
        .ok()
        .map(|v| ValveReading {
            state: v.state.is_finite().then_some(v.state),
            commanded: v.commanded.is_finite().then_some(v.commanded),
        })
}

pub(super) fn valve_state(system: &System, id: ValveId) -> Option<f32> {
    valve_reading(system, id).and_then(|r| r.state)
}

// An unknown (NaN) reported state is treated as a fault for now. Otherwise a
// mismatch is a known reported state disagreeing with a known command; an unknown
// command alone is not a warning.
pub(super) fn valve_mismatch(r: ValveReading) -> bool {
    match r.state {
        None => true,
        Some(s) => r
            .commanded
            .is_some_and(|c| (c - s).abs() > VALVE_MISMATCH_DEADBAND),
    }
}

// A pressure vessel warns when it reports an overpressure. No vessel means
// nothing to warn about.
fn vessel_warn(v: Option<&PressureVessel>) -> bool {
    v.is_some_and(|v| v.flags.contains(PressureVesselFlag::OVERPRESSURE))
}

// An external/offboard (ground support) vessel, as flagged by the firmware.
fn is_external(v: Option<&PressureVessel>) -> bool {
    v.is_some_and(|v| v.flags.contains(PressureVesselFlag::EXTERNAL))
}

// N2 and N2O are the fluids' actual names.
#[allow(clippy::similar_names)]
pub fn draw_hybrid(
    ui: &mut egui::Ui,
    system: &System,
    square: Rect,
    mode: &mut ValveInteractionMode,
    pulse: &mut usize,
    pulse_durations: [f32; 3],
    blink: [bool; super::VALVE_COUNT],
) {
    let pulse_secs = pulse_durations[*pulse];
    let visuals = ui.visuals().clone();
    let n2_color = readable(N2_COLOR, &visuals);
    let n2o_color = readable(N2O_COLOR, &visuals);
    let fuel_color = readable(FUEL_COLOR, &visuals);
    let cc_color = readable(CC_COLOR, &visuals);
    let node_color = readable(NODE_COLOR, &visuals);
    let ext_n2_color = readable(EXT_N2_COLOR, &visuals);
    let ext_n2o_color = readable(EXT_N2O_COLOR, &visuals);

    // Keep the mismatch blink animating even when no telemetry frame arrives.
    if blink.iter().any(|&b| b) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(60));
    }

    // Reserve the left portion of the strip as a ground-support lane for the
    // external tanks and fill valves; the flight plant is drawn in the remainder.
    // Shadowing `square` insets every downstream computation without touching it.
    let strip = square;
    // The external tanks stack above their fill valves and hug the boundary, so the
    // lane only needs to be a tank plus its P-tag wide (sized in height units to match
    // the rest of the layout).
    let ground_lane_w = 0.112 * strip.height();
    let square = Rect::from_min_max(pos2(strip.left() + ground_lane_w, strip.top()), strip.max);

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
    let cc_cy = f32::midpoint(cc_top, cc_bottom);

    let valve_half = 0.022 * n;
    let valve_bot_cy = f32::midpoint(tank_rect.bottom(), cc_top);

    let batteries = super::battery_indicators(system, true);
    let battery_count = batteries.len() as f32;
    let battery_band = Rangef::new(square.top() + 0.005 * n, top_tank_rect.top() - 0.035 * n);
    let battery_size = BatteryIndicator::compact_size(
        ui,
        vec2(
            f32::min(
                0.1 * n,
                (square.width() - (battery_count + 1.0) * 0.008 * n) / battery_count,
            ),
            battery_band.span(),
        ),
    );
    let battery_gap = f32::min(
        (square.width() - battery_count * battery_size.x) / (battery_count + 1.0),
        0.03 * n,
    );
    let batteries_w = battery_count * (battery_size.x + battery_gap) - battery_gap;
    for (i, indicator) in batteries.into_iter().enumerate() {
        let left = center_x - batteries_w / 2.0 + i as f32 * (battery_size.x + battery_gap);
        let battery_rect = Rect::from_min_size(
            pos2(left, battery_band.center() - battery_size.y / 2.0),
            battery_size,
        );
        ui.place(battery_rect, indicator);
    }

    let tank_left = center_x - tank_w / 2.0;
    let tank_right = center_x + tank_w / 2.0;
    let pressure_cx = f32::midpoint(square.left(), tank_left);
    let temp_cx = f32::midpoint(tank_right, square.right());

    let pressurant = system.last_instance_message::<PressureVessel>(0).ok();
    let oxidizer = system.last_instance_message::<PressureVessel>(1).ok();
    let chamber = system.last_instance_message::<PressureVessel>(2).ok();
    // Post-regulator volume (carries the PReg sensors); occupies the junction node.
    let regulated = system.last_instance_message::<PressureVessel>(3).ok();
    // Ground-support tanks, only populated while filling.
    let ext_pressurant = system.last_instance_message::<PressureVessel>(4).ok();
    let ext_oxidizer = system.last_instance_message::<PressureVessel>(5).ok();

    // Firmware reports an unavailable sensor as the type's max value.
    let pressure_bar = |p: u16| (p != u16::MAX).then(|| f32::from(p) / 100.0);
    let temperature_c = |t: i16| (t != i16::MAX).then(|| f32::from(t) / 100.0);

    let reg_pressure1_bar = regulated.as_ref().and_then(|p| pressure_bar(p.pressure1));
    let reg_pressure2_bar = regulated.as_ref().and_then(|p| pressure_bar(p.pressure2));
    let ext_pressurant_bar = ext_pressurant
        .as_ref()
        .and_then(|p| pressure_bar(p.pressure1));
    let ext_oxidizer_bar = ext_oxidizer
        .as_ref()
        .and_then(|p| pressure_bar(p.pressure1));
    let ext_oxidizer_level = ext_oxidizer
        .as_ref()
        .and_then(|p| (p.level != u16::MAX).then(|| f32::from(p.level) / 10000.0));

    // A ground-support tank is available while the firmware reports it (flagged
    // EXTERNAL) with a valid pressure; otherwise its controls are muted.
    let ext_pressurant_available =
        is_external(ext_pressurant.as_ref()) && ext_pressurant_bar.is_some();
    let ext_oxidizer_available = is_external(ext_oxidizer.as_ref()) && ext_oxidizer_bar.is_some();

    let n2_pressure_bar = pressurant.as_ref().and_then(|p| pressure_bar(p.pressure1));
    let n2o_pressure1_bar = oxidizer.as_ref().and_then(|p| pressure_bar(p.pressure1));
    let n2o_pressure2_bar = oxidizer.as_ref().and_then(|p| pressure_bar(p.pressure2));
    let cc_pressure_bar = chamber.as_ref().and_then(|p| pressure_bar(p.pressure1));

    let n2o_temp1 = oxidizer
        .as_ref()
        .and_then(|p| temperature_c(p.temperature1));
    let n2o_temp2 = oxidizer
        .as_ref()
        .and_then(|p| temperature_c(p.temperature2));

    // Firmware reports an unknown level as u16::MAX.
    let tank_fill_level = oxidizer
        .as_ref()
        .and_then(|p| (p.level != u16::MAX).then(|| f32::from(p.level) / 10000.0));

    for (cy, color, values, warn) in [
        (
            top_tank_rect.center().y,
            n2_color,
            vec![n2_pressure_bar],
            vessel_warn(pressurant.as_ref()),
        ),
        (
            junction_cy,
            node_color,
            vec![reg_pressure1_bar, reg_pressure2_bar],
            vessel_warn(regulated.as_ref()),
        ),
        (
            tank_rect.center().y,
            n2o_color,
            vec![n2o_pressure1_bar, n2o_pressure2_bar],
            vessel_warn(oxidizer.as_ref()),
        ),
        (
            cc_cy,
            cc_color,
            vec![cc_pressure_bar],
            vessel_warn(chamber.as_ref()),
        ),
    ] {
        // Warn on an overpressure flag or a missing reading.
        let blink = warn || values.iter().any(Option::is_none);
        let indicator = MeasurementIndicator {
            values,
            unit: "bar",
            color,
            decimals: None,
            blink,
        };
        let size = indicator.intrinsic_size(ui.ctx());
        ui.place(
            Rect::from_center_size(pos2(pressure_cx, cy), size),
            indicator,
        );
    }

    let stroke_col = schematic_line(&visuals);
    let stroke = Stroke::new(1.5_f32, stroke_col);
    let fill = schematic_void(&visuals);
    let painter = ui.painter().clone();
    let hatch_stride = (0.012 * n).max(4.0);

    // Instance-ID tags next to each vessel and valve, so a reading can be cross-referenced against
    // the raw MAVLink logs (PressureVessel instance / ValveId).
    let label_font = egui::FontId::monospace(9.5);
    let label_color = schematic_line(&visuals);
    // Off the top-left corner, not inside: a capsule's widest point is `bulkhead_h` below the top
    // of its bounding rect, so the corner itself is empty.
    let draw_tank_label = |rect: Rect, id: i64, color: Color32| {
        let galley = painter.layout_no_wrap(format!("P{id}"), label_font.clone(), color);
        let pos = rect.left_top() - galley.size() + Vec2::splat(LABEL_GAP);
        painter.galley(pos.max(strip.left_top()), galley, color);
    };
    let draw_valve_label = |center: Pos2, half: f32, horizontal: bool, id: ValveId, muted: bool| {
        let color = readable(super::Valve::color(id), &visuals);
        let color = if muted {
            dim(color, gse_muted_opacity(&visuals))
        } else {
            color
        };
        // Off the glyph's own edge, not its bounding `half`, or the tag hangs out far enough
        // to look like it labels whatever sits above.
        let edge = half * VALVE_GLYPH_BASE_RATIO + LABEL_GAP;
        let (pos, anchor) = if horizontal {
            (pos2(center.x, center.y - edge), Align2::CENTER_BOTTOM)
        } else {
            (pos2(center.x + edge, center.y), Align2::LEFT_CENTER)
        };
        painter.text(
            pos,
            anchor,
            format!("V{}", id.value()),
            label_font.clone(),
            color,
        );
    };

    draw_capsule_tank(&painter, top_tank_rect, bulkhead_h, fill, stroke);
    draw_tank_label(top_tank_rect, 0, n2_color);
    draw_hatching(
        &painter,
        &capsule_polygon(top_tank_rect, bulkhead_h),
        n2_color,
        pressure_coverage(n2_pressure_bar.unwrap_or(0.0), N2_MAX_PRESSURE_BAR),
        hatch_stride,
        None,
    );

    draw_capsule_tank(&painter, tank_rect, bulkhead_h, fill, stroke);
    draw_tank_label(tank_rect, 1, n2o_color);
    draw_tank_fill(
        &painter,
        tank_rect,
        bulkhead_h,
        tank_fill_level,
        pressure_coverage(n2o_pressure1_bar.unwrap_or(0.0), N2O_MAX_PRESSURE_BAR),
        n2o_color,
        stroke,
        hatch_stride,
    );

    let tank_fill_indicator = MeasurementIndicator {
        values: vec![tank_fill_level.map(|l| l * 100.0)],
        unit: "%",
        color: schematic_ink(&visuals),
        decimals: Some(0),
        blink: tank_fill_level.is_none(),
    };
    let intrinsic = MeasurementIndicator {
        values: vec![Some(99.0)],
        unit: "%",
        color: schematic_ink(&visuals),
        decimals: Some(0),
        blink: false,
    }
    .intrinsic_size(ui.ctx());
    let pad = ui.ctx().global_style().spacing.button_padding;
    let size = egui::vec2(intrinsic.x - 2.0 * pad.x + 6.0, intrinsic.y);
    ui.place(
        Rect::from_center_size(tank_rect.center(), size),
        tank_fill_indicator,
    );
    let time = ui.input(|i| i.time);
    // Fill reflects the reported state; while the commanded position disagrees
    // with it, only that is drawn, as hatching, and a lasting mismatch blinks a box.
    let cmd = |id: ValveId| {
        valve_reading(system, id)
            .filter(|r| valve_mismatch(*r))
            .and_then(|r| r.commanded)
    };
    let style_for = |id: ValveId| -> (Stroke, Color32) {
        let color = readable(super::Valve::color(id), &visuals);
        let fill = match valve_state(system, id) {
            Some(s) if s > 0.0 && cmd(id).is_none() => color,
            _ => schematic_void(&visuals),
        };
        (Stroke::new(1.5_f32, color), fill)
    };
    let (stroke_pressurant_vent, fill_pressurant_vent) = style_for(ValveId::PressurantVent);
    let (valve_stroke_pressurization, valve_fill_pressurization) =
        style_for(ValveId::Pressurization);
    let (stroke_oxidizer_vent, fill_oxidizer_vent) = style_for(ValveId::OxidizerVent);
    let (stroke_oxidizer_fill, fill_oxidizer_fill) = style_for(ValveId::OxidizerFill);
    let (valve_stroke_main, valve_fill_main) = style_for(ValveId::Main);

    draw_pressure_regulator(&painter, pos2(center_x, reg_cy), valve_half, stroke);
    draw_valve(
        &painter,
        pos2(center_x, valve_top_cy),
        valve_half,
        false,
        valve_fill_pressurization,
        valve_stroke_pressurization,
        cmd(ValveId::Pressurization),
        hatch_stride,
        blink[super::Valve::index(ValveId::Pressurization)],
        time,
    );
    interact_valve(
        ui,
        &painter,
        system,
        pos2(center_x, valve_top_cy),
        valve_half,
        ValveId::Pressurization,
        false,
        *mode,
        pulse_secs,
    );
    draw_valve_label(
        pos2(center_x, valve_top_cy),
        valve_half,
        false,
        ValveId::Pressurization,
        false,
    );
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
    painter.circle_filled(pos2(center_x, junction_cy), junction_r, node_color);
    painter.text(
        pos2(center_x - junction_r - LABEL_GAP, junction_cy - junction_r),
        Align2::RIGHT_TOP,
        "P3",
        label_font.clone(),
        node_color,
    );

    let vent_end_x = square.right();
    let vent_valve_cx = (center_x + tank_w * 0.35 + vent_end_x) / 2.0;
    draw_valve(
        &painter,
        pos2(vent_valve_cx, junction_cy),
        valve_half,
        true,
        fill_pressurant_vent,
        stroke_pressurant_vent,
        cmd(ValveId::PressurantVent),
        hatch_stride,
        blink[super::Valve::index(ValveId::PressurantVent)],
        time,
    );
    interact_valve(
        ui,
        &painter,
        system,
        pos2(vent_valve_cx, junction_cy),
        valve_half,
        ValveId::PressurantVent,
        true,
        *mode,
        pulse_secs,
    );
    draw_valve_label(
        pos2(vent_valve_cx, junction_cy),
        valve_half,
        true,
        ValveId::PressurantVent,
        false,
    );
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
    draw_valve(
        &painter,
        pos2(tank_vent_valve_cx, tank_vent_y),
        valve_half,
        true,
        fill_oxidizer_vent,
        stroke_oxidizer_vent,
        cmd(ValveId::OxidizerVent),
        hatch_stride,
        blink[super::Valve::index(ValveId::OxidizerVent)],
        time,
    );
    interact_valve(
        ui,
        &painter,
        system,
        pos2(tank_vent_valve_cx, tank_vent_y),
        valve_half,
        ValveId::OxidizerVent,
        true,
        *mode,
        pulse_secs,
    );
    draw_valve_label(
        pos2(tank_vent_valve_cx, tank_vent_y),
        valve_half,
        true,
        ValveId::OxidizerVent,
        false,
    );
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

    let oxidizer_vent = system
        .last_instance_message::<Valve>(i64::from(ValveId::OxidizerVent.value()))
        .ok();
    let oxidizer_vent_temp = oxidizer_vent
        .as_ref()
        .and_then(|v| temperature_c(v.temperature));
    let heater = oxidizer_vent
        .as_ref()
        .filter(|v| v.flags.contains(ValveFlag::HEATED))
        .map(|v| v.flags.contains(ValveFlag::HEATER_ON));
    let temp_font = egui::FontId::monospace(12.0);
    let temp_readout = oxidizer_vent_temp.map(|t| Readout {
        value: t,
        decimals: 0,
        unit: Some("\u{00b0}C"),
        font: temp_font.clone(),
        color: schematic_ink(&visuals),
        ..Default::default()
    });
    let temp_missing = painter.layout_no_wrap("--\u{00b0}C".to_owned(), temp_font, label_color);
    let size = temp_readout
        .as_ref()
        .map_or(temp_missing.size(), |r| r.size(ui.ctx()));
    let heater_size = Vec2::new(0.8, 1.0) * size.y;
    let group_w = size.x + heater.map_or(0.0, |_| LABEL_GAP + heater_size.x);
    let group_left =
        (tank_vent_valve_cx + LABEL_GAP - group_w / 2.0).min(square.right() - LABEL_GAP - group_w);
    let temp_rect = Rect::from_min_size(
        pos2(
            group_left,
            tank_vent_y + valve_half * VALVE_GLYPH_BASE_RATIO + LABEL_GAP,
        ),
        size,
    );
    match temp_readout {
        Some(r) => {
            r.paint(&painter, temp_rect.min, Align2::LEFT_TOP);
        }
        None => painter.galley(temp_rect.min, temp_missing, label_color),
    }
    if let Some(on) = heater {
        let color = if on {
            readable(COLOR_INDICATOR_LIMITS, &visuals)
        } else {
            schematic_line(&visuals)
        };
        let heater_rect = Rect::from_center_size(
            pos2(
                temp_rect.right() + LABEL_GAP + heater_size.x / 2.0,
                temp_rect.center().y,
            ),
            heater_size,
        );
        draw_heater(&painter, heater_rect, Stroke::new(1.5_f32, color));
    }

    let indicator = MeasurementIndicator {
        values: vec![n2o_temp1, n2o_temp2],
        unit: "\u{00b0}C",
        color: schematic_ink(&visuals),
        decimals: Some(0),
        // No temperature limit in the message yet; flag missing readings instead.
        blink: n2o_temp1.is_none() || n2o_temp2.is_none(),
    };
    let size = indicator.intrinsic_size(ui.ctx());
    let tank_temp_cy = tank_rect
        .center()
        .y
        .max(temp_rect.bottom() + LABEL_GAP + size.y / 2.0);
    ui.place(
        Rect::from_center_size(pos2(temp_cx, tank_temp_cy), size),
        indicator,
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
    draw_valve(
        &painter,
        pos2(bot_vent_valve_cx, tank_vent_bot_y),
        valve_half,
        true,
        fill_oxidizer_fill,
        stroke_oxidizer_fill,
        cmd(ValveId::OxidizerFill),
        hatch_stride,
        blink[super::Valve::index(ValveId::OxidizerFill)],
        time,
    );
    interact_valve(
        ui,
        &painter,
        system,
        pos2(bot_vent_valve_cx, tank_vent_bot_y),
        valve_half,
        ValveId::OxidizerFill,
        true,
        *mode,
        pulse_secs,
    );
    draw_valve_label(
        pos2(bot_vent_valve_cx, tank_vent_bot_y),
        valve_half,
        true,
        ValveId::OxidizerFill,
        false,
    );
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

    // Ground-support lane: the two external tanks sit to the left of the plant.
    // Each tank is entered from the bottom by its fill line (from the boundary
    // through the external fill valve) and vented from the top through its external
    // vent valve; both valves ride the tank's vertical riser.
    let lane_right = square.left();
    let ext_tank_w = 0.075 * n;
    let ext_tank_h = 0.13 * n;
    let ext_bulkhead_h = TANK_BULKHEAD_RATIO * ext_tank_h;
    let gse_valve_half = valve_half * 0.85;
    // Tanks hug the boundary; the fill valve rides the vertical riser directly
    // below each tank.
    let ext_cx = lane_right - ext_tank_w / 2.0 - 0.006 * n;
    let gse_gap = 0.012 * n;
    // Length of each riser (fill below, vent above): a valve plus a gap either side.
    let ext_riser = 2.0 * gse_gap + 2.0 * gse_valve_half;

    // Dashed skin: everything left of it is off-vehicle (ground support). Centered
    // between the external and onboard oxidizer-fill valves, and stopped short of
    // the bottom so it clears the mode toggle in the corner.
    let skin_x = f32::midpoint(ext_cx, bot_vent_valve_cx);
    for shape in Shape::dashed_line(
        &[
            pos2(skin_x, strip.top() + 0.02 * n),
            pos2(skin_x, strip.bottom() - 0.1 * n),
        ],
        stroke,
        0.01 * n,
        0.008 * n,
    ) {
        painter.add(shape);
    }

    // Tank hangs a riser above its fill line, entered from the bottom.
    let ext_tank_rect = |fill_cy: f32| {
        Rect::from_center_size(
            pos2(ext_cx, fill_cy - ext_riser - ext_tank_h / 2.0),
            egui::vec2(ext_tank_w, ext_tank_h),
        )
    };
    let ext_press_rect = ext_tank_rect(top_tank_fill_y);
    let ext_ox_rect = ext_tank_rect(tank_vent_bot_y);

    draw_capsule_tank(&painter, ext_press_rect, ext_bulkhead_h, fill, stroke);
    draw_tank_label(ext_press_rect, 4, ext_n2_color);
    draw_hatching(
        &painter,
        &capsule_polygon(ext_press_rect, ext_bulkhead_h),
        ext_n2_color,
        pressure_coverage(ext_pressurant_bar.unwrap_or(0.0), N2_MAX_PRESSURE_BAR),
        hatch_stride,
        None,
    );

    draw_capsule_tank(&painter, ext_ox_rect, ext_bulkhead_h, fill, stroke);
    draw_tank_label(ext_ox_rect, 5, ext_n2o_color);
    draw_tank_fill(
        &painter,
        ext_ox_rect,
        ext_bulkhead_h,
        ext_oxidizer_level,
        pressure_coverage(ext_oxidizer_bar.unwrap_or(0.0), N2O_MAX_PRESSURE_BAR),
        ext_n2o_color,
        stroke,
        hatch_stride,
    );

    for (rect, color, pressure, warn) in [
        (
            ext_press_rect,
            ext_n2_color,
            ext_pressurant_bar,
            vessel_warn(ext_pressurant.as_ref()),
        ),
        (
            ext_ox_rect,
            ext_n2o_color,
            ext_oxidizer_bar,
            vessel_warn(ext_oxidizer.as_ref()),
        ),
    ] {
        let indicator = MeasurementIndicator {
            values: vec![pressure],
            unit: "bar",
            color,
            decimals: None,
            // External vessels legitimately read unavailable when disconnected, so
            // a missing value is not a fault; only a real overpressure warns.
            blink: warn,
        };
        let size = indicator.intrinsic_size(ui.ctx());
        ui.place(Rect::from_center_size(rect.center(), size), indicator);
    }

    for (rect, fill_cy, fill_id, vent_id, available) in [
        (
            ext_press_rect,
            top_tank_fill_y,
            ValveId::ExternalPressurantFill,
            ValveId::ExternalPressurantVent,
            ext_pressurant_available,
        ),
        (
            ext_ox_rect,
            tank_vent_bot_y,
            ValveId::ExternalOxidizerFill,
            ValveId::ExternalOxidizerVent,
            ext_oxidizer_available,
        ),
    ] {
        // When the ground tank isn't reporting, mute its valves and plumbing: dim
        // them and drop the commanded/mismatch cues since they can't do anything.
        let muted = gse_muted_opacity(&visuals);
        let pipe_stroke = if available {
            stroke
        } else {
            Stroke::new(stroke.width, dim(stroke.color, muted))
        };
        let valve_visual = |id: ValveId, s: Stroke, c: Color32| {
            if available {
                (c, s, cmd(id), blink[super::Valve::index(id)])
            } else {
                (
                    dim(c, muted),
                    Stroke::new(s.width, dim(s.color, muted)),
                    None,
                    false,
                )
            }
        };

        // Fill valve on the riser below the tank; the fill line runs in from the boundary.
        let (fill_stroke, fill_color) = style_for(fill_id);
        let (fill_color, fill_stroke, fill_cmd, fill_blink) =
            valve_visual(fill_id, fill_stroke, fill_color);
        let lower_valve_cy = fill_cy - gse_gap - gse_valve_half;
        draw_valve(
            &painter,
            pos2(ext_cx, lower_valve_cy),
            gse_valve_half,
            false,
            fill_color,
            fill_stroke,
            fill_cmd,
            hatch_stride,
            fill_blink,
            time,
        );
        if available {
            interact_valve(
                ui,
                &painter,
                system,
                pos2(ext_cx, lower_valve_cy),
                gse_valve_half,
                fill_id,
                false,
                *mode,
                pulse_secs,
            );
        }
        draw_valve_label(
            pos2(ext_cx, lower_valve_cy),
            gse_valve_half,
            false,
            fill_id,
            !available,
        );
        painter.line(
            vec![pos2(lane_right, fill_cy), pos2(ext_cx, fill_cy)],
            pipe_stroke,
        );
        painter.line(
            vec![
                pos2(ext_cx, fill_cy),
                pos2(ext_cx, lower_valve_cy + gse_valve_half),
            ],
            pipe_stroke,
        );
        painter.line(
            vec![
                pos2(ext_cx, lower_valve_cy - gse_valve_half),
                pos2(ext_cx, rect.bottom()),
            ],
            pipe_stroke,
        );

        // Vent valve mirrored on the riser above the tank, venting to atmosphere.
        let (vent_stroke, vent_color) = style_for(vent_id);
        let (vent_color, vent_stroke, vent_cmd, vent_blink) =
            valve_visual(vent_id, vent_stroke, vent_color);
        let upper_valve_cy = rect.top() - gse_gap - gse_valve_half;
        draw_valve(
            &painter,
            pos2(ext_cx, upper_valve_cy),
            gse_valve_half,
            false,
            vent_color,
            vent_stroke,
            vent_cmd,
            hatch_stride,
            vent_blink,
            time,
        );
        if available {
            interact_valve(
                ui,
                &painter,
                system,
                pos2(ext_cx, upper_valve_cy),
                gse_valve_half,
                vent_id,
                false,
                *mode,
                pulse_secs,
            );
        }
        draw_valve_label(
            pos2(ext_cx, upper_valve_cy),
            gse_valve_half,
            false,
            vent_id,
            !available,
        );
        painter.line(
            vec![
                pos2(ext_cx, rect.top()),
                pos2(ext_cx, upper_valve_cy + gse_valve_half),
            ],
            pipe_stroke,
        );
        painter.line(
            vec![
                pos2(ext_cx, upper_valve_cy - gse_valve_half),
                pos2(ext_cx, rect.top() - ext_riser),
            ],
            pipe_stroke,
        );
    }

    draw_valve(
        &painter,
        pos2(center_x, valve_bot_cy),
        valve_half,
        false,
        valve_fill_main,
        valve_stroke_main,
        cmd(ValveId::Main),
        hatch_stride,
        blink[super::Valve::index(ValveId::Main)],
        time,
    );
    interact_valve(
        ui,
        &painter,
        system,
        pos2(center_x, valve_bot_cy),
        valve_half,
        ValveId::Main,
        false,
        *mode,
        pulse_secs,
    );
    draw_valve_label(
        pos2(center_x, valve_bot_cy),
        valve_half,
        false,
        ValveId::Main,
        false,
    );
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
        cc_color,
        pressure_coverage(cc_pressure_bar.unwrap_or(0.0), CC_MAX_PRESSURE_BAR),
        hatch_stride,
        None,
    );

    draw_fuel_grain(&painter, cc_interior, fuel_port_half, stroke, fuel_color);

    painter.add(Shape::Path(PathShape::closed_line(chamber_path, stroke)));
    draw_tank_label(cc_interior, 2, cc_color);

    draw_valve_mode_toggle(ui, strip, mode, pulse, pulse_durations);
}

// Bottom-left selector that switches what a click on a valve does, bottom-right
// the pulse duration. Mirrors the altitude-source toggle in the artificial
// horizon pane.
fn draw_valve_mode_toggle(
    ui: &egui::Ui,
    square: Rect,
    mode: &mut ValveInteractionMode,
    pulse: &mut usize,
    pulse_durations: [f32; 3],
) {
    let font = FontId::proportional(12.0);
    // Wide enough for the longest label, so a stack's buttons line up.
    let size = |labels: &[&str]| {
        let text = labels
            .iter()
            .map(|label| {
                ui.ctx().fonts_mut(|f| {
                    f.layout_no_wrap((*label).to_owned(), font.clone(), Color32::PLACEHOLDER)
                        .size()
                        .x
                })
            })
            .fold(0.0, f32::max);
        Vec2::new(text + 2.0 * ui.spacing().button_padding.x, 18.0)
    };
    let visuals = ui.visuals().clone();
    let outline = |selected: bool, label: &str| {
        let stroke = if selected {
            Stroke::new(1.0_f32, schematic_ink(&visuals))
        } else {
            Stroke::new(0.5_f32, schematic_line(&visuals))
        };
        Button::new(RichText::new(label).font(font.clone()))
            .wrap_mode(egui::TextWrapMode::Extend)
            .fill(Color32::TRANSPARENT)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(3))
            .selected(false)
    };
    let bounds = square.intersect(ui.clip_rect());
    let stack = |ui: &mut egui::Ui| {
        ui.set_clip_rect(bounds);
        ui.spacing_mut().item_spacing.y = 2.0;
        // A button's margins follow its state's expansion and stroke width even
        // with both overridden, so hovering would resize it and shift the stack.
        let widgets = &mut ui.visuals_mut().widgets;
        let inactive = widgets.inactive;
        for state in [&mut widgets.hovered, &mut widgets.active] {
            state.expansion = inactive.expansion;
            state.bg_stroke.width = inactive.bg_stroke.width;
        }
    };

    let modes = [
        (ValveInteractionMode::Pulse, "PLSE"),
        (ValveInteractionMode::Open, "OPEN"),
        (ValveInteractionMode::Close, "CLSE"),
    ];
    let mode_size = size(&modes.map(|(_, label)| label));
    let labels = pulse_durations.map(|secs| format!("{secs}s"));
    let pulse_size = size(&labels.each_ref().map(String::as_str));

    // Both stacks are three buttons tall; the pulse stack goes first when space runs out.
    let inset = 6.0;
    let fits_height = bounds.height() >= 3.0 * mode_size.y + 2.0 * 2.0 + 2.0 * inset;
    let show_mode = fits_height && bounds.width() >= mode_size.x + 2.0 * inset;
    let show_pulse = show_mode
        && *mode == ValveInteractionMode::Pulse
        && bounds.width() >= mode_size.x + pulse_size.x + 3.0 * inset;

    if show_mode {
        Area::new(Id::new("valve_mode_toggle"))
            .order(Order::Foreground)
            .fade_in(false)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(bounds.left_bottom() + Vec2::new(inset, -inset))
            .show(ui.ctx(), |ui| {
                stack(ui);
                for (m, label) in modes {
                    if ui
                        .add_sized(mode_size, outline(*mode == m, label))
                        .clicked()
                    {
                        *mode = m;
                    }
                }
            });
    }

    if show_pulse {
        Area::new(Id::new("valve_pulse_duration"))
            .order(Order::Foreground)
            .fade_in(false)
            .pivot(Align2::RIGHT_BOTTOM)
            .fixed_pos(bounds.right_bottom() + Vec2::new(-inset, -inset))
            .show(ui.ctx(), |ui| {
                stack(ui);
                for (i, label) in labels.iter().enumerate().rev() {
                    if ui
                        .add_sized(pulse_size, outline(*pulse == i, label))
                        .clicked()
                    {
                        *pulse = i;
                    }
                }
            });
    }
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

#[allow(clippy::too_many_arguments)]
fn draw_tank_fill(
    painter: &egui::Painter,
    rect: Rect,
    bulkhead_h: f32,
    level: Option<f32>,
    coverage: f32,
    color: Color32,
    stroke: Stroke,
    hatch_stride: f32,
) {
    let Some(level) = level else {
        // No level sensor: hatch the whole vessel for pressure, fading toward the
        // top so the missing surface reads as uncertainty, not a full tank.
        let polygon = capsule_polygon(rect, bulkhead_h);
        draw_hatching(
            painter,
            &polygon,
            color,
            coverage,
            hatch_stride,
            Some((rect.bottom(), rect.top())),
        );
        return;
    };

    let level = level.clamp(0.0, 1.0);
    if level <= 0.0 {
        return;
    }

    let polygon = tank_fill_polygon(rect, bulkhead_h, level);
    draw_hatching(painter, &polygon, color, coverage, hatch_stride, None);

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

#[allow(clippy::similar_names)]
fn draw_hatching(
    painter: &egui::Painter,
    polygon: &[Pos2],
    color: Color32,
    coverage: f32,
    stride: f32,
    fade: Option<(f32, f32)>,
) {
    let coverage = coverage.clamp(0.0, 1.0);
    if coverage <= 0.0 || polygon.is_empty() {
        return;
    }
    // With `fade`, opacity ramps from full at `y_full` to zero at `y_zero`, so an
    // unknown-level tank thins out toward the top.
    let fade_at = move |y: f32| match fade {
        Some((y_full, y_zero)) => {
            color.gamma_multiply(1.0 - ((y_full - y) / (y_full - y_zero)).clamp(0.0, 1.0))
        }
        None => color,
    };
    let fill_shard = |poly: Vec<Pos2>| {
        if fade.is_some() {
            let mut mesh = egui::Mesh::default();
            for p in &poly {
                mesh.colored_vertex(*p, fade_at(p.y));
            }
            for i in 1..poly.len() as u32 - 1 {
                mesh.add_triangle(0, i, i + 1);
            }
            painter.add(Shape::mesh(mesh));
        } else {
            painter.add(Shape::Path(PathShape::convex_polygon(
                poly,
                color,
                Stroke::NONE,
            )));
        }
    };
    if coverage >= 1.0 {
        fill_shard(polygon.to_vec());
        return;
    }

    // Any pressure at all keeps a perceptible hatch, so a nearly empty vessel still reads
    // differently from one that reports nothing.
    let line_width = (coverage * stride).max(MIN_HATCH_WIDTH);
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
        let (a, b) = (base + t_lo * dir, base + t_hi * dir);

        if line_width < MIN_FILLED_HATCH_WIDTH {
            // Inset by the half width so the stroke covers the same area the clipped band would,
            // without spilling over the vessel outline.
            let Some((a, b)) = clip_segment_to_convex(a, b, polygon, half_w) else {
                continue;
            };
            if fade.is_some() {
                // A stroke takes one color, so the ramp has to come from a per-vertex callback.
                painter.add(Shape::Path(PathShape {
                    points: vec![a, b],
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: PathStroke::new_uv(line_width, move |_, p| fade_at(p.y)),
                }));
            } else {
                painter.line_segment([a, b], Stroke::new(line_width, color));
            }
            continue;
        }

        let stripe = [
            a - half_w * perp,
            b - half_w * perp,
            b + half_w * perp,
            a + half_w * perp,
        ];
        let clipped = clip_polygon_to_convex(&stripe, polygon);
        if clipped.len() >= 3 {
            fill_shard(clipped);
        }
    }
}

/// Clips a segment against a convex polygon, shrinking the polygon by `inset` first. `None` if
/// nothing of the segment survives.
fn clip_segment_to_convex(a: Pos2, b: Pos2, polygon: &[Pos2], inset: f32) -> Option<(Pos2, Pos2)> {
    let sum: egui::Vec2 = polygon
        .iter()
        .fold(egui::Vec2::ZERO, |acc, v| acc + v.to_vec2());
    let center = (sum / polygon.len() as f32).to_pos2();

    let dir = b - a;
    let mut t_enter = 0.0_f32;
    let mut t_exit = 1.0_f32;
    for i in 0..polygon.len() {
        let edge_a = polygon[i];
        let edge = polygon[(i + 1) % polygon.len()] - edge_a;
        let mut normal = egui::vec2(-edge.y, edge.x).normalized();
        if normal == egui::Vec2::ZERO {
            // Coincident points, e.g. where a fill surface meets a bulkhead exactly.
            continue;
        }
        if normal.dot(center - edge_a) < 0.0 {
            normal = -normal;
        }

        // Inside is `(p - edge_a).dot(normal) >= inset`; solve that for the segment parameter.
        let dist = (a - edge_a).dot(normal) - inset;
        let rate = dir.dot(normal);
        if rate == 0.0 {
            if dist < 0.0 {
                return None;
            }
            continue;
        }
        let t = -dist / rate;
        if rate > 0.0 {
            t_enter = t_enter.max(t);
        } else {
            t_exit = t_exit.min(t);
        }
        if t_enter > t_exit {
            return None;
        }
    }
    Some((a + t_enter * dir, a + t_exit * dir))
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

fn draw_heater(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    const STEPS: usize = 16;
    let amplitude = rect.width() / 10.0;
    for i in 1..=3 {
        let x = rect.left() + rect.width() * i as f32 / 4.0;
        let points = (0..=STEPS)
            .map(|s| {
                let t = s as f32 / STEPS as f32;
                let dx = amplitude * (t * std::f32::consts::TAU).sin();
                pos2(x + dx, rect.bottom() - t * rect.height())
            })
            .collect();
        painter.line(points, stroke);
    }
}

// The two bowtie triangles of a valve glyph. Horizontal valves point left/right,
// vertical ones up/down.
fn valve_glyph_polygons(center: Pos2, half: f32, horizontal: bool) -> [Vec<Pos2>; 2] {
    let base_half = half * VALVE_GLYPH_BASE_RATIO;
    if horizontal {
        [
            vec![
                pos2(center.x - half, center.y - base_half),
                pos2(center.x - half, center.y + base_half),
                center,
            ],
            vec![
                pos2(center.x + half, center.y - base_half),
                pos2(center.x + half, center.y + base_half),
                center,
            ],
        ]
    } else {
        [
            vec![
                pos2(center.x - base_half, center.y - half),
                pos2(center.x + base_half, center.y - half),
                center,
            ],
            vec![
                pos2(center.x - base_half, center.y + half),
                pos2(center.x + base_half, center.y + half),
                center,
            ],
        ]
    }
}

// Draws a valve: solid fill/stroke for the reported state, hatching for a
// differing commanded position, and a blinking orange box on a mismatch.
#[allow(clippy::too_many_arguments)]
fn draw_valve(
    painter: &egui::Painter,
    center: Pos2,
    half: f32,
    horizontal: bool,
    fill: Color32,
    stroke: Stroke,
    commanded: Option<f32>,
    hatch_stride: f32,
    blink_box: bool,
    time: f64,
) {
    let warning = readable(
        COLOR_INDICATOR_WARNING,
        &painter.ctx().global_style().visuals,
    );
    let polygons = valve_glyph_polygons(center, half, horizontal);
    for polygon in &polygons {
        painter.add(Shape::convex_polygon(polygon.clone(), fill, stroke));
    }

    // Intended position as hatching, like the tank pressure fill. Coverage is
    // capped well below full and the stride tightened so it always reads as
    // distinct stripes on the small glyph, not a solid actual-open fill.
    if let Some(c) = commanded {
        let coverage = (c * 0.6).clamp(0.0, 0.6);
        for polygon in &polygons {
            draw_hatching(
                painter,
                polygon,
                warning,
                coverage,
                hatch_stride * 0.6,
                None,
            );
        }
    }

    if blink_box && blink_on(time) {
        let size = if horizontal {
            Vec2::new(half * 2.0, half * 1.6)
        } else {
            Vec2::new(half * 1.6, half * 2.0)
        };
        painter.rect(
            Rect::from_center_size(center, size).expand(4.0),
            CornerRadius::same(2),
            Color32::TRANSPARENT,
            Stroke::new(2.0_f32, warning),
            StrokeKind::Outside,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn interact_valve(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    system: &System,
    center: Pos2,
    half: f32,
    id: ValveId,
    horizontal: bool,
    mode: ValveInteractionMode,
    pulse_duration: f32,
) {
    if system.muted() {
        return;
    }

    let size = if horizontal {
        egui::vec2(half * 2.0, half * 1.6)
    } else {
        egui::vec2(half * 1.6, half * 2.0)
    };
    let rect = Rect::from_center_size(center, size);
    let resp = ui.interact(
        rect,
        egui::Id::new(("valve", id.value())),
        egui::Sense::click(),
    );
    if Hazard::confirm(ui, &resp, system) {
        match mode {
            ValveInteractionMode::Pulse => system.do_pulse_valve(id, pulse_duration),
            ValveInteractionMode::Open => system.do_set_valve(id, 1.0),
            ValveInteractionMode::Close => system.do_set_valve(id, 0.0),
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let visuals = ui.visuals();
        painter.rect(
            rect.expand(2.0),
            CornerRadius::same(2),
            schematic_wash(visuals, 15),
            Stroke::new(1.0_f32, schematic_wash(visuals, 60)),
            StrokeKind::Middle,
        );
    }
}
