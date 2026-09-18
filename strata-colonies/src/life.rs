//! Conway's Game of Life on a toroidal grid.

use std::collections::HashSet;

/// Packed, row-major bit board. Bit 0 of byte 0 is cell (0, 0).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Board {
    width: u32,
    height: u32,
    cells: Vec<u8>,
}

impl Board {
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width > 0 && height > 0, "board must be non-empty");
        let bits = (width as usize) * (height as usize);
        Self {
            width,
            height,
            cells: vec![0; bits.div_ceil(8)],
        }
    }

    pub fn from_packed(width: u32, height: u32, packed: Vec<u8>) -> Option<Self> {
        let bits = (width as usize) * (height as usize);
        if packed.len() != bits.div_ceil(8) {
            return None;
        }
        Some(Self {
            width,
            height,
            cells: packed,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn packed(&self) -> &[u8] {
        &self.cells
    }

    pub fn packed_vec(&self) -> Vec<u8> {
        self.cells.clone()
    }

    fn bit_index(&self, x: u32, y: u32) -> (usize, u8) {
        let i = (y as usize) * (self.width as usize) + (x as usize);
        (i / 8, 1 << (i % 8))
    }

    pub fn live(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let (byte, mask) = self.bit_index(x, y);
        self.cells[byte] & mask != 0
    }

    pub fn set(&mut self, x: u32, y: u32, live: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let (byte, mask) = self.bit_index(x, y);
        if live {
            self.cells[byte] |= mask;
        } else {
            self.cells[byte] &= !mask;
        }
    }

    pub fn flip(&mut self, x: u32, y: u32) {
        self.set(x, y, !self.live(x, y));
    }

    pub fn live_count(&self) -> u32 {
        self.cells.iter().map(|byte| byte.count_ones()).sum()
    }

    pub fn live_cells(&self) -> HashSet<(u32, u32)> {
        let mut out = HashSet::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if self.live(x, y) {
                    out.insert((x, y));
                }
            }
        }
        out
    }

    /// Wrap-around Moore neighbourhood.
    fn neighbour_count(&self, x: u32, y: u32) -> u8 {
        let mut n = 0u8;
        for dy in [self.height - 1, 0, 1] {
            for dx in [self.width - 1, 0, 1] {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = (x + dx) % self.width;
                let ny = (y + dy) % self.height;
                if self.live(nx, ny) {
                    n += 1;
                }
            }
        }
        n
    }

    /// One simultaneous generation.
    pub fn step(&self) -> Self {
        let mut next = Self::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                let n = self.neighbour_count(x, y);
                let alive = self.live(x, y);
                let stay = alive && (n == 2 || n == 3);
                let born = !alive && n == 3;
                if stay || born {
                    next.set(x, y, true);
                }
            }
        }
        next
    }

    /// Cells that disagree with `other` (Hamming distance on the bitset).
    pub fn divergence(&self, other: &Self) -> u32 {
        if self.width != other.width || self.height != other.height {
            return u32::MAX;
        }
        self.cells
            .iter()
            .zip(other.cells.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        h ^= u64::from(self.width).rotate_left(7);
        h = h.wrapping_mul(0x1000_0000_01b3);
        h ^= u64::from(self.height).rotate_left(13);
        h = h.wrapping_mul(0x1000_0000_01b3);
        for byte in &self.cells {
            h ^= u64::from(*byte);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::Board;

    #[test]
    fn block_is_still() {
        let mut board = Board::new(8, 8);
        board.set(2, 2, true);
        board.set(3, 2, true);
        board.set(2, 3, true);
        board.set(3, 3, true);
        assert_eq!(board.step(), board);
    }

    #[test]
    fn blinker_oscillates() {
        let mut h = Board::new(5, 5);
        h.set(1, 2, true);
        h.set(2, 2, true);
        h.set(3, 2, true);
        let v = h.step();
        assert!(v.live(2, 1) && v.live(2, 2) && v.live(2, 3));
        assert_eq!(v.live_count(), 3);
        assert_eq!(v.step(), h);
    }

    #[test]
    fn flip_changes_one_cell() {
        let a = Board::new(4, 4);
        let mut b = a.clone();
        b.flip(1, 1);
        assert_eq!(a.divergence(&b), 1);
        assert_ne!(a.fingerprint(), b.fingerprint());
    }
}
