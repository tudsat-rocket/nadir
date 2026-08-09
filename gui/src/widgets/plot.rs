//! A widget for plotting telemetry data and the corresponding state.

use chrono::{DateTime, TimeDelta, Utc};
use eframe::egui;
use eframe::egui::PointerButton;
use egui::{Color32, TextStyle};
use egui_plot::{Corner, Legend, LineStyle, VLine};

use core::mav::{ComponentId, SystemId};
use core::{MessageInstance, TimeseriesArgs, format_message_label};

use crate::colors::{mode_color, readable};

/// Points a single line may put on screen. Fixed rather than derived from the plot's width: the
/// query costs the same either way, and a budget that moved with a splitter drag would keep
/// recrossing the chunk sizes underneath and make the line shimmer.
const MAX_POINTS: usize = 2_000;

/// Seconds of data read either side of the view, so a line reaches both edges rather than stopping
/// at the last sample inside it.
const MARGIN_SECS: i64 = 5;

/// State shared by all linked plots
pub struct SharedPlotState {
    /// Are we currently attached to the right edge?
    pub attached_to_edge: bool,
    /// Width of the view (in seconds)
    pub view_width: f64,
    pub box_dragging: bool,
}

impl SharedPlotState {
    pub fn new() -> Self {
        Self {
            attached_to_edge: true,
            view_width: 30.0,
            box_dragging: false,
        }
    }

    pub fn process_zoom(&mut self, zoom_delta: egui::Vec2) {
        self.view_width /= f64::from(zoom_delta[0]);
    }

    pub fn process_box_dragging(&mut self, box_dragging: bool) {
        self.box_dragging = self.box_dragging || box_dragging;
    }

    pub fn process_drag_released(&mut self, released: bool) {
        if released && self.box_dragging {
            self.attached_to_edge = false;
            self.box_dragging = false;
        }
    }
}

#[allow(dead_code)]
pub struct PlotLine {
    pub system_id: SystemId,
    pub component_id: ComponentId,
    pub message_name: String,
    pub instance: Option<MessageInstance>,
    pub field_name: String,
    pub alias: Option<String>,
    pub unit: Option<String>,
    pub color: Option<Color32>,
    pub scale: Option<f64>,
    // Raw stored value to treat as "no reading" and drop from the plot,
    // e.g. a firmware sentinel like u16::MAX. Compared before `scale`.
    pub sentinel: Option<f64>,
}

pub struct Plot<'a> {
    lines: &'a [PlotLine],
    source: &'a core::Source,
    shared: &'a mut SharedPlotState,
    ylimits: (Option<f32>, Option<f32>),
}

impl<'a> Plot<'a> {
    pub fn new(
        lines: &'a [PlotLine],
        source: &'a core::Source,
        shared: &'a mut SharedPlotState,
        ylimits: (Option<f32>, Option<f32>),
    ) -> Self {
        Self {
            lines,
            source,
            shared,
            ylimits,
        }
    }

    // Takes its arguments rather than `&self` so the plot closure captures only these fields,
    // leaving `shared` usable for the interaction handling afterwards.
    fn mode_transitions(
        lines: &[PlotLine],
        source: &core::Source,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Vec<(DateTime<Utc>, u32)> {
        let mut systems: Vec<_> = lines
            .iter()
            .map(|line| (line.system_id, line.component_id))
            .collect();
        systems.sort_unstable();
        systems.dedup();

        let mut transitions = Vec::new();
        for (system_id, component_id) in systems {
            // Read verbatim: a mode is a label, so the extremes of a chunk that held several would
            // invent a transition into the lowest of them and lose the rest. HEARTBEAT is 1 Hz, so
            // even a long recording is only thousands of samples.
            let Ok(heartbeats) = source.db.timeseries_by_name(
                "HEARTBEAT",
                "custom_mode",
                TimeseriesArgs {
                    system_id,
                    component_id,
                    since: Some(since),
                    until: Some(until),
                    ..Default::default()
                },
            ) else {
                continue;
            };

            let mut previous = None;
            transitions.extend(heartbeats.into_iter().filter_map(|(t, mode)| {
                let mode = mode as u32;
                let changed = previous.is_some_and(|previous| previous != mode);
                previous = Some(mode);
                changed.then_some((t, mode))
            }));
        }

        transitions
    }
}

impl egui::Widget for Plot<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        let legend = Legend::default()
            .background_alpha(0.5)
            .text_style(TextStyle::Small)
            .position(Corner::LeftTop);

        // Weaken the text color, used for the grid lines.
        //let text_color = ui.style().visuals.text_color();
        //ui.style_mut().visuals.override_text_color = Some(text_color.gamma_multiply(0.5));

        //let view_end = self.backend.fc_time().unwrap_or_default();
        // The source's own clock: a recording's right edge belongs at its last record, not at now.
        let view_end = (self.source.now() - self.source.plot_origin).as_seconds_f64();
        #[allow(deprecated)] // the axis widths in egui suck, TODO
        let mut plot = egui_plot::Plot::new(ui.next_auto_id())
            .link_axis("plot_axis_group", [true, false])
            .link_cursor("plot_cursor_group", [true, false])
            .set_margin_fraction(egui::Vec2::new(0.0, 0.15))
            .allow_scroll([true, false])
            .allow_drag([true, false])
            .allow_zoom([true, false])
            //.auto_bounds([false, true])
            .y_axis_position(egui_plot::HPlacement::Right)
            // These two are needed to avoid egui adding a huge amount of space for the y axis ticks
            .y_axis_width(3)
            .y_axis_formatter(|gm, _range| {
                let tick = gm.value;
                let digits = -gm.step_size.log10() as usize;
                format!("{tick:.digits$}")
            })
            .legend(legend.clone());

        if self.shared.attached_to_edge {
            plot = plot
                .default_x_bounds(view_end - self.shared.view_width, view_end)
                .reset();
        }

        if let Some(min) = self.ylimits.0 {
            plot = plot.include_y(min);
        }

        if let Some(max) = self.ylimits.1 {
            plot = plot.include_y(max);
        }

        let visuals = ui.visuals().clone();
        let ir = plot.show(ui, move |plot_ui| {
            #[cfg(feature = "profiling")]
            puffin::profile_scope!("plot_data");

            let last_bounds = plot_ui.plot_bounds();
            let min_x = *last_bounds.range_x().start();
            let max_x = *last_bounds.range_x().end();

            let since = self.source.plot_origin + TimeDelta::seconds(min_x as i64 - MARGIN_SECS);
            let until = self.source.plot_origin + TimeDelta::seconds(max_x as i64 + MARGIN_SECS);

            for line in self.lines {
                #[cfg(feature = "profiling")]
                puffin::profile_scope!("line");

                let labelled = format_message_label(&line.message_name, line.instance.as_ref());
                let id = format!("{labelled}.{}", line.field_name);
                let base_name = line.alias.as_deref().unwrap_or(&id);
                let name = match line.unit.as_deref() {
                    Some(unit) => format!("{base_name} [{unit}]"),
                    None => base_name.to_owned(),
                };

                let instance_arg = line.instance.as_ref().map(|i| (i.field.as_str(), i.value));
                let timeseries = match self.source.db.timeseries_by_name(
                    &line.message_name,
                    &line.field_name,
                    TimeseriesArgs {
                        system_id: line.system_id,
                        component_id: line.component_id,
                        since: Some(since),
                        until: Some(until),
                        instance: instance_arg,
                        sentinel: line.sentinel,
                        max_points: Some(MAX_POINTS),
                    },
                ) {
                    Ok(timeseries) => timeseries,
                    Err(e) => {
                        tracing::error!("Failed to plot {labelled}.{}: {e:?}", line.field_name);
                        continue;
                    }
                };

                // Scaling commutes with the extremes the query picked, so it can be applied after
                // the decimation rather than before it.
                let scale = line.scale.unwrap_or(1.0);
                let plot_data: Vec<_> = timeseries
                    .into_iter()
                    .map(|(t, v)| [(t - self.source.plot_origin).as_seconds_f64(), v * scale])
                    .collect();

                let mut l = egui_plot::Line::new(name, plot_data).width(1.0_f32);
                if let Some(color) = line.color {
                    l = l.color(color);
                }

                plot_ui.line(l);
            }

            for (t, mode) in Self::mode_transitions(self.lines, self.source, since, until) {
                // Unnamed so the transitions stay out of the legend.
                plot_ui.vline(
                    VLine::new("", (t - self.source.plot_origin).as_seconds_f64())
                        .color(readable(mode_color(mode), &visuals))
                        .style(LineStyle::Dashed { length: 4.0 }),
                );
            }

            //for (key, color) in &self.config.lines {
            //    let name = format!("{key}");
            //    let plot_data = self.backend.plot_metric(key, plot_ui.plot_bounds());
            //    let line = Line::new(plot_data).name(name).color(*color).width(1.2);
            //    plot_ui.line(line);
            //}

            //for (y, color) in &state.horizontal_lines {
            //    let hl = egui_plot::HLine::new(*y).color(*color);
            //    plot_ui.hline(hl.style(LineStyle::Dashed { length: 4.0 }));
            //}
        });

        // We have to check the interaction response to notice whether the plot
        // has been dragged or otherwise detached from the end of the data.
        if let Some(_hover_pos) = ir.response.hover_pos() {
            let zoom_delta = ui.input(egui::InputState::zoom_delta_2d);
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
            if zoom_delta.x != 1.0 {
                self.shared
                    .process_zoom(ui.input(egui::InputState::zoom_delta_2d));
            } else if scroll_delta.x != 0.0 {
                self.shared.attached_to_edge = false;
            }
        }

        if ir.response.dragged_by(PointerButton::Primary) {
            self.shared.attached_to_edge = false;
        }

        if ir.response.double_clicked_by(PointerButton::Primary) {
            self.shared.attached_to_edge = true;
        }

        self.shared
            .process_drag_released(ir.response.drag_stopped());
        self.shared
            .process_box_dragging(ir.response.dragged_by(PointerButton::Secondary));

        ir.response
    }
}
