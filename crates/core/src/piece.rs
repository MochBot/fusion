//! Piece types and mino definitions for Tetris pieces.

use serde::{Deserialize, Serialize};

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
    pub fn cw(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    pub fn ccw(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
        }
    }

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

    /// Get mino offsets for this piece at given rotation.
    /// Returns 4 (x, y) offsets relative to piece center.
    pub fn minos(self, rot: Rotation) -> [(i8, i8); 4] {
        let idx = match rot {
            Rotation::North => 0,
            Rotation::East => 1,
            Rotation::South => 2,
            Rotation::West => 3,
        };
        match self {
            Piece::I => [
                [(-1, 0), (0, 0), (1, 0), (2, 0)],
                [(0, -1), (0, 0), (0, 1), (0, 2)],
                [(-1, 0), (0, 0), (1, 0), (2, 0)],
                [(0, -1), (0, 0), (0, 1), (0, 2)],
            ][idx],
            Piece::O => [(0, 0), (1, 0), (0, 1), (1, 1)],
            Piece::T => [
                [(-1, 0), (0, 0), (1, 0), (0, 1)],
                [(0, -1), (0, 0), (0, 1), (1, 0)],
                [(-1, 0), (0, 0), (1, 0), (0, -1)],
                [(0, -1), (0, 0), (0, 1), (-1, 0)],
            ][idx],
            Piece::S => [
                [(-1, 0), (0, 0), (0, 1), (1, 1)],
                [(0, 1), (0, 0), (1, 0), (1, -1)],
                [(-1, -1), (0, -1), (0, 0), (1, 0)],
                [(-1, 1), (-1, 0), (0, 0), (0, -1)],
            ][idx],
            Piece::Z => [
                [(0, 0), (1, 0), (-1, 1), (0, 1)],
                [(0, -1), (0, 0), (1, 0), (1, 1)],
                [(0, -1), (1, -1), (-1, 0), (0, 0)],
                [(-1, -1), (-1, 0), (0, 0), (0, 1)],
            ][idx],
            Piece::J => [
                [(-1, 0), (0, 0), (1, 0), (-1, 1)],
                [(0, -1), (0, 0), (0, 1), (1, 1)],
                [(1, -1), (-1, 0), (0, 0), (1, 0)],
                [(-1, -1), (0, -1), (0, 0), (0, 1)],
            ][idx],
            Piece::L => [
                [(-1, 0), (0, 0), (1, 0), (1, 1)],
                [(0, -1), (0, 0), (0, 1), (1, -1)],
                [(-1, -1), (-1, 0), (0, 0), (1, 0)],
                [(-1, 1), (0, -1), (0, 0), (0, 1)],
            ][idx],
        }
    }

    /// Spawn x position (center column)
    pub fn spawn_x(self) -> i8 {
        match self {
            Piece::I => 4,
            Piece::O => 4,
            _ => 4,
        }
    }

    /// Spawn y position
    pub fn spawn_y(self) -> i8 {
        20
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
