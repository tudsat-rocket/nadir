#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::sync::Arc;

use eframe::egui;
use egui::FontFamily;

mod app;
mod colors;
mod panes;
mod shell;
mod views;
mod widgets;

/// Everything the app needs from the creation context, shared by both entry points.
fn build_app(
    cc: &eframe::CreationContext<'_>,
    collector: egui_tracing::EventCollector,
) -> Box<dyn eframe::App> {
    // This gives us image support:
    egui_extras::install_image_loaders(&cc.egui_ctx);

    let mut fonts = egui::FontDefinitions::default();
    let b612 = egui::FontData::from_static(include_bytes!("../assets/fonts/B612-Regular.ttf"));
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

    Box::new(app::App::new(collector, &cc.egui_ctx))
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), eframe::Error> {
    use tracing_subscriber::prelude::*;

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
        viewport: egui::ViewportBuilder::default().with_inner_size([1920.0, 1080.0]),
        //.with_maximized(true),
        ..Default::default()
    };

    #[cfg(feature = "profiling")]
    puffin::set_scopes_on(true);

    eframe::run_native(
        "rapid-control",
        options,
        Box::new(|cc| Ok(build_app(cc, collector))),
    )
}

/// Browser entry point. Trunk's generated loader calls this as the module's start function.
#[cfg(target_arch = "wasm32")]
fn main() {
    use tracing_subscriber::prelude::*;
    use wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();

    let collector = egui_tracing::EventCollector::default();
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::Targets::new().with_default(tracing::Level::DEBUG))
        .with(collector.clone())
        .init();

    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let canvas = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document")
        .get_element_by_id("the_canvas_id")
        .expect("no #the_canvas_id")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("#the_canvas_id is not a canvas");

    wasm_bindgen_futures::spawn_local(async move {
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(build_app(cc, collector))),
            )
            .await;

        match result {
            Ok(()) => {
                if let Some(status) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id("status"))
                {
                    status.remove();
                }
            }
            Err(e) => log::error!("failed to start: {e:?}"),
        }
    });
}
