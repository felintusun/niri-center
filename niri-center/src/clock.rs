//! The time now, the one place the library reads it: the browser has no
//! std clock (`SystemTime::now` and `Instant::now` panic there), so wasm
//! reads it through `web-time`.

use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
use web_time::{SystemTime, UNIX_EPOCH};

/// A moment, to wait from: std's, or the browser's.
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;

/// Time since the Unix epoch; zero for a clock set before it.
fn since_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// Milliseconds since the Unix epoch.
pub(crate) fn now_millis() -> u128 {
    since_epoch().as_millis()
}

/// Nanoseconds since the Unix epoch.
pub(crate) fn now_nanos() -> u128 {
    since_epoch().as_nanos()
}
