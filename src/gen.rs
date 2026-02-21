// gen.rs -- 1:1 port of gen.hpp
use crate::header::*;

pub(crate) const SPAWN_COL: usize = 4;

// C++ in_bounds<p, r>(x): checks pivot x and all 3 relative cells are valid columns
pub(crate) fn in_bounds(p: Piece, r: Rotation, x: i32) -> bool {
    if !is_ok_x(x) {
        return false;
    }
    let pc = piece_table(p, r);
    is_ok_x(pc[0].x as i32 + x) && is_ok_x(pc[1].x as i32 + x) && is_ok_x(pc[2].x as i32 + x)
}

pub(crate) const fn group2(p: Piece) -> bool {
    matches!(p, Piece::I | Piece::S | Piece::Z)
}

pub(crate) const fn canonical_size(p: Piece) -> usize {
    match p {
        Piece::O => 1,
        Piece::I | Piece::S | Piece::Z => 2,
        _ => 4, // L, J, T
    }
}

pub(crate) fn canonical_r(p: Piece, r: Rotation) -> Rotation {
    match p {
        Piece::O => Rotation::North,
        Piece::I | Piece::S | Piece::Z => {
            // r & 1: North/South -> North(0), East/West -> East(1)
            Rotation::from_u8((r as u8) & 1)
        }
        _ => r, // L, J, T
    }
}

pub(crate) fn canonical_offset(p: Piece, r: Rotation) -> Coordinates {
    match p {
        Piece::I => match r {
            Rotation::South => Coordinates::new(1, 0),
            Rotation::West => Coordinates::new(0, -1),
            _ => Coordinates::new(0, 0),
        },
        Piece::S | Piece::Z => match r {
            Rotation::South => Coordinates::new(0, 1),
            Rotation::West => Coordinates::new(1, 0),
            _ => Coordinates::new(0, 0),
        },
        _ => Coordinates::new(0, 0),
    }
}

// -- Direction --
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum Direction {
    Cw = 0,
    Ccw = 1,
    Flip = 2,
}

pub(crate) const DIRECTION_NB: usize = 2; // Cw and Ccw only (Flip is separate)

pub(crate) fn rotate(d: Direction, r: Rotation) -> Rotation {
    let ri = r as u8;
    let result = match d {
        Direction::Cw => (ri + 1) & 3,
        Direction::Ccw => (ri + 3) & 3,
        Direction::Flip => (ri + 2) & 3,
    };
    Rotation::from_u8(result)
}

// -- Kick tables --
// Offsets<N> = [Coordinates; N]
// OffsetsRot<N> = [[Coordinates; N]; ROTATION_NB]

// kicks[3][2]: [kick_set][direction]
//   kick_set 0 = LJSZT, 1 = I SRS, 2 = I SRS+
//   direction 0 = CW, 1 = CCW
// Each entry: [4 rotations][5 offsets]
pub(crate) type Offsets5 = [Coordinates; 5];

pub(crate) type Offsets6 = [Coordinates; 6];

macro_rules! c {
    ($x:expr, $y:expr) => {
        Coordinates { x: $x, y: $y }
    };
}

pub(crate) static KICKS: [[[Offsets5; ROTATION_NB]; DIRECTION_NB]; 3] = [
    // [0] LJSZT
    [
        // Cw
        [
            [c!(0, 0), c!(-1, 0), c!(-1, 1), c!(0, -2), c!(-1, -2)],
            [c!(0, 0), c!(1, 0), c!(1, -1), c!(0, 2), c!(1, 2)],
            [c!(0, 0), c!(1, 0), c!(1, 1), c!(0, -2), c!(1, -2)],
            [c!(0, 0), c!(-1, 0), c!(-1, -1), c!(0, 2), c!(-1, 2)],
        ],
        // CCW
        [
            [c!(0, 0), c!(1, 0), c!(1, 1), c!(0, -2), c!(1, -2)],
            [c!(0, 0), c!(1, 0), c!(1, -1), c!(0, 2), c!(1, 2)],
            [c!(0, 0), c!(-1, 0), c!(-1, 1), c!(0, -2), c!(-1, -2)],
            [c!(0, 0), c!(-1, 0), c!(-1, -1), c!(0, 2), c!(-1, 2)],
        ],
    ],
    // [1] I SRS
    [
        // CW
        [
            [c!(1, 0), c!(-1, 0), c!(2, 0), c!(-1, -1), c!(2, 2)],
            [c!(0, -1), c!(-1, -1), c!(2, -1), c!(-1, 1), c!(2, -2)],
            [c!(-1, 0), c!(1, 0), c!(-2, 0), c!(1, 1), c!(-2, -2)],
            [c!(0, 1), c!(1, 1), c!(-2, 1), c!(1, -1), c!(-2, 2)],
        ],
        // CCW
        [
            [c!(0, -1), c!(-1, -1), c!(2, -1), c!(-1, 1), c!(2, -2)],
            [c!(-1, 0), c!(1, 0), c!(-2, 0), c!(1, 1), c!(-2, -2)],
            [c!(0, 1), c!(1, 1), c!(-2, 1), c!(1, -1), c!(-2, 2)],
            [c!(1, 0), c!(-1, 0), c!(2, 0), c!(-1, -1), c!(2, 2)],
        ],
    ],
    // [2] I SRS+
    [
        // CW
        [
            [c!(1, 0), c!(2, 0), c!(-1, 0), c!(-1, -1), c!(2, 2)],
            [c!(0, -1), c!(-1, -1), c!(2, -1), c!(-1, 1), c!(2, -2)],
            [c!(-1, 0), c!(1, 0), c!(-2, 0), c!(1, 1), c!(-2, -2)],
            [c!(0, 1), c!(1, 1), c!(-2, 1), c!(1, -1), c!(-2, 2)],
        ],
        // CCW
        [
            [c!(0, -1), c!(-1, -1), c!(2, -1), c!(2, -2), c!(-1, 1)],
            [c!(-1, 0), c!(-2, 0), c!(1, 0), c!(-2, -2), c!(1, 1)],
            [c!(0, 1), c!(-2, 1), c!(1, 1), c!(-2, 2), c!(1, -1)],
            [c!(1, 0), c!(2, 0), c!(-1, 0), c!(2, 2), c!(-1, -1)],
        ],
    ],
];

pub(crate) static KICKS_180: [[Offsets6; ROTATION_NB]; 2] = [
    // [0] LJSZT
    [
        [c!(0, 0), c!(0, 1), c!(1, 1), c!(-1, 1), c!(1, 0), c!(-1, 0)],
        [c!(0, 0), c!(1, 0), c!(1, 2), c!(1, 1), c!(0, 2), c!(0, 1)],
        [
            c!(0, 0),
            c!(0, -1),
            c!(-1, -1),
            c!(1, -1),
            c!(-1, 0),
            c!(1, 0),
        ],
        [
            c!(0, 0),
            c!(-1, 0),
            c!(-1, 2),
            c!(-1, 1),
            c!(0, 2),
            c!(0, 1),
        ],
    ],
    // [1] I
    [
        [
            c!(1, -1),
            c!(1, 0),
            c!(2, 0),
            c!(0, 0),
            c!(2, -1),
            c!(0, -1),
        ],
        [
            c!(-1, -1),
            c!(0, -1),
            c!(0, 1),
            c!(0, 0),
            c!(-1, 1),
            c!(-1, 0),
        ],
        [
            c!(-1, 1),
            c!(-1, 0),
            c!(-2, 0),
            c!(0, 0),
            c!(-2, 1),
            c!(0, 1),
        ],
        [c!(1, 1), c!(0, 1), c!(0, 3), c!(0, 2), c!(1, 3), c!(1, 2)],
    ],
];

// kick table index: srs_plus uses (p==I)*2, srs uses (p==I)
pub(crate) fn kick_index(p: Piece, srs_plus: bool) -> usize {
    let is_i = (p == Piece::I) as usize;
    if srs_plus {
        is_i * 2
    } else {
        is_i
    }
}

pub(crate) fn kick_180_index(p: Piece) -> usize {
    (p == Piece::I) as usize
}

// -- CollisionMap --
// C++ CollisionMap<p>: board[COL_NB][canonicalSize] of Bitboard
// Each entry is OR of column bitboards shifted by piece cell offsets
pub(crate) struct CollisionMap {
    pub(crate) board: [[Bitboard; 4]; COL_NB], // max 4 canonical rotations
}

impl CollisionMap {
    pub(crate) fn new(cols: &[Bitboard; COL_NB], p: Piece) -> Self {
        let cs = canonical_size(p);
        let mut board = [[0u64; 4]; COL_NB];

        for x in 0..COL_NB as i32 {
            for (ri, entry) in board[x as usize].iter_mut().enumerate().take(cs) {
                let r: Rotation = Rotation::from_u8(ri as u8);
                if !in_bounds(p, r, x) {
                    *entry = !0u64;
                    continue;
                }
                let pc = piece_table(p, r);
                let mut result = cols[x as usize];
                for k in 0..3 {
                    let cx = x + pc[k].x as i32;
                    let cy = pc[k].y as i32;
                    if cy < 0 {
                        result |= !((!cols[cx as usize]) << ((-cy) as u32));
                    } else {
                        result |= cols[cx as usize] >> (cy as u32);
                    }
                }
                *entry = result;
            }
        }

        CollisionMap { board }
    }

    pub(crate) fn get(&self, x: usize, r: Rotation) -> Bitboard {
        self.board[x][r as usize]
    }
}

// -- CollisionMap16 --
// C++ CollisionMap16<p>: board[COL_NB] single Bitboard per column
// 4 rotations packed in 16-bit lanes: bits [0..15]=North, [16..31]=East, etc.
pub(crate) struct CollisionMap16 {
    pub(crate) board: [Bitboard; COL_NB],
}

impl CollisionMap16 {
    pub(crate) fn new(cols: &[Bitboard; COL_NB], p: Piece) -> Self {
        let mut board = [0u64; COL_NB];

        for x in 0..COL_NB as i32 {
            let mut val: Bitboard = 0;
            for ri in 0..ROTATION_NB as u8 {
                let r: Rotation = Rotation::from_u8(ri);
                let rr = canonical_r(p, r);

                let lane = if !in_bounds(p, rr, x) {
                    0xFFFFu64
                } else {
                    let pc = piece_table(p, rr);
                    let mut result = cols[x as usize];
                    for k in 0..3 {
                        let cx = x + pc[k].x as i32;
                        let cy = pc[k].y as i32;
                        if cy < 0 {
                            result |= !((!cols[cx as usize]) << ((-cy) as u32));
                        } else {
                            result |= cols[cx as usize] >> (cy as u32);
                        }
                    }
                    result & 0xFFFFu64
                };

                val |= lane << (ri as u32 * 16);
            }
            board[x as usize] = val;
        }

        CollisionMap16 { board }
    }

    pub(crate) fn get(&self, x: usize) -> Bitboard {
        self.board[x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;

    #[test]
    fn test_canonical_r() {
        assert_eq!(canonical_r(Piece::O, Rotation::East), Rotation::North);
        assert_eq!(canonical_r(Piece::I, Rotation::North), Rotation::North);
        assert_eq!(canonical_r(Piece::I, Rotation::East), Rotation::East);
        assert_eq!(canonical_r(Piece::I, Rotation::South), Rotation::North);
        assert_eq!(canonical_r(Piece::I, Rotation::West), Rotation::East);
        assert_eq!(canonical_r(Piece::T, Rotation::South), Rotation::South);
    }

    #[test]
    fn test_canonical_offset() {
        assert_eq!(
            canonical_offset(Piece::I, Rotation::South),
            Coordinates::new(1, 0)
        );
        assert_eq!(
            canonical_offset(Piece::I, Rotation::West),
            Coordinates::new(0, -1)
        );
        assert_eq!(
            canonical_offset(Piece::I, Rotation::North),
            Coordinates::new(0, 0)
        );
        assert_eq!(
            canonical_offset(Piece::S, Rotation::South),
            Coordinates::new(0, 1)
        );
        assert_eq!(
            canonical_offset(Piece::S, Rotation::West),
            Coordinates::new(1, 0)
        );
        assert_eq!(
            canonical_offset(Piece::T, Rotation::South),
            Coordinates::new(0, 0)
        );
    }

    #[test]
    fn test_rotate_direction() {
        assert_eq!(rotate(Direction::Cw, Rotation::North), Rotation::East);
        assert_eq!(rotate(Direction::Cw, Rotation::West), Rotation::North);
        assert_eq!(rotate(Direction::Ccw, Rotation::North), Rotation::West);
        assert_eq!(rotate(Direction::Flip, Rotation::North), Rotation::South);
    }

    #[test]
    fn test_collision_map_empty_board() {
        let b = Board::new();
        let cols = b.compute_cols();
        let cm = CollisionMap::new(&cols, Piece::T);
        assert_eq!(cm.get(4, Rotation::North), 0);
    }

    #[test]
    fn test_in_bounds() {
        assert!(!in_bounds(Piece::T, Rotation::North, 0));
        assert!(in_bounds(Piece::T, Rotation::North, 1));
    }

    #[test]
    fn test_kick_tables_size() {
        assert_eq!(KICKS[0][0].len(), ROTATION_NB);
        assert_eq!(KICKS[0][0][0].len(), 5);
        assert_eq!(KICKS_180[0].len(), ROTATION_NB);
        assert_eq!(KICKS_180[0][0].len(), 6);
    }
}
