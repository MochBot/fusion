//! Board with precomputed column heights for fast evaluation

use crate::Board;

/// Board wrapper with cached column heights
/// Use for eval/heuristics - NOT for movegen (misses kick positions)
#[derive(Clone, Debug)]
pub struct BoardWithHeights {
    board: Board,
    heights: [u8; 10],
}

impl BoardWithHeights {
    /// Create from board, computing heights
    pub fn new(board: &Board) -> Self {
        let mut heights = [0u8; 10];
        for (x, height) in heights.iter_mut().enumerate() {
            *height = Self::compute_height(board, x);
        }
        Self {
            board: board.clone(),
            heights,
        }
    }

    fn compute_height(board: &Board, x: usize) -> u8 {
        for y in (0..40).rev() {
            if board.get(x, y) {
                return (y + 1) as u8;
            }
        }
        0
    }

    /// O(1) height lookup
    #[inline(always)]
    pub fn height(&self, x: usize) -> u8 {
        self.heights[x]
    }

    /// O(1) max height across all columns
    #[inline(always)]
    pub fn max_height(&self) -> u8 {
        *self.heights.iter().max().unwrap_or(&0)
    }

    /// O(1) bumpiness (sum of height differences)
    pub fn bumpiness(&self) -> u32 {
        let mut sum = 0u32;
        for x in 0..9 {
            let diff = (self.heights[x] as i32 - self.heights[x + 1] as i32).abs();
            sum += diff as u32;
        }
        sum
    }

    /// Get underlying board reference
    #[inline(always)]
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Get heights array reference
    #[inline(always)]
    pub fn heights(&self) -> &[u8; 10] {
        &self.heights
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Board;

    #[test]
    fn test_empty_board_heights() {
        let board = Board::new();
        let bh = BoardWithHeights::new(&board);
        for x in 0..10 {
            assert_eq!(bh.height(x), 0);
        }
        assert_eq!(bh.max_height(), 0);
        assert_eq!(bh.bumpiness(), 0);
    }

    #[test]
    fn test_single_cell_height() {
        let mut board = Board::new();
        board.set(5, 3, true); // Cell at (5, 3)
        let bh = BoardWithHeights::new(&board);
        assert_eq!(bh.height(5), 4); // Height is y + 1
        assert_eq!(bh.max_height(), 4);
    }

    #[test]
    fn test_bumpiness() {
        let mut board = Board::new();
        // Column heights: [1, 3, 1, 3, 1, 3, 1, 3, 1, 3]
        for x in (1..10).step_by(2) {
            board.set(x, 0, true);
            board.set(x, 1, true);
            board.set(x, 2, true);
        }
        for x in (0..10).step_by(2) {
            board.set(x, 0, true);
        }
        let bh = BoardWithHeights::new(&board);
        // Differences: |1-3| + |3-1| + |1-3| + ... = 2 * 9 = 18
        assert_eq!(bh.bumpiness(), 18);
    }
}
