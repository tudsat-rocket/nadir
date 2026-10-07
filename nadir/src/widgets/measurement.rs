use egui::{Align2, Color32, Context, CornerRadius, FontId, Sense, Stroke, StrokeKind, Vec2, pos2};

use crate::colors::{
    COLOR_INDICATOR_WARNING, blink_on, readable, schematic_box_stroke, schematic_line,
};
use crate::widgets::Readout;

/// Stands in for a reading the vehicle is not sending.
const NO_VALUE: &str = "--";

/// A column of readings sharing a unit, which is set below them.
pub struct MeasurementSection {
    pub values: Vec<Option<f32>>,
    pub unit: &'static str,
    pub color: Color32,
    pub decimals: Option<u8>,
}

pub struct MeasurementIndicator {
    /// Header naming what is measured, e.g. a vessel id.
    pub label: Option<(String, Color32)>,
    pub sections: Vec<MeasurementSection>,
    // When set, the border blinks orange as a warning cue.
    pub blink: bool,
    /// Multiplies text and padding, for schematics drawn larger than their design size.
    pub scale: f32,
}

impl MeasurementSection {
    fn readout(&self, value: f32, font: FontId) -> Readout {
        Readout {
            value,
            // Without a fixed precision, keep four significant figures either side of 100.
            decimals: match self.decimals {
                Some(decimals) => usize::from(decimals),
                None if value.abs() < 100.0 => 1,
                None => 0,
            },
            font,
            color: self.color,
            ..Default::default()
        }
    }
}

impl MeasurementIndicator {
    pub(crate) fn value_font() -> FontId {
        FontId::monospace(13.0)
    }

    fn scaled_value_font(&self) -> FontId {
        FontId::monospace(Self::value_font().size * self.scale)
    }

    /// Proportional, unlike the values: monospace spaces out a unit like "\u{00b0}C" for no gain.
    fn unit_font(&self) -> FontId {
        FontId::proportional(11.0 * self.scale)
    }

    fn label_font(&self) -> FontId {
        FontId::monospace(9.5 * self.scale)
    }

    fn section_gap(&self) -> f32 {
        3.0 * self.scale
    }

    fn value_width(&self, ctx: &Context, section: &MeasurementSection, value: Option<f32>) -> f32 {
        let font = self.scaled_value_font();
        match value {
            Some(value) => section.readout(value, font).size(ctx).x,
            None => ctx.fonts_mut(|f| {
                f.layout_no_wrap(NO_VALUE.to_owned(), font, section.color)
                    .size()
                    .x
            }),
        }
    }

    fn text_width(ctx: &Context, text: &str, font: FontId) -> f32 {
        ctx.fonts_mut(|f| {
            f.layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
                .size()
                .x
        })
    }

    /// Row heights of the label, a value and a unit.
    fn row_heights(&self, ctx: &Context) -> (f32, f32, f32) {
        ctx.fonts_mut(|f| {
            (
                f.row_height(&self.label_font()),
                f.row_height(&self.scaled_value_font()),
                f.row_height(&self.unit_font()),
            )
        })
    }

    fn content_height(&self, ctx: &Context) -> f32 {
        let (label_h, value_h, unit_h) = self.row_heights(ctx);
        let sections: f32 = self
            .sections
            .iter()
            .map(|s| s.values.len().max(1) as f32 * value_h + unit_h)
            .sum();
        let gaps = self.sections.len().saturating_sub(1) as f32 * self.section_gap();
        self.label.as_ref().map_or(0.0, |_| label_h) + sections + gaps
    }

    pub fn intrinsic_size(&self, ctx: &Context) -> Vec2 {
        let pad = ctx.global_style().spacing.button_padding * self.scale;
        let w = self
            .sections
            .iter()
            .flat_map(|section| {
                section
                    .values
                    .iter()
                    .map(|v| self.value_width(ctx, section, *v))
                    .chain([Self::text_width(ctx, section.unit, self.unit_font())])
            })
            .chain(
                self.label
                    .iter()
                    .map(|(label, _)| Self::text_width(ctx, label, self.label_font())),
            )
            .fold(0.0_f32, f32::max);
        Vec2::new(w, self.content_height(ctx)) + 2.0 * pad
    }
}

impl egui::Widget for MeasurementIndicator {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::hover());

        let value_font = self.scaled_value_font();
        let unit_font = self.unit_font();
        let style = ui.style().clone();
        let (label_h, value_h, unit_h) = self.row_heights(ui.ctx());
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

        let cx = rect.center().x;
        let mut y = rect.center().y - self.content_height(ui.ctx()) / 2.0;
        if let Some((label, color)) = &self.label {
            painter.text(
                pos2(cx, y),
                Align2::CENTER_TOP,
                label,
                self.label_font(),
                *color,
            );
            y += label_h;
        }
        for (i, section) in self.sections.iter().enumerate() {
            if i > 0 {
                y += self.section_gap();
            }
            for v in &section.values {
                let pos = pos2(cx, y);
                match v {
                    Some(v) => {
                        section.readout(*v, value_font.clone()).paint(
                            painter,
                            pos,
                            Align2::CENTER_TOP,
                        );
                    }
                    None => {
                        painter.text(
                            pos,
                            Align2::CENTER_TOP,
                            NO_VALUE,
                            value_font.clone(),
                            section.color,
                        );
                    }
                }
                y += value_h;
            }
            if section.values.is_empty() {
                y += value_h;
            }
            painter.text(
                pos2(cx, y),
                Align2::CENTER_TOP,
                section.unit,
                unit_font.clone(),
                schematic_line(&style.visuals),
            );
            y += unit_h;
        }

        response
    }
}
