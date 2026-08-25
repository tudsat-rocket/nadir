use std::sync::Arc;

pub use eframe;

use eframe::egui;
use egui::FontFamily;

mod app;
mod colors;
mod panes;
mod shell;
mod views;
mod widgets;

/// Re-exported through eframe's winit, so `android_main` cannot pick a mismatched version.
#[cfg(target_os = "android")]
pub type AndroidApp = egui_winit::winit::platform::android::activity::AndroidApp;

/// Everything the app needs from the creation context, shared by all entry points.
pub fn build_app(
    cc: &eframe::CreationContext<'_>,
    collector: egui_tracing::EventCollector,
    logs: Vec<std::path::PathBuf>,
) -> Box<dyn eframe::App> {
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

    Box::new(app::App::new(collector, &cc.egui_ctx, logs))
}

/// Installs the tracing subscriber and returns the collector the log panel reads from.
pub fn init_tracing() -> egui_tracing::EventCollector {
    use tracing_subscriber::prelude::*;

    let collector = egui_tracing::EventCollector::default();
    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::Targets::new()
                .with_default(tracing::Level::DEBUG)
                .with_target("reqwest", tracing::Level::INFO)
                .with_target("hyper_util", tracing::Level::INFO),
        )
        .with(collector.clone());

    // Stdout is a null device in a browser, so there the panel is the only sink.
    #[cfg(not(target_arch = "wasm32"))]
    let registry = registry.with(tracing_subscriber::fmt::Layer::new());

    registry.init();

    collector
}

/// A `None` zoom leaves whatever eframe persisted, so the platforms that do not scale the UI cannot
/// clobber a user's setting.
#[cfg(not(target_arch = "wasm32"))]
fn run(
    options: eframe::NativeOptions,
    logs: Vec<std::path::PathBuf>,
    zoom: Option<f32>,
) -> Result<(), eframe::Error> {
    let collector = init_tracing();

    #[cfg(feature = "profiling")]
    puffin::set_scopes_on(true);

    eframe::run_native(
        "nadir",
        options,
        Box::new(move |cc| {
            if let Some(zoom) = zoom {
                cc.egui_ctx.set_zoom_factor(zoom);
            }

            Ok(build_app(cc, collector, logs))
        }),
    )
}

/// Desktop entry point.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn run_native(logs: Vec<std::path::PathBuf>) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1920.0, 1080.0]),
        ..Default::default()
    };

    run(options, logs, None)
}

/// Android entry point, called from the `android` crate's `android_main`.
#[cfg(target_os = "android")]
pub fn run_android(app: AndroidApp) -> Result<(), eframe::Error> {
    // Off the GL backend, egui-wgpu's default asks for `Limits::default()`, which a Mali GPU cannot
    // meet - `max_compute_workgroup_size_y` is 128 there against a default of 256 - and device
    // creation fails outright.
    let wgpu_setup = eframe::egui_wgpu::WgpuSetupCreateNew {
        device_descriptor: Arc::new(|adapter: &eframe::wgpu::Adapter| {
            eframe::wgpu::DeviceDescriptor {
                label: Some("egui wgpu device"),
                required_limits: adapter.limits(),
                ..Default::default()
            }
        }),
        ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
    };

    let options = eframe::NativeOptions {
        android_app: Some(app),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: wgpu_setup.into(),
            ..Default::default()
        },
        ..Default::default()
    };

    run(options, Vec::new(), Some(0.8))
}
