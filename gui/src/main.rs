#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::sync::Arc;

use eframe::egui;
use egui::FontFamily;
use tracing_subscriber::prelude::*;

mod app;
mod colors;
mod panes;
mod views;
mod widgets;

fn main() -> Result<(), eframe::Error> {
    let collector = egui_tracing::EventCollector::default();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::Targets::new()
                .with_default(tracing::Level::DEBUG)
                .with_target("reqwest", tracing::Level::INFO)
                .with_target("hyper_util", tracing::Level::INFO),
        )
        .with(tracing_subscriber::fmt::Layer::new())
        .with(collector.clone())
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };

    #[cfg(feature = "profiling")]
    puffin::set_scopes_on(true);

    eframe::run_native(
        "rapid-control",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            let mut fonts = egui::FontDefinitions::default();
            let b612 =
                egui::FontData::from_static(include_bytes!("../assets/fonts/B612-Regular.ttf"));
            let b612_mono =
                egui::FontData::from_static(include_bytes!("../assets/fonts/B612Mono-Regular.ttf"));

            fonts.font_data.insert("B612".to_owned(), Arc::new(b612));
            fonts
                .font_data
                .insert("B612 Mono".to_owned(), Arc::new(b612_mono));

            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "B612".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "B612 Mono".to_owned());
            cc.egui_ctx.set_fonts(fonts);

            let app = app::App::new(collector, &cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
