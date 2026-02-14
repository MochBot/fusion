//! Validity mask computation via Minkowski smear for Source-Subtraction movegen.
//! Precomputes where a piece center would collide - bit=1 means collision at (x,y).

use crate::row_board::RowBoard;
use fusion_core::{Board, Piece, Rotation};

/// Compute validity mask using Minkowski Sum approach.
/// For each mino offset, shift board in opposite direction and OR together.
/// Result: bit=1 at (x,y) means piece center at (x,y) would collide.
pub fn compute_validity_mask(board: &RowBoard, piece: Piece, rotation: Rotation) -> RowBoard {
    let minos = piece.minos(rotation);
    let mut mask = RowBoard::new();

    // Minkowski smear: for each mino, shift board by -offset and OR
    for (dx, dy) in minos {
        shift_and_or(&mut mask, board, -dx, -dy);
    }

    // Add boundary collisions - piece minos going off-board
    add_boundary_collisions(&mut mask, piece, rotation);

    mask
}

/// Shift board by (dx, dy) and OR into mask.
/// Shifting board left (negative dx): row bits shift right (>> dx)
/// Shifting board right (positive dx): row bits shift left (<< dx)
/// Shifting board down (negative dy): row[y] gets row[y - dy]
/// Shifting board up (positive dy): row[y] gets row[y - dy]
#[inline]
fn shift_and_or(mask: &mut RowBoard, board: &RowBoard, dx: i8, dy: i8) {
    let rows = mask.rows_mut();

    for y in 0..44i32 {
        // Source row after vertical shift
        let src_y = y - dy as i32;

        if !(0..44).contains(&src_y) {
            // Source out of bounds - no contribution from board
            // (boundary handling done separately)
            continue;
        }

        let src_row = board.get_row(src_y as usize);

        // Horizontal shift
        let shifted = if dx > 0 {
            // Shift left in board space = bits shift left
            src_row << (dx as u32)
        } else if dx < 0 {
            // Shift right in board space = bits shift right
            src_row >> ((-dx) as u32)
        } else {
            src_row
        };

        // Mask to 10 bits and OR in
        rows[y as usize] |= shifted & RowBoard::WIDTH_MASK;
    }
}

/// Board height used for ceiling collision (matches CollisionMap)
const BOARD_HEIGHT: i32 = 40;

/// Add boundary collisions - mark positions where any mino would go off-board.
/// Left wall: x < 0, Right wall: x >= 10, Floor: y < 0, Ceiling: y >= 40
#[inline]
fn add_boundary_collisions(mask: &mut RowBoard, piece: Piece, rotation: Rotation) {
    let minos = piece.minos(rotation);
    let rows = mask.rows_mut();

    // For each center position, check if any mino would be off-board
    for y in 0..44i32 {
        for x in 0..10i32 {
            let collides = minos.iter().any(|(dx, dy)| {
                let mx = x + (*dx as i32);
                let my = y + (*dy as i32);
                !(0..10).contains(&mx) || !(0..BOARD_HEIGHT).contains(&my)
            });

            if collides {
                rows[y as usize] |= 1u64 << x;
            }
        }
    }
}

/// Compute validity mask directly from Board (convenience wrapper)
pub fn compute_validity_mask_from_board(
    board: &Board,
    piece: Piece,
    rotation: Rotation,
) -> RowBoard {
    let row_board = RowBoard::from(board);
    compute_validity_mask(&row_board, piece, rotation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision_map::CollisionMap;

    #[test]
    fn test_validity_mask_empty_board_t_north() {
        let board = Board::new();
        let row_board = RowBoard::from(&board);
        let mask = compute_validity_mask(&row_board, Piece::T, Rotation::North);

        // T-piece North minos: [(-1, 0), (0, 0), (1, 0), (0, 1)]
        // On empty board, only boundary positions should collide

        // Center positions in the middle should NOT collide
        assert!(
            !mask.get_bit(4, 10),
            "T at (4,10) should not collide on empty board"
        );
        assert!(
            !mask.get_bit(5, 20),
            "T at (5,20) should not collide on empty board"
        );

        // Left wall: T at x=0 has mino at x=-1, should collide
        assert!(
            mask.get_bit(0, 10),
            "T at (0,10) should collide (left mino hits wall)"
        );

        // Right wall: T at x=9 has mino at x=10, should collide
        assert!(
            mask.get_bit(9, 10),
            "T at (9,10) should collide (right mino hits wall)"
        );

        // Floor: T at y=0 has mino at y=0 (ok), but T at y=-1 would have mino at y=-1
        // Actually T-North lowest mino is at dy=0, so y=0 is fine
        assert!(
            !mask.get_bit(4, 0),
            "T at (4,0) should not collide (no mino below y=0)"
        );
    }

    #[test]
    fn test_validity_mask_with_block() {
        let mut board = Board::new();
        board.set(5, 5, true);

        let row_board = RowBoard::from(&board);
        let mask = compute_validity_mask(&row_board, Piece::T, Rotation::North);

        // T-piece North minos: [(-1, 0), (0, 0), (1, 0), (0, 1)]
        // Block at (5, 5) should cause collision when:
        // - Center at (5, 5): center mino (0,0) hits block
        // - Center at (4, 5): right mino (1,0) hits block
        // - Center at (6, 5): left mino (-1,0) hits block
        // - Center at (5, 4): top mino (0,1) hits block

        assert!(
            mask.get_bit(5, 5),
            "T at (5,5) should collide (center hits block)"
        );
        assert!(
            mask.get_bit(4, 5),
            "T at (4,5) should collide (right mino hits block)"
        );
        assert!(
            mask.get_bit(6, 5),
            "T at (6,5) should collide (left mino hits block)"
        );
        assert!(
            mask.get_bit(5, 4),
            "T at (5,4) should collide (top mino hits block)"
        );

        // Positions away from block should not collide (if not at boundary)
        assert!(
            !mask.get_bit(5, 10),
            "T at (5,10) should not collide (far from block)"
        );
    }

    #[test]
    fn test_validity_mask_i_piece_boundaries() {
        let board = Board::new();
        let row_board = RowBoard::from(&board);

        // I-piece North: [(-1, 0), (0, 0), (1, 0), (2, 0)]
        let mask = compute_validity_mask(&row_board, Piece::I, Rotation::North);

        // I at x=0: mino at x=-1, collides
        assert!(
            mask.get_bit(0, 10),
            "I at (0,10) should collide (left wall)"
        );

        // I at x=1: mino at x=0, ok (leftmost at 0)
        assert!(!mask.get_bit(1, 10), "I at (1,10) should not collide");

        // I at x=7: rightmost mino at x=9, ok
        assert!(!mask.get_bit(7, 10), "I at (7,10) should not collide");

        // I at x=8: rightmost mino at x=10, collides
        assert!(
            mask.get_bit(8, 10),
            "I at (8,10) should collide (right wall)"
        );
    }

    #[test]
    fn test_validity_mask_matches_collision_map() {
        // Random board with some blocks
        let mut board = Board::new();
        board.set(2, 3, true);
        board.set(5, 7, true);
        board.set(8, 2, true);
        board.set(0, 0, true);
        board.set(9, 15, true);

        for piece in Piece::ALL {
            for rotation in [
                Rotation::North,
                Rotation::East,
                Rotation::South,
                Rotation::West,
            ] {
                let row_board = RowBoard::from(&board);
                let mask = compute_validity_mask(&row_board, piece, rotation);
                let cm = CollisionMap::new(&board, piece);

                // Check all positions
                for y in 0..40i8 {
                    for x in 0..10i8 {
                        let mask_collides = mask.get_bit(x as usize, y as usize);
                        let cm_collides = cm.collides(rotation, x, y);

                        assert_eq!(
                            mask_collides, cm_collides,
                            "Mismatch at ({},{}) for {:?} {:?}: mask={}, cm={}",
                            x, y, piece, rotation, mask_collides, cm_collides
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_validity_mask_o_piece() {
        let board = Board::new();
        let row_board = RowBoard::from(&board);

        // O-piece: [(0, 0), (1, 0), (0, 1), (1, 1)]
        let mask = compute_validity_mask(&row_board, Piece::O, Rotation::North);

        // O at x=0: minos at x=0,1 - ok
        assert!(!mask.get_bit(0, 10), "O at (0,10) should not collide");

        // O at x=8: minos at x=8,9 - ok
        assert!(!mask.get_bit(8, 10), "O at (8,10) should not collide");

        // O at x=9: rightmost mino at x=10 - collides
        assert!(mask.get_bit(9, 10), "O at (9,10) should collide");

        // O at y=0: lowest mino at y=0 - ok
        assert!(!mask.get_bit(4, 0), "O at (4,0) should not collide");
    }

    #[test]
    fn test_validity_mask_floor_collision() {
        let board = Board::new();
        let row_board = RowBoard::from(&board);

        // T-piece South: [(-1, 0), (0, 0), (1, 0), (0, -1)]
        // Has a mino at dy=-1, so y=0 should collide
        let mask = compute_validity_mask(&row_board, Piece::T, Rotation::South);

        assert!(
            mask.get_bit(4, 0),
            "T-South at (4,0) should collide (bottom mino hits floor)"
        );
        assert!(
            !mask.get_bit(4, 1),
            "T-South at (4,1) should not collide (bottom mino at y=0)"
        );
    }

    #[test]
    fn test_validity_mask_full_board_comparison() {
        // More comprehensive test with denser board
        let mut board = Board::new();

        // Create a partial stack
        for y in 0..5 {
            for x in 0..10 {
                if (x + y) % 3 != 0 {
                    board.set(x, y, true);
                }
            }
        }

        let row_board = RowBoard::from(&board);

        for piece in Piece::ALL {
            let rotation = Rotation::North;
            let mask = compute_validity_mask(&row_board, piece, rotation);
            let cm = CollisionMap::new(&board, piece);

            let mut mismatches = 0;
            for y in 0..40i8 {
                for x in 0..10i8 {
                    if mask.get_bit(x as usize, y as usize) != cm.collides(rotation, x, y) {
                        mismatches += 1;
                    }
                }
            }

            assert_eq!(
                mismatches, 0,
                "{:?} {:?} had {} mismatches",
                piece, rotation, mismatches
            );
        }
    }
}
