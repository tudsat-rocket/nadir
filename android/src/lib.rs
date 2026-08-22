#![cfg(target_os = "android")]

mod logcat;

/// Called from the `ANativeActivity_onCreate` that `android-activity` supplies.
#[unsafe(no_mangle)]
fn android_main(app: gui::AndroidApp) {
    // `directories` derives every path from $HOME, which Android leaves unset.
    // SAFETY: no other thread exists yet to observe the environment.
    if let Some(dir) = app.internal_data_path() {
        unsafe { std::env::set_var("HOME", dir) };
    }

    logcat::redirect_stdio();

    if let Err(e) = gui::run_android(app) {
        tracing::error!("{e}");
    }
}
