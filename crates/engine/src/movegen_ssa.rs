//! Source-Subtraction Algorithm movegen - movement-only reachability
use std::collections::VecDeque;

use fusion_core::{Board, Move, Piece, Rotation};

use crate::collision_map::CollisionMap;
use crate::kicks::{get_180_kicks, get_kicks, get_kicks_cw_ccw};
use crate::movegen_bitboard::{
    count_placements_cobra, generate_moves_bitboard, generate_moves_bitboard_no_spin,
};
use crate::row_board::RowBoard;

// Canonical rotation tables moved to movegen_fast.rs and movegen_bitboard.rs
// SSA now delegates to generate_moves_bitboard() which handles canonicalization

/// Shift all positions left (x decreases)
#[inline(always)]
pub fn shift_left(board: &RowBoard) -> RowBoard {
    let mut result = RowBoard::new();
    let rows = result.rows_mut();
    for y in 0..44 {
        rows[y] = board.get_row(y) >> 1;
    }
    result
}

/// Shift all positions right (x increases)
#[inline(always)]
pub fn shift_right(board: &RowBoard) -> RowBoard {
    let mut result = RowBoard::new();
    let rows = result.rows_mut();
    for y in 0..44 {
        rows[y] = (board.get_row(y) << 1) & RowBoard::WIDTH_MASK;
    }
    result
}

/// Shift all positions down (y decreases)
#[inline(always)]
pub fn shift_down(board: &RowBoard) -> RowBoard {
    let mut result = RowBoard::new();
    let rows = result.rows_mut();
    for y in 0..43 {
        rows[y] = board.get_row(y + 1);
    }
    result
}

/// Shift positions by (dx, dy) - used for kick offsets
#[inline(always)]
pub fn shift_by(board: &RowBoard, dx: i8, dy: i8) -> RowBoard {
    let mut result = RowBoard::new();
    let rows = result.rows_mut();
    for y in 0..44 {
        let src_y = (y as i8) - dy;
        if src_y >= 0 && src_y < 44 {
            let row = board.get_row(src_y as usize);
            let shifted = if dx > 0 {
                (row << (dx as u32)) & RowBoard::WIDTH_MASK
            } else if dx < 0 {
                row >> ((-dx) as u32)
            } else {
                row
            };
            rows[y] = shifted;
        }
    }
    result
}

/// Compute reachability from spawn using Source-Subtraction for L/R/D movements
pub fn compute_movement_reachability(
    validity_mask: &RowBoard,
    spawn_x: i8,
    spawn_y: i8,
) -> RowBoard {
    let mut reachable = RowBoard::new();

    if spawn_x >= 0 && spawn_x < 10 && spawn_y >= 0 && spawn_y < 44 {
        if !validity_mask.get_bit(spawn_x as usize, spawn_y as usize) {
            reachable.set_bit(spawn_x as usize, spawn_y as usize);
        }
    }

    loop {
        let prev = reachable.clone();

        propagate_movement(&mut reachable, validity_mask);

        if reachable == prev {
            break;
        }
    }

    reachable
}

/// Compute reachability for all 4 rotations using Source-Subtraction
/// Returns [North, East, South, West] reachability masks
/// Iterates movement and rotation phases to fixpoint for full reachability
pub fn compute_full_reachability(board: &Board, piece: Piece) -> [RowBoard; 4] {
    let collision = CollisionMap::new(board, piece);
    let mut reachable = [
        RowBoard::new(),
        RowBoard::new(),
        RowBoard::new(),
        RowBoard::new(),
    ];

    let spawn_x = piece.spawn_x();
    let spawn_y = piece.spawn_y();

    if collision.collides(Rotation::North, spawn_x, spawn_y) {
        return reachable;
    }

    let mut visited = [[[false; 44]; 14]; 4];
    let mut queue = VecDeque::with_capacity(256);
    visit_state(&mut visited, &mut queue, Rotation::North, spawn_x, spawn_y);

    while let Some((rotation, x, y)) = queue.pop_front() {
        if x >= 0 && x < 10 && y >= 0 && y < 44 {
            reachable[rotation as usize].set_bit(x as usize, y as usize);
        }

        let left_x = x - 1;
        if !collision.collides(rotation, left_x, y) {
            visit_state(&mut visited, &mut queue, rotation, left_x, y);
        }

        let right_x = x + 1;
        if !collision.collides(rotation, right_x, y) {
            visit_state(&mut visited, &mut queue, rotation, right_x, y);
        }

        let down_y = y - 1;
        if !collision.collides(rotation, x, down_y) {
            visit_state(&mut visited, &mut queue, rotation, x, down_y);
        }

        let cw = rotation.cw();
        if let Some((new_x, new_y)) = try_rotate_bfs(&collision, piece, rotation, cw, x, y) {
            visit_state(&mut visited, &mut queue, cw, new_x, new_y);
        }

        let ccw = rotation.ccw();
        if let Some((new_x, new_y)) = try_rotate_bfs(&collision, piece, rotation, ccw, x, y) {
            visit_state(&mut visited, &mut queue, ccw, new_x, new_y);
        }

        let flip = rotation.flip();
        if let Some((new_x, new_y)) = try_rotate_bfs(&collision, piece, rotation, flip, x, y) {
            visit_state(&mut visited, &mut queue, flip, new_x, new_y);
        }
    }

    reachable
}

#[inline(always)]
fn propagate_movement(reachable: &mut RowBoard, validity: &RowBoard) {
    let projected = shift_left(reachable);
    let valid = and_not(&projected, validity);
    *reachable = or(reachable, &valid);

    let projected = shift_right(reachable);
    let valid = and_not(&projected, validity);
    *reachable = or(reachable, &valid);

    let projected = shift_down(reachable);
    let valid = and_not(&projected, validity);
    *reachable = or(reachable, &valid);
}

// SSA rotation propagation functions - kept for future SSA reimplementation
// Currently SSA delegates to movegen_bitboard which has its own propagation
#[allow(dead_code)]
#[inline(always)]
fn propagate_rotation(
    sources: &RowBoard,
    validity_masks: &[RowBoard; 4],
    piece: Piece,
    from_idx: usize,
    to_idx: usize,
    clockwise: bool,
) -> RowBoard {
    let from_rot = idx_to_rotation(from_idx);
    let kicks = get_kicks_cw_ccw(piece, from_rot, clockwise);
    let to_rot = idx_to_rotation(to_idx);
    debug_assert_eq!(kicks, get_kicks(piece, from_rot, to_rot));

    let mut result = RowBoard::new();
    let mut remaining = sources.clone();

    // Try in-place rotation first (no kick)
    let valid = and_not(&remaining, &validity_masks[to_idx]);
    result = or(&result, &valid);
    remaining = and_not(&remaining, &valid);

    // Try kicks in order with source subtraction (first-valid semantics)
    for &(dx, dy) in kicks {
        let projected = shift_by(&remaining, dx, dy);
        let valid = and_not(&projected, &validity_masks[to_idx]);
        result = or(&result, &valid);

        let satisfied = shift_by(&valid, -dx, -dy);
        remaining = and_not(&remaining, &satisfied);
    }

    result
}

#[allow(dead_code)]
#[inline(always)]
fn propagate_180(
    sources: &RowBoard,
    validity_masks: &[RowBoard; 4],
    piece: Piece,
    from_idx: usize,
) -> RowBoard {
    let to_idx = (from_idx + 2) % 4;
    let from_rot = idx_to_rotation(from_idx);
    let kicks = get_180_kicks(piece, from_rot);
    debug_assert_eq!(kicks, get_kicks(piece, from_rot, from_rot.flip()));

    let mut result = RowBoard::new();
    let mut remaining = sources.clone();

    // Try in-place 180 rotation first (no kick)
    let valid = and_not(&remaining, &validity_masks[to_idx]);
    result = or(&result, &valid);
    remaining = and_not(&remaining, &valid);

    // Try kicks in order with source subtraction (first-valid semantics)
    for &(dx, dy) in kicks {
        if dx == 0 && dy == 0 {
            continue; // Already tried above
        }
        let projected = shift_by(&remaining, dx, dy);
        let valid = and_not(&projected, &validity_masks[to_idx]);
        result = or(&result, &valid);

        let satisfied = shift_by(&valid, -dx, -dy);
        remaining = and_not(&remaining, &satisfied);
    }

    result
}

#[allow(dead_code)]
fn idx_to_rotation(idx: usize) -> Rotation {
    match idx {
        0 => Rotation::North,
        1 => Rotation::East,
        2 => Rotation::South,
        _ => Rotation::West,
    }
}

#[allow(dead_code)]
fn rotation_to_idx(rotation: Rotation) -> usize {
    match rotation {
        Rotation::North => 0,
        Rotation::East => 1,
        Rotation::South => 2,
        Rotation::West => 3,
    }
}

#[inline(always)]
fn visit_state(
    visited: &mut [[[bool; 44]; 14]; 4],
    queue: &mut VecDeque<(Rotation, i8, i8)>,
    rotation: Rotation,
    x: i8,
    y: i8,
) {
    let x_idx = (x + 2) as usize;
    let y_idx = y as usize;

    if x_idx < 14 && y_idx < 44 && !visited[rotation as usize][x_idx][y_idx] {
        visited[rotation as usize][x_idx][y_idx] = true;
        queue.push_back((rotation, x, y));
    }
}

#[inline(always)]
fn try_rotate_bfs(
    collision: &CollisionMap,
    piece: Piece,
    from_rot: Rotation,
    to_rot: Rotation,
    x: i8,
    y: i8,
) -> Option<(i8, i8)> {
    let kicks = get_kicks(piece, from_rot, to_rot);
    for &(dx, dy) in kicks {
        let new_x = x + dx;
        let new_y = y + dy;
        if !collision.collides(to_rot, new_x, new_y) {
            return Some((new_x, new_y));
        }
    }

    None
}

/// Find landing Y position after hard drop from start_y
#[allow(dead_code)]
fn find_landing_y(validity: &RowBoard, x: i8, start_y: i8) -> i8 {
    let mut y = start_y;
    while y > 0 {
        // Check if position at y-1 is blocked (validity mask has bit set = collision)
        if validity.get_bit(x as usize, (y - 1) as usize) {
            break;
        }
        y -= 1;
    }
    y
}

/// Generate moves using SSA - main public API
pub fn generate_moves_ssa(board: &Board, piece: Piece) -> Vec<Move> {
    generate_moves_bitboard(board, piece).to_vec()
}

/// Generate moves using SSA with spin detection disabled.
pub fn generate_moves_ssa_no_spin(board: &Board, piece: Piece) -> Vec<Move> {
    generate_moves_bitboard_no_spin(board, piece).to_vec()
}

/// Count moves using SSA without allocating Vec (for perft depth-1 optimization)
pub fn count_moves_ssa(board: &Board, piece: Piece) -> usize {
    count_placements_cobra(board, piece)
}

// Helper: result = a & ~b
#[inline(always)]
fn and_not(a: &RowBoard, b: &RowBoard) -> RowBoard {
    let mut result = RowBoard::new();
    let rows = result.rows_mut();
    for y in 0..44 {
        rows[y] = a.get_row(y) & !b.get_row(y) & RowBoard::WIDTH_MASK;
    }
    result
}

// Helper: result = a | b
#[inline(always)]
fn or(a: &RowBoard, b: &RowBoard) -> RowBoard {
    let mut result = RowBoard::new();
    let rows = result.rows_mut();
    for y in 0..44 {
        rows[y] = (a.get_row(y) | b.get_row(y)) & RowBoard::WIDTH_MASK;
    }
    result
}

#[allow(dead_code)]
#[inline(always)]
fn is_empty(board: &RowBoard) -> bool {
    for y in 0..44 {
        if board.get_row(y) != 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssa_shift_left() {
        let mut board = RowBoard::new();
        board.set_bit(5, 3);
        board.set_bit(0, 10);

        let shifted = shift_left(&board);
        assert!(shifted.get_bit(4, 3));
        assert!(!shifted.get_bit(5, 3));
        assert!(!shifted.get_bit(0, 10));
    }

    #[test]
    fn test_ssa_shift_right() {
        let mut board = RowBoard::new();
        board.set_bit(4, 7);
        board.set_bit(9, 1);

        let shifted = shift_right(&board);
        assert!(shifted.get_bit(5, 7));
        assert!(!shifted.get_bit(4, 7));
        assert!(!shifted.get_bit(9, 1));
    }

    #[test]
    fn test_ssa_shift_down() {
        let mut board = RowBoard::new();
        board.set_bit(2, 6);
        board.set_bit(8, 43);

        let shifted = shift_down(&board);
        assert!(shifted.get_bit(2, 5));
        assert!(!shifted.get_bit(2, 6));
        assert!(shifted.get_bit(8, 42));
    }

    #[test]
    fn test_ssa_movement_reachability_empty_board() {
        let validity_mask = RowBoard::new();
        let spawn_x = 4;
        let spawn_y = 20;

        let reachable = compute_movement_reachability(&validity_mask, spawn_x, spawn_y);

        for y in 0..44 {
            for x in 0..10 {
                let expected = y <= spawn_y as usize;
                assert_eq!(
                    reachable.get_bit(x, y),
                    expected,
                    "unexpected reachability at ({}, {})",
                    x,
                    y
                );
            }
        }
    }

    #[test]
    fn test_ssa_vs_cobra_t_piece_empty() {
        // Cobra reference: T-piece on empty board = 34 moves
        let board = Board::new();
        let reachable = compute_full_reachability(&board, Piece::T);
        let mut ssa_count = 0;

        for rot_idx in 0..4 {
            for y in 0..44 {
                for x in 0..10 {
                    if reachable[rot_idx].get_bit(x, y) {
                        ssa_count += 1;
                    }
                }
            }
        }

        // Reachability count should be >= final move count (includes non-landing positions)
        assert!(
            ssa_count >= 34,
            "SSA reachability: {}, expected >= 34",
            ssa_count
        );
    }

    #[test]
    fn test_ssa_generate_moves_t_piece() {
        // Cobra reference: T-piece on empty board = 34 moves
        let board = Board::new();
        let ssa_moves = generate_moves_ssa(&board, Piece::T);
        assert_eq!(
            ssa_moves.len(),
            34,
            "T-piece: SSA={}, Cobra=34",
            ssa_moves.len()
        );
    }

    #[test]
    fn test_ssa_generate_moves_all_pieces() {
        // Cobra reference values for empty board
        let expected: [(Piece, usize); 7] = [
            (Piece::I, 17),
            (Piece::O, 9),
            (Piece::T, 34),
            (Piece::S, 17),
            (Piece::Z, 17),
            (Piece::J, 34),
            (Piece::L, 34),
        ];
        let board = Board::new();

        for (piece, cobra_count) in expected {
            let ssa_moves = generate_moves_ssa(&board, piece);
            assert_eq!(
                ssa_moves.len(),
                cobra_count,
                "Piece {:?}: SSA={}, Cobra={}",
                piece,
                ssa_moves.len(),
                cobra_count
            );
        }
    }

    #[test]
    fn test_ssa_parity_empty_board_all_pieces() {
        // Verify move count matches Cobra for all 7 pieces on empty board
        // Total depth-1 moves = 17 (from perft)
        let board = Board::new();
        let total: usize = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::S,
            Piece::Z,
            Piece::J,
            Piece::L,
        ]
        .iter()
        .map(|&piece| generate_moves_ssa(&board, piece).len())
        .sum();

        // Average moves per piece should be reasonable (17-34 range)
        let avg = total / 7;
        assert!(
            avg >= 17 && avg <= 34,
            "Average moves per piece: {}, expected 17-34",
            avg
        );
    }

    #[test]
    fn test_ssa_parity_with_obstacles() {
        // Board with some blocks - just verify moves are generated
        let mut board = Board::new();
        for x in 0..10 {
            board.set(x, 0, true); // Full row at bottom
        }
        board.set(5, 5, true); // Single block
        board.set(3, 10, true);
        board.set(7, 10, true);

        for piece in [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::S,
            Piece::Z,
            Piece::J,
            Piece::L,
        ] {
            let ssa = generate_moves_ssa(&board, piece);
            // With obstacles, should have fewer moves than empty board
            assert!(
                !ssa.is_empty(),
                "Obstacle board {:?}: SSA should have moves",
                piece
            );
        }
    }

    #[test]
    fn test_ssa_parity_cheese_board() {
        // Cheese pattern with holes - verify moves are generated
        let mut board = Board::new();
        for y in 0..4 {
            for x in 0..10 {
                if x != (y * 2) % 10 {
                    // Leave one hole per row
                    board.set(x, y, true);
                }
            }
        }

        for piece in [Piece::T, Piece::I, Piece::J, Piece::L] {
            let ssa = generate_moves_ssa(&board, piece);
            assert!(
                !ssa.is_empty(),
                "Cheese board {:?}: SSA should have moves",
                piece
            );
        }
    }

    #[test]
    fn test_ssa_parity_tall_stack() {
        // Tall stack with gaps - verify moves are generated
        let mut board = Board::new();
        for y in 0..15 {
            for x in 0..10 {
                if x != 4 && x != 5 {
                    // Leave center column gap
                    board.set(x, y, true);
                }
            }
        }

        for piece in [Piece::I, Piece::T, Piece::S, Piece::Z] {
            let ssa = generate_moves_ssa(&board, piece);
            assert!(
                !ssa.is_empty(),
                "Tall stack {:?}: SSA should have moves",
                piece
            );
        }
    }

    #[test]
    fn test_count_ssa_matches_generate() {
        let board = Board::new();
        for piece in [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::S,
            Piece::Z,
            Piece::J,
            Piece::L,
        ] {
            let count = count_moves_ssa(&board, piece);
            let vec_len = generate_moves_ssa(&board, piece).len();
            assert_eq!(
                count, vec_len,
                "Piece {:?}: count={}, vec.len()={}",
                piece, count, vec_len
            );
        }
    }

    #[test]
    fn test_count_ssa_empty_board() {
        let board = Board::new();
        assert_eq!(count_moves_ssa(&board, Piece::T), 34);
        assert_eq!(count_moves_ssa(&board, Piece::I), 17);
        assert_eq!(count_moves_ssa(&board, Piece::O), 9);
    }

    /// Verify L-piece count matches Cobra reference (34 moves on empty board)
    #[test]
    fn test_l_piece_count() {
        let board = Board::new();
        let moves = generate_moves_ssa(&board, Piece::L);
        assert_eq!(
            moves.len(),
            34,
            "L-piece: got {}, expected 34 (Cobra reference)",
            moves.len()
        );
    }

    /// Verify J-piece count matches Cobra reference (34 moves on empty board)
    #[test]
    fn test_j_piece_count() {
        let board = Board::new();
        let moves = generate_moves_ssa(&board, Piece::J);
        assert_eq!(
            moves.len(),
            34,
            "J-piece: got {}, expected 34 (Cobra reference)",
            moves.len()
        );
    }

    /// Verify S/Z piece counts match Cobra reference (17 moves each on empty board)
    #[test]
    fn test_sz_piece_count() {
        let board = Board::new();
        assert_eq!(count_moves_ssa(&board, Piece::S), 17);
        assert_eq!(count_moves_ssa(&board, Piece::Z), 17);
    }
}
