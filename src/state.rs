// state.rs -- game state for search with queue support
// extends board::State with piece queue for beam search

use crate::board::Board;
use crate::header::Piece;

/// game state carrying everything the search needs
#[derive(Clone)]
pub struct GameState {
    pub board: Board,
    pub current: Piece,
    pub hold: Option<Piece>,
    pub queue: Vec<Piece>,
    pub b2b: bool,
    pub combo: u32,
}

impl GameState {
    pub fn new(board: Board, current: Piece, queue: Vec<Piece>) -> Self {
        Self {
            board,
            current,
            hold: None,
            queue,
            b2b: false,
            combo: 0,
        }
    }

    /// next piece from queue, or None if exhausted
    pub fn queue_piece(&self, index: usize) -> Option<Piece> {
        self.queue.get(index).copied()
    }

    /// how many pieces remain in queue
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gamestate_creation() {
        let board = Board::new();
        let state = GameState::new(board, Piece::T, vec![Piece::I, Piece::O, Piece::S]);
        assert_eq!(state.current, Piece::T);
        assert!(state.hold.is_none());
        assert_eq!(state.queue_len(), 3);
        assert_eq!(state.queue_piece(0), Some(Piece::I));
        assert_eq!(state.queue_piece(2), Some(Piece::S));
        assert_eq!(state.queue_piece(5), None);
        assert!(!state.b2b);
        assert_eq!(state.combo, 0);
    }
}
