#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), eframe::Error> {
    gui::run_native(std::env::args_os().skip(1).map(Into::into).collect())
}

/// Browser entry point. Trunk's generated loader calls this as the module's start function.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();

    let collector = gui::init_tracing();
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
                Box::new(|cc| Ok(gui::build_app(cc, collector, Vec::new()))),
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
