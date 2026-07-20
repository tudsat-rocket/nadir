use egui::{Align2, Color32, Context, CornerRadius, FontId, Sense, Stroke, StrokeKind, Vec2, pos2};

use crate::colors::{COLOR_INDICATOR_WARNING, blink_on};

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
        FontId::proportional(13.0)
    }

    fn unit_font() -> FontId {
        FontId::proportional(11.0)
    }

    fn format_value(&self, v: Option<f32>) -> String {
        match (v, self.decimals) {
            (Some(v), Some(d)) => format!("{v:.*}", d as usize),
            (Some(v), None) if v.abs() < 100.0 => format!("{v:.1}"),
            (Some(v), None) => format!("{v:.0}"),
            (None, _) => "--".to_string(),
        }
    }

    pub fn intrinsic_size(&self, ctx: &Context) -> Vec2 {
        let value_font = Self::value_font();
        let unit_font = Self::unit_font();
        let pad = ctx.style().spacing.button_padding;

        let (value_row_h, unit_row_h) =
            ctx.fonts(|f| (f.row_height(&value_font), f.row_height(&unit_font)));
        let n_values = self.values.len().max(1) as f32;
        let h = n_values * value_row_h + unit_row_h;

        let measure = |text: String, font: FontId| {
            ctx.fonts(|f| f.layout_no_wrap(text, font, Color32::WHITE).size().x)
        };
        let max_value_w = self
            .values
            .iter()
            .map(|v| measure(self.format_value(*v), value_font.clone()))
            .fold(0.0_f32, f32::max);
        let unit_w = measure(self.unit.to_string(), unit_font);
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
        let style = ui.ctx().style();
        let (value_row_h, unit_row_h) = ui
            .ctx()
            .fonts(|f| (f.row_height(&value_font), f.row_height(&unit_font)));
        let border = if self.blink && blink_on(ui.input(|i| i.time)) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
            Stroke::new(2.0, COLOR_INDICATOR_WARNING)
        } else {
            style.visuals.window_stroke()
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
            painter.text(
                pos2(cx, top + i as f32 * value_row_h),
                Align2::CENTER_TOP,
                self.format_value(*v),
                value_font.clone(),
                self.color,
            );
        }
        painter.text(
            pos2(cx, top + n_values * value_row_h),
            Align2::CENTER_TOP,
            self.unit,
            unit_font,
            style.visuals.weak_text_color(),
        );

        response
    }
}
