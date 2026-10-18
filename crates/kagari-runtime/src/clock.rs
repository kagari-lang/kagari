//! Platform clock for session and GC measurements, not deterministic script inputs.
//!
//! The `web` capability supports wasm32-unknown-unknown browsers and Workers with
//! Performance.now(). Keep timestamps in their originating Worker; browser sleep
//! behavior and reduced precision can affect measurements. This does not establish
//! WASI or non-browser JS support. Other targets retain the standard library clock.

#[cfg(all(target_arch = "wasm32", target_os = "unknown", not(feature = "web")))]
compile_error!(
    "kagari-runtime on wasm32-unknown-unknown requires feature `web` and a browser/Worker with Performance.now(); non-browser hosts are unsupported"
);

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown", feature = "web")))]
use std::time;
#[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web"))]
use web_time as time;

/// Monotonic platform timestamp used only for runtime measurements.
pub(crate) type Instant = time::Instant;
