//! Legal move generation for a given game state.

use std::collections::VecDeque;

use crate::collision::{can_place, hard_drop_y};
use crate::movement::{detect_all_spin, try_drop, try_move, try_rotate, try_rotate_180};
use fusion_core::{Board, Move, Piece, Rotation};

/// Generate all legal placements for a piece on the board.
/// Uses BFS over movement/rotation to find reachable positions.
pub fn generate_moves(board: &Board, piece: Piece) -> Vec<Move> {
    let mut moves = Vec::new();
    let mut visited = [[[false; 44]; 14]; 4];
    let spawn_x = piece.spawn_x();
    let spawn_y = piece.spawn_y();
    let spawn_rotation = Rotation::North;

    if !can_place(board, piece, spawn_rotation, spawn_x, spawn_y) {
        return moves;
    }

    let mut queue = VecDeque::new();
    visit_state(&mut visited, &mut queue, spawn_rotation, spawn_x, spawn_y);

    while let Some(state) = queue.pop_front() {
        let drop_y = hard_drop_y(board, piece, state.rotation, state.x, state.y);
        let spin_type = detect_all_spin(board, piece, state.x, drop_y, state.rotation);
        moves.push(Move {
            piece,
            rotation: state.rotation,
            x: state.x,
            y: drop_y,
            hold_used: false,
            spin_type,
        });

        if let Some(nx) = try_move(board, piece, state.rotation, state.x, state.y, -1) {
            visit_state(&mut visited, &mut queue, state.rotation, nx, state.y);
        }
        if let Some(nx) = try_move(board, piece, state.rotation, state.x, state.y, 1) {
            visit_state(&mut visited, &mut queue, state.rotation, nx, state.y);
        }
        if let Some(ny) = try_drop(board, piece, state.rotation, state.x, state.y) {
            visit_state(&mut visited, &mut queue, state.rotation, state.x, ny);
        }
        if let Some(result) = try_rotate(board, piece, state.rotation, state.x, state.y, true) {
            visit_state(
                &mut visited,
                &mut queue,
                result.new_rotation,
                result.new_x,
                result.new_y,
            );
        }
        if let Some(result) = try_rotate(board, piece, state.rotation, state.x, state.y, false) {
            visit_state(
                &mut visited,
                &mut queue,
                result.new_rotation,
                result.new_x,
                result.new_y,
            );
        }
        if let Some(result) = try_rotate_180(board, piece, state.rotation, state.x, state.y) {
            visit_state(
                &mut visited,
                &mut queue,
                result.new_rotation,
                result.new_x,
                result.new_y,
            );
        }
    }

    // Deduplicate (same final position + rotation)
    moves.sort_by(|a, b| (a.x, a.y, a.rotation as u8).cmp(&(b.x, b.y, b.rotation as u8)));
    moves.dedup_by(|a, b| a.x == b.x && a.y == b.y && a.rotation == b.rotation);

    moves
}

#[derive(Clone, Copy, Debug)]
struct MoveState {
    rotation: Rotation,
    x: i8,
    y: i8,
}

fn visit_state(
    visited: &mut [[[bool; 44]; 14]; 4],
    queue: &mut VecDeque<MoveState>,
    rotation: Rotation,
    x: i8,
    y: i8,
) {
    if y < 0 || y >= 44 {
        return;
    }
    let xi = x + 2;
    if xi < 0 || xi >= 14 {
        return;
    }
    let rot_idx = rotation as usize;
    let x_idx = xi as usize;
    let y_idx = y as usize;
    if !visited[rot_idx][x_idx][y_idx] {
        visited[rot_idx][x_idx][y_idx] = true;
        queue.push_back(MoveState { rotation, x, y });
    }
}

/// Generate moves including hold piece option.
pub fn generate_moves_with_hold(
    board: &Board,
    current: Piece,
    hold: Option<Piece>,
    queue: &[Piece],
) -> Vec<Move> {
    let mut moves = generate_moves(board, current);

    // Add hold moves
    if let Some(hold_piece) = hold {
        for mut m in generate_moves(board, hold_piece) {
            m.hold_used = true;
            moves.push(m);
        }
    } else if let Some(&next_piece) = queue.first() {
        for mut m in generate_moves(board, next_piece) {
            m.hold_used = true;
            moves.push(m);
        }
    }

    moves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_moves_empty_board() {
        let board = Board::new();
        let moves = generate_moves(&board, Piece::T);
        // Should have multiple valid placements
        assert!(!moves.is_empty());
        // All moves should be at y=0 (floor)
        for m in &moves {
            assert!(m.y <= 1); // T piece can be at y=0 or y=1 depending on rotation
        }
    }

    #[test]
    fn test_all_rotations_present() {
        let board = Board::new();
        let moves = generate_moves(&board, Piece::T);

        let has_north = moves.iter().any(|m| m.rotation == Rotation::North);
        let has_east = moves.iter().any(|m| m.rotation == Rotation::East);
        let has_south = moves.iter().any(|m| m.rotation == Rotation::South);
        let has_west = moves.iter().any(|m| m.rotation == Rotation::West);

        assert!(has_north && has_east && has_south && has_west);
    }

    #[test]
    fn test_i_piece_placements() {
        let board = Board::new();
        let moves = generate_moves(&board, Piece::I);
        // I piece should have horizontal and vertical placements
        assert!(!moves.is_empty());
    }

    #[test]
    fn test_generate_with_hold() {
        let board = Board::new();
        let moves = generate_moves_with_hold(&board, Piece::T, Some(Piece::I), &[]);

        let t_moves = moves
            .iter()
            .filter(|m| m.piece == Piece::T && !m.hold_used)
            .count();
        let i_moves = moves
            .iter()
            .filter(|m| m.piece == Piece::I && m.hold_used)
            .count();

        assert!(t_moves > 0);
        assert!(i_moves > 0);
    }
}
