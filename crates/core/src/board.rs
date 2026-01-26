//! Board representation using u64 bitfields for fast operations.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 10x40 Tetris board using u64 bitfields.
/// Each row uses 10 bits (columns 0-9).
/// Row 0 is bottom, Row 39 is top (only 0-19 visible).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Board {
    rows: [u64; 40],
}

impl Serialize for Board {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.rows.as_slice().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Board {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u64> = Vec::deserialize(deserializer)?;
        if vec.len() != 40 {
            return Err(serde::de::Error::custom("expected 40 rows"));
        }
        let mut rows = [0u64; 40];
        for (i, &value) in vec.iter().enumerate() {
            rows[i] = value & 0x3FF;
        }
        Ok(Board { rows })
    }
}

impl Default for Board {
    fn default() -> Self {
        Self { rows: [0; 40] }
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
        (self.rows[y] >> x) & 1 == 1
    }

    pub fn set(&mut self, x: usize, y: usize, filled: bool) {
        if filled {
            self.rows[y] |= 1 << x;
        } else {
            self.rows[y] &= !(1 << x);
        }
    }

    pub fn is_row_full(&self, y: usize) -> bool {
        self.rows[y] & 0x3FF == 0x3FF
    }

    pub fn is_row_empty(&self, y: usize) -> bool {
        self.rows[y] == 0
    }

    pub fn clear_lines(&mut self) -> u8 {
        let mut cleared = 0u8;
        let mut write_y = 0;
        for read_y in 0..40 {
            if !self.is_row_full(read_y) {
                self.rows[write_y] = self.rows[read_y];
                write_y += 1;
            } else {
                cleared += 1;
            }
        }
        for y in write_y..40 {
            self.rows[y] = 0;
        }
        cleared
    }

    /// Get raw row data for collision detection
    pub fn row(&self, y: usize) -> u64 {
        self.rows[y]
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
        for x in 0..10 {
            b.set(x, 0, true);
        }
        b.set(5, 1, true);
        assert_eq!(b.clear_lines(), 1);
        assert!(b.get(5, 0)); // row 1 shifted down to row 0
    }

    #[test]
    fn test_clear_multiple_lines() {
        let mut b = Board::new();
        for x in 0..10 {
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
        for x in 0..10 {
            b.set(x, 5, true);
        }
        assert!(b.is_row_full(5));
        assert!(!b.is_row_full(4));
    }
}
