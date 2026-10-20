use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::openers::board::{cell_count, OPENER_BOARD_ROW_MASK};
use crate::openers::target::{
    letter_masks_of, letter_planes, ShapeTarget, ShapeTargets, TargetHead, ROWS_PER_GROUP,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardMatch {
    pub opener_id: String,
    pub label: String,
    pub mirrored: bool,
    pub colored: bool,
    pub overlap_cells: u32,
    pub target_cells: u32,
    pub stray_cells: u32,
    pub progress: f64,
    pub complete: bool,
    pub node_id: Option<u32>,
    pub route_name: Option<String>,
}

#[derive(Clone, Copy)]
struct Candidate<'a> {
    target: &'a ShapeTarget,
    head: TargetHead,
    overlap: u32,
}

pub(crate) fn match_board(
    targets: &ShapeTargets,
    board_masks: &[u16],
    limit: usize,
    board_letters: Option<&[String]>,
) -> Vec<BoardMatch> {
    let board_cells = cell_count(board_masks);
    if board_cells == 0 {
        return Vec::new();
    }

    let board_masks = match board_masks.iter().rposition(|mask| *mask != 0) {
        Some(last) => &board_masks[..=last],
        None => &board_masks[..0],
    };
    // Each board row: occupancy, then its letters in `letter_planes` form,
    // packed four rows a group as the targets are. A board cell whose letter
    // is missing or `X` has code 0, which matches no specific target letter,
    // the string comparison's wildcard semantics. Rows past the board's last
    // occupied one meet no target cell, so groups past it are not read.
    let mut board_rows: Vec<[u64; 4]> = Vec::new();
    for (y, mask) in board_masks.iter().enumerate() {
        let [p0, p1, p2] = board_letters
            .and_then(|rows| rows.get(y))
            .map_or([0; 3], |row| letter_planes(&letter_masks_of(row)));
        if y % ROWS_PER_GROUP == 0 {
            board_rows.push([0; 4]);
        }
        let group = board_rows.last_mut().expect("a group was pushed");
        let shift = 16 * (y % ROWS_PER_GROUP);
        for (lane, value) in group.iter_mut().zip([*mask & OPENER_BOARD_ROW_MASK, p0, p1, p2]) {
            *lane |= u64::from(value) << shift;
        }
    }
    // Without board letters every cell matches on occupancy alone.
    let letter_gate = if board_letters.is_some() { u64::MAX } else { 0 };
    let mut best_per_opener: Vec<Option<Candidate<'_>>> = vec![None; targets.opener_count()];
    for index in 0..targets.len() {
        let (head, rows) = targets.packed(index);
        let mut overlap = 0;
        for ([board_row, b0, b1, b2], [target_row, lettered, t0, t1, t2]) in
            board_rows.iter().zip(rows)
        {
            let common = board_row & target_row;
            // A common lettered cell counts only where both carry the same
            // specific letter; a lettered cell without one (code 0) never does.
            let disagree =
                (lettered & letter_gate) & ((b0 ^ t0) | (b1 ^ t1) | (b2 ^ t2) | !(t0 | t1 | t2));
            overlap += (common & !disagree).count_ones();
        }
        if overlap == 0 {
            continue;
        }

        let candidate = Candidate {
            target: &targets[index],
            head,
            overlap,
        };
        let slot = &mut best_per_opener[head.opener_ordinal as usize];
        match slot {
            Some(incumbent) if compare_raw(candidate, *incumbent, board_cells).is_lt() => {
                *incumbent = candidate;
            }
            Some(_) => {}
            None => *slot = Some(candidate),
        }
    }

    let mut ranked: Vec<_> = best_per_opener.into_iter().flatten().collect();
    // `ranked` is in ordinal order, so the ordinal tie-break reproduces the
    // stable sort, and a total order lets selection keep its first `limit`.
    let order = |left: &Candidate<'_>, right: &Candidate<'_>| {
        compare_raw(*left, *right, board_cells)
            .then(left.head.opener_ordinal.cmp(&right.head.opener_ordinal))
    };
    if limit > 0 && ranked.len() > limit {
        ranked.select_nth_unstable_by(limit - 1, order);
        ranked.truncate(limit);
    }
    ranked.sort_unstable_by(order);
    ranked
        .into_iter()
        .map(|candidate| to_match(candidate, board_cells))
        .collect()
}

fn compare_raw(left: Candidate<'_>, right: Candidate<'_>, board_cells: u32) -> Ordering {
    let left_stray = board_cells - left.overlap;
    let right_stray = board_cells - right.overlap;
    match (left_stray == 0, right_stray == 0) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }

    match (left.head.colored, right.head.colored) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    match right.overlap.cmp(&left.overlap) {
        Ordering::Equal => {}
        ordering => return ordering,
    }
    match left_stray.cmp(&right_stray) {
        Ordering::Equal => {}
        ordering => return ordering,
    }
    let left_progress = f64::from(left.overlap) / f64::from(left.head.cells);
    let right_progress = f64::from(right.overlap) / f64::from(right.head.cells);
    match right_progress.total_cmp(&left_progress) {
        Ordering::Equal => left.target.opener_id.cmp(&right.target.opener_id),
        ordering => ordering,
    }
}

fn to_match(candidate: Candidate<'_>, board_cells: u32) -> BoardMatch {
    let target = candidate.target;
    BoardMatch {
        opener_id: target.opener_id.clone(),
        label: target.label.clone(),
        mirrored: target.mirrored,
        colored: target.letters.is_some(),
        overlap_cells: candidate.overlap,
        target_cells: target.cells,
        stray_cells: board_cells - candidate.overlap,
        progress: f64::from(candidate.overlap) / f64::from(target.cells),
        complete: candidate.overlap == target.cells,
        node_id: target.node_id,
        route_name: target.route_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::match_board;
    use crate::openers::target::ShapeTarget;

    #[test]
    fn exact_occupancy_respects_piece_letters_for_weak_phase_matching() {
        let targets = vec![ShapeTarget {
            opener_id: "fixture".to_owned(),
            opener_ordinal: 0,
            label: "Fixture".to_owned(),
            rows: vec![0b0000001111],
            cells: 4,
            mirrored: false,
            letters: Some(vec!["IIII______".to_owned()]),
            lettered_cells: vec![0b0000001111],
            letter_masks: vec![[0b0000001111, 0, 0, 0, 0, 0, 0]],
            node_id: Some(1),
            route_name: None,
        }];

        let matches = match_board(
            &targets.into(),
            &[0b0000001111],
            1,
            Some(&["ZZZZ______".to_owned()]),
        );

        assert!(matches.is_empty());
    }
}
