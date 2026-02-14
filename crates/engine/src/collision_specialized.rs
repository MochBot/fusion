//! Macro-generated specialized collision functions
//! 28 functions (7 pieces × 4 rotations) with hardcoded mino offsets
//!
//! Benefits: No runtime mino lookup, loop fully unrolled, CPU can prefetch

use fusion_core::{Board, Piece, Rotation};

/// Macro to generate a specialized collision check function
macro_rules! define_collision_check {
    (
        $fn_name:ident,
        [($m0x:expr, $m0y:expr), ($m1x:expr, $m1y:expr),
         ($m2x:expr, $m2y:expr), ($m3x:expr, $m3y:expr)]
    ) => {
        #[inline(always)]
        pub fn $fn_name(board: &Board, x: i8, y: i8) -> bool {
            // Bounds check helper - returns true if out of bounds
            #[inline(always)]
            fn oob(nx: i8, ny: i8) -> bool {
                !(0..10).contains(&nx) || !(0..40).contains(&ny)
            }

            // Mino 0
            let (nx0, ny0) = (x + $m0x, y + $m0y);
            if oob(nx0, ny0) || (board.column(nx0 as usize) & (1u64 << ny0)) != 0 {
                return true;
            }

            // Mino 1
            let (nx1, ny1) = (x + $m1x, y + $m1y);
            if oob(nx1, ny1) || (board.column(nx1 as usize) & (1u64 << ny1)) != 0 {
                return true;
            }

            // Mino 2
            let (nx2, ny2) = (x + $m2x, y + $m2y);
            if oob(nx2, ny2) || (board.column(nx2 as usize) & (1u64 << ny2)) != 0 {
                return true;
            }

            // Mino 3
            let (nx3, ny3) = (x + $m3x, y + $m3y);
            if oob(nx3, ny3) || (board.column(nx3 as usize) & (1u64 << ny3)) != 0 {
                return true;
            }

            false
        }
    };
}

// I piece - horizontal and vertical
define_collision_check!(collides_i_north, [(-1, 0), (0, 0), (1, 0), (2, 0)]);
define_collision_check!(collides_i_east, [(0, -2), (0, -1), (0, 0), (0, 1)]);
define_collision_check!(collides_i_south, [(1, 0), (0, 0), (-1, 0), (-2, 0)]);
define_collision_check!(collides_i_west, [(0, -1), (0, 0), (0, 1), (0, 2)]);

// O piece - all identical (square)
define_collision_check!(collides_o_north, [(0, 0), (1, 0), (0, 1), (1, 1)]);
define_collision_check!(collides_o_east, [(0, 0), (1, 0), (0, 1), (1, 1)]);
define_collision_check!(collides_o_south, [(0, 0), (1, 0), (0, 1), (1, 1)]);
define_collision_check!(collides_o_west, [(0, 0), (1, 0), (0, 1), (1, 1)]);

// T piece
define_collision_check!(collides_t_north, [(-1, 0), (0, 0), (1, 0), (0, 1)]);
define_collision_check!(collides_t_east, [(0, -1), (0, 0), (0, 1), (1, 0)]);
define_collision_check!(collides_t_south, [(-1, 0), (0, 0), (1, 0), (0, -1)]);
define_collision_check!(collides_t_west, [(0, -1), (0, 0), (0, 1), (-1, 0)]);

// S piece
define_collision_check!(collides_s_north, [(-1, 0), (0, 0), (0, 1), (1, 1)]);
define_collision_check!(collides_s_east, [(0, 1), (0, 0), (1, 0), (1, -1)]);
define_collision_check!(collides_s_south, [(-1, -1), (0, -1), (0, 0), (1, 0)]);
define_collision_check!(collides_s_west, [(-1, 1), (-1, 0), (0, 0), (0, -1)]);

// Z piece
define_collision_check!(collides_z_north, [(0, 0), (1, 0), (-1, 1), (0, 1)]);
define_collision_check!(collides_z_east, [(0, -1), (0, 0), (1, 0), (1, 1)]);
define_collision_check!(collides_z_south, [(0, -1), (1, -1), (-1, 0), (0, 0)]);
define_collision_check!(collides_z_west, [(-1, -1), (-1, 0), (0, 0), (0, 1)]);

// J piece
define_collision_check!(collides_j_north, [(-1, 0), (0, 0), (1, 0), (-1, 1)]);
define_collision_check!(collides_j_east, [(0, -1), (0, 0), (0, 1), (1, 1)]);
define_collision_check!(collides_j_south, [(1, -1), (-1, 0), (0, 0), (1, 0)]);
define_collision_check!(collides_j_west, [(-1, -1), (0, -1), (0, 0), (0, 1)]);

// L piece
define_collision_check!(collides_l_north, [(-1, 0), (0, 0), (1, 0), (1, 1)]);
define_collision_check!(collides_l_east, [(0, -1), (0, 0), (0, 1), (1, -1)]);
define_collision_check!(collides_l_south, [(-1, -1), (-1, 0), (0, 0), (1, 0)]);
define_collision_check!(collides_l_west, [(-1, 1), (0, -1), (0, 0), (0, 1)]);

/// Dispatch to specialized collision function based on piece and rotation
#[inline(always)]
pub fn collides_specialized(board: &Board, piece: Piece, rotation: Rotation, x: i8, y: i8) -> bool {
    match (piece, rotation) {
        (Piece::I, Rotation::North) => collides_i_north(board, x, y),
        (Piece::I, Rotation::East) => collides_i_east(board, x, y),
        (Piece::I, Rotation::South) => collides_i_south(board, x, y),
        (Piece::I, Rotation::West) => collides_i_west(board, x, y),

        (Piece::O, Rotation::North) => collides_o_north(board, x, y),
        (Piece::O, Rotation::East) => collides_o_east(board, x, y),
        (Piece::O, Rotation::South) => collides_o_south(board, x, y),
        (Piece::O, Rotation::West) => collides_o_west(board, x, y),

        (Piece::T, Rotation::North) => collides_t_north(board, x, y),
        (Piece::T, Rotation::East) => collides_t_east(board, x, y),
        (Piece::T, Rotation::South) => collides_t_south(board, x, y),
        (Piece::T, Rotation::West) => collides_t_west(board, x, y),

        (Piece::S, Rotation::North) => collides_s_north(board, x, y),
        (Piece::S, Rotation::East) => collides_s_east(board, x, y),
        (Piece::S, Rotation::South) => collides_s_south(board, x, y),
        (Piece::S, Rotation::West) => collides_s_west(board, x, y),

        (Piece::Z, Rotation::North) => collides_z_north(board, x, y),
        (Piece::Z, Rotation::East) => collides_z_east(board, x, y),
        (Piece::Z, Rotation::South) => collides_z_south(board, x, y),
        (Piece::Z, Rotation::West) => collides_z_west(board, x, y),

        (Piece::J, Rotation::North) => collides_j_north(board, x, y),
        (Piece::J, Rotation::East) => collides_j_east(board, x, y),
        (Piece::J, Rotation::South) => collides_j_south(board, x, y),
        (Piece::J, Rotation::West) => collides_j_west(board, x, y),

        (Piece::L, Rotation::North) => collides_l_north(board, x, y),
        (Piece::L, Rotation::East) => collides_l_east(board, x, y),
        (Piece::L, Rotation::South) => collides_l_south(board, x, y),
        (Piece::L, Rotation::West) => collides_l_west(board, x, y),
    }
}

/// Inverse of collides - returns true if placement is valid
#[inline(always)]
pub fn can_place_specialized(
    board: &Board,
    piece: Piece,
    rotation: Rotation,
    x: i8,
    y: i8,
) -> bool {
    !collides_specialized(board, piece, rotation, x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::collides;

    #[test]
    fn test_specialized_matches_generic() {
        let board = Board::new();

        let rotations = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ];

        for piece in Piece::ALL {
            for rotation in rotations {
                for x in -2..12 {
                    for y in -2..42 {
                        let generic = collides(&board, piece, rotation, x, y);
                        let specialized = collides_specialized(&board, piece, rotation, x, y);
                        assert_eq!(
                            generic, specialized,
                            "Mismatch at piece={:?} rot={:?} x={} y={}",
                            piece, rotation, x, y
                        );
                    }
                }
            }
        }
    }
}
