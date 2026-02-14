//! collision detection - walls, floor, other blocks

use fusion_core::{Board, Piece, Rotation};

use crate::collision_specialized::collides_specialized;

/// does piece collide with anything?
/// Uses macro-generated specialized functions for each piece/rotation combo
#[inline(always)]
pub fn collides(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> bool {
    collides_specialized(board, piece, rotation, x, y)
}

/// can we place here? (just !collides)
pub fn can_place(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> bool {
    !collides(board, piece, rotation, x, y)
}

#[inline]
pub fn hard_drop_y(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> i8 {
    let mut landing_y = y;
    while !collides(board, piece, rotation, x, landing_y - 1) {
        landing_y -= 1;
    }
    landing_y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_collision_empty_board() {
        let board = Board::new();
        assert!(!collides(&board, Piece::T, Rotation::North, 4, 1));
    }

    #[test]
    fn test_collision_with_wall() {
        let board = Board::new();
        // T piece at x=-1 should collide with left wall
        assert!(collides(&board, Piece::T, Rotation::North, -1, 1));
    }

    #[test]
    fn test_collision_with_floor() {
        let board = Board::new();
        // T piece North at y=0 has minos at y=0 and y=1, should be valid
        assert!(!collides(&board, Piece::T, Rotation::North, 4, 0));
        // T piece South at y=0 has mino at y=-1, should collide
        assert!(collides(&board, Piece::T, Rotation::South, 4, 0));
    }

    #[test]
    fn test_collision_with_filled_cell() {
        let mut board = Board::new();
        board.set(4, 0, true);
        // T piece center at (4, 0) should collide
        assert!(collides(&board, Piece::T, Rotation::North, 4, 0));
    }

    #[test]
    fn test_hard_drop() {
        let board = Board::new();
        let y = hard_drop_y(&board, Piece::T, Rotation::North, 4, 20);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_hard_drop_with_obstacle() {
        let mut board = Board::new();
        // Fill row 5
        for x in 0..Board::WIDTH {
            board.set(x, 5, true);
        }
        let y = hard_drop_y(&board, Piece::T, Rotation::North, 4, 20);
        assert_eq!(y, 6); // Should land on row 6
    }
}
