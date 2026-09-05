use crate::board::{Board, BOARD_HEIGHT, FULL_ROW};
use crate::openers::board::{mirror_letter_row, mirror_mask_10, rows_to_masks_floor_up};
use crate::openers::catalog::OpenerTreeNode;

use super::compile::PlacementSpec;
use super::legality::engine_board_from_masks;

#[derive(Clone)]
pub(super) struct DeclaredFrame {
    masks: [u16; BOARD_HEIGHT],
    letters: [u8; BOARD_HEIGHT * 10],
}

#[derive(Clone, Copy)]
pub(super) struct PhysicalPlacement {
    pub(super) cells: [[u8; 2]; 4],
    pub(super) shifted: bool,
}

#[derive(Debug)]
pub(super) enum FrameError {
    PlacementCellOutsideBoard,
    PlacementCellAbsentFromDeclaredFrame,
    DeclaredStartDetached,
    PhysicalStartMismatch,
    DeclaredEndpointMismatch,
    ClearRowsMismatch,
    PostClearRowsMismatch,
}

impl FrameError {
    pub(super) const fn reason(&self) -> &'static str {
        match self {
            Self::PlacementCellOutsideBoard => "placement cell outside declared board",
            Self::PlacementCellAbsentFromDeclaredFrame => {
                "placement cell absent from declared child frame"
            }
            Self::DeclaredStartDetached => "declared start does not attach to parent frame",
            Self::PhysicalStartMismatch => {
                "declared start physical shadow differs from parent state"
            }
            Self::DeclaredEndpointMismatch => {
                "declared endpoint differs from child pre-clear frame"
            }
            Self::ClearRowsMismatch => "declared full rows differ from child clearRows",
            Self::PostClearRowsMismatch => {
                "child post-clear rows differ from declared physical shadow"
            }
        }
    }
}

impl DeclaredFrame {
    pub(super) fn for_node(node: &OpenerTreeNode, mirrored: bool) -> Self {
        Self {
            masks: masks_for_node_pre_clear(node, mirrored),
            letters: letters_for_node_pre_clear(node, mirrored),
        }
    }

    pub(super) fn start(
        node: &OpenerTreeNode,
        parent: Option<&OpenerTreeNode>,
        placements: &[PlacementSpec],
        mirrored: bool,
    ) -> Result<Self, FrameError> {
        let mut masks = masks_for_node_pre_clear(node, mirrored);
        let mut letters = letters_for_node_pre_clear(node, mirrored);
        for placement in placements {
            for [x, y] in placement.cells {
                let Some(mask) = masks.get_mut(usize::from(y)) else {
                    return Err(FrameError::PlacementCellOutsideBoard);
                };
                if x >= 10 {
                    return Err(FrameError::PlacementCellOutsideBoard);
                }
                let bit = 1u16 << x;
                if *mask & bit == 0 {
                    return Err(FrameError::PlacementCellAbsentFromDeclaredFrame);
                }
                *mask &= !bit;
                letters[usize::from(y) * 10 + usize::from(x)] = b'_';
            }
        }
        let start = Self { masks, letters };
        if !start.attaches_to(parent, mirrored) {
            return Err(FrameError::DeclaredStartDetached);
        }
        Ok(start)
    }

    pub(super) fn physical_board(&self) -> Board {
        engine_board_from_masks(&self.physical_masks())
    }

    pub(super) fn physical_letters(&self) -> [u8; BOARD_HEIGHT * 10] {
        let mut physical = [b'_'; BOARD_HEIGHT * 10];
        let mut write = 0;
        for (row, mask) in self.masks.iter().copied().enumerate() {
            if mask == FULL_ROW {
                continue;
            }
            physical[write * 10..(write + 1) * 10]
                .copy_from_slice(&self.letters[row * 10..(row + 1) * 10]);
            write += 1;
        }
        physical
    }

    pub(super) fn physical_placement(
        &self,
        placement: &PlacementSpec,
    ) -> Result<PhysicalPlacement, FrameError> {
        let mut cells = [[0; 2]; 4];
        let mut shifted = false;
        for (index, [x, y]) in placement.cells.into_iter().enumerate() {
            if x >= 10 || usize::from(y) >= BOARD_HEIGHT {
                return Err(FrameError::PlacementCellOutsideBoard);
            }
            let cleared_below = self.masks[..usize::from(y)]
                .iter()
                .filter(|mask| **mask == FULL_ROW)
                .count();
            let cleared_below = match u8::try_from(cleared_below) {
                Ok(cleared_below) => cleared_below,
                Err(_) => return Err(FrameError::PlacementCellOutsideBoard),
            };
            let Some(physical_y) = y.checked_sub(cleared_below) else {
                return Err(FrameError::PlacementCellOutsideBoard);
            };
            shifted |= physical_y != y;
            cells[index] = [x, physical_y];
        }
        Ok(PhysicalPlacement { cells, shifted })
    }

    pub(super) fn merge(&mut self, placement: &PlacementSpec) -> Result<(), FrameError> {
        for [x, y] in placement.cells {
            let Some(mask) = self.masks.get_mut(usize::from(y)) else {
                return Err(FrameError::PlacementCellOutsideBoard);
            };
            if x >= 10 {
                return Err(FrameError::PlacementCellOutsideBoard);
            }
            *mask |= 1u16 << x;
            self.letters[usize::from(y) * 10 + usize::from(x)] = placement.letter;
        }
        Ok(())
    }

    pub(super) fn validate_start_board(&self, board: &Board) -> Result<(), FrameError> {
        if self.physical_board().rows == board.rows {
            Ok(())
        } else {
            Err(FrameError::PhysicalStartMismatch)
        }
    }

    pub(super) fn validate_endpoint(
        &self,
        node: &OpenerTreeNode,
        mirrored: bool,
    ) -> Result<(), FrameError> {
        if self.masks != masks_for_node_pre_clear(node, mirrored) {
            return Err(FrameError::DeclaredEndpointMismatch);
        }
        let declared_clears = self
            .masks
            .iter()
            .enumerate()
            .filter_map(|(row, mask)| (*mask == FULL_ROW).then_some(row))
            .filter_map(|row| u32::try_from(row).ok())
            .collect::<Vec<_>>();
        let mut expected_clears = node.clear_rows.clone().unwrap_or_default();
        expected_clears.sort_unstable();
        expected_clears.dedup();
        if declared_clears != expected_clears {
            return Err(FrameError::ClearRowsMismatch);
        }
        if self.physical_board().rows == masks_to_board_rows(&node.rows, mirrored).rows {
            Ok(())
        } else {
            Err(FrameError::PostClearRowsMismatch)
        }
    }

    fn attaches_to(&self, parent: Option<&OpenerTreeNode>, mirrored: bool) -> bool {
        match parent {
            Some(parent) => {
                self.masks == masks_for_rows(&parent.rows, mirrored)
                    || self.masks == masks_for_node_pre_clear(parent, mirrored)
            }
            None => self.physical_board().rows == Board::new().rows,
        }
    }

    fn physical_masks(&self) -> [u16; BOARD_HEIGHT] {
        let mut physical = [0; BOARD_HEIGHT];
        let mut write = 0;
        for mask in self.masks {
            if mask != FULL_ROW {
                physical[write] = mask;
                write += 1;
            }
        }
        physical
    }
}

fn masks_for_node_pre_clear(node: &OpenerTreeNode, mirrored: bool) -> [u16; BOARD_HEIGHT] {
    masks_for_rows(
        node.pre_clear_rows.as_deref().unwrap_or(&node.rows),
        mirrored,
    )
}

fn letters_for_node_pre_clear(node: &OpenerTreeNode, mirrored: bool) -> [u8; BOARD_HEIGHT * 10] {
    letters_for_rows(
        node.pre_clear_rows.as_deref().unwrap_or(&node.rows),
        mirrored,
    )
}

fn masks_to_board_rows(rows: &[String], mirrored: bool) -> Board {
    engine_board_from_masks(&masks_for_rows(rows, mirrored))
}

fn masks_for_rows(rows: &[String], mirrored: bool) -> [u16; BOARD_HEIGHT] {
    let mut masks = [0; BOARD_HEIGHT];
    for (row, mask) in rows_to_masks_floor_up(rows)
        .into_iter()
        .enumerate()
        .take(BOARD_HEIGHT)
    {
        masks[row] = if mirrored { mirror_mask_10(mask) } else { mask };
    }
    masks
}

fn letters_for_rows(rows: &[String], mirrored: bool) -> [u8; BOARD_HEIGHT * 10] {
    let mut letters = [b'_'; BOARD_HEIGHT * 10];
    for (row, source) in rows.iter().rev().take(BOARD_HEIGHT).enumerate() {
        let source = if mirrored {
            mirror_letter_row(source)
        } else {
            source.clone()
        };
        for (column, letter) in source.as_bytes().iter().copied().take(10).enumerate() {
            letters[row * 10 + column] = letter;
        }
    }
    letters
}
