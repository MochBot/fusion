// Board-shape evaluation only; beam search handles tactics.

use crate::board::Board;
use crate::header::*;

#[derive(Clone, Debug)]
pub struct EvalWeights {
    // Board-shape features.
    pub holes: f32,
    pub cell_coveredness: f32,
    pub height: f32,
    pub height_upper_half: f32,
    pub height_upper_quarter: f32,
    pub bumpiness: f32,
    pub bumpiness_sq: f32,
    pub row_transitions: f32,
    pub well_depth: f32,
    // Structural pattern bonuses.
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
fn column_heights(board: &Board) -> ([usize; COL_NB], usize) {
    let mut heights = [0usize; COL_NB];
    let mut occupied = 0u64;
    for (x, h) in heights.iter_mut().enumerate() {
        // O(1) per column via leading_zeros on cached column bitboard
        let col = board.cols[x];
        occupied |= col;
        *h = 64 - col.leading_zeros() as usize;
    }
    let max_h = 64 - occupied.leading_zeros() as usize;
    (heights, max_h)
}

/// count holes and covered cells per column
/// hole = empty cell below column top
/// covered = filled cells above the topmost hole (capped at 6)
#[inline]
fn holes_and_covered(board: &Board, heights: &[usize; COL_NB]) -> (i32, i32) {
    let mut holes = 0i32;
    let mut covered = 0i32;
    for (&col, &h) in board.cols.iter().zip(heights) {
        // Every filled cell of a column is below its height.
        holes += h as i32 - col.count_ones() as i32;
        let empty_below = !col & ((1u64 << h) - 1);
        // One past the topmost hole, or 0 without holes (and then no cover).
        let above = 64 - empty_below.leading_zeros();
        let cov = ((col >> above).count_ones() as i32).min(6);
        covered += i32::from(empty_below != 0) * cov;
    }
    (holes, covered)
}

#[cfg(test)]
fn holes_and_covered_oracle(board: &Board, heights: &[usize; COL_NB]) -> (i32, i32) {
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

/// bumpiness: sum of |h[i]-h[i+1]| and (h[i]-h[i+1])^2
/// skips the well column (deepest col with both neighbors taller)
#[cfg(test)]
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

/// row transitions. XOR adjacent cells, count 1-bits; empty rows = 0.
///
/// SWAR over 4 rows per u64 lane group. Row values use 10 bits;
/// `v + 0x7FFF` per lane sets bit 15 iff nonzero without carrying
/// into the neighbor. Internal transitions collapse into one popcount.
/// Rows >= max_height are empty by definition and self-exclude.
#[inline]
fn row_transitions(board: &Board, max_height: usize) -> i32 {
    const LANE_LSB: u64 = 0x0001_0001_0001_0001;
    const LANE_LOW9: u64 = 0x01FF_01FF_01FF_01FF;
    const LANE_SHIFT_GUARD: u64 = 0x7FFF_7FFF_7FFF_7FFF;

    let mut total = 0u32;
    let mut y = 0usize;
    while y < max_height {
        let v = (board.rows[y] as u64)
            | (board.rows[y + 1] as u64) << 16
            | (board.rows[y + 2] as u64) << 32
            | (board.rows[y + 3] as u64) << 48;
        let nz = ((v + LANE_SHIFT_GUARD) >> 15) & LANE_LSB;
        let xor = v ^ ((v >> 1) & LANE_SHIFT_GUARD);
        total += (xor & LANE_LOW9).count_ones();
        total += ((!v) & LANE_LSB & nz).count_ones();
        total += ((!(v >> 9)) & LANE_LSB & nz).count_ones();
        y += 4;
    }
    total as i32
}

#[cfg(test)]
fn row_transitions_oracle(board: &Board, max_height: usize) -> i32 {
    let mut total = 0i32;
    for y in 0..max_height {
        let row = board.row(y);
        if row == 0 {
            continue;
        }
        let shifted = row >> 1;
        let xor = row ^ shifted;
        total += (xor & 0x1FF).count_ones() as i32;
        if row & 1 == 0 {
            total += 1;
        }
        if row & (1 << 9) == 0 {
            total += 1;
        }
    }
    total
}

/// find the deepest well column (both neighbors taller)
/// returns (well_col, well_depth)
#[cfg(test)]
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

/// `find_well` then `bumpiness` without the well's transitions, in one pass
/// with no data-dependent branches.
#[inline]
fn well_and_bumpiness(heights: &[usize; COL_NB]) -> (Option<usize>, i32, i32, i32) {
    let h = heights.map(|h| h as i32);
    let mut best_depth = 0i32;
    let mut best_col = COL_NB;
    for x in 0..COL_NB {
        let left = if x == 0 { 40 } else { h[x - 1] };
        let right = if x == COL_NB - 1 { 40 } else { h[x + 1] };
        // Positive exactly when both neighbours are taller.
        let depth = left.min(right) - h[x];
        let better = depth > best_depth;
        best_depth = if better { depth } else { best_depth };
        best_col = if better { x } else { best_col };
    }
    let mut bump = 0i32;
    let mut bump_sq = 0i32;
    for i in 0..COL_NB - 1 {
        let diff = h[i] - h[i + 1];
        let keep = i32::from(i != best_col && i + 1 != best_col);
        bump += keep * diff.abs();
        bump_sq += keep * diff * diff;
    }
    let well = (best_col < COL_NB).then_some(best_col);
    (well, best_depth, bump, bump_sq)
}

/// Detect T-spin double overhang setups.
///
/// Scans for the minimal geometric signature:
///   col c:   filled at h, empty at h-1  (overhang)
///   col c+/-1: filled at h-1 AND h (wall for the T-slot)
///   col c:   empty at h-2 OR h-2 < 0 (cavity)
///
/// Returns count of detected TSD-ready overhangs (0, 1, or 2).
/// Columns are padded with an empty column on each side, which is never a
/// wall and always open, as the board's edges are.
#[inline]
fn count_tsd_overhangs(board: &Board, heights: &[usize; COL_NB]) -> i32 {
    let mut cols = [0u64; COL_NB + 2];
    cols[1..=COL_NB].copy_from_slice(&board.cols);
    let mut count = 0i32;
    for c in 0..COL_NB {
        let h = heights[c] as u32;
        // Top cell of the column and the one under it; none below height 2.
        let top = (1u64 << h) >> 1;
        let cavity = top >> 1;
        let overhang = cavity != 0 && cols[c + 1] & cavity == 0;
        let slot = top | cavity;
        let (left, right) = (cols[c], cols[c + 2]);
        let wall_left = left & slot == slot;
        let wall_right = right & slot == slot;
        let open_left = left & cavity == 0;
        let open_right = right & cavity == 0;
        count += i32::from(overhang)
            * (i32::from(wall_left && open_right) + i32::from(wall_right && open_left));
    }
    count.min(2)
}

#[cfg(test)]
fn count_tsd_overhangs_oracle(board: &Board, heights: &[usize; COL_NB]) -> i32 {
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

/// Detect 4-wide combo well on either board edge.
///
/// Returns a continuous score (0.0 if no 4-wide detected).
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

/// The raw board-shape measurements `evaluate` weighs.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapeFeatures {
    pub heights: [usize; COL_NB],
    pub max_height: usize,
    pub holes: i32,
    pub covered: i32,
    pub bumpiness: i32,
    pub bumpiness_sq: i32,
    pub row_transitions: i32,
    pub well_column: Option<usize>,
    pub well_depth: i32,
    pub tsd_overhangs: i32,
    pub four_wide: f32,
}

pub fn shape_features(board: &Board) -> ShapeFeatures {
    let (heights, max_height) = column_heights(board);
    let (holes, covered) = holes_and_covered(board, &heights);
    let (well_column, well_depth, bumpiness, bumpiness_sq) = well_and_bumpiness(&heights);
    ShapeFeatures {
        heights,
        max_height,
        holes,
        covered,
        bumpiness,
        bumpiness_sq,
        row_transitions: row_transitions(board, max_height),
        well_column,
        well_depth,
        tsd_overhangs: count_tsd_overhangs(board, &heights),
        four_wide: four_wide_well_score(&heights),
    }
}

pub fn evaluate(board: &Board, weights: &EvalWeights) -> f32 {
    evaluate_shape(&shape_features(board), weights)
}

/// `evaluate` of a board from its shape measurements.
pub fn evaluate_shape(shape: &ShapeFeatures, weights: &EvalWeights) -> f32 {
    let max_h = shape.max_height;
    let mut score = 0.0f32;

    score += weights.holes * shape.holes as f32;
    score += weights.cell_coveredness * shape.covered as f32;

    score += weights.height * max_h as f32;
    if max_h > 10 {
        score += weights.height_upper_half * (max_h - 10) as f32;
    }
    if max_h > 15 {
        score += weights.height_upper_quarter * (max_h - 15) as f32;
    }

    score += weights.bumpiness * shape.bumpiness as f32;
    score += weights.bumpiness_sq * shape.bumpiness_sq as f32;
    score += weights.row_transitions * shape.row_transitions as f32;

    score += weights.well_depth * shape.well_depth as f32;
    score += weights.tsd_overhang * shape.tsd_overhangs as f32;
    score += weights.four_wide_well * shape.four_wide;

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, BOARD_HEIGHT, FULL_ROW};

    struct SplitMix64 {
        state: u64,
    }

    impl SplitMix64 {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_usize(&mut self, upper: usize) -> usize {
            (self.next_u64() as usize) % upper
        }
    }

    fn board_from_rows(rows: [u16; BOARD_HEIGHT]) -> Board {
        let mut board = Board::new();
        for (y, row) in rows.iter().enumerate() {
            board.rows[y] = row & FULL_ROW;
            let mut bits = board.rows[y] as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                board.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        board
    }

    fn set_column(rows: &mut [u16; BOARD_HEIGHT], x: usize, ys: &[usize]) {
        for &y in ys {
            rows[y] |= 1u16 << x;
        }
    }

    fn edge_case_boards() -> Vec<Board> {
        let mut boards = Vec::new();

        boards.push(Board::new());
        boards.push(board_from_rows([FULL_ROW; BOARD_HEIGHT]));

        let mut rows = [0u16; BOARD_HEIGHT];
        set_column(&mut rows, 3, &[1, 2, 3]);
        boards.push(board_from_rows(rows));

        let mut rows = [0u16; BOARD_HEIGHT];
        set_column(&mut rows, 4, &[0, 2]);
        boards.push(board_from_rows(rows));

        let mut rows = [0u16; BOARD_HEIGHT];
        set_column(&mut rows, 5, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        boards.push(board_from_rows(rows));

        let mut rows = [0u16; BOARD_HEIGHT];
        for y in 0..BOARD_HEIGHT {
            if y != 10 && y != 20 {
                rows[y] |= 1u16 << 6;
            }
        }
        boards.push(board_from_rows(rows));

        let mut rows = [0u16; BOARD_HEIGHT];
        for y in 0..BOARD_HEIGHT {
            rows[y] |= 1u16 << 7;
        }
        boards.push(board_from_rows(rows));

        let mut rows = [0u16; BOARD_HEIGHT];
        set_column(&mut rows, 2, &[0, 2, 3, 5, 8, 9]);
        boards.push(board_from_rows(rows));

        let mut rows = [0u16; BOARD_HEIGHT];
        set_column(&mut rows, 3, &[1, 2]);
        set_column(&mut rows, 4, &[2]);
        boards.push(board_from_rows(rows));

        let mut rows = [0u16; BOARD_HEIGHT];
        set_column(&mut rows, 4, &[1, 2]);
        set_column(&mut rows, 5, &[2]);
        set_column(&mut rows, 6, &[1, 2]);
        boards.push(board_from_rows(rows));

        boards
    }

    fn fill_by_density(
        rows: &mut [u16; BOARD_HEIGHT],
        rng: &mut SplitMix64,
        height: usize,
        threshold: u64,
        modulo: u64,
    ) {
        for row in rows.iter_mut().take(height) {
            for x in 0..COL_NB {
                if rng.next_u64() % modulo < threshold {
                    *row |= 1u16 << x;
                }
            }
        }
    }

    fn random_board(rng: &mut SplitMix64, case: usize) -> Board {
        let mut rows = [0u16; BOARD_HEIGHT];
        match case % 6 {
            0 => {
                let height = rng.next_usize(9);
                fill_by_density(&mut rows, rng, height, 1, 8);
            }
            1 => {
                let height = 1 + rng.next_usize(BOARD_HEIGHT);
                fill_by_density(&mut rows, rng, height, 7, 8);
            }
            2 => {
                let height = rng.next_usize(9);
                fill_by_density(&mut rows, rng, height, 1, 3);
            }
            3 => {
                let height = 32 + rng.next_usize(BOARD_HEIGHT - 31);
                fill_by_density(&mut rows, rng, height, 1, 2);
            }
            4 => {
                for x in 0..COL_NB {
                    let h = rng.next_usize(BOARD_HEIGHT + 1);
                    if h == 0 {
                        continue;
                    }
                    rows[h - 1] |= 1u16 << x;
                    for row in rows.iter_mut().take(h - 1) {
                        if rng.next_u64() & 3 != 0 {
                            *row |= 1u16 << x;
                        }
                    }
                    if h > 6 {
                        let y0 = rng.next_usize(h - 1);
                        let y1 = rng.next_usize(h - 1);
                        rows[y0] &= !(1u16 << x);
                        rows[y1] &= !(1u16 << x);
                    }
                }
            }
            _ => {
                for x in 0..COL_NB {
                    let h = rng.next_usize(BOARD_HEIGHT + 1);
                    for row in rows.iter_mut().take(h) {
                        *row |= 1u16 << x;
                    }
                    if h > 3 {
                        let y = rng.next_usize(h - 1);
                        rows[y] &= !(1u16 << x);
                    }
                }
            }
        }
        board_from_rows(rows)
    }

    fn check_holes_covered_parity(board: &Board, label: &str) {
        let (heights, _) = column_heights(board);
        assert_eq!(
            holes_and_covered(board, &heights),
            holes_and_covered_oracle(board, &heights),
            "{label} rows {:?}",
            board.rows
        );
    }

    fn check_well_bumpiness_parity(board: &Board, label: &str) {
        let (heights, _) = column_heights(board);
        let (well, depth) = find_well(&heights);
        let (bump, bump_sq) = bumpiness(&heights, well);
        assert_eq!(
            well_and_bumpiness(&heights),
            (well, depth, bump, bump_sq),
            "{label} heights {heights:?}"
        );
    }

    #[test]
    fn well_and_bumpiness_matches_oracle() {
        for (case, board) in edge_case_boards().into_iter().enumerate() {
            check_well_bumpiness_parity(&board, &format!("edge case {case}"));
        }
        let mut rng = SplitMix64::new(0x9E37_79B9_7F4A_7C15);
        for case in 0..100_000 {
            let board = random_board(&mut rng, case);
            check_well_bumpiness_parity(&board, &format!("random case {case}"));
        }
        // Ties between equal wells keep the first; heights beyond any board
        // exercise the edge sentinels.
        for heights in [
            [3, 0, 3, 0, 3, 5, 5, 5, 5, 5],
            [0; COL_NB],
            [40; COL_NB],
            [0, 40, 0, 40, 0, 40, 0, 40, 0, 40],
        ] {
            let (well, depth) = find_well(&heights);
            let (bump, bump_sq) = bumpiness(&heights, well);
            assert_eq!(
                well_and_bumpiness(&heights),
                (well, depth, bump, bump_sq),
                "{heights:?}"
            );
        }
    }

    fn check_tsd_overhang_parity(board: &Board, label: &str) {
        let (heights, _) = column_heights(board);
        assert_eq!(
            count_tsd_overhangs(board, &heights),
            count_tsd_overhangs_oracle(board, &heights),
            "{label} rows {:?}",
            board.rows
        );
    }

    #[test]
    fn bitboard_holes_covered_matches_oracle() {
        for (case, board) in edge_case_boards().into_iter().enumerate() {
            check_holes_covered_parity(&board, &format!("edge case {case}"));
        }

        let mut rng = SplitMix64::new(0xD1B5_4A32_D192_ED03);
        for case in 0..100_000 {
            let board = random_board(&mut rng, case);
            check_holes_covered_parity(&board, &format!("random case {case}"));
        }
    }

    fn check_row_transitions_parity(board: &Board, label: &str) {
        let (heights, max_h) = column_heights(board);
        assert_eq!(
            max_h,
            heights.iter().copied().max().unwrap_or(0),
            "{label} fused max_h mismatch"
        );
        assert_eq!(
            row_transitions(board, max_h),
            row_transitions_oracle(board, max_h),
            "{label} rows {:?}",
            board.rows
        );
    }

    #[test]
    fn swar_row_transitions_matches_oracle() {
        for (case, board) in edge_case_boards().into_iter().enumerate() {
            check_row_transitions_parity(&board, &format!("edge case {case}"));
        }

        let mut rng = SplitMix64::new(0x5851_F42D_4C95_7F2D);
        for case in 0..100_000 {
            let board = random_board(&mut rng, case);
            check_row_transitions_parity(&board, &format!("random case {case}"));
        }
    }

    #[test]
    fn bitboard_tsd_overhangs_matches_oracle() {
        for (case, board) in edge_case_boards().into_iter().enumerate() {
            check_tsd_overhang_parity(&board, &format!("edge case {case}"));
        }

        let mut rng = SplitMix64::new(0x9E37_79B9_7F4A_7C15);
        for case in 0..100_000 {
            let board = random_board(&mut rng, case);
            check_tsd_overhang_parity(&board, &format!("random case {case}"));
        }
    }

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

        let (heights, _) = column_heights(&board);
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

    /// Times each shape feature on the boards in `LEAGUE_BENCH_BOARDS`, a file
    /// of little-endian `[u16; 40]` boards.
    #[test]
    #[ignore]
    fn bench_shape_features() {
        use std::hint::black_box;
        use std::time::Instant;
        let Ok(path) = std::env::var("LEAGUE_BENCH_BOARDS") else {
            eprintln!("set LEAGUE_BENCH_BOARDS");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let (chunks, _) = bytes.as_chunks::<{ 2 * BOARD_HEIGHT }>();
        let boards: Vec<Board> = chunks
            .iter()
            .map(|b| {
                let (pairs, _) = b.as_chunks::<2>();
                Board::from_rows(std::array::from_fn(|y| u16::from_le_bytes(pairs[y])))
            })
            .collect();
        let n = boards.len() as f64;
        let time = |name: &str, f: &dyn Fn(&Board) -> i64| {
            let mut best = f64::MAX;
            for _ in 0..7 {
                let t = Instant::now();
                let mut s = 0i64;
                for b in &boards {
                    s = s.wrapping_add(f(black_box(b)));
                }
                black_box(s);
                best = best.min(t.elapsed().as_secs_f64() * 1e9 / n);
            }
            eprintln!("{name:>18}: {best:6.2} ns/board");
        };
        time("shape_features", &|b| {
            let s = shape_features(b);
            s.holes as i64 + s.tsd_overhangs as i64 + s.four_wide.to_bits() as i64
        });
        time("heights", &|b| column_heights(b).1 as i64);
        time("+holes", &|b| {
            let (h, _) = column_heights(b);
            holes_and_covered(b, &h).0 as i64
        });
        time("+well_bump", &|b| {
            let (h, _) = column_heights(b);
            well_and_bumpiness(&h).3 as i64
        });
        time("+rowtrans", &|b| {
            let (h, m) = column_heights(b);
            row_transitions(b, m) as i64 + h[0] as i64
        });
        time("+tsd", &|b| {
            let (h, _) = column_heights(b);
            count_tsd_overhangs(b, &h) as i64
        });
        let mut bad = 0;
        for b in &boards {
            let (h, _) = column_heights(b);
            let (wc, wd) = find_well(&h);
            let (bu, bs) = bumpiness(&h, wc);
            bad += usize::from(well_and_bumpiness(&h) != (wc, wd, bu, bs));
            bad += usize::from(count_tsd_overhangs(b, &h) != count_tsd_overhangs_oracle(b, &h));
            bad += usize::from(holes_and_covered(b, &h) != holes_and_covered_oracle(b, &h));
        }
        eprintln!("mismatches: {bad}");
        assert_eq!(bad, 0);
        time("+fourwide", &|b| {
            let (h, _) = column_heights(b);
            four_wide_well_score(&h).to_bits() as i64
        });
    }
}
