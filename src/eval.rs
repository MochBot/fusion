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

    for (x, &h) in heights.iter().enumerate() {
        if h == 0 {
            continue;
        }

        let below_mask = (1u64 << h) - 1;
        let filled_below = board.cols[x] & below_mask;
        let col_holes = h as i32 - filled_below.count_ones() as i32;
        holes += col_holes;

        if col_holes == 0 {
            continue;
        }

        let empty_below = !board.cols[x] & below_mask;
        let topmost_hole = 63usize - empty_below.leading_zeros() as usize;
        let at_or_below_hole = (1u64 << (topmost_hole + 1)) - 1;
        let cov = (filled_below & !at_or_below_hole).count_ones() as i32;
        covered += cov.min(6);
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

#[inline]
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

/// Detect T-spin double overhang setups.
///
/// Scans for the minimal geometric signature:
///   col c:   filled at h, empty at h-1  (overhang)
///   col c+/-1: filled at h-1 AND h (wall for the T-slot)
///   col c:   empty at h-2 OR h-2 < 0 (cavity)
///
/// Returns count of detected TSD-ready overhangs (0, 1, or 2).
#[inline]
fn count_tsd_overhangs(board: &Board, heights: &[usize; COL_NB]) -> i32 {
    let mut count = 0i32;

    for c in 0..COL_NB {
        let h = heights[c];
        if h < 2 {
            continue;
        }

        let top_bit = 1u64 << (h - 1);
        let cavity_bit = 1u64 << (h - 2);
        let col = board.cols[c];

        // Overhang: filled at top, empty directly below
        let has_overhang = (col & top_bit) != 0 && (col & cavity_bit) == 0;

        if !has_overhang {
            continue;
        }

        // Check for wall on either side providing the T-slot
        let wall_left = c > 0
            && heights[c - 1] >= h
            && (board.cols[c - 1] & top_bit) != 0
            && (board.cols[c - 1] & cavity_bit) != 0;

        let wall_right = c < COL_NB - 1
            && heights[c + 1] >= h
            && (board.cols[c + 1] & top_bit) != 0
            && (board.cols[c + 1] & cavity_bit) != 0;

        // Need cavity on the opposite side of the wall
        if wall_left {
            let open_right = c < COL_NB - 1 && (board.cols[c + 1] & cavity_bit) == 0;
            let open_right = open_right || c == COL_NB - 1;
            if open_right {
                count += 1;
            }
        }
        if wall_right {
            let open_left = c > 0 && (board.cols[c - 1] & cavity_bit) == 0;
            let open_left = open_left || c == 0;
            if open_left {
                count += 1;
            }
        }
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

pub fn evaluate(board: &Board, weights: &EvalWeights) -> f32 {
    let (heights, max_h) = column_heights(board);

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

        let mut rng = SplitMix64::new(0x9E37_79B9_7F4_A7C15);
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
}
