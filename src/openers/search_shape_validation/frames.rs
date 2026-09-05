use crate::board::{Board, FULL_ROW};
use crate::header::Piece;
use crate::openers::board::{mirror_letter_row, rows_to_masks_floor_up};
use crate::openers::catalog::{OpenerPlacement, OpenerTreeNode};
use crate::openers::segments::derive_placements;

#[derive(Clone)]
pub(super) struct TargetPlacement {
    pub(super) piece: Piece,
    pub(super) cells: [[u8; 2]; 4],
}

pub(super) fn node_pre_clear_rows(node: &OpenerTreeNode) -> &[String] {
    match node.pre_clear_rows.as_deref() {
        Some(rows) => rows,
        None => &node.rows,
    }
}

pub(super) fn node_placements(
    parent: Option<&OpenerTreeNode>,
    node: &OpenerTreeNode,
    mirrored: bool,
    construction_rows: Option<&[String]>,
) -> Option<Vec<TargetPlacement>> {
    let derived;
    let placements: Vec<OpenerPlacement> = if let Some(rows) = construction_rows {
        let canonical_rows = mirrored_rows(rows, mirrored)
            .into_iter()
            .rev()
            .collect::<Vec<_>>();
        let parent_rows = parent
            .map(|parent| parent.rows.iter().rev().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        derived = derive_placements(&parent_rows, &canonical_rows)?;
        derived
            .placements
            .iter()
            .map(|placement| OpenerPlacement {
                letter: placement.letter.clone(),
                cells: placement.cells.clone(),
            })
            .collect()
    } else {
        match &node.placements {
            Some(placements) => placements.clone(),
            None if declared_frame_is_epsilon(node, mirrored) => Vec::new(),
            None => {
                let parent_rows = parent.map_or(&[][..], |parent| parent.rows.as_slice());
                derived = derive_placements(parent_rows, node_pre_clear_rows(node))?;
                derived
                    .placements
                    .iter()
                    .map(|placement| OpenerPlacement {
                        letter: placement.letter.clone(),
                        cells: placement.cells.clone(),
                    })
                    .collect()
            }
        }
    };
    placements
        .iter()
        .map(|placement| {
            let cells: [[u8; 2]; 4] = placement.cells.clone().try_into().ok()?;
            Some(TargetPlacement {
                piece: piece_from_letter(&placement.letter, mirrored)?,
                cells: if mirrored {
                    cells.map(|[x, y]| [9 - x, y])
                } else {
                    cells
                },
            })
        })
        .collect()
}

pub(super) fn board_matches(board: &Board, rows: &[String], mirrored: bool) -> bool {
    let masks = rows_to_masks_floor_up(&mirrored_rows(rows, mirrored));
    board.rows[..masks.len()] == masks && board.rows[masks.len()..].iter().all(|row| *row == 0)
}

fn declared_frame_is_epsilon(node: &OpenerTreeNode, mirrored: bool) -> bool {
    let shadow = rows_to_masks_floor_up(&mirrored_rows(node_pre_clear_rows(node), mirrored))
        .into_iter()
        .filter(|row| *row != FULL_ROW)
        .collect::<Vec<_>>();
    shadow == rows_to_masks_floor_up(&node.rows)
}

fn mirrored_rows(rows: &[String], mirrored: bool) -> Vec<String> {
    if mirrored {
        rows.iter().map(|row| mirror_letter_row(row)).collect()
    } else {
        rows.to_vec()
    }
}

fn piece_from_letter(letter: &str, mirrored: bool) -> Option<Piece> {
    let [letter] = letter.as_bytes() else {
        return None;
    };
    let letter = if mirrored {
        mirror_letter(*letter)
    } else {
        *letter
    };
    match letter {
        b'I' => Some(Piece::I),
        b'O' => Some(Piece::O),
        b'T' => Some(Piece::T),
        b'L' => Some(Piece::L),
        b'J' => Some(Piece::J),
        b'S' => Some(Piece::S),
        b'Z' => Some(Piece::Z),
        _ => None,
    }
}

fn mirror_letter(letter: u8) -> u8 {
    match letter {
        b'L' => b'J',
        b'J' => b'L',
        b'S' => b'Z',
        b'Z' => b'S',
        other => other,
    }
}
