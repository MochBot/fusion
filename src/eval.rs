// eval.rs -- Cold Clear-inspired heuristic evaluation
// board quality + move rewards for coaching engine

use crate::board::Board;
use crate::header::*;

/// weight set — higher score = better position
/// defaults tuned from Cold Clear's eval + some local adjustments
#[derive(Clone, Debug)]
pub struct EvalWeights {
    // board quality (static)
    pub hole_cells: i32,
    pub covered_cells: i32,
    pub covered_cells_sq: i32,

    // height danger
    pub height: i32,
    pub top_half: i32,
    pub top_quarter: i32,

    // surface quality
    pub bumpiness: i32,
    pub bumpiness_sq: i32,
    pub row_transitions: i32,

    // well / tetris potential
    pub well_depth: i32,
    pub max_well_depth: i32,

    // clear rewards
    pub clear1: i32,
    pub clear2: i32,
    pub clear3: i32,
    pub clear4: i32,
    pub tspin1: i32,
    pub tspin2: i32,
    pub tspin3: i32,
    pub perfect_clear: i32,
    pub b2b_clear: i32,
    pub combo_garbage: i32,
    pub wasted_t: i32,
}

impl Default for EvalWeights {
    fn default() -> Self {
        Self {
            hole_cells: -173,
            covered_cells: -17,
            covered_cells_sq: -1,

            height: -39,
            top_half: -150,
            top_quarter: -511,

            bumpiness: -24,
            bumpiness_sq: -7,
            row_transitions: -5,

            well_depth: 57,
            max_well_depth: 17,

            clear1: -143,
            clear2: -100,
            clear3: -58,
            clear4: 390,
            tspin1: 121,
            tspin2: 410,
            tspin3: 602,
            perfect_clear: 999,
            b2b_clear: 104,
            combo_garbage: 150,
            wasted_t: -152,
        }
    }
}

/// column heights (0-based, number of filled cells in column from bottom)
fn column_heights(board: &Board) -> [usize; COL_NB] {
    let mut heights = [0usize; COL_NB];
    for (x, h) in heights.iter_mut().enumerate() {
        // scan from top down to find highest occupied cell
        for y in (0..40usize).rev() {
            if board.occupied(x as i32, y as i32) {
                *h = y + 1;
                break;
            }
        }
    }
    heights
}

/// count holes and covered cells per column
/// hole = empty cell below column top
/// covered = filled cells above the topmost hole (capped at 6)
fn holes_and_covered(board: &Board, heights: &[usize; COL_NB]) -> (i32, i32) {
    let mut holes = 0i32;
    let mut covered = 0i32;

    for (x, &h) in heights.iter().enumerate() {
        if h == 0 {
            continue;
        }

        let mut topmost_hole: Option<usize> = None;
        for y in (0..h).rev() {
            if !board.occupied(x as i32, y as i32) {
                holes += 1;
                if topmost_hole.is_none() {
                    topmost_hole = Some(y);
                }
            }
        }

        // covered cells = filled cells above the topmost hole
        if let Some(hole_y) = topmost_hole {
            let mut cov = 0i32;
            for y in (hole_y + 1)..h {
                if board.occupied(x as i32, y as i32) {
                    cov += 1;
                }
            }
            // cap at 6 to avoid runaway penalty
            covered += cov.min(6);
        }
    }

    (holes, covered)
}

/// bumpiness — sum of |h[i]-h[i+1]| and (h[i]-h[i+1])^2
/// skips the well column (deepest col with both neighbors taller)
fn bumpiness(heights: &[usize; COL_NB], well_col: Option<usize>) -> (i32, i32) {
    let mut bump = 0i32;
    let mut bump_sq = 0i32;

    for i in 0..(COL_NB - 1) {
        // skip transitions involving the well column
        if let Some(wc) = well_col {
            if i == wc || i + 1 == wc {
                continue;
            }
        }
        let diff = (heights[i] as i32) - (heights[i + 1] as i32);
        bump += diff.abs();
        bump_sq += diff * diff;
    }

    (bump, bump_sq)
}

/// row transitions — count bit transitions in each occupied row
/// XOR adjacent cells, count 1-bits
fn row_transitions(board: &Board, max_height: usize) -> i32 {
    let mut total = 0i32;
    for y in 0..max_height {
        let row = board.row(y);
        if row == 0 {
            continue;
        }
        // transitions within the row: XOR row with shifted version
        // also count wall transitions (bit 0 and bit 9 borders)
        let shifted = row >> 1;
        let xor = row ^ shifted;
        // count internal transitions (bits 0..8 of xor)
        total += (xor & 0x1FF).count_ones() as i32;
        // left wall transition
        if row & 1 == 0 {
            total += 1;
        }
        // right wall transition
        if row & (1 << 9) == 0 {
            total += 1;
        }
    }
    total
}

/// find the deepest well column (both neighbors taller)
/// returns (well_col, well_depth)
fn find_well(heights: &[usize; COL_NB]) -> (Option<usize>, i32) {
    let mut best_col = None;
    let mut best_depth = 0i32;

    for x in 0..COL_NB {
        let h = heights[x] as i32;
        let left = if x == 0 { 40 } else { heights[x - 1] as i32 };
        let right = if x == COL_NB - 1 {
            40
        } else {
            heights[x + 1] as i32
        };

        if left > h && right > h {
            let depth = left.min(right) - h;
            if depth > best_depth {
                best_depth = depth;
                best_col = Some(x);
            }
        }
    }

    (best_col, best_depth)
}

/// evaluate board quality (static position score)
/// higher = better
pub fn evaluate(board: &Board, weights: &EvalWeights) -> i32 {
    let heights = column_heights(board);
    let max_h = heights.iter().copied().max().unwrap_or(0);

    let (holes, covered) = holes_and_covered(board, &heights);
    let (well_col, well_depth) = find_well(&heights);
    let (bump, bump_sq) = bumpiness(&heights, well_col);
    let transitions = row_transitions(board, max_h);

    let mut score = 0i32;

    // holes
    score += weights.hole_cells * holes;
    score += weights.covered_cells * covered;
    score += weights.covered_cells_sq * covered * covered;

    // height
    score += weights.height * (max_h as i32);
    if max_h > 10 {
        score += weights.top_half * ((max_h - 10) as i32);
    }
    if max_h > 15 {
        score += weights.top_quarter * ((max_h - 15) as i32);
    }

    // surface
    score += weights.bumpiness * bump;
    score += weights.bumpiness_sq * bump_sq;
    score += weights.row_transitions * transitions;

    // well potential (reward having a clean well for tetrises)
    score += weights.well_depth * well_depth;
    // bonus for deep wells (max_well_depth applies only to depth > 1)
    if well_depth > 1 {
        score += weights.max_well_depth * (well_depth - 1);
    }

    score
}

/// evaluate a move result — combines board eval with clear/spin rewards
/// called after applying the move to get the resulting board
pub fn evaluate_move(
    result_board: &Board,
    m: &Move,
    lines_cleared: u8,
    b2b: bool,
    combo: u32,
    weights: &EvalWeights,
) -> i32 {
    let mut score = evaluate(result_board, weights);

    // clear rewards
    let spin = m.spin();
    let is_spin = spin != SpinType::NoSpin;

    match (lines_cleared, is_spin) {
        (0, _) => {
            // no clear — penalize wasted T if it was a T piece with no spin
            if m.piece() == Piece::T && spin == SpinType::NoSpin {
                score += weights.wasted_t;
            }
        }
        (1, false) => score += weights.clear1,
        (2, false) => score += weights.clear2,
        (3, false) => score += weights.clear3,
        (4, false) => score += weights.clear4,
        (1, true) => score += weights.tspin1,
        (2, true) => score += weights.tspin2,
        (3, true) => score += weights.tspin3,
        _ => {
            // 4+ with spin shouldn't happen, but handle gracefully
            score += weights.clear4;
        }
    }

    // b2b bonus
    if b2b && lines_cleared > 0 {
        score += weights.b2b_clear;
    }

    // combo reward (scales with combo count)
    if combo > 0 && lines_cleared > 0 {
        score += weights.combo_garbage * (combo as i32);
    }

    // perfect clear
    if lines_cleared > 0 && result_board.empty() {
        score += weights.perfect_clear;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, FULL_ROW};

    #[test]
    fn test_empty_board_eval() {
        let board = Board::new();
        let weights = EvalWeights::default();
        let score = evaluate(&board, &weights);
        // empty board: no holes, no height, but well_depth should be 0
        // (no column has both neighbors taller since all are 0)
        assert_eq!(score, 0, "empty board should eval to 0");
    }

    #[test]
    fn test_holes_reduce_score() {
        let weights = EvalWeights::default();

        // clean flat board — fill 3 rows fully
        let mut clean = Board::new();
        for y in 0..3 {
            clean.rows[y] = FULL_ROW;
        }
        clean.cols = [0; COL_NB];
        // rebuild cols from rows
        for y in 0..3 {
            for x in 0..COL_NB {
                clean.cols[x] |= 1u64 << y;
            }
        }

        // board with a hole — fill 3 rows but leave a gap at (5, 0)
        let mut holey = Board::new();
        holey.rows[0] = FULL_ROW & !(1 << 5); // hole at col 5
        holey.rows[1] = FULL_ROW;
        holey.rows[2] = FULL_ROW;
        holey.cols = [0; COL_NB];
        for y in 0..3 {
            let row = holey.rows[y];
            for x in 0..COL_NB {
                if row & (1 << x) != 0 {
                    holey.cols[x] |= 1u64 << y;
                }
            }
        }

        let clean_score = evaluate(&clean, &weights);
        let holey_score = evaluate(&holey, &weights);
        assert!(
            holey_score < clean_score,
            "holey board ({}) should score lower than clean ({})",
            holey_score,
            clean_score
        );
    }

    #[test]
    fn test_tetris_clear_scores_high() {
        let weights = EvalWeights::default();
        let board = Board::new(); // result board is empty after clear

        // simulate tetris (4 lines cleared, no spin)
        let m = Move::new(Piece::I, Rotation::North, 5, 0, false);
        let score_tetris = evaluate_move(&board, &m, 4, false, 0, &weights);

        // simulate single clear
        let score_single = evaluate_move(&board, &m, 1, false, 0, &weights);

        assert!(
            score_tetris > score_single,
            "tetris ({}) should score higher than single ({})",
            score_tetris,
            score_single
        );
    }

    #[test]
    fn test_tspin_double_scores_very_high() {
        let weights = EvalWeights::default();
        let board = Board::new();

        // t-spin double — 2 lines with spin
        let m = Move::new_tspin(Rotation::South, 4, 0, true);
        let score_tsd = evaluate_move(&board, &m, 2, false, 0, &weights);

        // regular double — 2 lines no spin
        let m_reg = Move::new(Piece::L, Rotation::North, 4, 0, false);
        let score_double = evaluate_move(&board, &m_reg, 2, false, 0, &weights);

        assert!(
            score_tsd > score_double,
            "t-spin double ({}) should beat regular double ({})",
            score_tsd,
            score_double
        );
    }

    #[test]
    fn test_wasted_t_penalty() {
        let weights = EvalWeights::default();
        let board = Board::new();

        // T placed with no clear and no spin
        let m_t = Move::new(Piece::T, Rotation::North, 4, 0, false);
        let score_t = evaluate_move(&board, &m_t, 0, false, 0, &weights);

        // L placed with no clear — no wasted_t penalty
        let m_l = Move::new(Piece::L, Rotation::North, 4, 0, false);
        let score_l = evaluate_move(&board, &m_l, 0, false, 0, &weights);

        assert!(
            score_t < score_l,
            "wasted T ({}) should score lower than L placement ({})",
            score_t,
            score_l
        );
    }

    #[test]
    fn test_b2b_and_combo_bonuses() {
        let weights = EvalWeights::default();
        let board = Board::new();
        let m = Move::new(Piece::I, Rotation::North, 5, 0, false);

        let score_plain = evaluate_move(&board, &m, 4, false, 0, &weights);
        let score_b2b = evaluate_move(&board, &m, 4, true, 0, &weights);
        let score_combo = evaluate_move(&board, &m, 4, false, 3, &weights);

        assert!(score_b2b > score_plain, "b2b should add bonus");
        assert!(score_combo > score_plain, "combo should add bonus");
    }

    #[test]
    fn test_column_heights_basic() {
        let mut board = Board::new();
        board.rows[0] = 1 << 3; // col 3, row 0
        board.rows[4] = 1 << 3; // col 3, row 4
        board.cols[3] = (1u64 << 0) | (1u64 << 4);

        let heights = column_heights(&board);
        assert_eq!(heights[3], 5); // highest occupied is row 4 → height 5
        assert_eq!(heights[0], 0);
    }

    #[test]
    fn test_well_detection() {
        // col 9 (rightmost) is the well — all others height 4, col 9 height 0
        let mut heights = [4usize; COL_NB];
        heights[9] = 0;
        let (well_col, well_depth) = find_well(&heights);
        assert_eq!(well_col, Some(9));
        assert_eq!(well_depth, 4);
    }
}
