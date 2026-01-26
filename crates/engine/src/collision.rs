//! Collision detection for piece placement.

use fusion_core::{Board, Piece, Rotation};

/// Check if a piece at given position collides with the board or walls.
pub fn collides(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> bool {
    let minos = piece.minos(rotation);
    for (dx, dy) in minos {
        let nx = x + dx;
        let ny = y + dy;

        // Check bounds
        if nx < 0 || nx >= Board::WIDTH as i8 || ny < 0 || ny >= Board::HEIGHT as i8 {
            return true;
        }

        // Check board collision
        if board.get(nx as usize, ny as usize) {
            return true;
        }
    }
    false
}

/// Check if piece can be placed (not colliding)
pub fn can_place(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> bool {
    !collides(board, piece, rotation, x, y)
}

/// Find the lowest Y position for a piece (hard drop destination)
pub fn hard_drop_y(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> i8 {
    let mut final_y = y;
    while !collides(board, piece, rotation, x, final_y - 1) {
        final_y -= 1;
    }
    final_y
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
        for x in 0..10 {
            board.set(x, 5, true);
        }
        let y = hard_drop_y(&board, Piece::T, Rotation::North, 4, 20);
        assert_eq!(y, 6); // Should land on row 6
    }
}
