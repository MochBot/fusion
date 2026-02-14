//! Rotation and movement logic with kick application.

use crate::collision::can_place;
use crate::kicks::get_kicks;
use fusion_core::{Board, Piece, Rotation, SpinType};

/// Result of a rotation attempt
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotationResult {
    pub new_rotation: Rotation,
    pub new_x: i8,
    pub new_y: i8,
    pub spin_type: SpinType,
    pub kick_index: usize,
}

/// Try to rotate a piece, applying kicks if necessary.
/// Returns None if rotation is not possible.
pub fn try_rotate(
    board: &Board,
    piece: Piece,
    rotation: Rotation,
    x: i8,
    y: i8,
    clockwise: bool,
) -> Option<RotationResult> {
    let new_rotation = if clockwise {
        rotation.cw()
    } else {
        rotation.ccw()
    };
    try_rotate_to(board, piece, rotation, new_rotation, x, y)
}

/// Try to rotate to a specific rotation state.
pub fn try_rotate_to(
    board: &Board,
    piece: Piece,
    from: Rotation,
    to: Rotation,
    x: i8,
    y: i8,
) -> Option<RotationResult> {
    // First try without kicks
    if can_place(board, piece, to, x, y) {
        let spin_type = detect_all_spin_with_kick(board, piece, x, y, to, false);
        return Some(RotationResult {
            new_rotation: to,
            new_x: x,
            new_y: y,
            spin_type,
            kick_index: 0,
        });
    }

    // Try each kick offset
    let kicks = get_kicks(piece, from, to);
    for (i, (dx, dy)) in kicks.iter().enumerate() {
        let nx = x + dx;
        let ny = y + dy;
        if can_place(board, piece, to, nx, ny) {
            let spin_type = detect_all_spin_with_kick(board, piece, nx, ny, to, true);
            return Some(RotationResult {
                new_rotation: to,
                new_x: nx,
                new_y: ny,
                spin_type,
                kick_index: i + 1, // +1 because index 0 is no-kick
            });
        }
    }

    None
}

/// Try 180 rotation (SRS+ feature)
pub fn try_rotate_180(
    board: &Board,
    piece: Piece,
    rotation: Rotation,
    x: i8,
    y: i8,
) -> Option<RotationResult> {
    let new_rotation = rotation.flip();
    try_rotate_to(board, piece, rotation, new_rotation, x, y)
}

/// Try to move piece horizontally
pub fn try_move(
    board: &Board,
    piece: Piece,
    rotation: Rotation,
    x: i8,
    y: i8,
    dx: i8,
) -> Option<i8> {
    let new_x = x + dx;
    if can_place(board, piece, rotation, new_x, y) {
        Some(new_x)
    } else {
        None
    }
}

/// Try to move piece down (soft drop)
pub fn try_drop(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> Option<i8> {
    let new_y = y - 1;
    if can_place(board, piece, rotation, x, new_y) {
        Some(new_y)
    } else {
        None
    }
}

/// Detect T-spin using 3-corner rule
fn detect_tspin(
    board: &Board,
    piece: Piece,
    rotation: Rotation,
    x: i8,
    y: i8,
    used_kick: bool,
) -> SpinType {
    if piece != Piece::T {
        return SpinType::None;
    }

    // T-spin detection: check 4 corners around T piece center
    let corners = [
        (x - 1, y + 1),
        (x + 1, y + 1),
        (x - 1, y - 1),
        (x + 1, y - 1),
    ];
    let mut filled = 0;
    let mut front_filled = 0;

    for (i, &(cx, cy)) in corners.iter().enumerate() {
        let is_filled = if cx < 0 || cx >= Board::WIDTH as i8 || cy < 0 || cy >= Board::HEIGHT as i8
        {
            true // Out of bounds counts as filled
        } else {
            board.get(cx as usize, cy as usize)
        };

        if is_filled {
            filled += 1;
            // Front corners depend on rotation
            let is_front = match rotation {
                Rotation::North => i < 2,           // Top corners
                Rotation::East => i == 1 || i == 3, // Right corners
                Rotation::South => i >= 2,          // Bottom corners
                Rotation::West => i == 0 || i == 2, // Left corners
            };
            if is_front {
                front_filled += 1;
            }
        }
    }

    if filled >= 3 {
        if front_filled >= 2 {
            SpinType::Full
        } else if used_kick {
            SpinType::Mini
        } else {
            SpinType::None
        }
    } else {
        SpinType::None
    }
}

pub(crate) fn detect_all_spin_with_kick(
    board: &Board,
    piece: Piece,
    x: i8,
    y: i8,
    rotation: Rotation,
    used_kick: bool,
) -> SpinType {
    if piece == Piece::T {
        let tspin = detect_tspin(board, piece, rotation, x, y, used_kick);
        if tspin != SpinType::None {
            return tspin;
        }
    }

    let can_left = can_place(board, piece, rotation, x - 1, y);
    let can_right = can_place(board, piece, rotation, x + 1, y);
    let can_down = can_place(board, piece, rotation, x, y - 1);

    if !can_left && !can_right && !can_down {
        SpinType::Mini
    } else {
        SpinType::None
    }
}

/// Detect if placement is an All-Mini+ spin (S2 Beta 1.5.0+)
/// All pieces can spin - non-T pieces use immobile detection
// All-Mini+: tetris.wiki/TETR.IO - Beta 1.5.0 (Jan 18, 2025)
pub fn detect_all_spin(board: &Board, piece: Piece, x: i8, y: i8, rotation: Rotation) -> SpinType {
    detect_all_spin_with_kick(board, piece, x, y, rotation, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caged_board(piece: Piece, rotation: Rotation, x: i8, y: i8) -> Board {
        let mut board = Board::new();
        for row in 0..Board::HEIGHT {
            for col in 0..Board::WIDTH {
                board.set(col, row, true);
            }
        }
        for (dx, dy) in piece.minos(rotation) {
            let nx = (x + dx) as usize;
            let ny = (y + dy) as usize;
            board.set(nx, ny, false);
        }
        board
    }

    fn assert_all_mini_spin(piece: Piece) {
        let rotation = Rotation::North;
        let x = 4;
        let y = 1;
        let board = caged_board(piece, rotation, x, y);
        assert!(can_place(&board, piece, rotation, x, y));
        let spin = detect_all_spin(&board, piece, x, y, rotation);
        assert_eq!(spin, SpinType::Mini);
    }

    #[test]
    fn test_simple_rotation() {
        let board = Board::new();
        let result = try_rotate(&board, Piece::T, Rotation::North, 4, 5, true);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.new_rotation, Rotation::East);
        assert_eq!(r.kick_index, 0); // No kick needed
    }

    #[test]
    fn test_wall_kick() {
        let board = Board::new();
        // T piece at x=0, rotating CW should need a kick
        let result = try_rotate(&board, Piece::T, Rotation::North, 0, 5, true);
        assert!(result.is_some());
    }

    #[test]
    fn test_move_left() {
        let board = Board::new();
        let new_x = try_move(&board, Piece::T, Rotation::North, 4, 0, -1);
        assert_eq!(new_x, Some(3));
    }

    #[test]
    fn test_move_blocked() {
        let board = Board::new();
        // T piece at x=0 can't move left
        let new_x = try_move(&board, Piece::T, Rotation::North, 0, 0, -1);
        assert_eq!(new_x, None);
    }

    #[test]
    fn test_180_rotation() {
        let board = Board::new();
        let result = try_rotate_180(&board, Piece::T, Rotation::North, 4, 5);
        assert!(result.is_some());
        assert_eq!(result.unwrap().new_rotation, Rotation::South);
    }

    #[test]
    fn test_all_mini_spins_non_t_pieces() {
        for piece in [Piece::I, Piece::O, Piece::S, Piece::Z, Piece::J, Piece::L] {
            assert_all_mini_spin(piece);
        }
    }
}
