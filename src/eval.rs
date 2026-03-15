// eval.rs -- board-quality-only evaluation
// presim (beam search) handles tactics; eval scores board shape only

use crate::board::Board;
use crate::header::*;

#[derive(Clone, Debug)]
pub struct EvalWeights {
    // -- existing board-shape features --
    pub holes: f32,
    pub cell_coveredness: f32,
    pub height: f32,
    pub height_upper_half: f32,
    pub height_upper_quarter: f32,
    pub bumpiness: f32,
    pub bumpiness_sq: f32,
    pub row_transitions: f32,
    pub well_depth: f32,
    // -- structural pattern bonuses --
    pub tsd_overhang: f32,
    pub four_wide_well: f32,
}

impl Default for EvalWeights {
    fn default() -> Self {
        Self {
            holes: -4.0,
            cell_coveredness: -0.5,
            height: -0.2,
            height_upper_half: -1.0,
            height_upper_quarter: -5.0,
            bumpiness: -0.3,
            bumpiness_sq: -0.1,
            row_transitions: -0.3,
            well_depth: 0.2,
            tsd_overhang: 6.0,
            four_wide_well: 1.5,
        }
    }
}

#[inline]
fn column_heights(board: &Board) -> [usize; COL_NB] {
    let mut heights = [0usize; COL_NB];
    for (x, h) in heights.iter_mut().enumerate() {
        // Use leading_zeros on cached column bitboard: O(1) per column vs O(40) scan
        let col = board.cols[x];
        *h = if col == 0 {
            0
        } else {
            (64 - col.leading_zeros()) as usize
        };
    }
    heights
}

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
fn count_tsd_overhangs(board: &Board, heights: &[usize; COL_NB]) -> i32 {
    let mut count = 0i32;

    for c in 0..COL_NB {
        let h = heights[c];
        if h < 2 {
            continue;
        }

        // Overhang: filled at top, empty directly below
        let has_overhang =
            board.occupied(c as i32, h as i32 - 1) && !board.occupied(c as i32, h as i32 - 2);

        if !has_overhang {
            continue;
        }

        // Check for wall on either side providing the T-slot
        let wall_left = c > 0
            && heights[c - 1] >= h
            && board.occupied(c as i32 - 1, h as i32 - 1)
            && board.occupied(c as i32 - 1, h as i32 - 2);

        let wall_right = c < COL_NB - 1
            && heights[c + 1] >= h
            && board.occupied(c as i32 + 1, h as i32 - 1)
            && board.occupied(c as i32 + 1, h as i32 - 2);

        // Need cavity on the opposite side of the wall
        if wall_left {
            let open_right = c < COL_NB - 1 && !board.occupied(c as i32 + 1, h as i32 - 2);
            let open_right = open_right || c == COL_NB - 1;
            if open_right {
                count += 1;
            }
        }
        if wall_right {
            let open_left = c > 0 && !board.occupied(c as i32 - 1, h as i32 - 2);
            let open_left = open_left || c == 0;
            if open_left {
                count += 1;
            }
        }
    }

    count.min(2)
}

#[inline]
fn four_wide_well_score(heights: &[usize; COL_NB]) -> f32 {
    let left_well_avg: f32 = (heights[0] + heights[1] + heights[2] + heights[3]) as f32 / 4.0;
    let left_rest_avg: f32 =
        (heights[4] + heights[5] + heights[6] + heights[7] + heights[8] + heights[9]) as f32 / 6.0;

    let right_well_avg: f32 = (heights[6] + heights[7] + heights[8] + heights[9]) as f32 / 4.0;
    let right_rest_avg: f32 =
        (heights[0] + heights[1] + heights[2] + heights[3] + heights[4] + heights[5]) as f32 / 6.0;

    // Minimum depth difference to qualify as a 4-wide setup
    const MIN_DEPTH_DIFF: f32 = 3.0;

    let left_diff = left_rest_avg - left_well_avg;
    let right_diff = right_rest_avg - right_well_avg;

    let mut score = 0.0f32;
    if left_diff >= MIN_DEPTH_DIFF {
        score = score.max(left_diff - MIN_DEPTH_DIFF + 1.0);
    }
    if right_diff >= MIN_DEPTH_DIFF {
        score = score.max(right_diff - MIN_DEPTH_DIFF + 1.0);
    }

    score
}

pub fn evaluate(board: &Board, weights: &EvalWeights) -> f32 {
    let heights = column_heights(board);
    let max_h = heights.iter().copied().max().unwrap_or(0);

    let (holes, covered) = holes_and_covered(board, &heights);
    let (well_col, well_depth) = find_well(&heights);
    let (bump, bump_sq) = bumpiness(&heights, well_col);
    let r_transitions = row_transitions(board, max_h);

    let mut score = 0.0f32;

    score += weights.holes * holes as f32;
    score += weights.cell_coveredness * covered as f32;

    score += weights.height * max_h as f32;
    if max_h > 10 {
        score += weights.height_upper_half * (max_h - 10) as f32;
    }
    if max_h > 15 {
        score += weights.height_upper_quarter * (max_h - 15) as f32;
    }

    score += weights.bumpiness * bump as f32;
    score += weights.bumpiness_sq * bump_sq as f32;
    score += weights.row_transitions * r_transitions as f32;

    score += weights.well_depth * well_depth as f32;

    let tsd_count = count_tsd_overhangs(board, &heights);
    score += weights.tsd_overhang * tsd_count as f32;

    let four_wide = four_wide_well_score(&heights);
    score += weights.four_wide_well * four_wide;

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
        assert!(
            score.abs() < 0.001,
            "empty board score {} should be ~0",
            score,
        );
    }

    #[test]
    fn test_holes_reduce_score() {
        let weights = EvalWeights::default();

        let mut clean = Board::new();
        for y in 0..3 {
            clean.rows[y] = FULL_ROW;
        }
        clean.cols = [0; COL_NB];
        for y in 0..3 {
            for x in 0..COL_NB {
                clean.cols[x] |= 1u64 << y;
            }
        }

        let mut holey = Board::new();
        holey.rows[0] = FULL_ROW & !(1 << 5);
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
    fn test_column_heights_basic() {
        let mut board = Board::new();
        board.rows[0] = 1 << 3;
        board.rows[4] = 1 << 3;
        board.cols[3] = (1u64 << 0) | (1u64 << 4);

        let heights = column_heights(&board);
        assert_eq!(heights[3], 5);
        assert_eq!(heights[0], 0);
    }

    #[test]
    fn test_well_detection() {
        let mut heights = [4usize; COL_NB];
        heights[9] = 0;
        let (well_col, well_depth) = find_well(&heights);
        assert_eq!(well_col, Some(9));
        assert_eq!(well_depth, 4);
    }
}
