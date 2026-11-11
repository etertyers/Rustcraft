//! Time. `std::time::Instant::now()` panics on `wasm32-unknown-unknown`, so
//! the browser build uses `web-time`, which is the same API over
//! `performance.now()`. `Duration` is std's in both.

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
