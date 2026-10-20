//! Lifts the catalogued path from the start node to the target node into one
//! frame: the stages, the slots each adds, and the outline cells.

use std::collections::HashMap;
use std::ops::Range;

use crate::board::{BOARD_HEIGHT, FULL_ROW};
use crate::header::Piece;
use crate::openers::board::{mirror_letter_row, mirror_mask_10, rows_to_masks_floor_up};
use crate::openers::catalog::OpenerTreeNode;
use crate::openers::segments::{derive_placements, ShowcasePlacement};

use super::clears::RequiredClear;
use super::frame::{mirror_letter, physical_masks, piece_for_letter, Cells, Masks, CELLS};

pub(super) const MAX_SLOTS: usize = 64;

/// One catalogued piece position of the build, in frame cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Slot {
    pub piece: Piece,
    pub letter: u8,
    pub cells: Cells,
}

/// One catalogued tree edge on the path: the node it reaches and the frame
/// occupancy once every cell up to that node is filled.
#[derive(Clone, Debug)]
pub(super) struct Stage {
    pub node_id: u32,
    pub pieces: u32,
    pub goal: Masks,
    /// Frame rows this stage clears.
    pub cleared: Vec<u8>,
    /// The catalog's name for this stage's clear, when it clears rows.
    pub label: Option<String>,
    /// Indices of the slots this stage adds.
    pub slots: Range<usize>,
    /// The clear this stage must perform; set from `label` by `clears`.
    pub clear: Option<RequiredClear>,
    /// The label claims more than any order can perform, so `clear` was
    /// lowered to what the diagram allows.
    pub relaxed: bool,
}

/// The whole path from the start board to the target node, lifted into one
/// frame. The frame keeps every row the path ever fills; a row that is full
/// in it has been cleared, so the physical board is the frame without its
/// full rows. Placing pieces of a later stage before an earlier stage clears
/// therefore lands them where the real board would put them.
#[derive(Clone, Debug)]
pub(super) struct BuildTarget {
    pub start: Masks,
    pub goal: Masks,
    /// Outline cells: target cells no slot covers; any piece the cell's
    /// letter allows may fill them.
    pub outline: Masks,
    /// Letter per frame cell: the start board's letters, slot letters, and
    /// for outline cells the source letter or `X` for any piece.
    pub letters: [u8; CELLS],
    pub slots: Vec<Slot>,
    pub stages: Vec<Stage>,
    /// Index of the stage that adds each frame cell.
    pub stage_of: [u8; CELLS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TargetError {
    UnknownNode(u32),
    StartNotAncestor,
    NothingToBuild,
    /// The node's diagram does not extend its parent's, or its clears do not
    /// produce its post-clear rows.
    SourceMismatch(u32),
    TooTall(u32),
    TooManyPieces,
}

/// Outline cells one plan fills: eight pieces' worth. Nine in ten of the live
/// catalogue's outlined versions that plan at all fit within it; past it almost
/// none plan and the search runs for tens of seconds.
pub(super) const MAX_OUTLINE_CELLS: u32 = 32;

/// An outline up to four pieces' worth costs no more to plan than lettered
/// cells; past it, each expansion tries far more placements.
pub(super) const SMALL_OUTLINE_CELLS: u32 = 16;

impl BuildTarget {
    /// Outline cells still to fill from the start board.
    pub(super) fn open_outline_cells(&self) -> u32 {
        self.outline
            .iter()
            .zip(&self.start)
            .map(|(outline, start)| (outline & !start).count_ones())
            .sum()
    }

    /// The first stage with unfilled cells.
    pub(super) fn open_stage(&self, filled: &Masks) -> Option<usize> {
        self.stages.iter().position(|stage| {
            stage
                .goal
                .iter()
                .zip(filled)
                .any(|(goal, filled)| goal & !filled != 0)
        })
    }

    /// The earliest stage that adds one of these cells.
    pub(super) fn earliest_stage(&self, cells: &Cells) -> usize {
        cells
            .iter()
            .map(|[x, y]| self.stage_at(*x, *y))
            .min()
            .unwrap_or(0)
    }

    /// The latest stage that adds one of these cells.
    pub(super) fn latest_stage(&self, cells: &Cells) -> usize {
        cells
            .iter()
            .map(|[x, y]| self.stage_at(*x, *y))
            .max()
            .unwrap_or(0)
    }

    fn stage_at(&self, x: u8, y: u8) -> usize {
        usize::from(self.stage_of[usize::from(y) * 10 + usize::from(x)])
    }

    /// Whether some unfilled outline cell takes `letter`.
    pub(super) fn outline_accepts(&self, filled: &Masks, letter: u8) -> bool {
        (0..BOARD_HEIGHT).any(|row| {
            let open = self.outline[row] & !filled[row];
            (0..10).any(|x| {
                open & (1 << x) != 0 && {
                    let cell = self.letters[row * 10 + x];
                    cell == b'X' || cell == letter
                }
            })
        })
    }

    /// Whether every cell is an unfilled outline cell that takes `letter`.
    pub(super) fn outline_fits(&self, filled: &Masks, cells: &Cells, letter: u8) -> bool {
        cells.iter().all(|[x, y]| {
            let row = usize::from(*y);
            let bit = 1u16 << x;
            self.outline[row] & bit != 0 && filled[row] & bit == 0 && {
                let cell = self.letters[row * 10 + usize::from(*x)];
                cell == b'X' || cell == letter
            }
        })
    }

    /// Unfilled outline cells, counted in whole pieces.
    pub(super) fn outline_pieces(&self, filled: &Masks) -> u32 {
        self.outline
            .iter()
            .zip(filled)
            .map(|(outline, filled)| (outline & !filled).count_ones())
            .sum::<u32>()
            / 4
    }

    /// Letters of the start board, `_` elsewhere.
    pub(super) fn start_letters(&self) -> [u8; CELLS] {
        let mut letters = [b'_'; CELLS];
        for (row, mask) in self.start.iter().enumerate() {
            for x in 0..10 {
                if mask & (1 << x) != 0 {
                    letters[row * 10 + x] = self.letters[row * 10 + x];
                }
            }
        }
        letters
    }
}

pub(super) fn build_target(
    tree: &[OpenerTreeNode],
    mirrored: bool,
    start: Option<u32>,
    target: u32,
) -> Result<BuildTarget, TargetError> {
    let nodes = tree
        .iter()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    if let Some(start) = start {
        if !nodes.contains_key(&start) {
            return Err(TargetError::UnknownNode(start));
        }
        if start == target {
            return Err(TargetError::NothingToBuild);
        }
    }
    let mut chain = Vec::new();
    let mut current = Some(target);
    let mut reached_start = start.is_none();
    while let Some(id) = current {
        if Some(id) == start {
            reached_start = true;
            break;
        }
        let node = nodes
            .get(&id)
            .copied()
            .ok_or(TargetError::UnknownNode(id))?;
        chain.push(node);
        current = node.parent;
    }
    if !reached_start {
        return Err(TargetError::StartNotAncestor);
    }
    chain.reverse();

    let start_node = start.and_then(|id| nodes.get(&id).copied());
    let (mut goal, mut letters) = match start_node {
        Some(node) => frame(pre_clear_rows(node), mirrored),
        None => ([0; BOARD_HEIGHT], [b'_'; CELLS]),
    };
    let start_masks = goal;
    let mut outline = [0u16; BOARD_HEIGHT];
    let mut stage_of = [0u8; CELLS];
    let mut slots = Vec::new();
    let mut stages = Vec::new();
    let mut previous_map = (0..BOARD_HEIGHT).collect::<Vec<_>>();
    let mut parent = start_node;

    for child in chain {
        let before = goal;
        let first_slot = slots.len();
        let (child_masks, child_letters) = frame(pre_clear_rows(child), mirrored);
        let specs = placements_for(parent, child, mirrored);
        let (parent_masks, row_map) = attach(parent, mirrored, &goal, &previous_map)
            .into_iter()
            .find(|(parent_masks, _)| {
                extends(&child_masks, parent_masks)
                    && specs
                        .as_ref()
                        .is_none_or(|specs| specs_fit(specs, &child_masks, parent_masks))
            })
            .ok_or(TargetError::SourceMismatch(child.id))?;

        let mut new_cells = child_masks;
        for (row, mask) in new_cells.iter_mut().enumerate() {
            *mask &= !parent_masks[row];
        }
        let lift = |[x, y]: [u8; 2]| -> Result<[u8; 2], TargetError> {
            let row = row_map
                .get(usize::from(y))
                .copied()
                .ok_or(TargetError::TooTall(child.id))?;
            Ok([
                x,
                u8::try_from(row).map_err(|_| TargetError::TooTall(child.id))?,
            ])
        };
        let mut stage_pieces = 0u32;
        for spec in specs.iter().flatten() {
            let mut cells = [[0u8; 2]; 4];
            for (index, cell) in spec.cells.iter().enumerate() {
                cells[index] = lift(*cell)?;
                new_cells[usize::from(cell[1])] &= !(1 << cell[0]);
            }
            cells.sort_unstable();
            let piece =
                piece_for_letter(spec.letter).ok_or(TargetError::SourceMismatch(child.id))?;
            for [x, y] in cells {
                goal[usize::from(y)] |= 1 << x;
                stage_of[usize::from(y) * 10 + usize::from(x)] = stages.len() as u8;
                letters[usize::from(y) * 10 + usize::from(x)] = spec.letter;
            }
            slots.push(Slot {
                piece,
                letter: spec.letter,
                cells,
            });
            stage_pieces += 1;
        }
        let mut stage_outline = 0u32;
        for (row, mask) in new_cells.iter().enumerate() {
            for x in 0..10u8 {
                if mask & (1 << x) == 0 {
                    continue;
                }
                let [gx, gy] = lift([
                    x,
                    u8::try_from(row).map_err(|_| TargetError::TooTall(child.id))?,
                ])?;
                goal[usize::from(gy)] |= 1 << gx;
                stage_of[usize::from(gy) * 10 + usize::from(gx)] = stages.len() as u8;
                outline[usize::from(gy)] |= 1 << gx;
                let letter = child_letters[row * 10 + usize::from(x)];
                letters[usize::from(gy) * 10 + usize::from(gx)] =
                    if piece_for_letter(letter).is_some() {
                        letter
                    } else {
                        b'X'
                    };
                stage_outline += 1;
            }
        }
        if !stage_outline.is_multiple_of(4) {
            return Err(TargetError::SourceMismatch(child.id));
        }
        if physical_masks(&goal) != frame(&child.rows, mirrored).0 {
            return Err(TargetError::SourceMismatch(child.id));
        }
        let cleared = (0..BOARD_HEIGHT)
            .filter(|row| goal[*row] == FULL_ROW && before[*row] != FULL_ROW)
            .map(|row| row as u8)
            .collect::<Vec<_>>();
        let label = child
            .est
            .as_ref()
            .and_then(|est| est.clear.clone())
            .filter(|_| !cleared.is_empty());
        stages.push(Stage {
            node_id: child.id,
            pieces: stage_pieces + stage_outline / 4,
            goal,
            cleared,
            label,
            slots: first_slot..slots.len(),
            clear: None,
            relaxed: false,
        });
        previous_map = row_map;
        parent = Some(child);
    }
    if slots.len() > MAX_SLOTS {
        return Err(TargetError::TooManyPieces);
    }
    Ok(BuildTarget {
        stage_of,
        start: start_masks,
        goal,
        outline,
        letters,
        slots,
        stages,
    })
}

#[derive(Clone, Debug)]
struct PlacementSpec {
    letter: u8,
    cells: Cells,
}

/// The parent boards a child's diagram may extend, each with the map from the
/// child's rows to frame rows: the parent after its clears (child rows are the
/// frame's rows that are not full yet), or the parent before its clears (child
/// rows are the parent's own diagram rows).
fn attach(
    parent: Option<&OpenerTreeNode>,
    mirrored: bool,
    goal: &Masks,
    previous_map: &[usize],
) -> Vec<(Masks, Vec<usize>)> {
    match parent {
        None => vec![([0; BOARD_HEIGHT], (0..BOARD_HEIGHT).collect())],
        Some(parent) => {
            let post_map = (0..BOARD_HEIGHT)
                .filter(|row| goal[*row] != FULL_ROW)
                .collect::<Vec<_>>();
            vec![
                (frame(&parent.rows, mirrored).0, post_map),
                (
                    frame(pre_clear_rows(parent), mirrored).0,
                    previous_map.to_vec(),
                ),
            ]
        }
    }
}

fn extends(child: &Masks, parent: &Masks) -> bool {
    child
        .iter()
        .zip(parent)
        .all(|(child, parent)| parent & !child == 0)
}

fn specs_fit(specs: &[PlacementSpec], child: &Masks, parent: &Masks) -> bool {
    let mut seen = [0u16; BOARD_HEIGHT];
    specs.iter().all(|spec| {
        spec.cells.iter().all(|[x, y]| {
            let row = usize::from(*y);
            let bit = 1u16 << x;
            let fits = child[row] & bit != 0 && parent[row] & bit == 0 && seen[row] & bit == 0;
            seen[row] |= bit;
            fits
        })
    })
}

fn placements_for(
    parent: Option<&OpenerTreeNode>,
    node: &OpenerTreeNode,
    mirrored: bool,
) -> Option<Vec<PlacementSpec>> {
    let placements = match &node.placements {
        Some(placements) => placements
            .iter()
            .map(|placement| ShowcasePlacement {
                letter: placement.letter.clone(),
                cells: placement.cells.clone(),
            })
            .collect::<Vec<_>>(),
        None => {
            let parent_rows = parent.map_or(&[][..], |parent| parent.rows.as_slice());
            derive_placements(parent_rows, pre_clear_rows(node))?.placements
        }
    };
    placements
        .iter()
        .map(|placement| {
            let [letter] = placement.letter.as_bytes() else {
                return None;
            };
            let cells: Cells = placement.cells.clone().try_into().ok()?;
            Some(PlacementSpec {
                letter: if mirrored {
                    mirror_letter(*letter)
                } else {
                    *letter
                },
                cells: if mirrored {
                    cells.map(|[x, y]| [9 - x, y])
                } else {
                    cells
                },
            })
        })
        .collect()
}

fn pre_clear_rows(node: &OpenerTreeNode) -> &[String] {
    node.pre_clear_rows.as_deref().unwrap_or(&node.rows)
}

/// Floor-up masks and letters of top-down catalog rows.
fn frame(rows: &[String], mirrored: bool) -> (Masks, [u8; CELLS]) {
    let mut masks = [0u16; BOARD_HEIGHT];
    for (row, mask) in rows_to_masks_floor_up(rows)
        .into_iter()
        .enumerate()
        .take(BOARD_HEIGHT)
    {
        masks[row] = if mirrored { mirror_mask_10(mask) } else { mask };
    }
    let mut letters = [b'_'; CELLS];
    for (row, source) in rows.iter().rev().take(BOARD_HEIGHT).enumerate() {
        let source = if mirrored {
            mirror_letter_row(source)
        } else {
            source.clone()
        };
        for (column, letter) in source.bytes().take(10).enumerate() {
            letters[row * 10 + column] = letter;
        }
    }
    (masks, letters)
}
