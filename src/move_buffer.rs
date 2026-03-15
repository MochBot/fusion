use crate::board::Board;
use crate::header::*;
use crate::movegen::generate;

pub const MAX_MOVES: usize = 256;

pub struct MoveBuffer {
    data: [Move; MAX_MOVES],
    len: usize,
}

impl MoveBuffer {
    #[inline]
    pub fn new() -> Self {
        MoveBuffer {
            data: [Move::none(); MAX_MOVES],
            len: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, m: Move) {
        debug_assert!(self.len < MAX_MOVES);
        unsafe {
            *self.data.get_unchecked_mut(self.len) = m;
        }
        self.len += 1;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[Move] {
        &self.data[..self.len]
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, Move> {
        self.as_slice().iter()
    }
}

impl Default for MoveBuffer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MoveList {
    moves: MoveBuffer,
}

impl MoveList {
    pub fn new(b: &Board, p: Piece) -> Self {
        let mut moves = MoveBuffer::new();
        generate(b, &mut moves, p, false);
        debug_assert!(moves.len() < MAX_MOVES);
        let ml = MoveList { moves };
        debug_assert!(ml.all_valid(b));
        ml
    }

    pub fn with_hold(b: &Board, p: Piece, hold: Option<Piece>, force: bool) -> Self {
        let mut moves = MoveBuffer::new();
        generate(b, &mut moves, p, force);
        if !moves.is_empty() {
            if let Some(h) = hold {
                if p != h {
                    generate(b, &mut moves, h, force);
                }
            }
        }
        debug_assert!(moves.len() < MAX_MOVES);
        let ml = MoveList { moves };
        debug_assert!(ml.all_valid(b));
        ml
    }

    fn all_valid(&self, b: &Board) -> bool {
        for m in self.moves.iter() {
            if !is_ok_move(m) {
                return false;
            }
            let pc = m.cells();
            let off = Coordinates::new(m.x(), m.y());
            if b.obstructed_coord(&off)
                || b.obstructed_coord(&(pc[0] + off))
                || b.obstructed_coord(&(pc[1] + off))
                || b.obstructed_coord(&(pc[2] + off))
            {
                return false;
            }
            let below = Coordinates::new(off.x as i32, off.y as i32 - 1);
            if !b.obstructed_coord(&below)
                && !b.obstructed_coord(&(pc[0] + below))
                && !b.obstructed_coord(&(pc[1] + below))
                && !b.obstructed_coord(&(pc[2] + below))
            {
                return false;
            }
        }
        true
    }

    pub fn size(&self) -> usize {
        self.moves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    pub fn contains(&self, m: &Move) -> bool {
        self.moves.as_slice().contains(m)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Move> {
        self.moves.as_slice().iter()
    }

    pub fn moves(&self) -> &[Move] {
        self.moves.as_slice()
    }
}
