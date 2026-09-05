use crate::board::{Board, LockMechanics, BOARD_HEIGHT, FULL_ROW};
use crate::header::{Move, Piece};
use crate::move_buffer::MoveBuffer;
use crate::pathfinder::get_input;
use crate::smear_core::generate_placements;

pub(crate) enum LegalityVerdict {
    Legal {
        target: Move,
        mechanics: LockMechanics,
    },
    UnreachableFromSpawn,
    Unsupported,
    NotAPlacement,
}

pub(crate) struct LegalityCheck {
    pub(crate) support_valid: bool,
    #[cfg(test)]
    pub(crate) geometry_emitted: bool,
    #[cfg(test)]
    pub(crate) label_free_reachable: bool,
    #[cfg(test)]
    pub(crate) spin_labelled_reachable: bool,
    pub(crate) verdict: LegalityVerdict,
}

pub(crate) fn placement_is_srs_legal(
    board: &Board,
    letter: u8,
    cells: &[[u8; 2]; 4],
) -> LegalityCheck {
    let Some(piece) = piece_for_letter(letter) else {
        return LegalityCheck {
            support_valid: false,
            #[cfg(test)]
            geometry_emitted: false,
            #[cfg(test)]
            label_free_reachable: false,
            #[cfg(test)]
            spin_labelled_reachable: false,
            verdict: LegalityVerdict::NotAPlacement,
        };
    };
    let expected = sorted_cells(*cells);
    let mut moves = MoveBuffer::new();
    generate_placements(board, &mut moves, piece, false);
    let candidates = moves
        .iter()
        .copied()
        .filter(|candidate| placement_cells(*candidate) == Some(expected))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return LegalityCheck {
            support_valid: false,
            #[cfg(test)]
            geometry_emitted: false,
            #[cfg(test)]
            label_free_reachable: false,
            #[cfg(test)]
            spin_labelled_reachable: false,
            verdict: LegalityVerdict::NotAPlacement,
        };
    }
    let geometry_emitted = true;
    let mut support_valid = false;
    let mut label_free_reachable = false;
    let mut spin_labelled_reachable = false;

    for candidate in candidates {
        if !board.legal_lock_placement(&candidate) {
            continue;
        }
        support_valid = true;
        let bare = Move::new(
            piece,
            candidate.rotation(),
            candidate.x(),
            candidate.y(),
            false,
        );
        if get_input(board, &bare, false, false).size() > 0 {
            label_free_reachable = true;
            let mechanics = lock_mechanics(board, bare);
            return legal_check(
                bare,
                mechanics,
                geometry_emitted,
                label_free_reachable,
                spin_labelled_reachable,
            );
        }
        for spin_target in spin_targets(piece, bare) {
            if get_input(board, &spin_target, false, false).size() == 0 {
                continue;
            }
            spin_labelled_reachable = true;
            let mechanics = lock_mechanics(board, spin_target);
            return legal_check(
                spin_target,
                mechanics,
                geometry_emitted,
                label_free_reachable,
                spin_labelled_reachable,
            );
        }
    }

    LegalityCheck {
        support_valid,
        #[cfg(test)]
        geometry_emitted,
        #[cfg(test)]
        label_free_reachable,
        #[cfg(test)]
        spin_labelled_reachable,
        verdict: if support_valid {
            LegalityVerdict::UnreachableFromSpawn
        } else {
            LegalityVerdict::Unsupported
        },
    }
}

pub(crate) fn engine_board_from_masks(masks: &[u16]) -> Board {
    let mut board = Board::new();
    for (y, mask) in masks.iter().copied().enumerate().take(BOARD_HEIGHT) {
        board.rows[y] = mask & FULL_ROW;
        for x in 0..10 {
            if board.rows[y] & (1 << x) != 0 {
                board.cols[x] |= 1u64 << y;
            }
        }
    }
    board
}

pub(super) fn placement_cells(target: Move) -> Option<[[u8; 2]; 4]> {
    let offsets = target.cells();
    let coordinates = [
        (target.x(), target.y()),
        (
            target.x() + i32::from(offsets[0].x),
            target.y() + i32::from(offsets[0].y),
        ),
        (
            target.x() + i32::from(offsets[1].x),
            target.y() + i32::from(offsets[1].y),
        ),
        (
            target.x() + i32::from(offsets[2].x),
            target.y() + i32::from(offsets[2].y),
        ),
    ];
    let mut cells = [[0; 2]; 4];
    for (index, (x, y)) in coordinates.into_iter().enumerate() {
        cells[index] = [u8::try_from(x).ok()?, u8::try_from(y).ok()?];
    }
    Some(sorted_cells(cells))
}

fn piece_for_letter(letter: u8) -> Option<Piece> {
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

fn sorted_cells(mut cells: [[u8; 2]; 4]) -> [[u8; 2]; 4] {
    cells.sort_unstable();
    cells
}

fn spin_targets(piece: Piece, bare: Move) -> Vec<Move> {
    match piece {
        Piece::T => vec![
            Move::new_tspin(bare.rotation(), bare.x(), bare.y(), false),
            Move::new_tspin(bare.rotation(), bare.x(), bare.y(), true),
        ],
        Piece::O => Vec::new(),
        _ => vec![Move::new_allspin_mini(
            piece,
            bare.rotation(),
            bare.x(),
            bare.y(),
        )],
    }
}

fn lock_mechanics(board: &Board, target: Move) -> LockMechanics {
    let mut locked = board.clone();
    locked.lock(&target)
}

fn legal_check(
    target: Move,
    mechanics: LockMechanics,
    _geometry_emitted: bool,
    _label_free_reachable: bool,
    _spin_labelled_reachable: bool,
) -> LegalityCheck {
    LegalityCheck {
        support_valid: true,
        #[cfg(test)]
        geometry_emitted: _geometry_emitted,
        #[cfg(test)]
        label_free_reachable: _label_free_reachable,
        #[cfg(test)]
        spin_labelled_reachable: _spin_labelled_reachable,
        verdict: LegalityVerdict::Legal { target, mechanics },
    }
}
