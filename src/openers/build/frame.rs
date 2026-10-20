//! The planning frame: a tall board in which rows the build has cleared stay
//! in place, its cells, and the board and letters it stands for.

use crate::board::{Board, BOARD_HEIGHT, FULL_ROW};
use crate::header::Piece;

pub(super) const CELLS: usize = BOARD_HEIGHT * 10;

/// Occupancy per frame row, floor up. A full row has been cleared.
pub(super) type Masks = [u16; BOARD_HEIGHT];
/// The four `[column, row]` cells of one piece.
pub(super) type Cells = [[u8; 2]; 4];

pub(super) const PIECES: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::T,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
];

pub(super) fn fill(filled: &Masks, cells: &Cells) -> Masks {
    let mut next = *filled;
    for [x, y] in cells {
        next[usize::from(*y)] |= 1 << x;
    }
    next
}

pub(super) fn is_filled(filled: &Masks, cells: &Cells) -> bool {
    cells
        .iter()
        .any(|[x, y]| filled[usize::from(*y)] & (1 << x) != 0)
}

/// Frame rows that placing `cells` on `filled` makes full.
pub(super) fn completed_rows(filled: &Masks, cells: &Cells) -> Vec<u8> {
    let after = fill(filled, cells);
    (0..BOARD_HEIGHT)
        .filter(|row| filled[*row] != FULL_ROW && after[*row] == FULL_ROW)
        .map(|row| row as u8)
        .collect()
}

/// The board a frame occupancy stands for: its full rows are cleared.
pub(super) fn physical_masks(frame: &Masks) -> Masks {
    let mut physical = [0u16; BOARD_HEIGHT];
    for (write, mask) in frame
        .iter()
        .copied()
        .filter(|mask| *mask != FULL_ROW)
        .enumerate()
    {
        physical[write] = mask;
    }
    physical
}

pub(super) fn board_of(filled: &Masks) -> Board {
    Board::from_rows(physical_masks(filled))
}

/// Frame cells to board cells: every full row below a cell has cleared.
pub(super) fn to_physical(filled: &Masks, cells: &Cells) -> Cells {
    cells.map(|[x, y]| {
        let cleared = filled[..usize::from(y)]
            .iter()
            .filter(|mask| **mask == FULL_ROW)
            .count();
        [x, y - cleared as u8]
    })
}

/// Board cells to frame cells: board row k is the k-th frame row not full.
pub(super) fn to_frame(filled: &Masks, cells: &Cells) -> Option<Cells> {
    // The frame rows not yet cleared, bottom up; on the stack, as this runs
    // for every candidate the planner weighs.
    let mut rows = [0u8; BOARD_HEIGHT];
    let mut open = 0;
    for (row, mask) in filled.iter().enumerate() {
        if *mask != FULL_ROW {
            rows[open] = u8::try_from(row).ok()?;
            open += 1;
        }
    }
    let rows = &rows[..open];
    let mut frame = [[0u8; 2]; 4];
    for (index, [x, y]) in cells.iter().enumerate() {
        frame[index] = [*x, *rows.get(usize::from(*y))?];
    }
    frame.sort_unstable();
    Some(frame)
}

/// Floor-up letter rows of the board a frame stands for, without empty rows
/// on top.
pub(super) fn board_rows(filled: &Masks, letters: &[u8; CELLS]) -> Vec<String> {
    let mut rows = Vec::new();
    for (row, mask) in filled.iter().enumerate() {
        if *mask == FULL_ROW {
            continue;
        }
        rows.push(
            (0..10)
                .map(|x| {
                    if mask & (1 << x) == 0 {
                        '_'
                    } else {
                        char::from(letters[row * 10 + x])
                    }
                })
                .collect::<String>(),
        );
    }
    while rows
        .last()
        .is_some_and(|row| row.bytes().all(|cell| cell == b'_'))
    {
        rows.pop();
    }
    rows
}

pub(super) fn piece_for_letter(letter: u8) -> Option<Piece> {
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

pub(super) const fn letter_for_piece(piece: Piece) -> u8 {
    match piece {
        Piece::I => b'I',
        Piece::O => b'O',
        Piece::T => b'T',
        Piece::L => b'L',
        Piece::J => b'J',
        Piece::S => b'S',
        Piece::Z => b'Z',
    }
}

pub(super) fn letter_string(piece: Piece) -> String {
    char::from(letter_for_piece(piece)).to_string()
}

/// The letter a piece becomes when the board is mirrored.
pub(super) fn mirror_letter(letter: u8) -> u8 {
    match letter {
        b'L' => b'J',
        b'J' => b'L',
        b'S' => b'Z',
        b'Z' => b'S',
        other => other,
    }
}
