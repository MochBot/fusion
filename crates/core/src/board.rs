//! board representation - column-major u64s for fast bitwise ops
//! zobrist hash maintained incrementally on every set/clear

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Precomputed random values for Zobrist hashing.
/// ZOBRIST_TABLE[col][row] — 10 columns × 44 rows (only 40 used, 44 for alignment).
/// Generated from a fixed seed so hashes are deterministic across runs.
const ZOBRIST_TABLE: [[u64; 44]; 10] = {
    // xorshift64 PRNG with fixed seed — deterministic, good distribution
    let mut table = [[0u64; 44]; 10];
    let mut state: u64 = 0xdeadbeefcafe1234;
    let mut col = 0;
    while col < 10 {
        let mut row = 0;
        while row < 44 {
            // xorshift64
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            table[col][row] = state;
            row += 1;
        }
        col += 1;
    }
    table
};

/// Compute full Zobrist hash from column data — used for init/deserialization
#[inline]
fn compute_zobrist_hash(cols: &[u64; 10]) -> u64 {
    let mut hash = 0u64;
    let mut col = 0;
    while col < 10 {
        let mut bits = cols[col];
        while bits != 0 {
            let row = bits.trailing_zeros() as usize;
            hash ^= ZOBRIST_TABLE[col][row];
            bits &= bits - 1; // clear lowest set bit
        }
        col += 1;
    }
    hash
}

/// 10x40 Tetris board using column-major u64 bitfields.
/// Each column uses 40 bits (rows 0-39).
/// Row 0 is bottom, Row 39 is top (only 0-19 visible).
/// Zobrist hash maintained incrementally — O(1) per cell change.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Board {
    cols: [u64; 10],
    hash: u64,
}

impl Board {
    /// Incremental Zobrist hash — O(1), no recomputation
    #[inline(always)]
    pub fn zobrist_hash(&self) -> u64 {
        self.hash
    }

    #[inline(always)]
    pub fn set_raw(&mut self, x: usize, y: usize) {
        let mask = 1u64 << y;
        if (self.cols[x] & mask) == 0 {
            self.cols[x] |= mask;
            self.hash ^= ZOBRIST_TABLE[x][y];
        }
    }

    #[inline(always)]
    pub fn clear_raw(&mut self, x: usize, y: usize) {
        let mask = 1u64 << y;
        if (self.cols[x] & mask) != 0 {
            self.cols[x] &= !mask;
            self.hash ^= ZOBRIST_TABLE[x][y];
        }
    }
}

impl Serialize for Board {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut rows = [0u16; Board::HEIGHT];
        for (y, row) in rows.iter_mut().enumerate() {
            *row = self.row(y);
        }
        rows.as_slice().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Board {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u16> = Vec::deserialize(deserializer)?;
        if vec.len() != Board::HEIGHT {
            return Err(serde::de::Error::custom("expected 40 rows"));
        }
        let mut cols = [0u64; Board::WIDTH];
        for (y, &value) in vec.iter().enumerate() {
            let row = (value & 0x3FF) as u64;
            for (x, col) in cols.iter_mut().enumerate() {
                if ((row >> x) & 1) == 1 {
                    *col |= 1u64 << y;
                }
            }
        }
        let hash = compute_zobrist_hash(&cols);
        Ok(Board { cols, hash })
    }
}

impl Default for Board {
    fn default() -> Self {
        Self {
            cols: [0; Board::WIDTH],
            hash: 0,
        }
    }
}

impl Board {
    pub const WIDTH: usize = 10;
    pub const HEIGHT: usize = 40;
    pub const VISIBLE_HEIGHT: usize = 20;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        (self.cols[x] >> y) & 1 == 1
    }

    pub fn set(&mut self, x: usize, y: usize, filled: bool) {
        let was_set = (self.cols[x] >> y) & 1 == 1;
        if filled != was_set {
            self.cols[x] ^= 1u64 << y;
            self.hash ^= ZOBRIST_TABLE[x][y];
        }
    }

    pub fn is_row_full(&self, y: usize) -> bool {
        (0..Self::WIDTH).all(|x| (self.cols[x] >> y) & 1 == 1)
    }

    pub fn is_row_empty(&self, y: usize) -> bool {
        (0..Self::WIDTH).all(|x| (self.cols[x] >> y) & 1 == 0)
    }

    pub fn clear_lines(&mut self) -> u8 {
        let mut cleared = 0u8;
        let mut y = 0;
        while y < Self::HEIGHT {
            if self.is_row_full(y) {
                let lower_mask = (1u64 << y) - 1;
                for x in 0..Self::WIDTH {
                    let col = self.cols[x];
                    let lower = col & lower_mask;
                    let upper = col >> (y + 1);
                    self.set_column(x, lower | (upper << y));
                }
                cleared += 1;
            } else {
                y += 1;
            }
        }
        cleared
    }

    /// Get raw row data for collision detection
    pub fn row(&self, y: usize) -> u16 {
        (0..Self::WIDTH).fold(0u16, |acc, x| acc | (((self.cols[x] >> y) & 1) as u16) << x)
    }

    /// Get raw column data for fast height calculation
    #[inline]
    pub fn column(&self, x: usize) -> u64 {
        self.cols[x]
    }

    /// Set raw column data - for fast line clear/restore ops
    #[inline]
    pub fn set_column(&mut self, x: usize, value: u64) {
        let old = self.cols[x];
        let diff = old ^ value;
        // XOR out changed bits
        let mut bits = diff;
        while bits != 0 {
            let row = bits.trailing_zeros() as usize;
            self.hash ^= ZOBRIST_TABLE[x][row];
            bits &= bits - 1;
        }
        self.cols[x] = value;
    }

    /// Get all columns as slice - for fast board operations
    #[inline]
    pub fn columns(&self) -> &[u64; 10] {
        &self.cols
    }
}

impl std::fmt::Display for Board {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for y in (0..Self::VISIBLE_HEIGHT).rev() {
            for x in 0..Self::WIDTH {
                write!(f, "{}", if self.get(x, y) { "[]" } else { "  " })?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_get() {
        let mut b = Board::new();
        b.set(5, 10, true);
        assert!(b.get(5, 10));
        assert!(!b.get(4, 10));
    }

    #[test]
    fn test_clear_single_line() {
        let mut b = Board::new();
        for x in 0..Board::WIDTH {
            b.set(x, 0, true);
        }
        b.set(5, 1, true);
        assert_eq!(b.clear_lines(), 1);
        assert!(b.get(5, 0)); // row 1 shifted down to row 0
    }

    #[test]
    fn test_clear_multiple_lines() {
        let mut b = Board::new();
        for x in 0..Board::WIDTH {
            b.set(x, 0, true);
            b.set(x, 1, true);
        }
        b.set(3, 2, true);
        assert_eq!(b.clear_lines(), 2);
        assert!(b.get(3, 0));
    }

    #[test]
    fn test_row_full() {
        let mut b = Board::new();
        for x in 0..Board::WIDTH {
            b.set(x, 5, true);
        }
        assert!(b.is_row_full(5));
        assert!(!b.is_row_full(4));
    }
}
