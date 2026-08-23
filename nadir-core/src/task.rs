//! Handing work to whichever executor the target has.

/// A browser has no tokio runtime to hand a future to - `tokio::spawn` panics without one - but it
/// does have its own event loop. No `Send` bound, because there is one thread.
#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}

/// Hands the executor a chance to run something else, for a loop long enough to starve it.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn yield_now() {
    tokio::task::yield_now().await;
}

/// A zero-length timer, because the browser's event loop has no yield of its own: this is what
/// hands the frame back to whatever draws it.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn yield_now() {
    crate::time::sleep(std::time::Duration::ZERO).await;
}
