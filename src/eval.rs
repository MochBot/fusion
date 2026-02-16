// eval.rs -- heuristic evaluation using real TETR.IO S2 attack values
// board quality + garbage-based move scoring for coaching engine

use crate::attack::AttackConfig;
use crate::board::Board;
use crate::header::*;

#[derive(Clone, Debug)]
pub struct EvalWeights {
    // board quality (static)
    pub holes: f32,
    pub cell_coveredness: f32,

    // height danger
    pub height: f32,
    pub height_upper_half: f32,
    pub height_upper_quarter: f32,

    // surface quality
    pub bumpiness: f32,
    pub bumpiness_sq: f32,
    pub row_transitions: f32,

    // well / tetris potential
    pub tetris_well_depth: f32,

    pub tslot: [f32; 4],
    pub has_back_to_back: f32,

    pub normal_clears: [f32; 5],
    pub spin_clears: [f32; 4],
    pub mini_spin_clears: [f32; 3],
    pub back_to_back_clear: f32,
    pub combo_attack: f32,
    pub perfect_clear: f32,

    // T-piece waste
    pub wasted_t: f32,
}

impl Default for EvalWeights {
    fn default() -> Self {
        Self {
            holes: -1.5,
            cell_coveredness: -0.2,

            height: -0.4,
            height_upper_half: -1.5,
            height_upper_quarter: -5.0,

            bumpiness: -0.2,
            bumpiness_sq: -0.05,
            row_transitions: -0.2,

            tetris_well_depth: 0.3,

            tslot: [0.1, 1.5, 2.0, 4.0],
            has_back_to_back: 0.5,

            normal_clears: [0.0, -2.0, -1.5, -1.0, 3.5],
            spin_clears: [0.0, 1.0, 4.0, 6.0],
            mini_spin_clears: [0.0, -1.5, -1.0],
            back_to_back_clear: 1.0,
            combo_attack: 1.5,
            perfect_clear: 15.0,

            wasted_t: -1.5,
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

fn tslot_tier(heights: &[usize; COL_NB]) -> usize {
    let mut slots = 0usize;

    for x in 1..(COL_NB - 1) {
        let left = heights[x - 1] as i32;
        let mid = heights[x] as i32;
        let right = heights[x + 1] as i32;
        let notch_depth = left.min(right) - mid;
        if notch_depth >= 2 {
            slots += 1;
        }
    }

    slots.min(3)
}

/// evaluate board quality (static position score)
/// higher = better
pub fn evaluate(board: &Board, weights: &EvalWeights) -> f32 {
    let heights = column_heights(board);
    let max_h = heights.iter().copied().max().unwrap_or(0);

    let (holes, covered) = holes_and_covered(board, &heights);
    let (well_col, well_depth) = find_well(&heights);
    let (bump, bump_sq) = bumpiness(&heights, well_col);
    let transitions = row_transitions(board, max_h);
    let tslot_tier = tslot_tier(&heights);

    let mut score = 0.0f32;

    // holes
    score += weights.holes * holes as f32;
    score += weights.cell_coveredness * covered as f32;

    // height
    score += weights.height * max_h as f32;
    if max_h > 10 {
        score += weights.height_upper_half * (max_h - 10) as f32;
    }
    if max_h > 15 {
        score += weights.height_upper_quarter * (max_h - 15) as f32;
    }

    // surface
    score += weights.bumpiness * bump as f32;
    score += weights.bumpiness_sq * bump_sq as f32;
    score += weights.row_transitions * transitions as f32;

    // well potential (reward having a clean well for tetrises)
    score += weights.tetris_well_depth * well_depth as f32;

    score += weights.tslot[tslot_tier];

    score
}

fn clear_reward(lines_cleared: u8, spin: SpinType, weights: &EvalWeights) -> f32 {
    match spin {
        SpinType::NoSpin => {
            let idx = usize::from(lines_cleared).min(weights.normal_clears.len() - 1);
            weights.normal_clears[idx]
        }
        SpinType::Mini => {
            let idx = usize::from(lines_cleared).min(weights.mini_spin_clears.len() - 1);
            weights.mini_spin_clears[idx]
        }
        SpinType::Full => {
            let idx = usize::from(lines_cleared).min(weights.spin_clears.len() - 1);
            weights.spin_clears[idx]
        }
    }
}

/// evaluate a move — board quality + real garbage sent via attack calc
#[allow(clippy::too_many_arguments)]
pub fn evaluate_move(
    result_board: &Board,
    m: &Move,
    lines_cleared: u8,
    spin: SpinType,
    b2b_before: u8,
    combo: u32,
    is_pc: bool,
    _config: &AttackConfig,
    weights: &EvalWeights,
) -> f32 {
    let mut score = evaluate(result_board, weights);

    if b2b_before > 0 {
        score += weights.has_back_to_back;
    }

    if lines_cleared == 0 {
        // no clear — penalize wasted T
        if m.piece() == Piece::T && spin == SpinType::NoSpin {
            score += weights.wasted_t;
        }
        return score;
    }

    score += clear_reward(lines_cleared, spin, weights);

    let is_b2b_eligible = spin != SpinType::NoSpin || lines_cleared >= 4;
    if is_b2b_eligible && b2b_before > 0 {
        score += weights.back_to_back_clear;
    }

    if combo > 0 {
        score += combo as f32 * weights.combo_attack;
    }

    if is_pc {
        score += weights.perfect_clear;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attack::AttackConfig;
    use crate::board::{Board, FULL_ROW};

    #[test]
    fn test_empty_board_eval() {
        let board = Board::new();
        let weights = EvalWeights::default();
        let score = evaluate(&board, &weights);
        assert!(
            (score - weights.tslot[0]).abs() < 0.001,
            "empty board score {} should equal no-slot baseline {}",
            score,
            weights.tslot[0]
        );
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
        let config = AttackConfig::tetra_league();
        let board = Board::new();

        let m = Move::new(Piece::I, Rotation::North, 5, 0, false);
        let score_tetris = evaluate_move(
            &board,
            &m,
            4,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );
        let score_single = evaluate_move(
            &board,
            &m,
            1,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );

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
        let config = AttackConfig::tetra_league();
        let board = Board::new();

        let m = Move::new_tspin(Rotation::South, 4, 0, true);
        let score_tsd = evaluate_move(
            &board,
            &m,
            2,
            SpinType::Full,
            0,
            0,
            false,
            &config,
            &weights,
        );

        let m_reg = Move::new(Piece::L, Rotation::North, 4, 0, false);
        let score_double = evaluate_move(
            &board,
            &m_reg,
            2,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );

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
        let config = AttackConfig::tetra_league();
        let board = Board::new();

        let m_t = Move::new(Piece::T, Rotation::North, 4, 0, false);
        let score_t = evaluate_move(
            &board,
            &m_t,
            0,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );

        let m_l = Move::new(Piece::L, Rotation::North, 4, 0, false);
        let score_l = evaluate_move(
            &board,
            &m_l,
            0,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );

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
        let config = AttackConfig::tetra_league();
        let board = Board::new();
        let m = Move::new(Piece::I, Rotation::North, 5, 0, false);

        let score_plain = evaluate_move(
            &board,
            &m,
            4,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );
        let score_b2b = evaluate_move(
            &board,
            &m,
            4,
            SpinType::NoSpin,
            1,
            0,
            false,
            &config,
            &weights,
        );
        let score_combo = evaluate_move(
            &board,
            &m,
            4,
            SpinType::NoSpin,
            0,
            3,
            false,
            &config,
            &weights,
        );

        assert!(score_b2b > score_plain, "b2b should add bonus");
        assert!(score_combo > score_plain, "combo should add bonus");
    }

    #[test]
    fn test_surge_break_penalty() {
        let weights = EvalWeights::default();
        let config = AttackConfig::tetra_league();
        let board = Board::new();

        // quad clear — B2B eligible, gets surge_value
        let m_quad = Move::new(Piece::I, Rotation::North, 5, 0, false);
        let score_quad = evaluate_move(
            &board,
            &m_quad,
            4,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );

        // double clear — not B2B eligible, gets surge_break_penalty
        let score_double = evaluate_move(
            &board,
            &m_quad,
            2,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );

        // quad should beat double (more garbage + surge_value vs surge_break_penalty)
        assert!(
            score_quad > score_double,
            "quad ({}) should beat double ({}) due to surge",
            score_quad,
            score_double
        );
    }

    #[test]
    fn test_allspin_eval_bonus() {
        let weights = EvalWeights::default();
        let config = AttackConfig::tetra_league();
        let board = Board::new();

        // S-spin double vs regular S double
        let m_s = Move::new(Piece::S, Rotation::North, 4, 0, false);
        let score_spin = evaluate_move(
            &board,
            &m_s,
            2,
            SpinType::Full,
            0,
            0,
            false,
            &config,
            &weights,
        );
        let score_nospin = evaluate_move(
            &board,
            &m_s,
            2,
            SpinType::NoSpin,
            0,
            0,
            false,
            &config,
            &weights,
        );

        assert!(
            score_spin > score_nospin,
            "S-spin double ({}) should beat regular S double ({})",
            score_spin,
            score_nospin
        );
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
