//! Row-major board representation for Source-Subtraction Algorithm movegen.
//! Each row is a u64 with bits 0-9 representing columns (bit 0 = x=0).

use fusion_core::Board;

/// Row-major board layout - one u64 per row, 44 rows for spawn safety.
/// Bits 0-9 used per row (10 columns), bits 10-63 unused padding.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RowBoard {
    rows: [u64; 44],
}

impl RowBoard {
    /// Width mask - 10 bits set (0x3FF)
    pub const WIDTH_MASK: u64 = 0x3FF;

    /// Create empty row board
    #[inline]
    pub fn new() -> Self {
        Self { rows: [0u64; 44] }
    }

    /// Get raw row data - bits 0-9 represent columns
    #[inline(always)]
    pub fn get_row(&self, y: usize) -> u64 {
        if y < 44 {
            self.rows[y]
        } else {
            0
        }
    }

    /// Set a single bit at (x, y)
    #[inline(always)]
    pub fn set_bit(&mut self, x: usize, y: usize) {
        if x < 10 && y < 44 {
            self.rows[y] |= 1u64 << x;
        }
    }

    /// Clear a single bit at (x, y)
    #[inline(always)]
    pub fn clear_bit(&mut self, x: usize, y: usize) {
        if x < 10 && y < 44 {
            self.rows[y] &= !(1u64 << x);
        }
    }

    /// Get bit at (x, y)
    #[inline(always)]
    pub fn get_bit(&self, x: usize, y: usize) -> bool {
        if x < 10 && y < 44 {
            (self.rows[y] >> x) & 1 == 1
        } else {
            false
        }
    }

    /// Set entire row value (masked to 10 bits)
    #[inline(always)]
    pub fn set_row(&mut self, y: usize, value: u64) {
        if y < 44 {
            self.rows[y] = value & Self::WIDTH_MASK;
        }
    }

    /// Get raw rows slice for bulk operations
    #[inline]
    pub fn rows(&self) -> &[u64; 44] {
        &self.rows
    }

    /// Get mutable rows slice
    #[inline]
    pub fn rows_mut(&mut self) -> &mut [u64; 44] {
        &mut self.rows
    }
}

impl Default for RowBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl From<&Board> for RowBoard {
    fn from(board: &Board) -> Self {
        let mut rows = [0u64; 44];

        // Board uses row() method that already extracts row as u16
        // Only need to copy 40 rows (Board::HEIGHT), rows 40-43 stay empty
        for (y, row) in rows.iter_mut().enumerate().take(Board::HEIGHT) {
            *row = board.row(y) as u64;
        }

        RowBoard { rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_board_empty() {
        let board = Board::new();
        let rb = RowBoard::from(&board);

        // All rows should be empty
        for y in 0..44 {
            assert_eq!(rb.get_row(y), 0, "row {} should be empty", y);
        }
    }

    #[test]
    fn test_row_board_full_row() {
        let mut board = Board::new();
        for x in 0..10 {
            board.set(x, 0, true);
        }

        let rb = RowBoard::from(&board);

        // Row 0 should have all 10 bits set
        assert_eq!(rb.get_row(0), 0x3FF, "row 0 should be full");
        assert_eq!(rb.get_row(1), 0, "row 1 should be empty");
    }

    #[test]
    fn test_row_board_scattered() {
        let mut board = Board::new();
        board.set(0, 0, true);
        board.set(5, 10, true);
        board.set(9, 39, true);

        let rb = RowBoard::from(&board);

        // Verify get_bit matches Board::get
        for y in 0..40 {
            for x in 0..10 {
                assert_eq!(
                    rb.get_bit(x, y),
                    board.get(x, y),
                    "mismatch at ({}, {})",
                    x,
                    y
                );
            }
        }

        // Specific checks
        assert!(rb.get_bit(0, 0));
        assert!(rb.get_bit(5, 10));
        assert!(rb.get_bit(9, 39));
        assert!(!rb.get_bit(1, 0));
    }

    #[test]
    fn test_row_board_height_44() {
        let rb = RowBoard::new();

        // Rows 40-43 should be accessible (for spawn above visible)
        assert_eq!(rb.get_row(40), 0);
        assert_eq!(rb.get_row(41), 0);
        assert_eq!(rb.get_row(42), 0);
        assert_eq!(rb.get_row(43), 0);

        // Out of bounds returns 0
        assert_eq!(rb.get_row(44), 0);
        assert_eq!(rb.get_row(100), 0);
    }

    #[test]
    fn test_row_board_set_get_bit() {
        let mut rb = RowBoard::new();

        rb.set_bit(3, 5);
        assert!(rb.get_bit(3, 5));
        assert!(!rb.get_bit(2, 5));
        assert!(!rb.get_bit(3, 4));

        rb.clear_bit(3, 5);
        assert!(!rb.get_bit(3, 5));
    }

    #[test]
    fn test_row_board_set_row() {
        let mut rb = RowBoard::new();

        // Set row with value, should be masked to 10 bits
        rb.set_row(5, 0xFFFF);
        assert_eq!(rb.get_row(5), 0x3FF);

        rb.set_row(10, 0b1010101010);
        assert_eq!(rb.get_row(10), 0b1010101010);
    }

    #[test]
    fn test_row_board_roundtrip() {
        // Build a board with various patterns
        let mut board = Board::new();

        // Checkerboard pattern on first few rows
        for y in 0..5 {
            for x in 0..10 {
                if (x + y) % 2 == 0 {
                    board.set(x, y, true);
                }
            }
        }

        let rb = RowBoard::from(&board);

        // Verify every cell matches
        for y in 0..40 {
            for x in 0..10 {
                assert_eq!(
                    rb.get_bit(x, y),
                    board.get(x, y),
                    "roundtrip mismatch at ({}, {})",
                    x,
                    y
                );
            }
        }
    }
}
