//! Genesis soup and one-cell lies.

use std::collections::HashSet;

use crate::life::Board;

/// Seeded xorshift. Deterministic across runs so the twenty lies stay comparable.
fn mix(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

/// A dense primordial soup in the inner rectangle, plus a glider and an r-pentomino
/// so the plate is never a blank field at generation zero.
pub fn genesis(width: u32, height: u32, seed: u64) -> Board {
    let mut board = Board::new(width, height);
    let mut state = mix(seed ^ 0x9e37_79b9_7f4a_7c15);

    let margin_x = width / 6;
    let margin_y = height / 6;
    for y in margin_y..height.saturating_sub(margin_y) {
        for x in margin_x..width.saturating_sub(margin_x) {
            state = mix(state);
            if state % 5 < 2 {
                board.set(x, y, true);
            }
        }
    }

    // Glider, north-west.
    let gx = 2;
    let gy = 2;
    for (dx, dy) in [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)] {
        board.set(gx + dx, gy + dy, true);
    }

    // r-pentomino, south-east of centre — a methuselah.
    let rx = width / 2 + 3;
    let ry = height / 2 + 2;
    for (dx, dy) in [(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)] {
        if rx + dx < width && ry + dy < height {
            board.set(rx + dx, ry + dy, true);
        }
    }

    board
}

/// Pick a cell to flip on colony `index` (1-based mutation number).
///
/// Prefers a dead cell adjacent to a live one so the lie actually seeds a
/// butterfly, rather than flipping a cell in empty space that dies immediately.
pub fn perturbation(board: &Board, index: usize, used: &HashSet<(u32, u32)>) -> (u32, u32) {
    let w = board.width();
    let h = board.height();
    let start_x = ((index as u32) * 7 + 13) % w;
    let start_y = ((index as u32) * 11 + 5) % h;

    // Spiral search from the hash point.
    let max = (w * h) as i32;
    for radius in 0..max {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs() != radius && dy.abs() != radius {
                    continue;
                }
                let x = (i64::from(start_x) + i64::from(dx)).rem_euclid(i64::from(w)) as u32;
                let y = (i64::from(start_y) + i64::from(dy)).rem_euclid(i64::from(h)) as u32;
                if used.contains(&(x, y)) {
                    continue;
                }
                if is_useful_flip(board, x, y) {
                    return (x, y);
                }
            }
        }
    }
    (start_x, start_y)
}

fn is_useful_flip(board: &Board, x: u32, y: u32) -> bool {
    if board.live(x, y) {
        return false;
    }
    let w = board.width();
    let h = board.height();
    for dy in [h - 1, 0, 1] {
        for dx in [w - 1, 0, 1] {
            if dx == 0 && dy == 0 {
                continue;
            }
            if board.live((x + dx) % w, (y + dy) % h) {
                return true;
            }
        }
    }
    false
}

pub fn colony_name(index: usize) -> String {
    if index == 0 {
        "control".to_owned()
    } else {
        format!("mut-{index:02}")
    }
}

pub const COLONY_COUNT: usize = 20;
pub const SEED_BRANCH: &str = "default";
pub const SPACE: &str = "life";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nineteen_lies_are_unique() {
        let board = genesis(64, 48, 42);
        let mut used = HashSet::new();
        for index in 1..=19 {
            let cell = perturbation(&board, index, &used);
            assert!(
                used.insert(cell),
                "duplicate lie {cell:?} at mut-{index:02}"
            );
        }
        assert_eq!(used.len(), 19);
    }
}
