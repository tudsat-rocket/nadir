use std::sync::Arc;

use egui::epaint::Galley;
use egui::{Align2, Color32, Context, FontId, Painter, Pos2, Rect, Sense, Vec2};

/// Size of the decimals and unit, relative to the integer part.
const SMALL_SCALE: f32 = 0.80;
/// Gap left between the ink of the decimal point and the decimals, relative to the font size.
const GAP: f32 = 0.12;

/// A number, with its decimals and unit set smaller and tucked against the decimal point.
///
/// Monospace fonts put the ink of a `.` at the left of its cell, so a plain `{:.1}` leaves a hole
/// wide enough to read as a space. This measures that overhang and closes it.
pub struct Readout {
    pub value: f32,
    pub decimals: usize,
    /// Force a leading `+` on positive values.
    pub signed: bool,
    /// Pads the number so a column of them keeps a fixed width as the value grows.
    pub width_chars: usize,
    /// Label set ahead of the number, at the full size.
    pub prefix: &'static str,
    pub unit: Option<&'static str>,
    pub font: FontId,
    pub color: Color32,
}

struct Layout {
    head: Arc<Galley>,
    tail: Arc<Galley>,
    tail_offset: Vec2,
    size: Vec2,
}

impl Default for Readout {
    fn default() -> Self {
        Self {
            value: 0.0,
            decimals: 1,
            signed: false,
            width_chars: 0,
            prefix: "",
            unit: None,
            font: FontId::monospace(14.0),
            color: Color32::WHITE,
        }
    }
}

impl Readout {
    /// Paints the number, returning the rect it occupies.
    pub fn paint(&self, painter: &Painter, pos: Pos2, anchor: Align2) -> Rect {
        let layout = self.layout(painter.ctx());
        let rect = anchor.anchor_size(pos, layout.size);
        self.paint_layout(painter, rect.min, layout);
        rect
    }

    /// Space the number occupies, for callers that place themselves.
    pub fn size(&self, ctx: &Context) -> Vec2 {
        self.layout(ctx).size
    }

    fn paint_layout(&self, painter: &Painter, min: Pos2, layout: Layout) {
        painter.galley(min, layout.head, self.color);
        painter.galley(min + layout.tail_offset, layout.tail, self.color);
    }

    fn layout(&self, ctx: &Context) -> Layout {
        // Formatting the value whole and splitting it keeps the parts from disagreeing: rounding
        // the integer separately turns 12.6 into "13.6". The padding lands ahead of the sign, so
        // the head stays a fixed number of cells wide and the columns line up.
        let (width, decimals) = (self.width_chars, self.decimals);
        let text = if self.signed {
            format!("{:+width$.decimals$}", self.value)
        } else {
            format!("{:width$.decimals$}", self.value)
        };
        let (integer, decimals) = text.split_once('.').unwrap_or((text.as_str(), ""));

        let head = if decimals.is_empty() {
            format!("{}{integer}", self.prefix)
        } else {
            format!("{}{integer}.", self.prefix)
        };
        let tail = format!("{decimals}{}", self.unit.unwrap_or_default());
        let small = FontId::new(self.font.size * SMALL_SCALE, self.font.family.clone());

        let (head, tail) = ctx.fonts(|f| {
            (
                f.layout_no_wrap(head, self.font.clone(), self.color),
                f.layout_no_wrap(tail, small, self.color),
            )
        });

        let round = |v: f32| (v * ctx.pixels_per_point()).round() / ctx.pixels_per_point();
        let tail_offset = Vec2::new(
            round(Self::ink_right(&head) + GAP * self.font.size),
            round(Self::baseline(&head) - Self::baseline(&tail)),
        );
        let size = Vec2::new(
            head.rect.width().max(tail_offset.x + tail.rect.width()),
            head.rect.height(),
        );

        Layout {
            head,
            tail,
            tail_offset,
            size,
        }
    }

    /// Right edge of the last glyph's ink, rather than of its advance width.
    fn ink_right(galley: &Galley) -> f32 {
        galley
            .rows
            .first()
            .and_then(|row| {
                let glyph = row.row.glyphs.last()?;
                if glyph.uv_rect.is_nothing() {
                    return None;
                }
                Some(row.pos.x + glyph.pos.x + glyph.uv_rect.offset.x + glyph.uv_rect.size.x)
            })
            .unwrap_or_else(|| galley.rect.right())
    }

    fn baseline(galley: &Galley) -> f32 {
        galley.rows.first().map_or(0.0, |row| {
            row.pos.y + row.row.glyphs.first().map_or(0.0, |glyph| glyph.pos.y)
        })
    }
}

impl egui::Widget for Readout {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let layout = self.layout(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(layout.size, Sense::hover());
        if ui.is_rect_visible(rect) {
            self.paint_layout(ui.painter(), rect.min, layout);
        }
        response
    }
}
