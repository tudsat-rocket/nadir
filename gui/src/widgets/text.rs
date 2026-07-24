use eframe::egui;
use egui::{Color32, RichText};

/// Size of the dense value text used across the status bar.
pub(crate) const TEXT_SIZE: f32 = 11.0;

/// One dense monospace value, truncated rather than wrapped: these sit in fixed-width columns where
/// a second line would push everything below it out of the zone.
pub(crate) fn small_text(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(RichText::new(text).monospace().size(TEXT_SIZE).color(color))
            .wrap_mode(egui::TextWrapMode::Truncate),
    );
}

pub(crate) fn column_header(ui: &mut egui::Ui, title: &str) {
    ui.add(
        egui::Label::new(RichText::new(title).size(10.0).weak())
            .wrap_mode(egui::TextWrapMode::Truncate),
    );
}
