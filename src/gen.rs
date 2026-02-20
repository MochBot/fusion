// gen.rs -- 1:1 port of gen.hpp
use crate::header::*;

pub const SPAWN_COL: usize = 4;

// C++ in_bounds<p, r>(x): checks pivot x and all 3 relative cells are valid columns
pub fn in_bounds(p: Piece, r: Rotation, x: i32) -> bool {
    if !is_ok_x(x) {
        return false;
    }
    let pc = piece_table(p, r);
    is_ok_x(pc[0].x as i32 + x) && is_ok_x(pc[1].x as i32 + x) && is_ok_x(pc[2].x as i32 + x)
}

pub const fn group2(p: Piece) -> bool {
    matches!(p, Piece::I | Piece::S | Piece::Z)
}

pub const fn canonical_size(p: Piece) -> usize {
    match p {
        Piece::O => 1,
        Piece::I | Piece::S | Piece::Z => 2,
        _ => 4, // L, J, T
    }
}

pub fn canonical_r(p: Piece, r: Rotation) -> Rotation {
    match p {
        Piece::O => Rotation::North,
        Piece::I | Piece::S | Piece::Z => {
            // r & 1: North/South -> North(0), East/West -> East(1)
            Rotation::from_u8((r as u8) & 1)
        }
        _ => r, // L, J, T
    }
}

pub fn canonical_offset(p: Piece, r: Rotation) -> Coordinates {
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
pub enum Direction {
    CW = 0,
    CCW = 1,
    Flip = 2,
}

pub const DIRECTION_NB: usize = 2; // CW and CCW only (Flip is separate)

pub fn rotate(d: Direction, r: Rotation) -> Rotation {
    let ri = r as u8;
    let result = match d {
        Direction::CW => (ri + 1) & 3,
        Direction::CCW => (ri + 3) & 3,
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
pub type Offsets5 = [Coordinates; 5];
pub type OffsetsRot5 = [Offsets5; ROTATION_NB as usize];

pub type Offsets6 = [Coordinates; 6];
pub type OffsetsRot6 = [Offsets6; ROTATION_NB as usize];

macro_rules! c {
    ($x:expr, $y:expr) => {
        Coordinates { x: $x, y: $y }
    };
}

pub static KICKS: [[[Offsets5; ROTATION_NB as usize]; DIRECTION_NB]; 3] = [
    // [0] LJSZT
    [
        // CW
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

pub static KICKS_180: [[Offsets6; ROTATION_NB as usize]; 2] = [
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
pub fn kick_index(p: Piece, srs_plus: bool) -> usize {
    let is_i = (p == Piece::I) as usize;
    if srs_plus {
        is_i * 2
    } else {
        is_i
    }
}

pub fn kick_180_index(p: Piece) -> usize {
    (p == Piece::I) as usize
}

// -- CollisionMap --
// C++ CollisionMap<p>: board[COL_NB][canonicalSize] of Bitboard
// Each entry is OR of column bitboards shifted by piece cell offsets
pub struct CollisionMap {
    pub board: [[Bitboard; 4]; COL_NB as usize], // max 4 canonical rotations
    pub canonical_size: usize,
}

impl CollisionMap {
    pub fn new(cols: &[Bitboard; COL_NB as usize], p: Piece) -> Self {
        let cs = canonical_size(p);
        let mut board = [[0u64; 4]; COL_NB as usize];

        for x in 0..COL_NB as i32 {
            for ri in 0..cs {
                let r: Rotation = Rotation::from_u8(ri as u8);
                if !in_bounds(p, r, x) {
                    board[x as usize][ri] = !0u64;
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
                board[x as usize][ri] = result;
            }
        }

        CollisionMap {
            board,
            canonical_size: cs,
        }
    }

    pub fn get(&self, x: usize, r: Rotation) -> Bitboard {
        self.board[x][r as usize]
    }
}

// -- CollisionMap16 --
// C++ CollisionMap16<p>: board[COL_NB] single Bitboard per column
// 4 rotations packed in 16-bit lanes: bits [0..15]=North, [16..31]=East, etc.
pub struct CollisionMap16 {
    pub board: [Bitboard; COL_NB as usize],
}

impl CollisionMap16 {
    pub fn new(cols: &[Bitboard; COL_NB as usize], p: Piece) -> Self {
        let mut board = [0u64; COL_NB as usize];

        for x in 0..COL_NB as i32 {
            let mut val: Bitboard = 0;
            for ri in 0..ROTATION_NB as u8 {
                let r: Rotation = Rotation::from_u8(ri);
                let rr = canonical_r(p, r);
                let lane: u64;

                if !in_bounds(p, rr, x) {
                    lane = 0xFFFFu64;
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
                    lane = result & 0xFFFFu64;
                }

                val |= lane << (ri as u32 * 16);
            }
            board[x as usize] = val;
        }

        CollisionMap16 { board }
    }

    pub fn get(&self, x: usize) -> Bitboard {
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
        assert_eq!(rotate(Direction::CW, Rotation::North), Rotation::East);
        assert_eq!(rotate(Direction::CW, Rotation::West), Rotation::North);
        assert_eq!(rotate(Direction::CCW, Rotation::North), Rotation::West);
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
        assert_eq!(KICKS[0][0].len(), ROTATION_NB as usize);
        assert_eq!(KICKS[0][0][0].len(), 5);
        assert_eq!(KICKS_180[0].len(), ROTATION_NB as usize);
        assert_eq!(KICKS_180[0][0].len(), 6);
    }
}
