//! Fusion eval crate - heuristics for board evaluation.

use fusion_core::Board;

#[derive(Clone, Debug)]
pub struct EvalWeights {
    pub height: f32,
    pub holes: f32,
    pub bumpiness: f32,
    pub wells: f32,
    pub lines_cleared: f32,
    pub i_dependency: f32,
}

impl Default for EvalWeights {
    fn default() -> Self {
        Self {
            height: -0.5,
            holes: -2.0,
            bumpiness: -0.1,
            wells: -0.1,
            lines_cleared: 1.0,
            i_dependency: -0.1,
        }
    }
}

pub fn evaluate(board: &Board, weights: &EvalWeights) -> f32 {
    evaluate_with_clear(board, 0, weights)
}

pub fn evaluate_with_clear(board: &Board, lines: u8, weights: &EvalWeights) -> f32 {
    let mut score = lines as f32 * weights.lines_cleared;

    let mut heights = [0usize; Board::WIDTH];
    for x in 0..Board::WIDTH {
        for y in (0..Board::HEIGHT).rev() {
            if board.get(x, y) {
                heights[x] = y + 1;
                break;
            }
        }
    }

    // Height penalty
    let max_height = heights.iter().max().cloned().unwrap_or(0);
    score += max_height as f32 * weights.height;

    // Holes penalty
    let mut holes = 0usize;
    for x in 0..Board::WIDTH {
        for y in 0..heights[x] {
            if !board.get(x, y) {
                holes += 1;
            }
        }
    }
    score += holes as f32 * weights.holes;

    // Bumpiness penalty
    let mut bumpiness = 0usize;
    for x in 0..Board::WIDTH - 1 {
        bumpiness += (heights[x] as i32 - heights[x + 1] as i32).abs() as usize;
    }
    score += bumpiness as f32 * weights.bumpiness;

    // Wells penalty
    let mut wells = 0usize;
    let mut max_well = 0usize;
    for x in 0..Board::WIDTH {
        let left = if x == 0 {
            Board::HEIGHT
        } else {
            heights[x - 1]
        };
        let right = if x == Board::WIDTH - 1 {
            Board::HEIGHT
        } else {
            heights[x + 1]
        };
        let min_neighbor = left.min(right);
        if min_neighbor > heights[x] {
            let depth = min_neighbor - heights[x];
            wells += depth;
            max_well = max_well.max(depth);
        }
    }
    score += wells as f32 * weights.wells;

    // I-dependency penalty (deepest single-column well)
    score += max_well as f32 * weights.i_dependency;

    score
}

/// Count total holes in the board (empty cells below filled cells)
pub fn count_holes(board: &Board) -> u32 {
    let mut holes = 0u32;
    for x in 0..Board::WIDTH {
        let mut found_block = false;
        for y in (0..Board::HEIGHT).rev() {
            if board.get(x, y) {
                found_block = true;
            } else if found_block {
                holes += 1;
            }
        }
    }
    holes
}
