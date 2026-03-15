//! Core types — 1:1 port of header.hpp

pub type Bitboard = u64;

pub const COL_NB: usize = 10;
pub const ROW_NB: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Piece {
    I = 0,
    O = 1,
    T = 2,
    L = 3,
    J = 4,
    S = 5,
    Z = 6,
}

impl Piece {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Piece::I,
            1 => Piece::O,
            2 => Piece::T,
            3 => Piece::L,
            4 => Piece::J,
            5 => Piece::S,
            6 => Piece::Z,
            _ => panic!("invalid Piece discriminant"),
        }
    }
}

pub const PIECE_NB: usize = 7;
/// Sentinel used in Move bitfield to mark T-spin moves
pub const TSPIN: u16 = 7;
pub const NO_PIECE: u8 = 8;

pub const ALL_PIECES: [Piece; PIECE_NB] = [
    Piece::I,
    Piece::O,
    Piece::T,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rotation {
    North = 0,
    East = 1,
    South = 2,
    West = 3,
}

impl Rotation {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Rotation::North,
            1 => Rotation::East,
            2 => Rotation::South,
            3 => Rotation::West,
            _ => panic!("invalid Rotation discriminant"),
        }
    }
}

pub const ROTATION_NB: usize = 4;

pub const ALL_ROTATIONS: [Rotation; ROTATION_NB] = [
    Rotation::North,
    Rotation::East,
    Rotation::South,
    Rotation::West,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SpinType {
    NoSpin = 0,
    Mini = 1,
    Full = 2,
}

impl SpinType {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => SpinType::NoSpin,
            1 => SpinType::Mini,
            2 => SpinType::Full,
            _ => panic!("invalid SpinType discriminant"),
        }
    }
}

pub const SPIN_NB: usize = 3;

// -- Coordinates --

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coordinates {
    pub x: i8,
    pub y: i8,
}

impl Coordinates {
    pub const fn new(x: i32, y: i32) -> Self {
        Self {
            x: x as i8,
            y: y as i8,
        }
    }

    pub const fn add(self, c: Coordinates) -> Coordinates {
        Coordinates {
            x: self.x.wrapping_add(c.x),
            y: self.y.wrapping_add(c.y),
        }
    }

    pub const fn sub(self, c: Coordinates) -> Coordinates {
        Coordinates {
            x: self.x.wrapping_sub(c.x),
            y: self.y.wrapping_sub(c.y),
        }
    }
}

impl std::ops::Add for Coordinates {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Sub for Coordinates {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl std::ops::AddAssign for Coordinates {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl std::ops::SubAssign for Coordinates {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

// -- Move --
// C++ layout: y:6, x:4, piece:3, rotation:2, spin:1 = 16 bits

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Move {
    data: u16,
}

impl Move {
    pub const fn new(p: Piece, r: Rotation, x: i32, y: i32, fullspin: bool) -> Self {
        let piece_val = if fullspin { TSPIN } else { p as u16 };
        let data = (y as u16 & 0x3F)
            | ((x as u16 & 0xF) << 6)
            | ((piece_val & 0x7) << 10)
            | (((r as u16) & 0x3) << 13)
            | ((fullspin as u16) << 15);
        Self { data }
    }

    /// C++ Move(TSPIN, r, x, y, fullspin) — for T-spin move emission
    pub const fn new_tspin(r: Rotation, x: i32, y: i32, fullspin: bool) -> Self {
        let data = (y as u16 & 0x3F)
            | ((x as u16 & 0xF) << 6)
            | ((TSPIN & 0x7) << 10)
            | (((r as u16) & 0x3) << 13)
            | ((fullspin as u16) << 15);
        Self { data }
    }

    pub const fn new_allspin_mini(p: Piece, r: Rotation, x: i32, y: i32) -> Self {
        let data = (y as u16 & 0x3F)
            | ((x as u16 & 0xF) << 6)
            | (((p as u16) & 0x7) << 10)
            | (((r as u16) & 0x3) << 13)
            | (1u16 << 15); // spin_bit = 1
        Self { data }
    }

    pub const fn none() -> Self {
        Self { data: 0 }
    }

    pub const fn piece(self) -> Piece {
        let raw = (self.data >> 10) & 0x7;
        if raw == TSPIN {
            // TSPIN maps to T
            Piece::T
        } else {
            Piece::from_u8(raw as u8)
        }
    }

    pub const fn rotation(self) -> Rotation {
        Rotation::from_u8(((self.data >> 13) & 0x3) as u8)
    }

    pub const fn spin(self) -> SpinType {
        let piece_raw = (self.data >> 10) & 0x7;
        let spin_bit = (self.data >> 15) & 0x1;
        let val = (piece_raw == TSPIN) as u8 + spin_bit as u8;
        SpinType::from_u8(val)
    }

    pub const fn x(self) -> i32 {
        ((self.data >> 6) & 0xF) as i32
    }

    pub const fn y(self) -> i32 {
        (self.data & 0x3F) as i32
    }

    pub const fn raw(self) -> u16 {
        self.data
    }

    pub fn cells(self) -> PieceCoordinates {
        piece_table(self.piece(), self.rotation())
    }
}

impl std::fmt::Debug for Move {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Move({:?}, {:?}, x={}, y={}, spin={:?})",
            self.piece(),
            self.rotation(),
            self.x(),
            self.y(),
            self.spin()
        )
    }
}

// -- PieceCoordinates --
// 3 offsets relative to pivot (pivot itself is implicit at (x,y))

#[derive(Debug, Clone, Copy)]
pub struct PieceCoordinates {
    pub coords: [Coordinates; 3],
}

impl PieceCoordinates {
    pub const fn new(a: Coordinates, b: Coordinates, c: Coordinates) -> Self {
        Self { coords: [a, b, c] }
    }
}

impl std::ops::Index<usize> for PieceCoordinates {
    type Output = Coordinates;
    fn index(&self, i: usize) -> &Self::Output {
        debug_assert!(i < 3);
        &self.coords[i]
    }
}

// -- Validation --

pub const fn is_ok_piece(p: Piece) -> bool {
    (p as u8) < PIECE_NB as u8
}

pub const fn is_ok_rotation(r: Rotation) -> bool {
    (r as u8) < ROTATION_NB as u8
}

pub const fn is_ok_x(x: i32) -> bool {
    x >= 0 && x < COL_NB as i32
}

pub const fn is_ok_y(y: i32) -> bool {
    y >= 0 && y < ROW_NB as i32
}

pub fn is_ok_move(m: &Move) -> bool {
    is_ok_x(m.x()) && is_ok_y(m.y())
}

// -- piece_table --
// C++ constexpr — build piece cells for given piece+rotation

pub const fn make_piece(p: Piece) -> PieceCoordinates {
    use Coordinates as C;
    match p {
        Piece::I => PieceCoordinates::new(C::new(-1, 0), C::new(1, 0), C::new(2, 0)),
        Piece::O => PieceCoordinates::new(C::new(1, 0), C::new(0, 1), C::new(1, 1)),
        Piece::T => PieceCoordinates::new(C::new(-1, 0), C::new(1, 0), C::new(0, 1)),
        Piece::L => PieceCoordinates::new(C::new(-1, 0), C::new(1, 0), C::new(1, 1)),
        Piece::J => PieceCoordinates::new(C::new(-1, 0), C::new(1, 0), C::new(-1, 1)),
        Piece::S => PieceCoordinates::new(C::new(-1, 0), C::new(0, 1), C::new(1, 1)),
        Piece::Z => PieceCoordinates::new(C::new(-1, 1), C::new(0, 1), C::new(1, 0)),
    }
}

const fn rotate_coord(r: Rotation, c: Coordinates) -> Coordinates {
    match r {
        Rotation::East => Coordinates::new(c.y as i32, -(c.x as i32)),
        Rotation::South => Coordinates::new(-(c.x as i32), -(c.y as i32)),
        Rotation::West => Coordinates::new(-(c.y as i32), c.x as i32),
        Rotation::North => c,
    }
}

pub const fn piece_table(p: Piece, r: Rotation) -> PieceCoordinates {
    let cells = make_piece(p);
    PieceCoordinates::new(
        rotate_coord(r, cells.coords[0]),
        rotate_coord(r, cells.coords[1]),
        rotate_coord(r, cells.coords[2]),
    )
}

// -- Bitboard operations --

pub const fn clz(v: Bitboard) -> u32 {
    if v != 0 {
        v.leading_zeros()
    } else {
        64
    }
}

pub const fn ctz(v: Bitboard) -> u32 {
    debug_assert!(v != 0);
    v.trailing_zeros()
}

pub const fn popcount(v: Bitboard) -> u32 {
    v.count_ones()
}

pub const fn bitlen(v: Bitboard) -> u32 {
    64 - clz(v)
}

/// 1 << v
pub const fn bb(v: i32) -> Bitboard {
    debug_assert!(v >= 0);
    1u64 << v
}

/// (1 << v) - 1
pub const fn bb_low(v: i32) -> Bitboard {
    debug_assert!(v >= 0);
    (1u64 << v) - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_piece_table_i_north() {
        let pc = piece_table(Piece::I, Rotation::North);
        assert_eq!((pc[0].x, pc[0].y), (-1, 0));
        assert_eq!((pc[1].x, pc[1].y), (1, 0));
        assert_eq!((pc[2].x, pc[2].y), (2, 0));
    }

    #[test]
    fn test_piece_table_i_east() {
        let pc = piece_table(Piece::I, Rotation::East);
        // EAST: (y, -x) applied to (-1,0),(1,0),(2,0)
        assert_eq!((pc[0].x, pc[0].y), (0, 1));
        assert_eq!((pc[1].x, pc[1].y), (0, -1));
        assert_eq!((pc[2].x, pc[2].y), (0, -2));
    }

    #[test]
    fn test_move_roundtrip() {
        let m = Move::new(Piece::T, Rotation::East, 5, 10, false);
        assert_eq!(m.piece(), Piece::T);
        assert_eq!(m.rotation(), Rotation::East);
        assert_eq!(m.x(), 5);
        assert_eq!(m.y(), 10);
        assert_eq!(m.spin(), SpinType::NoSpin);
    }

    #[test]
    fn test_move_tspin() {
        // T-spin FULL: piece=TSPIN(7), fullspin=true → spin = (7==7) + 1 = 2 = FULL
        let m = Move::new_tspin(Rotation::South, 3, 5, true);
        assert_eq!(m.piece(), Piece::T);
        assert_eq!(m.spin(), SpinType::Full);
    }

    #[test]
    fn test_move_tspin_mini() {
        // T-spin MINI: piece=TSPIN(7), fullspin=false → spin = (7==7) + 0 = 1 = MINI
        let m = Move::new_tspin(Rotation::North, 4, 0, false);
        assert_eq!(m.piece(), Piece::T);
        assert_eq!(m.spin(), SpinType::Mini);
    }

    #[test]
    fn test_bitboard_ops() {
        assert_eq!(clz(0), 64);
        assert_eq!(clz(1), 63);
        assert_eq!(popcount(0b1011), 3);
        assert_eq!(bitlen(0b1000), 4);
        assert_eq!(bb(3), 8);
        assert_eq!(bb_low(3), 7);
    }
}
