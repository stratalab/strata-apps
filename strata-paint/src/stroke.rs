//! What a stroke is.
//!
//! A stroke is the unit of history here: one press-drag-release, one commit.
//! It is immutable once written, which is what lets a scrub read the keys that
//! existed at a timestamp and then read each at latest - a stroke cannot have
//! changed since, so the two answers are the same one.

use serde::{Deserialize, Serialize};

/// One press-drag-release, in canvas coordinates.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Stroke {
    /// CSS colour, carried through untouched so the palette lives in the UI.
    pub colour: String,
    pub width: f32,
    /// `[x, y]` pairs in a 0..1 square, so a painting survives a resized
    /// window and a phone opening what a laptop drew.
    pub points: Vec<[f32; 2]>,
    /// Set by the eraser. Kept as a flag rather than a background-coloured
    /// stroke so the canvas can be drawn on any backdrop later.
    #[serde(default)]
    pub erase: bool,
}

impl Stroke {
    /// Rejects what would be stored but could never be drawn.
    ///
    /// A stroke with no points is a click that never moved; the UI drops those
    /// before they reach here, and the store refuses them so a malformed call
    /// from anywhere else cannot leave a commit with nothing in it.
    pub fn validate(&self) -> Result<(), String> {
        if self.points.is_empty() {
            return Err("a stroke needs at least one point".into());
        }
        if self.points.len() > MAX_POINTS {
            return Err(format!(
                "a stroke is capped at {MAX_POINTS} points, got {}",
                self.points.len()
            ));
        }
        if !self.width.is_finite() || self.width <= 0.0 {
            return Err(format!("stroke width must be positive, got {}", self.width));
        }
        if self.colour.is_empty() {
            return Err("a stroke needs a colour".into());
        }
        if let Some(bad) = self
            .points
            .iter()
            .find(|p| !p[0].is_finite() || !p[1].is_finite())
        {
            return Err(format!("stroke point is not finite: {bad:?}"));
        }
        Ok(())
    }
}

/// A long drag at 120Hz is a few thousand points; the UI thins them before
/// sending. This is the backstop that keeps one commit from holding a
/// megabyte of coordinates.
pub const MAX_POINTS: usize = 4_000;
