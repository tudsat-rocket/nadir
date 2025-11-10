#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use tracing_subscriber::prelude::*;

mod app;
mod panes;
mod views;
mod widgets;

fn ui_main(core: core::Core, collector: egui_tracing::EventCollector) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };

    eframe::run_native(
        "rapid-control",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            let app = app::App::new(core, collector, &cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

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

    let core = core::Core::init();
    let c = core.clone();

    let join_handle =
        std::thread::spawn(|| tokio::runtime::Runtime::new().unwrap().block_on(c.run()));

    ui_main(core, collector)?;

    join_handle.join().unwrap();

    Ok(())
}
