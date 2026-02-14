//! SRS+ kick tables for piece rotation.
//! finally got these right after staring at Triangle.js for way too long

use fusion_core::{Piece, Rotation};

const EMPTY_KICKS: [(i8, i8); 0] = [];

const JLSTZ_01: [(i8, i8); 5] = [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)];
const JLSTZ_12: [(i8, i8); 5] = [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)];
const JLSTZ_23: [(i8, i8); 5] = [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)];
const JLSTZ_30: [(i8, i8); 5] = [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)];

const JLSTZ_03: [(i8, i8); 5] = [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)];
const JLSTZ_32: [(i8, i8); 5] = [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)];
const JLSTZ_21: [(i8, i8); 5] = [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)];
const JLSTZ_10: [(i8, i8); 5] = [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)];

const JLSTZ_02: [(i8, i8); 6] = [(0, 0), (0, 1), (1, 1), (-1, 1), (1, 0), (-1, 0)];
const JLSTZ_13: [(i8, i8); 6] = [(0, 0), (1, 0), (1, 2), (1, 1), (0, 2), (0, 1)];
const JLSTZ_20: [(i8, i8); 6] = [(0, 0), (0, -1), (-1, -1), (1, -1), (-1, 0), (1, 0)];
const JLSTZ_31: [(i8, i8); 6] = [(0, 0), (-1, 0), (-1, 2), (-1, 1), (0, 2), (0, 1)];

const I_01: [(i8, i8); 5] = [(0, 0), (1, 0), (-2, 0), (-2, -1), (1, 2)];
const I_12: [(i8, i8); 5] = [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)];
const I_23: [(i8, i8); 5] = [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)];
const I_30: [(i8, i8); 5] = [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)];

const I_03: [(i8, i8); 5] = [(0, 0), (-1, 0), (2, 0), (2, 1), (-1, -2)];
const I_32: [(i8, i8); 5] = [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)];
const I_21: [(i8, i8); 5] = [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)];
const I_10: [(i8, i8); 5] = [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)];

const I_02: [(i8, i8); 6] = [(1, -1), (1, 0), (2, 0), (0, 0), (2, -1), (0, -1)];
const I_20: [(i8, i8); 6] = [(-1, 1), (-1, 0), (-2, 0), (0, 0), (-2, 1), (0, 1)];
const I_13: [(i8, i8); 6] = [(-1, -1), (0, -1), (0, 1), (0, 0), (-1, 1), (-1, 0)];
const I_31: [(i8, i8); 6] = [(1, 1), (0, 1), (0, 3), (0, 2), (1, 3), (1, 2)];

/// Kick table type: [piece_index][from_rotation][to_rotation] -> kick offsets
type KickTable = [[[&'static [(i8, i8)]; 4]; 4]; 7];

/// Compile-time SRS+ kick table
/// [piece_index][from_rotation][to_rotation]
pub const SRS_PLUS_KICKS: KickTable = [
    // I
    [
        [&EMPTY_KICKS, &I_01, &I_02, &I_03],
        [&I_10, &EMPTY_KICKS, &I_12, &I_13],
        [&I_20, &I_21, &EMPTY_KICKS, &I_23],
        [&I_30, &I_31, &I_32, &EMPTY_KICKS],
    ],
    // O
    [
        [&EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS],
        [&EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS],
        [&EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS],
        [&EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS, &EMPTY_KICKS],
    ],
    // T
    [
        [&EMPTY_KICKS, &JLSTZ_01, &JLSTZ_02, &JLSTZ_03],
        [&JLSTZ_10, &EMPTY_KICKS, &JLSTZ_12, &JLSTZ_13],
        [&JLSTZ_20, &JLSTZ_21, &EMPTY_KICKS, &JLSTZ_23],
        [&JLSTZ_30, &JLSTZ_31, &JLSTZ_32, &EMPTY_KICKS],
    ],
    // S
    [
        [&EMPTY_KICKS, &JLSTZ_01, &JLSTZ_02, &JLSTZ_03],
        [&JLSTZ_10, &EMPTY_KICKS, &JLSTZ_12, &JLSTZ_13],
        [&JLSTZ_20, &JLSTZ_21, &EMPTY_KICKS, &JLSTZ_23],
        [&JLSTZ_30, &JLSTZ_31, &JLSTZ_32, &EMPTY_KICKS],
    ],
    // Z
    [
        [&EMPTY_KICKS, &JLSTZ_01, &JLSTZ_02, &JLSTZ_03],
        [&JLSTZ_10, &EMPTY_KICKS, &JLSTZ_12, &JLSTZ_13],
        [&JLSTZ_20, &JLSTZ_21, &EMPTY_KICKS, &JLSTZ_23],
        [&JLSTZ_30, &JLSTZ_31, &JLSTZ_32, &EMPTY_KICKS],
    ],
    // J
    [
        [&EMPTY_KICKS, &JLSTZ_01, &JLSTZ_02, &JLSTZ_03],
        [&JLSTZ_10, &EMPTY_KICKS, &JLSTZ_12, &JLSTZ_13],
        [&JLSTZ_20, &JLSTZ_21, &EMPTY_KICKS, &JLSTZ_23],
        [&JLSTZ_30, &JLSTZ_31, &JLSTZ_32, &EMPTY_KICKS],
    ],
    // L
    [
        [&EMPTY_KICKS, &JLSTZ_01, &JLSTZ_02, &JLSTZ_03],
        [&JLSTZ_10, &EMPTY_KICKS, &JLSTZ_12, &JLSTZ_13],
        [&JLSTZ_20, &JLSTZ_21, &EMPTY_KICKS, &JLSTZ_23],
        [&JLSTZ_30, &JLSTZ_31, &JLSTZ_32, &EMPTY_KICKS],
    ],
];

/// Get kicks as const - zero overhead
#[inline(always)]
pub const fn get_kicks_const(piece_idx: usize, from: usize, to: usize) -> &'static [(i8, i8)] {
    SRS_PLUS_KICKS[piece_idx][from][to]
}

/// kick offsets for rotation - returns (dx, dy) to try in order
pub fn get_kicks(piece: Piece, from: Rotation, to: Rotation) -> &'static [(i8, i8)] {
    let key = rotation_key(from, to);

    match piece {
        Piece::I => get_i_kicks(key),
        Piece::O => &[], // O piece doesn't kick
        _ => get_jlstz_kicks(key),
    }
}

/// Get kicks for CW/CCW only (for fast movegen)
pub fn get_kicks_cw_ccw(piece: Piece, from: Rotation, clockwise: bool) -> &'static [(i8, i8)] {
    let to = if clockwise { from.cw() } else { from.ccw() };
    get_kicks(piece, from, to)
}

/// Get 180 kicks only (for fast movegen)
pub fn get_180_kicks(piece: Piece, from: Rotation) -> &'static [(i8, i8)] {
    let to = from.flip();
    get_kicks(piece, from, to)
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

/// JLSTZ kicks - SRS+ spec with (0,0) first
fn get_jlstz_kicks(key: u8) -> &'static [(i8, i8)] {
    match key {
        // CW rotations (from deep_research SRS+ spec)
        1 => &[(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)], // N -> E
        12 => &[(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],    // E -> S
        23 => &[(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],   // S -> W
        30 => &[(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)], // W -> N

        // CCW rotations
        3 => &[(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)], // N -> W
        32 => &[(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)], // W -> S
        21 => &[(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)], // S -> E
        10 => &[(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)], // E -> N

        // 180 kicks - SRS+ extension (6 kicks)
        2 => &[(0, 0), (0, 1), (1, 1), (-1, 1), (1, 0), (-1, 0)], // N -> S
        13 => &[(0, 0), (1, 0), (1, 2), (1, 1), (0, 2), (0, 1)],  // E -> W
        20 => &[(0, 0), (0, -1), (-1, -1), (1, -1), (-1, 0), (1, 0)], // S -> N
        31 => &[(0, 0), (-1, 0), (-1, 2), (-1, 1), (0, 2), (0, 1)], // W -> E

        _ => &[],
    }
}

/// I piece kicks - SRS+ symmetric (from deep_research spec)
fn get_i_kicks(key: u8) -> &'static [(i8, i8)] {
    match key {
        // CW rotations (SRS+ symmetric)
        1 => &[(0, 0), (1, 0), (-2, 0), (-2, -1), (1, 2)], // N -> E
        12 => &[(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)], // E -> S
        23 => &[(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)], // S -> W
        30 => &[(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)], // W -> N

        // CCW rotations (symmetric mirrors)
        3 => &[(0, 0), (-1, 0), (2, 0), (2, 1), (-1, -2)], // N -> W
        32 => &[(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)], // W -> S
        21 => &[(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)], // S -> E
        10 => &[(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)], // E -> N

        // 180 kicks - I-piece has 6 kicks matching Cobra/TETR.IO
        2 => &[(1, -1), (1, 0), (2, 0), (0, 0), (2, -1), (0, -1)], // N -> S
        20 => &[(-1, 1), (-1, 0), (-2, 0), (0, 0), (-2, 1), (0, 1)], // S -> N
        13 => &[(-1, -1), (0, -1), (0, 1), (0, 0), (-1, 1), (-1, 0)], // E -> W
        31 => &[(1, 1), (0, 1), (0, 3), (0, 2), (1, 3), (1, 2)],   // W -> E

        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_t_cw_kicks() {
        // SRS+ spec: 5 kicks including (0,0) first
        let kicks = get_kicks(Piece::T, Rotation::North, Rotation::East);
        assert_eq!(kicks.len(), 5);
        assert_eq!(kicks[0], (0, 0)); // basic rotation first
        assert_eq!(kicks[1], (-1, 0));
    }

    #[test]
    fn test_i_kicks() {
        // I-piece SRS+ symmetric: 5 kicks including (0,0)
        let kicks = get_kicks(Piece::I, Rotation::North, Rotation::East);
        assert_eq!(kicks.len(), 5);
        assert_eq!(kicks[0], (0, 0));
        assert_eq!(kicks[1], (1, 0));
    }

    #[test]
    fn test_o_no_kicks() {
        let kicks = get_kicks(Piece::O, Rotation::North, Rotation::East);
        assert!(kicks.is_empty());
    }

    #[test]
    fn test_180_kicks() {
        // JLSTZ 180 has 6 kicks per SRS+ spec
        let kicks = get_kicks(Piece::T, Rotation::North, Rotation::South);
        assert_eq!(kicks.len(), 6);
        assert_eq!(kicks[0], (0, 0));
    }

    #[test]
    fn test_i_srsplus_kicks() {
        // I-piece CW kicks - 5 total with (0,0) first
        let kicks_ne = get_kicks(Piece::I, Rotation::North, Rotation::East);
        assert_eq!(kicks_ne.len(), 5);
        assert_eq!(kicks_ne[0], (0, 0));
        assert_eq!(kicks_ne[1], (1, 0));

        let kicks_es = get_kicks(Piece::I, Rotation::East, Rotation::South);
        assert_eq!(kicks_es.len(), 5);

        // CCW kicks - symmetric
        let kicks_en = get_kicks(Piece::I, Rotation::East, Rotation::North);
        assert_eq!(kicks_en.len(), 5);
        assert_eq!(kicks_en[0], (0, 0));
    }

    #[test]
    fn test_i_180_kicks() {
        // I-piece 180 has 6 kicks matching Cobra/TETR.IO
        let kicks_ns = get_kicks(Piece::I, Rotation::North, Rotation::South);
        assert_eq!(kicks_ns.len(), 6);
        assert_eq!(kicks_ns[0], (1, -1));

        let kicks_sn = get_kicks(Piece::I, Rotation::South, Rotation::North);
        assert_eq!(kicks_sn.len(), 6);

        let kicks_ew = get_kicks(Piece::I, Rotation::East, Rotation::West);
        assert_eq!(kicks_ew.len(), 6);

        let kicks_we = get_kicks(Piece::I, Rotation::West, Rotation::East);
        assert_eq!(kicks_we.len(), 6);
    }

    #[test]
    fn test_const_kicks_match_runtime() {
        let rotations = [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ];

        for piece in Piece::ALL {
            for from in rotations {
                for to in rotations {
                    let runtime = get_kicks(piece, from, to);
                    let const_kicks = get_kicks_const(piece as usize, from as usize, to as usize);
                    assert_eq!(runtime, const_kicks);
                }
            }
        }
    }
}
