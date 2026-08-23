//! Timers and a monotonic clock, from whichever source the target has.
//!
//! `tokio::time` compiles for wasm32-unknown-unknown but panics the moment it is used.
//! `std::time::Instant::now` panics on that target too. Both are replaced using web APIs instead.

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
pub use tokio::time::{sleep, timeout};

#[cfg(target_arch = "wasm32")]
pub use wasmtimer::tokio::{sleep, timeout};
#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;
