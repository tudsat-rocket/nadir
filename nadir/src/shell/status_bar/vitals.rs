use std::time::Duration;

use nadir_core::{System, mav_type_icon};

use eframe::egui;
use eframe::emath::GuiRounding as _;
use egui::{
    Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Vec2, pos2,
};
use mavspec::rust::dialects::common::enums::{GpsFixType, MavSysStatusSensor, StorageStatus};
use mavspec::rust::dialects::common::messages::{
    GpsRawInt, Heartbeat, LocalPositionNed, StorageInformation, SysStatus,
};

use crate::colors::{
    COLOR_INDICATOR_GOOD, COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, dim, readable,
};
use crate::panes::LogsPane;
use crate::widgets::{
    ArmedBadge, AutopilotLogo, BatteryReading, Camera, Readout, STALE_AFTER, TEXT_SIZE,
    column_header, format_silence, link_quality, link_silence, small_text, soc_color,
};

/// Same dim-to-strong ramp as the battery indicator widget: current only lights up as it climbs, so
/// an idle bus does not read as an event. Uses the magnitude, since firmware that reports discharge
/// as negative would otherwise never brighten.
fn current_color(ui: &egui::Ui, amps: f32) -> Color32 {
    const I_MIN: f32 = 0.1;
    const I_MAX: f32 = 10.0;

    let ramp = (f32::max(amps.abs() / I_MAX, I_MIN).log2() - I_MIN.log2()) / (-I_MIN.log2());
    ui.visuals()
        .weak_text_color()
        .lerp_to_gamma(ui.visuals().strong_text_color(), f32::min(ramp, 1.0))
}

/// One piece of a value cell. The rows mix icons and prose (the "--" placeholder) with numbers that
/// want the readout's tucked decimals and small units, so they cannot all be one string.
enum Segment {
    Text(String, Color32),
    Caption(&'static str),
    Value(Readout),
    /// Narrower than a monospace space.
    Gap(f32),
    /// Fixed width, so the values after it line up across rows.
    Icon(&'static str),
    Battery(Option<f32>, Color32),
    Camera(Camera),
}

impl Segment {
    const ICON_WIDTH: f32 = 20.0;

    fn value(
        value: f32,
        decimals: usize,
        width_chars: usize,
        unit: Option<&'static str>,
        color: Color32,
    ) -> Self {
        Self::Value(Readout {
            value,
            decimals,
            width_chars,
            unit,
            font: FontId::monospace(TEXT_SIZE),
            color,
            ..Default::default()
        })
    }

    fn show(self, ui: &mut egui::Ui) {
        match self {
            Self::Text(text, color) => small_text(ui, &text, color),
            Self::Caption(text) => {
                ui.add(
                    egui::Label::new(RichText::new(text).size(TEXT_SIZE * 0.8).weak())
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
            Self::Value(readout) => {
                ui.add(readout);
            }
            Self::Gap(width) => ui.add_space(width),
            Self::Icon(glyph) => {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(Self::ICON_WIDTH, TEXT_SIZE), Sense::hover());
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    glyph,
                    FontId::proportional(TEXT_SIZE),
                    ui.visuals().weak_text_color(),
                );
            }
            Self::Battery(soc, color) => {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(Self::ICON_WIDTH, TEXT_SIZE), Sense::hover());
                let weak = ui.visuals().weak_text_color();
                let painter = ui.painter();

                // Snapped, since at this size a half-pixel offset smears the outline into the fill.
                let body = Rect::from_center_size(rect.center() - Vec2::X, Vec2::new(15.0, 8.0))
                    .round_to_pixels(ui.pixels_per_point());
                let nub = Rect::from_min_max(
                    pos2(body.right(), body.center().y - 2.0),
                    pos2(body.right() + 2.0, body.center().y + 2.0),
                );
                painter.rect_filled(nub, 0.0, weak);
                painter.rect_stroke(body, 1.0, Stroke::new(1.0, weak), StrokeKind::Inside);

                if let Some(soc) = soc {
                    let mut level = body.shrink(2.0);
                    level.set_right(level.left() + soc.clamp(0.0, 1.0) * level.width());
                    painter.rect_filled(level, 0.0, color);
                    response.on_hover_text(format!("{:.0}%", soc * 100.0));
                }
            }
            Self::Camera(camera) => {
                ui.add(camera);
            }
        }
    }
}

/// Right-hand columns of the status bar: the consumables and RF numbers a pilot watches
/// continuously, plus the component inventory. In compact mode (narrow windows) only the
/// consumables column survives.
pub struct Vitals<'a> {
    pub system: &'a System,
    pub compact: bool,
}

/// Width the consumables rows need before their widest value (battery: voltage and current, e.g.
/// "12.2V, -200mA" beside its label and icon) starts truncating.
const CONSUMABLES_MIN_WIDTH: f32 = 180.0 + Segment::ICON_WIDTH;
/// Share of the zone the consumables column takes. Its rows are label-plus-value and of known
/// length, while the component list wraps into further sub-columns, so the larger share goes there.
const CONSUMABLES_SHARE: f32 = 0.45;
const SEPARATOR_WIDTH: f32 = 13.0;
const ROW_GAP: f32 = 2.0;
const CELL_GAP: f32 = 10.0;

impl Vitals<'_> {
    /// Mirrors what [`Self::component_row`] draws, since the column count is measured off it.
    const WIDEST_ROW: &'static str = "⚙ 0x00 DISARMED";

    /// Width the two-column layout needs before the consumables values start truncating; below this,
    /// callers should ask for the compact form.
    pub const FULL_MIN_WIDTH: f32 = CONSUMABLES_MIN_WIDTH / CONSUMABLES_SHARE + SEPARATOR_WIDTH;

    fn consumables_column(&self, ui: &mut egui::Ui) {
        let system = self.system;
        let weak = ui.visuals().weak_text_color();
        let nodata = dim(weak, 0.5);
        let normal = ui.visuals().text_color();
        let good = readable(COLOR_INDICATOR_GOOD, ui.visuals());
        let warning = readable(COLOR_INDICATOR_WARNING, ui.visuals());
        let limits = readable(COLOR_INDICATOR_LIMITS, ui.visuals());

        // Identity header: icon + system id on the left, firmware logo on the right (both used to
        // live on the status pane's removed header line).
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} 0x{:02x}", system.icon(), system.system_id))
                    .monospace()
                    .size(TEXT_SIZE),
            );
            if system.muted() {
                ui.label(
                    RichText::new("⛔ MUTED")
                        .monospace()
                        .size(TEXT_SIZE)
                        .color(limits),
                )
                .on_hover_text("Nothing is transmitted to this system; unmute it in the sidebar.");
            }
            if let Ok(hb) = system.last_message::<Heartbeat>() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_sized(Vec2::new(60.0, 12.0), AutopilotLogo(hb.autopilot, hb.type_));
                });
            }
        });

        // The voltage shares the state-of-charge color of the glyph; the current is a separate
        // segment so it can carry the battery widget's brightness ramp instead. Several packs leave
        // room for the voltage only.
        let batteries = BatteryReading::all(system);
        let mut battery_cell = Vec::new();
        for (pack, (_, reading)) in batteries.iter().enumerate() {
            // A pack reporting only volts stays neutral, with nothing to color it by.
            let charge_color = reading
                .soc
                .map_or(normal, |soc| soc_color(soc, ui.visuals()));
            if pack > 0 {
                battery_cell.push(Segment::Gap(6.0));
            }
            battery_cell.push(Segment::Battery(reading.soc, charge_color));

            let mut cell = Vec::new();
            if let Some(u) = reading.voltage {
                cell.push(Segment::value(u, 1, 4, Some("V"), charge_color));
            }
            if batteries.len() == 1
                && let Some(i) = reading.current
            {
                let color = current_color(ui, i);
                if !cell.is_empty() {
                    cell.push(Segment::Text(", ".into(), color));
                }
                // Sub-amp draws are the norm on an avionics bus, where "0.0A" would hide the
                // reading.
                cell.push(if i.abs() < 1.0 {
                    Segment::value(i * 1000.0, 0, 0, Some("mA"), color)
                } else {
                    Segment::value(i, 1, 0, Some("A"), color)
                });
            }
            if cell.is_empty() && reading.soc.is_none() {
                cell.push(Segment::Text("--".into(), nodata));
            }
            battery_cell.extend(cell);
        }
        if battery_cell.is_empty() {
            battery_cell.extend([
                Segment::Battery(None, nodata),
                Segment::Text("--".into(), nodata),
            ]);
        }

        let flash_cell = match system.last_message::<StorageInformation>() {
            Ok(info)
                if matches!(info.status, StorageStatus::Ready | StorageStatus::Empty)
                    && info.total_capacity > 0.0 =>
            {
                let used = (info.used_capacity / info.total_capacity).clamp(0.0, 1.0);
                let color = if 1.0 - used < LogsPane::STORAGE_WARN_FREE {
                    warning
                } else {
                    normal
                };
                vec![
                    Segment::value(used * 100.0, 0, 3, Some("%"), color),
                    Segment::Text(" (".into(), weak),
                    Segment::value(info.used_capacity, 1, 0, None, weak),
                    Segment::Text("/".into(), weak),
                    Segment::value(info.total_capacity, 1, 0, Some("MiB"), weak),
                    Segment::Text(")".into(), weak),
                ]
            }
            Ok(info) if info.status == StorageStatus::Unformatted => {
                vec![Segment::Text("unformatted".into(), warning)]
            }
            _ => vec![Segment::Text("--".into(), nodata)],
        };

        let gps_healthy = system.last_message::<SysStatus>().map_or(true, |s| {
            let sensor = MavSysStatusSensor::GPS;
            !s.onboard_control_sensors_enabled.contains(sensor)
                || s.onboard_control_sensors_health.contains(sensor)
        });
        let (satellites, hdop, gps_color) = match system.last_message::<GpsRawInt>() {
            Ok(g)
                if gps_healthy && !matches!(g.fix_type, GpsFixType::NoFix | GpsFixType::NoGps) =>
            {
                let color = if g.satellites_visible >= 6 && g.satellites_visible != u8::MAX {
                    good
                } else {
                    warning
                };
                let satellites = match g.satellites_visible {
                    u8::MAX => Segment::Text("--".into(), color),
                    n => Segment::value(f32::from(n), 0, 2, None, color),
                };
                (satellites, g.eph.min(9999), color)
            }
            _ => (Segment::value(0.0, 0, 2, None, limits), 9999, limits),
        };
        let gps_cell = vec![
            satellites,
            Segment::Gap(6.0),
            Segment::Caption("HDOP"),
            Segment::Gap(3.0),
            Segment::value(f32::from(hdop) / 100.0, 2, 0, None, gps_color),
        ];

        // On narrow windows the Links pane is dropped from the status bar entirely, and these two
        // numbers are all that is left of it.
        let (downlink, uplink) = link_quality(system);
        let (downlink_silence, uplink_silence) = link_silence(ui, system);
        let link_silence_segments = |silence: Option<Duration>| {
            silence.into_iter().flat_map(|silence| {
                [
                    Segment::Gap(4.0),
                    Segment::Text(format_silence(silence), limits),
                ]
            })
        };
        let link_quality_segment = |lq: Option<f32>| match lq {
            Some(lq) => {
                let color = if lq > 0.9 {
                    good
                } else if lq > 0.5 {
                    warning
                } else {
                    limits
                };
                Segment::value(100.0 * lq, 0, 3, Some("%"), color)
            }
            None => Segment::Text("--".into(), nodata),
        };
        let link_cell = std::iter::once(link_quality_segment(downlink))
            .chain(link_silence_segments(downlink_silence))
            .chain([Segment::Icon("⬆"), link_quality_segment(uplink)])
            .chain(link_silence_segments(uplink_silence))
            .collect();

        let mut camera_cell = Vec::new();
        for (i, camera) in Camera::all(system).into_iter().enumerate() {
            if i > 0 {
                camera_cell.push(Segment::Gap(4.0));
            }
            camera_cell.push(Segment::Camera(camera));
        }
        if camera_cell.is_empty() {
            camera_cell.push(Segment::Text("--".into(), nodata));
        }

        let uptime_cell = match system.last_message::<LocalPositionNed>() {
            Ok(lp) => vec![Segment::value(
                lp.time_boot_ms as f32 / 1000.0,
                1,
                0,
                Some("s"),
                normal,
            )],
            Err(_) => vec![Segment::Text("--".into(), nodata)],
        };

        // Same presentation as the preflight pane's status checks: a striped two-column grid, weak
        // labels left, colored values right.
        egui::Grid::new("bar_consumables")
            .num_columns(2)
            .striped(true)
            .spacing(Vec2::new(10.0, 2.0))
            .min_col_width(0.0)
            .show(ui, |ui| {
                for (label, icon, segments) in [
                    ("Uptime", Some("🕑"), uptime_cell),
                    ("Battery", None, battery_cell),
                    ("Flash", Some("💾"), flash_cell),
                    ("GPS", Some("📡"), gps_cell),
                    ("Link", Some("⬇"), link_cell),
                    ("Cameras", Some("🎥"), camera_cell),
                ] {
                    // Extend, not Truncate: inside a grid cell a truncating label shrinks to the
                    // column width it is itself defining, which collapses to an ellipsis.
                    ui.add(
                        egui::Label::new(RichText::new(label).size(TEXT_SIZE).weak())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    // Pad the value cell out to the right edge so the striped row backgrounds span
                    // the whole zone instead of ending where the text does. The label column keeps
                    // its natural width, so the values stay aligned across rows.
                    ui.horizontal(|ui| {
                        // Segments carry their own separators, so they butt up against each other.
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for segment in icon.map(Segment::Icon).into_iter().chain(segments) {
                            segment.show(ui);
                        }
                        ui.add_space(ui.available_width().max(0.0));
                    });
                    ui.end_row();
                }
            });
    }

    fn components_column(&self, ui: &mut egui::Ui) {
        let system = self.system;
        let weak = ui.visuals().weak_text_color();
        let nodata = dim(weak, 0.5);

        column_header(ui, "COMPONENTS");

        // Component 1 is the vehicle itself, which the rest of the bar already speaks for.
        let mut components: Vec<_> = system
            .components()
            .into_iter()
            .filter(|(id, _)| *id > 1)
            .collect();

        if components.is_empty() {
            small_text(ui, "no component data", nodata);
            return;
        }

        // What `Grid` settles on for a row: its own minimum, or the text if that is taller.
        let row_h = ui
            .fonts_mut(|fonts| fonts.row_height(&FontId::monospace(TEXT_SIZE)))
            .max(ui.spacing().interact_size.y)
            + ROW_GAP;
        let row_w = ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(
                    Self::WIDEST_ROW.to_owned(),
                    FontId::monospace(TEXT_SIZE),
                    Color32::PLACEHOLDER,
                )
                .size()
                .x
        }) + CELL_GAP
            + ui.spacing().item_spacing.x;

        // The gap sits between rows, not after the last one.
        let per_column = (((ui.available_height() + ROW_GAP) / row_h) as usize).max(1);
        let max_columns = ((ui.available_width() / row_w) as usize).max(1);
        let columns = components.len().div_ceil(per_column).min(max_columns);

        // Too narrow even for wrapping: the tail is stated as a count rather than dropped.
        let capacity = columns * per_column;
        let elided = if components.len() > capacity {
            let shown = capacity - 1;
            let elided = components.len() - shown;
            components.truncate(shown);
            elided
        } else {
            0
        };

        let now = system.now();

        ui.columns(columns, |sub_columns| {
            for (index, (chunk, column)) in components
                .chunks(per_column)
                .zip(sub_columns.iter_mut())
                .enumerate()
            {
                egui::Grid::new(("bar_components", index))
                    .num_columns(2)
                    .striped(true)
                    .spacing(Vec2::new(CELL_GAP, ROW_GAP))
                    .min_col_width(0.0)
                    .show(column, |ui| {
                        for (id, last) in chunk {
                            Self::component_row(ui, system, *id, now - *last > STALE_AFTER, nodata);
                        }
                        if elided > 0 && index + 1 == columns {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("+{elided} more"))
                                        .monospace()
                                        .size(TEXT_SIZE)
                                        .color(nodata),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                            ui.end_row();
                        }
                    });
            }
        });
    }

    fn component_row(ui: &mut egui::Ui, system: &System, id: u8, stale: bool, nodata: Color32) {
        let heartbeat = system.last_message_from::<Heartbeat>(id).ok();
        let icon = heartbeat.as_ref().map_or("?", |hb| mav_type_icon(hb.type_));

        // Extend, not Truncate: see the consumables labels.
        ui.add(
            egui::Label::new(
                RichText::new(format!("{icon} 0x{id:02x}"))
                    .monospace()
                    .size(TEXT_SIZE)
                    .color(if stale {
                        nodata
                    } else {
                        ui.visuals().text_color()
                    }),
            )
            .wrap_mode(egui::TextWrapMode::Extend),
        );
        ui.horizontal(|ui| {
            match heartbeat {
                Some(hb) => {
                    ui.add(ArmedBadge {
                        mode: hb.base_mode,
                        faded: stale,
                    });
                }
                // Heard from, but never with a heartbeat: nothing states its arm state.
                None => small_text(ui, "--", nodata),
            }
            ui.add_space(ui.available_width().max(0.0));
        });
        ui.end_row();
    }
}

impl egui::Widget for Vitals<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 2.0);
            ui.add_space(4.0);

            if self.compact {
                self.consumables_column(ui);
            } else {
                ui.horizontal(|ui| {
                    let usable = ui.available_width() - SEPARATOR_WIDTH;
                    let consumables_w = usable * CONSUMABLES_SHARE;

                    ui.vertical(|ui| {
                        ui.set_width(consumables_w);
                        self.consumables_column(ui);
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(usable - consumables_w);
                        self.components_column(ui);
                    });
                });
            }
        })
        .response
    }
}
