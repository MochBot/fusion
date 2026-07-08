use crate::board::Board;
use crate::header::*;
use crate::movegen::generate;
use std::mem::MaybeUninit;

pub const MAX_MOVES: usize = 256;

pub struct MoveBuffer {
    // Uninitialized backing store - only `data[..len]` is ever read.
    // Avoids the 512-byte memset that `[Move::none(); MAX_MOVES]` emitted on
    // every node (a dominant cost in movegen-heavy workloads like perft/search).
    data: [MaybeUninit<Move>; MAX_MOVES],
    len: usize,
}

impl MoveBuffer {
    #[inline]
    pub fn new() -> Self {
        MoveBuffer {
            // SAFETY: an array of `MaybeUninit<T>` is always valid uninitialized -
            // the elements themselves do not require initialization.
            data: unsafe { MaybeUninit::uninit().assume_init() },
            len: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, m: Move) {
        debug_assert!(self.len < MAX_MOVES);
        // SAFETY: `len < MAX_MOVES` by the caller-upheld invariant (debug-checked);
        // writing through the uninit slot is valid for `MaybeUninit`.
        unsafe {
            *self.data.get_unchecked_mut(self.len) = MaybeUninit::new(m);
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
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn as_slice(&self) -> &[Move] {
        // SAFETY: the first `len` elements were written by `push` and are
        // initialized; `Move` is `Copy` with no drop glue.
        unsafe { std::slice::from_raw_parts(self.data.as_ptr() as *const Move, self.len) }
    }

    /// Canonical-order the buffer by `Move::raw()`. Makes generation output
    /// deterministic and independent of which internal path (engine BFS vs
    /// packed reachability) produced it, so order-sensitive consumers see the
    /// same sequence regardless of piece/height routing.
    #[inline]
    pub fn sort_by_raw(&mut self) {
        // SAFETY: same initialized-prefix invariant as as_slice; Move is Copy.
        let s = unsafe {
            std::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut Move, self.len)
        };
        s.sort_unstable_by_key(|m| m.raw());
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, Move> {
        self.as_slice().iter()
    }

    /// Compact the buffer in place, keeping only moves for which `keep` returns
    /// true and preserving their original relative order.
    #[inline]
    pub fn retain<F: FnMut(&Move) -> bool>(&mut self, mut keep: F) {
        let mut w = 0;
        for r in 0..self.len {
            // SAFETY: r < len, so data[r] was written by `push` and is initialized.
            let m = unsafe { self.data.get_unchecked(r).assume_init() };
            if keep(&m) {
                // SAFETY: w <= r < len; writing an initialized Move into a valid slot.
                unsafe {
                    *self.data.get_unchecked_mut(w) = MaybeUninit::new(m);
                }
                w += 1;
            }
        }
        self.len = w;
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
