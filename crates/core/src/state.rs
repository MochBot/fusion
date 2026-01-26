//! Game state representation.

use crate::{Board, Piece};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct GameState {
    pub board: Board,
    pub current_piece: Option<Piece>,
    pub hold: Option<Piece>,
    pub hold_used_this_turn: bool,
    pub queue: Vec<Piece>,
    pub b2b_level: u32,
    pub combo: u32,
    pub pieces_placed: u32,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            board: Board::new(),
            current_piece: None,
            hold: None,
            hold_used_this_turn: false,
            queue: Vec::new(),
            b2b_level: 0,
            combo: 0,
            pieces_placed: 0,
        }
    }
}

impl GameState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_queue(queue: Vec<Piece>) -> Self {
        let current = queue.first().copied();
        Self {
            queue: if queue.is_empty() {
                queue
            } else {
                queue[1..].to_vec()
            },
            current_piece: current,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let s = GameState::new();
        assert_eq!(s.b2b_level, 0);
        assert_eq!(s.combo, 0);
        assert!(s.current_piece.is_none());
        assert!(s.hold.is_none());
    }

    #[test]
    fn test_clone_equality() {
        let s1 = GameState::new();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_with_queue() {
        let s = GameState::with_queue(vec![Piece::T, Piece::I, Piece::O]);
        assert_eq!(s.current_piece, Some(Piece::T));
        assert_eq!(s.queue.len(), 2);
    }
}
