// board.rs -- row-major board using [u16; 40]
// Y-up convention (row 0 = bottom), matching Cobra

use crate::header::*;
use std::fmt;

pub const BOARD_HEIGHT: usize = 40;
pub const FULL_ROW: u16 = (1 << COL_NB) - 1; // 0x3FF

pub struct Board {
    pub rows: [u16; BOARD_HEIGHT],
    pub cols: [Bitboard; COL_NB],
}

impl Board {
    pub fn new() -> Self {
        Board {
            rows: [0; BOARD_HEIGHT],
            cols: [0; COL_NB],
        }
    }

    pub fn occupied(&self, x: i32, y: i32) -> bool {
        let yu = y as usize;
        if yu >= BOARD_HEIGHT {
            return false;
        }
        self.rows[yu] & (1 << x) != 0
    }

    pub fn occupied_coord(&self, c: &Coordinates) -> bool {
        self.occupied(c.x as i32, c.y as i32)
    }

    pub fn obstructed(&self, x: i32, y: i32) -> bool {
        !is_ok_x(x) || !is_ok_y(y) || self.occupied(x, y)
    }

    pub fn obstructed_coord(&self, c: &Coordinates) -> bool {
        self.obstructed(c.x as i32, c.y as i32)
    }

    pub fn obstructed_move(&self, m: &Move) -> bool {
        let pc = m.cells();
        let off = Coordinates::new(m.x(), m.y());
        self.obstructed_coord(&off)
            || self.obstructed_coord(&(pc[0] + off))
            || self.obstructed_coord(&(pc[1] + off))
            || self.obstructed_coord(&(pc[2] + off))
    }

    pub fn legal_lock_placement(&self, m: &Move) -> bool {
        if !is_ok_move(m) || self.obstructed_move(m) {
            return false;
        }

        let pc = m.cells();
        let off = Coordinates::new(m.x(), m.y());
        let cells = [off, pc[0] + off, pc[1] + off, pc[2] + off];

        cells
            .iter()
            .any(|c| self.obstructed(c.x as i32, c.y as i32 - 1))
    }

    /// Build a column bitboard on-the-fly from row data.
    /// Bit y of result is set iff cell (x, y) is occupied.
    pub fn col(&self, x: usize) -> Bitboard {
        let mask = 1u16 << x;
        let mut result: Bitboard = 0;
        for y in 0..BOARD_HEIGHT {
            if self.rows[y] & mask != 0 {
                result |= 1u64 << y;
            }
        }
        result
    }

    /// Return cached column bitboards — O(1).
    /// Maintained in sync with rows by place/clear_lines/spawn_garbage/clear.
    #[inline(always)]
    pub fn compute_cols(&self) -> [Bitboard; COL_NB] {
        self.cols
    }

    /// Rebuild cols cache from rows. Used after bulk mutations (clear_lines, spawn_garbage).
    fn rebuild_cols(&mut self) {
        self.cols = [0; COL_NB];
        for y in 0..BOARD_HEIGHT {
            let row = self.rows[y];
            if row == 0 {
                continue;
            }
            let mut bits = row as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                self.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
    }

    pub fn empty(&self) -> bool {
        self.rows.iter().all(|&r| r == 0)
    }

    pub fn line_clears(&self) -> Bitboard {
        let mut result: Bitboard = 0;
        for y in 0..BOARD_HEIGHT {
            if self.rows[y] == FULL_ROW {
                result |= 1u64 << y;
            }
        }
        result
    }

    pub fn clear(&mut self) {
        self.rows = [0; BOARD_HEIGHT];
        self.cols = [0; COL_NB];
    }

    /// Remove filled lines and compact remaining rows down.
    pub fn clear_lines(&mut self, l: Bitboard) {
        debug_assert!(l != 0);
        let mut write = 0usize;
        for read in 0..BOARD_HEIGHT {
            if l & (1u64 << read) == 0 {
                self.rows[write] = self.rows[read];
                write += 1;
            }
        }
        for y in write..BOARD_HEIGHT {
            self.rows[y] = 0;
        }
        // Compress each cleared row out of the column bitsets directly,
        // highest row first so lower bit positions stay valid; this replaces
        // the full rows scan of rebuild_cols on the do_move hot path.
        let mut rem = l;
        while rem != 0 {
            let y = 63 - rem.leading_zeros();
            let low = (1u64 << y) - 1;
            for c in self.cols.iter_mut() {
                *c = (*c & low) | ((*c >> 1) & !low);
            }
            rem &= !(1u64 << y);
        }
    }

    pub fn place(&mut self, m: &Move) {
        let pc = m.cells();
        let x = m.x();
        let y = m.y();

        let xu = x as usize;
        let yu = y as usize;
        if xu < COL_NB && yu < BOARD_HEIGHT {
            self.rows[yu] |= 1 << x;
            self.cols[xu] |= 1u64 << y;
        }

        for i in 0..3 {
            let cx = (pc[i].x as i32 + x) as usize;
            let cy = (pc[i].y as i32 + y) as usize;
            if cx < COL_NB && cy < BOARD_HEIGHT {
                self.rows[cy] |= 1 << cx;
                self.cols[cx] |= 1u64 << cy;
            }
        }
    }

    pub fn spawn_garbage(&mut self, lines: i32, x: i32) {
        debug_assert!(is_ok_x(x));
        debug_assert!(lines > 0);
        let n = lines as usize;
        for y in (n..BOARD_HEIGHT).rev() {
            self.rows[y] = self.rows[y - n];
        }
        let garbage_row = FULL_ROW & !(1u16 << x);
        for y in 0..n {
            self.rows[y] = garbage_row;
        }
        self.rebuild_cols();
    }

    pub fn do_move(&mut self, m: &Move) -> i32 {
        if !self.legal_lock_placement(m) {
            return 0;
        }

        self.place(m);
        let clears = self.line_clears();
        if clears == 0 {
            return 0;
        }

        self.clear_lines(clears);
        popcount(clears) as i32
    }

    /// Max occupied row index + 1 (= height)
    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(|&r| r == 0)
    }

    pub fn height(&self) -> u32 {
        for y in (0..BOARD_HEIGHT).rev() {
            if self.rows[y] != 0 {
                return y as u32 + 1;
            }
        }
        0
    }

    pub fn to_string_with_move(&self, m: &Move) -> String {
        let mut output = self.to_string();
        if !self.obstructed_move(m) {
            let lines: i32 = 20;
            let pc = m.cells();
            let x = m.x();
            let y = m.y();
            for i in 0..4usize {
                let inverse_y = lines - if i == 0 { y } else { pc[i - 1].y as i32 + y };
                if inverse_y < 0 {
                    continue;
                }
                let cell_x = if i == 0 { x } else { pc[i - 1].x as i32 + x };
                let idx = (inverse_y * 86 + cell_x * 4 + 47) as usize;
                if idx < output.len() {
                    unsafe {
                        output.as_bytes_mut()[idx] = b'.';
                    }
                }
            }
        }
        output
    }

    pub fn row(&self, y: usize) -> u16 {
        self.rows[y]
    }
}

impl Clone for Board {
    fn clone(&self) -> Self {
        Board {
            rows: self.rows,
            cols: self.cols,
        }
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

// -- MoveInfo --

pub struct MoveInfo {
    pub piece: Piece,
    pub spin: SpinType,
    pub clear: i32,
    pub b2b: i16,
    pub combo: i16,
    pub pc: bool,
}

// -- State --

#[derive(Clone)]
pub struct State {
    pub board: Board,
    pub hold: Option<Piece>,
    pub b2b: i16,
    pub combo: i16,
}

impl State {
    pub fn init(&mut self) {
        self.board.clear();
        self.hold = None;
        self.b2b = 0;
        self.combo = 0;
    }

    pub fn new() -> Self {
        State {
            board: Board::new(),
            hold: None,
            b2b: 0,
            combo: 0,
        }
    }

    pub fn do_move(&mut self, m: &Move) -> MoveInfo {
        debug_assert!(is_ok_move(m));

        let clear_count = self.board.do_move(m);
        if clear_count == 0 {
            self.combo = 0;
            return MoveInfo {
                piece: m.piece(),
                spin: SpinType::NoSpin,
                clear: 0,
                b2b: 0,
                combo: 0,
                pc: false,
            };
        }

        let spin = m.spin();
        let has_spin = spin != SpinType::NoSpin;

        self.b2b = if has_spin || clear_count == 4 {
            self.b2b + 1
        } else {
            0
        };
        self.combo += 1;

        MoveInfo {
            piece: m.piece(),
            spin,
            clear: clear_count,
            b2b: self.b2b,
            combo: self.combo,
            pc: self.board.empty(),
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines = 20;
        let mut output = String::with_capacity((lines + 1) * 86 + 44);
        output.push_str("\n +---+---+---+---+---+---+---+---+---+---+\n");
        for y in (0..=lines).rev() {
            for x in 0..COL_NB {
                output.push_str(" | ");
                output.push(if self.rows[y] & (1 << x) != 0 {
                    '#'
                } else {
                    ' '
                });
            }
            output.push_str(" |\n +---+---+---+---+---+---+---+---+---+---+\n");
        }
        write!(f, "{}", output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_board() {
        let board = Board::new();
        assert!(board.empty());
    }

    #[test]
    fn test_place_and_occupied() {
        let mut board = Board::new();
        let m = Move::new(Piece::T, Rotation::North, 4, 0, false);
        board.place(&m);
        assert!(board.occupied(4, 0));
    }

    #[test]
    fn test_line_clear() {
        let mut board = Board::new();
        board.rows[0] = FULL_ROW;
        let clears = board.line_clears();
        assert_eq!(clears & bb(0), bb(0));

        board.clear_lines(clears);
        assert!(board.empty());
    }

    #[test]
    fn test_spawn_garbage() {
        let mut board = Board::new();
        board.spawn_garbage(1, 3);
        for x in 0..COL_NB {
            if x == 3 {
                assert!(!board.occupied(x as i32, 0));
            } else {
                assert!(board.occupied(x as i32, 0));
            }
        }
    }

    #[test]
    fn test_col_roundtrip() {
        let mut board = Board::new();
        board.rows[0] = 0b0000010000; // col 4
        board.rows[5] = 0b0000010000; // col 4
                                      // col() reads from rows directly, not the cache
        let col4 = board.col(4);
        assert_eq!(col4, (1u64 << 0) | (1u64 << 5));
        // verify cache matches after rebuild
        board.rebuild_cols();
        assert_eq!(board.cols[4], col4);
    }

    #[test]
    fn test_height() {
        let mut board = Board::new();
        assert_eq!(board.height(), 0);
        board.rows[0] = 1;
        assert_eq!(board.height(), 1);
        board.rows[10] = 1;
        assert_eq!(board.height(), 11);
    }

    #[test]
    fn test_do_move_rejects_obstructed_overlap() {
        let mut board = Board::new();
        board.rows[0] = 0b0000010000;
        board.rebuild_cols();

        let m = Move::new(Piece::T, Rotation::North, 4, 0, false);
        let before = board.rows;
        let clears = board.do_move(&m);

        assert_eq!(clears, 0);
        assert_eq!(board.rows, before);
    }

    #[test]
    fn test_do_move_rejects_floating_lock() {
        let mut board = Board::new();
        let m = Move::new(Piece::T, Rotation::North, 4, 10, false);

        let before = board.rows;
        let clears = board.do_move(&m);

        assert_eq!(clears, 0);
        assert_eq!(board.rows, before);
    }

    #[test]
    fn clear_lines_cols_match_rebuild_on_seeded_boards() {
        let mut state = 0x5EED_C1EA_2026_0611u64;
        let mut xs = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..4000 {
            let mut board = Board::new();
            let h = 2 + (xs() % 16) as usize;
            for y in 0..h {
                board.rows[y] = (xs() & 0x3FF) as u16;
            }
            let full = 1 + (xs() % 3) as usize;
            for _ in 0..full {
                let y = (xs() % h as u64) as usize;
                board.rows[y] = FULL_ROW;
            }
            board.rebuild_cols();
            let l = board.line_clears();
            assert_ne!(l, 0, "case={case}");
            board.clear_lines(l);

            let mut oracle = [0u64; COL_NB];
            for y in 0..BOARD_HEIGHT {
                let mut bits = board.rows[y] as u64;
                while bits != 0 {
                    let x = bits.trailing_zeros() as usize;
                    oracle[x] |= 1u64 << y;
                    bits &= bits - 1;
                }
            }
            assert_eq!(board.cols, oracle, "case={case} l={l:#x}");
        }
    }

    #[test]
    fn do_move_clears_match_full_scan_on_seeded_placements() {
        use crate::move_buffer::MoveBuffer;
        let mut state = 0xD0_30FE_2026_0611u64;
        let mut xs = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let pieces = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ];
        let mut clearing = 0u32;
        for case in 0..12000 {
            let mut board = Board::new();
            let h = 1 + (xs() % 12) as usize;
            for y in 0..h {
                let mut row = (xs() & 0x3FF) as u16;
                if xs() % 3 != 0 {
                    row = FULL_ROW;
                }
                board.rows[y] = row & !(1u16 << (xs() % 10));
            }
            board.rebuild_cols();
            assert_eq!(
                board.line_clears(),
                0,
                "case={case} seeded board must start clear-free"
            );

            let p = pieces[(xs() % 7) as usize];
            let mut moves = MoveBuffer::new();
            crate::movegen::generate(&board, &mut moves, p, false);
            if moves.len() == 0 {
                continue;
            }
            let m = moves.as_slice()[(xs() % moves.len() as u64) as usize];
            if !board.legal_lock_placement(&m) {
                continue;
            }

            let mut oracle_board = board.clone();
            oracle_board.place(&m);
            let oracle_clears = oracle_board.line_clears();
            if oracle_clears != 0 {
                oracle_board.clear_lines(oracle_clears);
                clearing += 1;
            }

            let cleared = board.do_move(&m);
            assert_eq!(cleared, popcount(oracle_clears) as i32, "case={case}");
            assert_eq!(board.rows, oracle_board.rows, "case={case}");
            assert_eq!(board.cols, oracle_board.cols, "case={case}");
        }
        assert!(
            clearing > 200,
            "want real clearing coverage, got {clearing}"
        );
    }
}
