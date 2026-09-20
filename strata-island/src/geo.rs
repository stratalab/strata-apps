//! Integer local meters. Camera and snap live here.
//!
//! Keep this module integer-only. `tests/geo.rs` greps the source.

use crate::error::IslandError;

/// Battery origin latitude × 10_000 (40.7003).
pub const ORIGIN_LAT_E4: i32 = 407_003;
/// Battery origin longitude × 10_000 (−74.0170).
pub const ORIGIN_LON_E4: i32 = -740_170;

pub const AABB_X_MIN: i32 = -1600;
pub const AABB_X_MAX: i32 = 9600;
pub const AABB_Y_MIN: i32 = -600;
pub const AABB_Y_MAX: i32 = 20400;

/// Snap radius squared: 80 m.
pub const SNAP_R2: i64 = 80 * 80;

#[must_use]
pub const fn in_aabb(x: i32, y: i32) -> bool {
    x >= AABB_X_MIN && x <= AABB_X_MAX && y >= AABB_Y_MIN && y <= AABB_Y_MAX
}

#[must_use]
pub fn dist2(ax: i32, ay: i32, bx: i32, by: i32) -> i64 {
    let dx = i64::from(ax) - i64::from(bx);
    let dy = i64::from(ay) - i64::from(by);
    dx * dx + dy * dy
}

/// Nearest node within 80 m. Tie-break: lower index.
pub fn snap(xy: &[(i32, i32)], x: i32, y: i32) -> Result<usize, IslandError> {
    let mut best: Option<(i64, usize)> = None;
    for (index, &(nx, ny)) in xy.iter().enumerate() {
        let d2 = dist2(nx, ny, x, y);
        if d2 > SNAP_R2 {
            continue;
        }
        let take = match best {
            None => true,
            Some((best_d2, best_index)) => d2 < best_d2 || (d2 == best_d2 && index < best_index),
        };
        if take {
            best = Some((d2, index));
        }
    }
    best.map(|(_, index)| index)
        .ok_or(IslandError::code("invalid_argument.island.snap"))
}
