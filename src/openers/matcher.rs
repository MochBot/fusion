use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::openers::board::{cell_count, OPENER_BOARD_ROW_MASK};
use crate::openers::target::{letter_masks_of, ShapeTarget, PIECE_LETTERS};

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
    overlap: u32,
}

pub(crate) fn match_board(
    targets: &[ShapeTarget],
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
    let board_letter_masks: Option<Vec<[u16; PIECE_LETTERS.len()]>> = board_letters.map(|rows| {
        board_masks
            .iter()
            .enumerate()
            .map(|(y, _)| {
                rows.get(y)
                    .map_or([0; PIECE_LETTERS.len()], |row| letter_masks_of(row))
            })
            .collect()
    });
    let opener_count = targets
        .iter()
        .map(|target| target.opener_ordinal as usize + 1)
        .max()
        .unwrap_or_default();
    let mut best_per_opener: Vec<Option<Candidate<'_>>> = vec![None; opener_count];
    for target in targets {
        let mut overlap = 0;
        match (board_letter_masks.as_deref(), target.letters.is_some()) {
            (Some(board_letter_masks), true) => {
                for (y, (board_row, target_row)) in board_masks.iter().zip(&target.rows).enumerate()
                {
                    let common = (*board_row & OPENER_BOARD_ROW_MASK) & *target_row;
                    let lettered = target.lettered_cells.get(y).copied().unwrap_or_default();
                    let mut kept = common & !lettered;
                    if common & lettered != 0 {
                        kept |= common
                            & agreeing_letter_cells(
                                &board_letter_masks[y],
                                &target.letter_masks[y],
                            );
                    }
                    overlap += kept.count_ones();
                }
            }
            _ => {
                for (board_row, target_row) in board_masks.iter().zip(&target.rows) {
                    overlap += ((*board_row & OPENER_BOARD_ROW_MASK) & *target_row).count_ones();
                }
            }
        }
        if overlap == 0 {
            continue;
        }

        let candidate = Candidate { target, overlap };
        let slot = &mut best_per_opener[target.opener_ordinal as usize];
        match slot {
            Some(incumbent) if compare_raw(candidate, *incumbent, board_cells).is_lt() => {
                *incumbent = candidate;
            }
            Some(_) => {}
            None => *slot = Some(candidate),
        }
    }

    let mut ranked: Vec<_> = best_per_opener.into_iter().flatten().collect();
    ranked.sort_by(|left, right| compare_raw(*left, *right, board_cells));
    if limit > 0 {
        ranked.truncate(limit);
    }
    ranked
        .into_iter()
        .map(|candidate| to_match(candidate, board_cells))
        .collect()
}

/// Cells where the board and target carry the same specific piece letter.
/// A board cell whose letter is missing or `X` matches no specific target
/// letter, which preserves the string comparison's wildcard semantics.
fn agreeing_letter_cells(
    board: &[u16; PIECE_LETTERS.len()],
    target: &[u16; PIECE_LETTERS.len()],
) -> u16 {
    board
        .iter()
        .zip(target)
        .fold(0, |kept, (board_mask, target_mask)| {
            kept | (board_mask & target_mask)
        })
}

fn compare_raw(left: Candidate<'_>, right: Candidate<'_>, board_cells: u32) -> Ordering {
    let left_stray = board_cells - left.overlap;
    let right_stray = board_cells - right.overlap;
    match (left_stray == 0, right_stray == 0) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }

    match (
        left.target.letters.is_some(),
        right.target.letters.is_some(),
    ) {
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
    let left_progress = f64::from(left.overlap) / f64::from(left.target.cells);
    let right_progress = f64::from(right.overlap) / f64::from(right.target.cells);
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
        let targets = [ShapeTarget {
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
            &targets,
            &[0b0000001111],
            1,
            Some(&["ZZZZ______".to_owned()]),
        );

        assert!(matches.is_empty());
    }
}
