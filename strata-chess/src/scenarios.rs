//! The positions, and what counts as solving them.
//!
//! Every one was checked with the same engine that defends it: the mates are
//! mates, the fork really wins the rook, the king-and-pawn ending really is
//! won, and Réti really is a draw. `tests/scenarios.rs` is where that is
//! written down, so a position cannot quietly rot into an unsolvable one.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Win {
    /// Deliver checkmate.
    Mate,
    /// Reach an evaluation this far ahead, in centipawns.
    Material { cp: i32 },
    /// Get a pawn to the last rank.
    Promote,
    /// Survive `plies` half-moves without the evaluation leaving `band`.
    Hold { plies: u32, band: i32 },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Scenario {
    pub id: String,
    pub level: String,
    pub title: String,
    pub brief: String,
    pub fen: String,
    /// "w" or "b": the side the visitor plays.
    pub you: String,
    pub goal: String,
    pub win: Win,
    pub hint: String,
}

/// Compiled in, so the browser build needs nothing fetched to start a game.
const RAW: &str = include_str!("../scenarios.json");

pub fn all() -> Vec<Scenario> {
    serde_json::from_str(RAW).expect("scenarios.json parses")
}

pub fn get(id: &str) -> Option<Scenario> {
    all().into_iter().find(|s| s.id == id)
}
