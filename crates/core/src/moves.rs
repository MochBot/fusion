//! Move and spin type definitions.

use crate::{Piece, Rotation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SpinType {
    None,
    Mini,
    Full,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Move {
    pub piece: Piece,
    pub rotation: Rotation,
    pub x: i8,
    pub y: i8,
    pub hold_used: bool,
    pub spin_type: SpinType,
}

impl Move {
    pub const ZERO: Self = Self {
        piece: Piece::I,
        rotation: Rotation::North,
        x: 0,
        y: 0,
        hold_used: false,
        spin_type: SpinType::None,
    };

    pub fn new(piece: Piece, rotation: Rotation, x: i8, y: i8) -> Self {
        Self {
            piece,
            rotation,
            x,
            y,
            hold_used: false,
            spin_type: SpinType::None,
        }
    }

    pub fn with_spin(mut self, spin_type: SpinType) -> Self {
        self.spin_type = spin_type;
        self
    }

    pub fn with_hold(mut self) -> Self {
        self.hold_used = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_new() {
        let m = Move::new(Piece::T, Rotation::North, 4, 0);
        assert_eq!(m.piece, Piece::T);
        assert_eq!(m.rotation, Rotation::North);
        assert_eq!(m.x, 4);
        assert_eq!(m.y, 0);
        assert!(!m.hold_used);
        assert_eq!(m.spin_type, SpinType::None);
    }

    #[test]
    fn test_move_with_spin() {
        let m = Move::new(Piece::T, Rotation::South, 5, 2).with_spin(SpinType::Full);
        assert_eq!(m.spin_type, SpinType::Full);
    }
}
