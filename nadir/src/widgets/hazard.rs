use nadir_core::System;

use egui::{Color32, Rect, Response, Shape, Stroke, Ui, pos2, vec2};

use crate::colors::{COLOR_INDICATOR_ADVANCED, readable};

/// Guard for actions that need a press-and-hold, unless the system is [`System::hot`].
pub struct Hazard;

impl Hazard {
    const HOLD_SECS: f64 = 1.0;

    pub const HINT: &str = "Hold to confirm";

    /// Whether the guarded action should fire this frame. `response` needs to sense clicks.
    pub fn confirm(ui: &Ui, response: &Response, system: &System) -> bool {
        if !system.hot() {
            let _ = response.clone().on_hover_text(Self::HINT);
        }
        Self::held(ui, response, system)
    }

    /// [`Self::confirm`] without the hover hint, for widgets that fold [`Self::HINT`] into their own.
    pub fn held(ui: &Ui, response: &Response, system: &System) -> bool {
        if system.hot() {
            return response.clicked();
        }

        // `None` once fired, so a hold that outlasts the timer does not repeat.
        let id = response.id.with("hazard_hold");
        // Not `is_pointer_button_down_on`: egui stops reporting a click-only widget as pressed once
        // the press outlasts its max click duration, which is shorter than the hold.
        let held = ui.input(|i| {
            i.pointer.primary_down()
                && i.pointer
                    .press_origin()
                    .is_some_and(|p| response.rect.contains(p))
        });
        if !(held && response.enabled() && response.contains_pointer()) {
            ui.data_mut(|d| d.remove::<Option<f64>>(id));
            return false;
        }
        let now = ui.input(|i| i.time);
        let Some(start) = ui.data_mut(|d| *d.get_temp_mut_or(id, Some(now))) else {
            return false;
        };

        let progress = ((now - start) / Self::HOLD_SECS) as f32;
        if progress >= 1.0 {
            ui.data_mut(|d| d.insert_temp::<Option<f64>>(id, None));
            return true;
        }

        let rect = response.rect;
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(rect.width() * progress, rect.height())),
            ui.visuals().widgets.active.corner_radius,
            readable(COLOR_INDICATOR_ADVANCED, ui.visuals()).gamma_multiply(0.8),
        );
        ui.ctx().request_repaint();
        false
    }

    /// Amber corner tick marking a control as hazardous, hollow while the system is hot.
    pub fn tick(ui: &Ui, rect: Rect, system: &System) {
        let size = (rect.height() * 0.3).min(8.0);
        let (right, top) = (rect.max.x - 2.0, rect.min.y + 2.0);
        let color = readable(COLOR_INDICATOR_ADVANCED, ui.visuals());
        let (fill, stroke) = if system.hot() {
            (Color32::TRANSPARENT, Stroke::new(1.0_f32, color))
        } else {
            (color, Stroke::NONE)
        };
        ui.painter().add(Shape::convex_polygon(
            vec![
                pos2(right - size, top),
                pos2(right, top),
                pos2(right, top + size),
            ],
            fill,
            stroke,
        ));
    }
}
