//! Mini Kerbal on Strata — library surface for the `ksp` binary and tests.
//!
//! PR9: golden freeze, archive launches, compare, clippy-clean.

pub mod ascent;
pub mod clock;
pub mod craft;
pub mod findings;
pub mod physics;
pub mod snapshot;
pub mod store;
pub mod telemetry;

#[cfg(feature = "wasm")]
pub mod wasm;
pub mod world;
