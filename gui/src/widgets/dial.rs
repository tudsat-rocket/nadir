use std::f32::consts::PI;

use egui::{Align2, Color32, FontId, Sense, Stroke, Vec2};

use crate::colors::COLOR_INDICATOR_GOOD;

pub struct Dial {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub absolute_min: f32,
    pub absolute_max: f32,
    pub trim: Option<f32>,
}

const N: usize = 32;

impl egui::Widget for Dial {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let minside = f32::min(ui.available_width() / 2.0, ui.available_height());

        let (response, painter) =
            ui.allocate_painter(Vec2::new(minside * 2.0, minside), Sense::empty());

        let dial_radius = minside * 0.7;
        let dial_center = response.rect.center() + Vec2::new(0.0, dial_radius / 2.0);

        let range = self.absolute_max - self.absolute_min;
        let min = (self.min - self.absolute_min) / range;
        let max = (self.max - self.absolute_min) / range;

        let points = (0..=N)
            .map(|i| min * PI * (i as f32) / (N as f32))
            .map(|r| dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius)
            .collect();
        painter.line(points, Stroke::new(1.5, Color32::RED));

        let points = (0..=N)
            .map(|i| min * PI + (max - min) * PI * (i as f32) / (N as f32))
            .map(|r| dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius)
            .collect();
        painter.line(points, Stroke::new(1.5, Color32::WHITE));

        let points = (0..=N)
            .map(|i| max * PI + (1.0 - max) * PI * (i as f32) / (N as f32))
            .map(|r| dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius)
            .collect();
        painter.line(points, Stroke::new(1.5, Color32::RED));

        for i in 0..=10 {
            let f = (i as f32) / 10.0;
            let r = PI * f;
            let color = if f <= min || f >= max {
                Color32::RED
            } else {
                Color32::WHITE
            };

            painter.line(
                vec![
                    dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius,
                    dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius * 0.9,
                ],
                Stroke::new(0.75, color),
            );
        }

        let f = (self.value - self.absolute_min) / (self.absolute_max - self.absolute_min);
        let r = PI * f;
        painter.line(
            vec![
                dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius * 0.8,
                dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius * 1.3,
            ],
            Stroke::new(2.5, COLOR_INDICATOR_GOOD),
        );

        if let Some(trim) = self.trim {
            let f = (trim - self.min) / (self.max - self.min);
            let r = PI * f;
            painter.line(
                vec![
                    dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius,
                    dial_center + Vec2::new(-r.cos(), -r.sin()) * dial_radius * 0.7,
                ],
                Stroke::new(2.0, Color32::WHITE),
            );
        }

        painter.text(
            dial_center,
            Align2::CENTER_CENTER,
            format!("{:.0}", self.value),
            FontId::monospace(dial_radius * 0.4),
            Color32::WHITE,
        );

        painter.text(
            dial_center - Vec2::new(0.0, dial_radius * 0.55),
            Align2::CENTER_CENTER,
            "µs",
            FontId::monospace(dial_radius * 0.3),
            Color32::WHITE,
        );

        response
    }
}
