use egui::{Align2, Color32, Context, CornerRadius, FontId, Sense, Stroke, StrokeKind, Vec2, pos2};

use crate::colors::{
    COLOR_INDICATOR_WARNING, blink_on, readable, schematic_box_stroke, schematic_line,
};
use crate::widgets::Readout;

/// Stands in for a reading the vehicle is not sending.
const NO_VALUE: &str = "--";

pub struct MeasurementIndicator {
    pub values: Vec<Option<f32>>,
    pub unit: &'static str,
    pub color: Color32,
    pub decimals: Option<u8>,
    // When set, the border blinks orange as a warning cue.
    pub blink: bool,
}

impl MeasurementIndicator {
    fn value_font() -> FontId {
        FontId::monospace(13.0)
    }

    /// Proportional, unlike the values: monospace spaces out a unit like "\u{00b0}C" for no gain.
    fn unit_font() -> FontId {
        FontId::proportional(11.0)
    }

    fn readout(&self, value: f32) -> Readout {
        Readout {
            value,
            // Without a fixed precision, keep four significant figures either side of 100.
            decimals: match self.decimals {
                Some(decimals) => usize::from(decimals),
                None if value.abs() < 100.0 => 1,
                None => 0,
            },
            font: Self::value_font(),
            color: self.color,
            ..Default::default()
        }
    }

    fn value_width(&self, ctx: &Context, value: Option<f32>) -> f32 {
        match value {
            Some(value) => self.readout(value).size(ctx).x,
            None => ctx.fonts_mut(|f| {
                f.layout_no_wrap(NO_VALUE.to_owned(), Self::value_font(), self.color)
                    .size()
                    .x
            }),
        }
    }

    pub fn intrinsic_size(&self, ctx: &Context) -> Vec2 {
        let pad = ctx.global_style().spacing.button_padding;

        let (value_row_h, unit_row_h) = ctx.fonts_mut(|f| {
            (
                f.row_height(&Self::value_font()),
                f.row_height(&Self::unit_font()),
            )
        });
        let n_values = self.values.len().max(1) as f32;
        let h = n_values * value_row_h + unit_row_h;

        let max_value_w = self
            .values
            .iter()
            .map(|v| self.value_width(ctx, *v))
            .fold(0.0_f32, f32::max);
        let unit_w = ctx.fonts_mut(|f| {
            f.layout_no_wrap(self.unit.to_owned(), Self::unit_font(), self.color)
                .size()
                .x
        });
        let w = max_value_w.max(unit_w);

        Vec2::new(w, h) + 2.0 * pad
    }
}

impl egui::Widget for MeasurementIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::hover());

        let value_font = Self::value_font();
        let unit_font = Self::unit_font();
        let style = ui.style().clone();
        let (value_row_h, unit_row_h) = ui
            .ctx()
            .fonts_mut(|f| (f.row_height(&value_font), f.row_height(&unit_font)));
        let border = if self.blink && blink_on(ui.input(|i| i.time)) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
            Stroke::new(2.0_f32, readable(COLOR_INDICATOR_WARNING, &style.visuals))
        } else {
            schematic_box_stroke(&style.visuals)
        };
        let painter = ui.painter();

        painter.rect(
            rect,
            CornerRadius::ZERO,
            style.visuals.extreme_bg_color,
            border,
            StrokeKind::Inside,
        );

        let n_values = self.values.len().max(1) as f32;
        let total_h = n_values * value_row_h + unit_row_h;
        let top = rect.center().y - total_h / 2.0;
        let cx = rect.center().x;

        for (i, v) in self.values.iter().enumerate() {
            let pos = pos2(cx, top + i as f32 * value_row_h);
            match v {
                Some(v) => {
                    self.readout(*v).paint(painter, pos, Align2::CENTER_TOP);
                }
                None => {
                    painter.text(
                        pos,
                        Align2::CENTER_TOP,
                        NO_VALUE,
                        value_font.clone(),
                        self.color,
                    );
                }
            }
        }
        painter.text(
            pos2(cx, top + n_values * value_row_h),
            Align2::CENTER_TOP,
            self.unit,
            unit_font,
            schematic_line(&style.visuals),
        );

        response
    }
}
