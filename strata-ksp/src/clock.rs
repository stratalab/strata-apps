//! One clock for two targets.
//!
//! `std::time::Instant` is not implemented on wasm32-unknown-unknown: calling
//! it panics at runtime rather than failing to compile, which is the worst of
//! both. `web_time` re-exports the std type on native and reads
//! `performance.now()` in a browser, so the persist timings this app reports
//! are measured the same way on both builds.

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;
