//! Fusion search crate - beam search and lookahead for move selection.

mod beam;
mod lookahead;

pub use beam::BeamSearch;
pub use lookahead::LookaheadSearch;

use fusion_core::{Board, Move};

/// Apply a move to a board and return the resulting board and lines cleared.
pub fn apply_move(board: &Board, mv: &Move) -> (Board, u8) {
    let mut next = board.clone();

    for (dx, dy) in mv.piece.minos(mv.rotation) {
        let x = mv.x + dx;
        let y = mv.y + dy;
        if x >= 0 && y >= 0 && x < Board::WIDTH as i8 && y < Board::HEIGHT as i8 {
            next.set(x as usize, y as usize, true);
        }
    }

    let lines = next.clear_lines();
    (next, lines)
}
