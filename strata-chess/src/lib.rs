//! An analysis board where every variation is a real database branch.
//!
//! A chess GUI keeps its variation tree in memory and serialises it to PGN
//! with brackets. Here a variation is a branch: playing a different move from a
//! position you have already visited forks the database at that commit, and the
//! tree of lines in the sidebar is the branch list read back from the engine.
//!
//! The rules are `cozy-chess` (MIT). The opponent is Stockfish (GPL-3.0),
//! vendored under `vendor/stockfish/` and run as a separate Web Worker: see the
//! notice there. Neither is linked into this crate.

pub mod game;
pub mod generate;
pub mod scenarios;
pub mod store;

#[cfg(feature = "wasm")]
mod wasm;

pub use scenarios::Scenario;
pub use store::Ply;
