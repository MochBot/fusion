//! SRS+ kick tables for piece rotation.
//! Ported from Triangle.js src/lib/triangle/src/engine/utils/kicks/data.ts

use fusion_core::{Piece, Rotation};

/// Get kick offsets for a rotation transition.
/// Returns slice of (dx, dy) offsets to try in order.
pub fn get_kicks(piece: Piece, from: Rotation, to: Rotation) -> &'static [(i8, i8)] {
    let key = rotation_key(from, to);

    match piece {
        Piece::I => get_i_kicks(key),
        Piece::O => &[], // O piece doesn't kick
        _ => get_jlstz_kicks(key),
    }
}

fn rotation_key(from: Rotation, to: Rotation) -> u8 {
    let f = rotation_index(from);
    let t = rotation_index(to);
    f * 10 + t
}

fn rotation_index(r: Rotation) -> u8 {
    match r {
        Rotation::North => 0,
        Rotation::East => 1,
        Rotation::South => 2,
        Rotation::West => 3,
    }
}

/// JLSTZ kick table (SRS+)
fn get_jlstz_kicks(key: u8) -> &'static [(i8, i8)] {
    match key {
        // CW rotations
        01 => &[(-1, 0), (-1, -1), (0, 2), (-1, 2)], // N -> E
        12 => &[(1, 0), (1, 1), (0, -2), (1, -2)],   // E -> S
        23 => &[(1, 0), (1, -1), (0, 2), (1, 2)],    // S -> W
        30 => &[(-1, 0), (-1, 1), (0, -2), (-1, -2)], // W -> N

        // CCW rotations
        10 => &[(1, 0), (1, 1), (0, -2), (1, -2)], // E -> N
        21 => &[(-1, 0), (-1, -1), (0, 2), (-1, 2)], // S -> E
        32 => &[(-1, 0), (-1, 1), (0, -2), (-1, -2)], // W -> S
        03 => &[(1, 0), (1, -1), (0, 2), (1, 2)],  // N -> W

        // 180 rotations (SRS+ extension)
        02 => &[(0, -1), (1, -1), (-1, -1), (1, 0), (-1, 0)], // N -> S
        20 => &[(0, 1), (-1, 1), (1, 1), (-1, 0), (1, 0)],    // S -> N
        13 => &[(1, 0), (1, -2), (1, -1), (0, -2), (0, -1)],  // E -> W
        31 => &[(-1, 0), (-1, -2), (-1, -1), (0, -2), (0, -1)], // W -> E

        _ => &[],
    }
}

/// I piece kick table (SRS+)
fn get_i_kicks(key: u8) -> &'static [(i8, i8)] {
    match key {
        // CW rotations
        01 => &[(-2, 0), (1, 0), (-2, 1), (1, -2)], // N -> E
        12 => &[(-1, 0), (2, 0), (-1, -2), (2, 1)], // E -> S
        23 => &[(2, 0), (-1, 0), (2, -1), (-1, 2)], // S -> W
        30 => &[(1, 0), (-2, 0), (1, 2), (-2, -1)], // W -> N

        // CCW rotations
        10 => &[(2, 0), (-1, 0), (2, -1), (-1, 2)], // E -> N
        21 => &[(1, 0), (-2, 0), (1, 2), (-2, -1)], // S -> E
        32 => &[(-2, 0), (1, 0), (-2, 1), (1, -2)], // W -> S
        03 => &[(-1, 0), (2, 0), (-1, -2), (2, 1)], // N -> W

        // 180 rotations - I piece has no 180 kicks in standard SRS+
        02 | 20 | 13 | 31 => &[],

        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_t_cw_kicks() {
        let kicks = get_kicks(Piece::T, Rotation::North, Rotation::East);
        assert_eq!(kicks.len(), 4);
        assert_eq!(kicks[0], (-1, 0));
    }

    #[test]
    fn test_i_kicks() {
        let kicks = get_kicks(Piece::I, Rotation::North, Rotation::East);
        assert_eq!(kicks.len(), 4);
        assert_eq!(kicks[0], (-2, 0));
    }

    #[test]
    fn test_o_no_kicks() {
        let kicks = get_kicks(Piece::O, Rotation::North, Rotation::East);
        assert!(kicks.is_empty());
    }

    #[test]
    fn test_180_kicks() {
        let kicks = get_kicks(Piece::T, Rotation::North, Rotation::South);
        assert_eq!(kicks.len(), 5); // SRS+ 180 has 5 kick tests
    }
}
