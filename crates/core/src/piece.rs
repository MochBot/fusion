//! piece definitions - the 7 sacred tetrominos

use serde::{Deserialize, Serialize};

/// mino offsets - precomputed so we don't burn cycles in hot loops
/// [piece][rotation] -> 4 (x,y) offsets from center
const PIECE_MINOS: [[[(i8, i8); 4]; 4]; 7] = [
    // I piece - 4 distinct states (SRS+ grid-intersection pivot, N≠S E≠W)
    [
        [(-1, 0), (0, 0), (1, 0), (2, 0)],  // North - extends right
        [(0, -2), (0, -1), (0, 0), (0, 1)], // East - extends down (Y-up)
        [(1, 0), (0, 0), (-1, 0), (-2, 0)], // South - extends left (reversed from N)
        [(0, -1), (0, 0), (0, 1), (0, 2)],  // West - extends up (reversed from E)
    ],
    // O piece (all rotations identical)
    [
        [(0, 0), (1, 0), (0, 1), (1, 1)],
        [(0, 0), (1, 0), (0, 1), (1, 1)],
        [(0, 0), (1, 0), (0, 1), (1, 1)],
        [(0, 0), (1, 0), (0, 1), (1, 1)],
    ],
    // T piece
    [
        [(-1, 0), (0, 0), (1, 0), (0, 1)],  // North
        [(0, -1), (0, 0), (0, 1), (1, 0)],  // East
        [(-1, 0), (0, 0), (1, 0), (0, -1)], // South
        [(0, -1), (0, 0), (0, 1), (-1, 0)], // West
    ],
    // S piece
    [
        [(-1, 0), (0, 0), (0, 1), (1, 1)],   // North
        [(0, 1), (0, 0), (1, 0), (1, -1)],   // East
        [(-1, -1), (0, -1), (0, 0), (1, 0)], // South
        [(-1, 1), (-1, 0), (0, 0), (0, -1)], // West
    ],
    // Z piece
    [
        [(0, 0), (1, 0), (-1, 1), (0, 1)],   // North
        [(0, -1), (0, 0), (1, 0), (1, 1)],   // East
        [(0, -1), (1, -1), (-1, 0), (0, 0)], // South
        [(-1, -1), (-1, 0), (0, 0), (0, 1)], // West
    ],
    // J piece
    [
        [(-1, 0), (0, 0), (1, 0), (-1, 1)],  // North
        [(0, -1), (0, 0), (0, 1), (1, 1)],   // East
        [(1, -1), (-1, 0), (0, 0), (1, 0)],  // South
        [(-1, -1), (0, -1), (0, 0), (0, 1)], // West
    ],
    // L piece
    [
        [(-1, 0), (0, 0), (1, 0), (1, 1)],   // North
        [(0, -1), (0, 0), (0, 1), (1, -1)],  // East
        [(-1, -1), (-1, 0), (0, 0), (1, 0)], // South
        [(-1, 1), (0, -1), (0, 0), (0, 1)],  // West
    ],
];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum Piece {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, Serialize, Deserialize)]
pub enum Rotation {
    #[default]
    North,
    East,
    South,
    West,
}

impl Rotation {
    #[inline(always)]
    pub fn cw(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    #[inline(always)]
    pub fn ccw(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
        }
    }

    #[inline(always)]
    pub fn flip(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::East => Self::West,
            Self::South => Self::North,
            Self::West => Self::East,
        }
    }
}

impl Piece {
    pub const ALL: [Piece; 7] = [
        Piece::I,
        Piece::O,
        Piece::T,
        Piece::S,
        Piece::Z,
        Piece::J,
        Piece::L,
    ];

    /// mino offsets for a rotation - O(1) lookup
    #[inline(always)]
    pub fn minos(self, rot: Rotation) -> [(i8, i8); 4] {
        PIECE_MINOS[self as usize][rot as usize]
    }

    /// Spawn x position (center column)
    #[inline(always)]
    pub fn spawn_x(self) -> i8 {
        match self {
            Piece::I => 4,
            Piece::O => 4,
            _ => 4,
        }
    }

    /// Spawn y position - TETR.IO uses row 21
    #[inline(always)]
    pub fn spawn_y(self) -> i8 {
        21
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_t_north_minos() {
        let m = Piece::T.minos(Rotation::North);
        assert!(m.contains(&(0, 1))); // top of T
        assert!(m.contains(&(-1, 0)));
        assert!(m.contains(&(0, 0)));
        assert!(m.contains(&(1, 0)));
    }

    #[test]
    fn test_rotation_cw() {
        assert_eq!(Rotation::North.cw(), Rotation::East);
        assert_eq!(Rotation::East.cw(), Rotation::South);
        assert_eq!(Rotation::South.cw(), Rotation::West);
        assert_eq!(Rotation::West.cw(), Rotation::North);
    }

    #[test]
    fn test_rotation_ccw() {
        assert_eq!(Rotation::North.ccw(), Rotation::West);
        assert_eq!(Rotation::West.ccw(), Rotation::South);
    }

    #[test]
    fn test_all_pieces() {
        assert_eq!(Piece::ALL.len(), 7);
    }
}
