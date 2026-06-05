//! Phase-1.5: packed-lane whole-board reachability (optimized port of the
//! post-rewrite upstream algorithm) for an honest optimized-vs-optimized
//! comparison against the current engine.
//!
//! Packing (matches upstream `Board<H>`): one `u64` lane holds 6 rows of 10
//! columns = 60 bits, no gaps. Bit for cell (x, y): lane = y / 6, bit index =
//! (y % 6) * 10 + x. `PH` rows => `NL` lanes. Whole-board boolean ops are NL
//! native 64-bit ops; the y-shift is a cross-lane bit shift; the x-shift is a
//! masked per-row shift (rows are gapless so x-wrap must be masked explicitly).

// Macro (not const generics) because stable Rust can't size arrays as `[u16;
// NL*6]` without nightly generic_const_exprs; stamping per lane-count keeps PH
// and NL concrete literals. Fewer lanes = fewer whole-board ops; at NL=4 the
// `0..NL` lane loops autovectorize to one SIMD vector.
macro_rules! rotate_into_lits_steps {
    ($temp:ident, $result:ident, $usable_r1:expr, ($kx:literal, $ky:literal)) => {{
        if any(&$temp) {
            let kicked = shift_const::<$kx, $ky>(&$temp);
            or_into(&mut $result, &kicked);
        }
    }};
    ($temp:ident, $result:ident, $usable_r1:expr, ($kx:literal, $ky:literal), $(($rx:literal, $ry:literal)),+ $(,)?) => {{
        if any(&$temp) {
            let kicked = shift_const::<$kx, $ky>(&$temp);
            or_into(&mut $result, &kicked);
            let back = shift_const::<{ -$kx }, { -$ky }>($usable_r1);
            for l in 0..NL {
                $temp[l] &= !back[l];
            }
        }
        rotate_into_lits_steps!($temp, $result, $usable_r1, $(($rx, $ry)),+);
    }};
}

macro_rules! rotate_into_lits {
    ($search_r:expr, $usable_r1:expr, [$(($kx:literal, $ky:literal)),+ $(,)?]) => {{
        let mut temp = *$search_r;
        let mut result = [0u64; NL];
        rotate_into_lits_steps!(temp, result, $usable_r1, $(($kx, $ky)),+);
        result
    }};
}

macro_rules! usable_map_lits {
    ($e:expr, [$(($dx:literal, $dy:literal)),+ $(,)?]) => {{
        let mut u = *$e;
        $(
            let s = shift_const::<$dx, $dy>($e);
            and_into(&mut u, &s);
        )+
        u
    }};
}

macro_rules! impl_packed {
    ($ph:literal) => {
        use crate::default_ruleset::ACTIVE_RULES;
        use crate::gen::{
            canonical_offset, canonical_r, canonical_size, group2, in_bounds, kick_180_index,
            kick_index, rotate, Direction, KICKS, KICKS_180, SPAWN_COL,
        };
        use crate::header::*;

        pub const PH: usize = $ph;
        const NL: usize = PH / 6;
        const MASK60: u64 = (1u64 << 60) - 1;
        const COLS: u64 = (1u64 << COL_NB) - 1; // 0x3FF, one row

        pub type Pb = [u64; NL];

        const fn lane_colmask(rowmask: u64) -> u64 {
            let mut m = 0u64;
            let mut r = 0;
            while r < 6 {
                m |= rowmask << (10 * r);
                r += 1;
            }
            m
        }

        // Precomputed per-shift column-wrap masks, indexed by |dx|. The mask depends
        // only on the shift distance, not the board data, so recomputing lane_colmask
        // (a 6-iteration loop) on every shift_x in the hot rotation/flood path is pure
        // waste; this folds it to a constant array lookup.
        const fn build_xmask(neg: bool) -> [u64; COL_NB] {
            let mut t = [0u64; COL_NB];
            let mut d = 0;
            while d < COL_NB {
                let row = if neg {
                    (COLS >> d) & COLS
                } else {
                    (COLS << d) & COLS
                };
                t[d] = lane_colmask(row);
                d += 1;
            }
            t
        }
        const XMASK_POS: [u64; COL_NB] = build_xmask(false);
        const XMASK_NEG: [u64; COL_NB] = build_xmask(true);

        /// x-shift: result.get(x,y) = src.get(x-dx, y). Rows are gapless, so bits that
        /// would wrap past a row edge are removed by masking each row to valid columns.
        #[inline(always)]
        fn shift_x(b: &Pb, dx: i32) -> Pb {
            let mut o = [0u64; NL];
            if dx >= 0 {
                let d = dx as u32;
                let mask = XMASK_POS[dx as usize];
                for l in 0..NL {
                    o[l] = (b[l] << d) & mask;
                }
            } else {
                let d = (-dx) as u32;
                let mask = XMASK_NEG[(-dx) as usize];
                for l in 0..NL {
                    o[l] = (b[l] >> d) & mask;
                }
            }
            o
        }

        /// y-shift by k rows (k>0 = +y/up): result.get(x,y) = src.get(x, y-k).
        /// Handles arbitrary spans across the 6-rows-per-lane boundary.
        #[inline(always)]
        fn shift_y(b: &Pb, k: i32) -> Pb {
            let mut o = [0u64; NL];
            if k >= 0 {
                let lo = (k / 6) as usize;
                let bit = (10 * (k % 6)) as u32;
                for l in 0..NL {
                    let mut v = 0u64;
                    if l >= lo {
                        v |= b[l - lo] << bit;
                    }
                    if bit > 0 && l > lo {
                        v |= b[l - lo - 1] >> (60 - bit);
                    }
                    o[l] = v & MASK60;
                }
            } else {
                let kk = (-k) as usize;
                let lo = kk / 6;
                let bit = (10 * (kk % 6)) as u32;
                for l in 0..NL {
                    let mut v = 0u64;
                    if l + lo < NL {
                        v |= b[l + lo] >> bit;
                    }
                    if bit > 0 && l + lo + 1 < NL {
                        v |= b[l + lo + 1] << (60 - bit);
                    }
                    o[l] = v & MASK60;
                }
            }
            o
        }

        #[inline(always)]
        fn shift(b: &Pb, dx: i32, dy: i32) -> Pb {
            shift_y(&shift_x(b, dx), dy)
        }

        #[inline(always)]
        fn lane_shift_x_const<const DX: i32>(v: u64) -> u64 {
            if DX >= 0 {
                (v << (DX as u32)) & XMASK_POS[DX as usize]
            } else {
                (v >> ((-DX) as u32)) & XMASK_NEG[(-DX) as usize]
            }
        }

        #[inline(always)]
        fn shift_const<const DX: i32, const DY: i32>(b: &Pb) -> Pb {
            let mut o = [0u64; NL];
            if DY >= 0 {
                let lo = (DY / 6) as usize;
                let bit = (10 * (DY % 6)) as u32;
                for l in 0..NL {
                    let mut v = 0u64;
                    if l >= lo {
                        v |= lane_shift_x_const::<DX>(b[l - lo]) << bit;
                    }
                    if bit > 0 && l > lo {
                        v |= lane_shift_x_const::<DX>(b[l - lo - 1]) >> (60 - bit);
                    }
                    o[l] = v & MASK60;
                }
            } else {
                let kk = (-DY) as usize;
                let lo = kk / 6;
                let bit = (10 * (kk % 6)) as u32;
                for l in 0..NL {
                    let mut v = 0u64;
                    if l + lo < NL {
                        v |= lane_shift_x_const::<DX>(b[l + lo]) >> bit;
                    }
                    if bit > 0 && l + lo + 1 < NL {
                        v |= lane_shift_x_const::<DX>(b[l + lo + 1]) << (60 - bit);
                    }
                    o[l] = v & MASK60;
                }
            }
            o
        }

        #[inline(always)]
        fn and_into(a: &mut Pb, b: &Pb) {
            for l in 0..NL {
                a[l] &= b[l];
            }
        }

        #[inline(always)]
        fn or_into(a: &mut Pb, b: &Pb) {
            for l in 0..NL {
                a[l] |= b[l];
            }
        }

        #[inline(always)]
        fn any(a: &Pb) -> bool {
            let mut acc = 0u64;
            for l in 0..NL {
                acc |= a[l];
            }
            acc != 0
        }

        fn pack_rows(rows: &[u16; PH]) -> Pb {
            let mut p = [0u64; NL];
            for y in 0..PH {
                let lane = y / 6;
                let off = ((y % 6) * 10) as u32;
                p[lane] |= ((rows[y] as u64) & COLS) << off;
            }
            p
        }

        fn empty_from_rows(rows: &[u16; PH]) -> Pb {
            let occ = pack_rows(rows);
            let mut e = [0u64; NL];
            for l in 0..NL {
                e[l] = !occ[l] & MASK60;
            }
            e
        }

        // Only the first `canonical_size` rotation maps are ever read: every consumer
        // indexes usable[canonical_r(..)] (< cs), except T's spin path where cs=4. For
        // group2 (I/S/Z, cs=2) this skips building 2 unused maps (6 shifts each).
        fn usable_map_runtime(e: &Pb, p: Piece) -> [Pb; ROTATION_NB] {
            let mut out = [[0u64; NL]; ROTATION_NB];
            for ri in 0..canonical_size(p) {
                let rc = canonical_r(p, Rotation::from_u8(ri as u8));
                let pc = piece_table(p, rc);
                let mut u = *e;
                for k in 0..3 {
                    let s = shift(e, -(pc[k].x as i32), -(pc[k].y as i32));
                    and_into(&mut u, &s);
                }
                out[ri] = u;
            }
            out
        }

        #[inline]
        fn usable_map_const_dispatch(e: &Pb, p: Piece) -> Option<[Pb; ROTATION_NB]> {
            let mut out = [[0u64; NL]; ROTATION_NB];
            match p {
                Piece::I => {
                    out[0] = usable_map_lits!(e, [(1, 0), (-1, 0), (-2, 0)]);
                    out[1] = usable_map_lits!(e, [(0, -1), (0, 1), (0, 2)]);
                    Some(out)
                }
                Piece::L => {
                    out[0] = usable_map_lits!(e, [(1, 0), (-1, 0), (-1, -1)]);
                    out[1] = usable_map_lits!(e, [(0, -1), (0, 1), (-1, 1)]);
                    out[2] = usable_map_lits!(e, [(-1, 0), (1, 0), (1, 1)]);
                    out[3] = usable_map_lits!(e, [(0, 1), (0, -1), (1, -1)]);
                    Some(out)
                }
                Piece::J => {
                    out[0] = usable_map_lits!(e, [(1, 0), (-1, 0), (1, -1)]);
                    out[1] = usable_map_lits!(e, [(0, -1), (0, 1), (-1, -1)]);
                    out[2] = usable_map_lits!(e, [(-1, 0), (1, 0), (-1, 1)]);
                    out[3] = usable_map_lits!(e, [(0, 1), (0, -1), (1, 1)]);
                    Some(out)
                }
                Piece::S => {
                    out[0] = usable_map_lits!(e, [(1, 0), (0, -1), (-1, -1)]);
                    out[1] = usable_map_lits!(e, [(0, -1), (-1, 0), (-1, 1)]);
                    Some(out)
                }
                Piece::Z => {
                    out[0] = usable_map_lits!(e, [(1, -1), (0, -1), (-1, 0)]);
                    out[1] = usable_map_lits!(e, [(-1, -1), (-1, 0), (0, 1)]);
                    Some(out)
                }
                _ => None,
            }
        }

        fn usable_map(e: &Pb, p: Piece) -> [Pb; ROTATION_NB] {
            usable_map_const_dispatch(e, p).unwrap_or_else(|| usable_map_runtime(e, p))
        }

        fn landable_of(u: &Pb) -> Pb {
            let up = shift_const::<0, 1>(u);
            let mut out = [0u64; NL];
            for l in 0..NL {
                out[l] = u[l] & !up[l] & MASK60;
            }
            out
        }

        // Frontier fixpoint over {left, right, soft-drop} through `usable` (matches
        // upstream cobra movegen.hpp). `unsearched` carries usable & !search so the
        // hot loop needs one AND per lane instead of recomputing `& usable & !search`.
        #[inline]
        fn flood(search: &mut Pb, usable: &Pb) {
            let mut unsearched = [0u64; NL];
            for i in 0..NL {
                unsearched[i] = usable[i] & !search[i];
            }
            loop {
                let l = shift_const::<-1, 0>(search);
                let r = shift_const::<1, 0>(search);
                let d = shift_const::<0, -1>(search);
                let mut changed = false;
                for i in 0..NL {
                    let temp = (l[i] | r[i] | d[i]) & unsearched[i];
                    if temp != 0 {
                        search[i] |= temp;
                        unsearched[i] ^= temp;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }

        // Group2 (I/S/Z) South/West slots mirror the engine's actual-rotation lanes
        // whose collision map is all-clear (cm.get(x,South)=0): soft-drop falls
        // fully unobstructed (engine: `(to_search>>1) & !cm.get(South)` with cm=0, so
        // no collision and no spawn-row cap — the fall passes straight through the
        // stack), while horizontal moves are gated by the canonical collision
        // (searched[South]=North copy). These lanes are never emitted; they exist
        // solely as rotation sources.
        #[inline]
        fn flood_source(search: &mut Pb, usable_canon: &Pb) {
            loop {
                let l = shift_const::<-1, 0>(search);
                let r = shift_const::<1, 0>(search);
                let d = shift_const::<0, -1>(search);
                let mut changed = false;
                for i in 0..NL {
                    let exp = ((l[i] | r[i]) & usable_canon[i] | d[i]) & !search[i] & MASK60;
                    if exp != 0 {
                        search[i] |= exp;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }

        fn rotate_into(
            search_r: &Pb,
            usable_r1: &Pb,
            kicks: &[Coordinates],
            off: Coordinates,
        ) -> Pb {
            let mut temp = *search_r;
            let mut result = [0u64; NL];
            let n = kicks.len();
            for (i, kick) in kicks.iter().enumerate() {
                if !any(&temp) {
                    break;
                }
                let kx = kick.x as i32 + off.x as i32;
                let ky = kick.y as i32 + off.y as i32;
                let kicked = shift(&temp, kx, ky);
                or_into(&mut result, &kicked);
                if i + 1 < n {
                    let back = shift(usable_r1, -kx, -ky);
                    for l in 0..NL {
                        temp[l] &= !back[l];
                    }
                }
            }
            result
        }

        #[inline]
        fn rotate_into_const_dispatch(
            search_r: &Pb,
            usable_r1: &Pb,
            p: Piece,
            ri: usize,
            d: Direction,
            srs_plus: bool,
        ) -> Option<Pb> {
            match (p, srs_plus, ri, d) {
                (Piece::L | Piece::J, _, 0, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)]
                )),
                (Piece::L | Piece::J, _, 1, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)]
                )),
                (Piece::L | Piece::J, _, 2, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)]
                )),
                (Piece::L | Piece::J, _, 3, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)]
                )),
                (Piece::L | Piece::J, _, 0, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)]
                )),
                (Piece::L | Piece::J, _, 1, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)]
                )),
                (Piece::L | Piece::J, _, 2, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)]
                )),
                (Piece::L | Piece::J, _, 3, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)]
                )),
                (Piece::S | Piece::Z, _, 0, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)]
                )),
                (Piece::S | Piece::Z, _, 1, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, -1), (1, -1), (1, -2), (0, 1), (1, 1)]
                )),
                (Piece::S | Piece::Z, _, 2, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, 1), (0, 1), (0, 2), (-1, -1), (0, -1)]
                )),
                (Piece::S | Piece::Z, _, 3, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, 0), (0, 0), (0, -1), (1, 2), (0, 2)]
                )),
                (Piece::S | Piece::Z, _, 0, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, 0), (0, 0), (0, 1), (-1, -2), (0, -2)]
                )),
                (Piece::S | Piece::Z, _, 1, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)]
                )),
                (Piece::S | Piece::Z, _, 2, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 1), (-1, 1), (-1, 2), (0, -1), (-1, -1)]
                )),
                (Piece::S | Piece::Z, _, 3, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, -1), (0, -1), (0, -2), (1, 1), (0, 1)]
                )),
                (Piece::I, false, 0, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, 0), (-1, 0), (2, 0), (-1, -1), (2, 2)]
                )),
                (Piece::I, false, 1, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, -1), (-2, -1), (1, -1), (-2, 1), (1, -2)]
                )),
                (Piece::I, false, 2, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 1), (2, 1), (-1, 1), (2, 2), (-1, -1)]
                )),
                (Piece::I, false, 3, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)]
                )),
                (Piece::I, false, 0, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)]
                )),
                (Piece::I, false, 1, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, 0), (1, 0), (-2, 0), (1, 1), (-2, -2)]
                )),
                (Piece::I, false, 2, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, 1), (2, 1), (-1, 1), (2, -1), (-1, 2)]
                )),
                (Piece::I, false, 3, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, -1), (-2, -1), (1, -1), (-2, -2), (1, 1)]
                )),
                (Piece::I, true, 0, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, 0), (2, 0), (-1, 0), (-1, -1), (2, 2)]
                )),
                (Piece::I, true, 1, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, -1), (-2, -1), (1, -1), (-2, 1), (1, -2)]
                )),
                (Piece::I, true, 2, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 1), (2, 1), (-1, 1), (2, 2), (-1, -1)]
                )),
                (Piece::I, true, 3, Direction::Cw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)]
                )),
                (Piece::I, true, 0, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (2, 0), (2, -1), (-1, 2)]
                )),
                (Piece::I, true, 1, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, 0), (-2, 0), (1, 0), (-2, -2), (1, 1)]
                )),
                (Piece::I, true, 2, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, 1), (-1, 1), (2, 1), (-1, 2), (2, -1)]
                )),
                (Piece::I, true, 3, Direction::Ccw) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, -1), (1, -1), (-2, -1), (1, 1), (-2, -2)]
                )),
                (Piece::L | Piece::J, false, 0, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, 0), (0, 1)]))
                }
                (Piece::L | Piece::J, false, 1, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, 0), (1, 0)]))
                }
                (Piece::L | Piece::J, false, 2, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, 0), (0, -1)]))
                }
                (Piece::L | Piece::J, false, 3, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, 0), (-1, 0)]))
                }
                (Piece::L | Piece::J, true, 0, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (0, 1), (1, 1), (-1, 1), (1, 0), (-1, 0)]
                )),
                (Piece::L | Piece::J, true, 1, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (1, 0), (1, 2), (1, 1), (0, 2), (0, 1)]
                )),
                (Piece::L | Piece::J, true, 2, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (0, -1), (-1, -1), (1, -1), (-1, 0), (1, 0)]
                )),
                (Piece::L | Piece::J, true, 3, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 0), (-1, 0), (-1, 2), (-1, 1), (0, 2), (0, 1)]
                )),
                (Piece::S | Piece::Z, false, 0, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, -1), (0, 0)]))
                }
                (Piece::S | Piece::Z, false, 1, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(-1, 0), (0, 0)]))
                }
                (Piece::S | Piece::Z, false, 2, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, 1), (0, 0)]))
                }
                (Piece::S | Piece::Z, false, 3, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(1, 0), (0, 0)]))
                }
                (Piece::S | Piece::Z, true, 0, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, -1), (0, 0), (1, 0), (-1, 0), (1, -1), (-1, -1)]
                )),
                (Piece::S | Piece::Z, true, 1, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, 0), (0, 0), (0, 2), (0, 1), (-1, 2), (-1, 1)]
                )),
                (Piece::S | Piece::Z, true, 2, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 1), (0, 0), (-1, 0), (1, 0), (-1, 1), (1, 1)]
                )),
                (Piece::S | Piece::Z, true, 3, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, 0), (0, 0), (0, 2), (0, 1), (1, 2), (1, 1)]
                )),
                (Piece::I, false, 0, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, -1), (0, 0)]))
                }
                (Piece::I, false, 1, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(-1, 0), (0, 0)]))
                }
                (Piece::I, false, 2, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(0, 1), (0, 0)]))
                }
                (Piece::I, false, 3, Direction::Flip) => {
                    Some(rotate_into_lits!(search_r, usable_r1, [(1, 0), (0, 0)]))
                }
                (Piece::I, true, 0, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, -1), (0, 0), (1, 0), (-1, 0), (1, -1), (-1, -1)]
                )),
                (Piece::I, true, 1, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(-1, 0), (0, 0), (0, 2), (0, 1), (-1, 2), (-1, 1)]
                )),
                (Piece::I, true, 2, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(0, 1), (0, 0), (-1, 0), (1, 0), (-1, 1), (1, 1)]
                )),
                (Piece::I, true, 3, Direction::Flip) => Some(rotate_into_lits!(
                    search_r,
                    usable_r1,
                    [(1, 0), (0, 0), (0, 2), (0, 1), (1, 2), (1, 1)]
                )),
                _ => None,
            }
        }

        fn spawn_cap() -> Pb {
            let mut m = [0u64; NL];
            // Band-relative cap: the ceiling footprint-overflow artifact lives in the
            // top 2 rows of the band (a vertical piece pivot sits 2 below its top
            // cell), so the free-fall seed must be capped at PH-2. For the tall bands
            // (PH>=24) this is >= spawn_row, so min() keeps them byte-identical to the
            // original spawn_row cap; short bands clamp to their own ceiling instead.
            let sr = (ACTIVE_RULES.spawn_row as usize).min(PH - 2);
            for y in 0..PH {
                if y < sr {
                    let lane = y / 6;
                    let off = ((y % 6) * 10) as u32;
                    m[lane] |= COLS << off;
                }
            }
            m
        }

        // All-spin "immobile" test (engine generate_inner CHECK_SPIN, spin_map=None,
        // movegen.rs:439-449): a rotated-in cell is a mini-spin iff the piece cannot
        // move in any of the 4 directions from there — every orthogonal neighbour
        // position is a collision. `notu = !usable[rc]` is the canonical collision map;
        // walls (off-board columns) and the floor count as blocked, matching the
        // engine's `!0` boundary columns and `(same_col<<1)|1` floor bit.
        #[inline(always)]
        fn blocked_dirs(usable_rc: &Pb) -> (Pb, Pb, Pb, Pb) {
            let mut notu = [0u64; NL];
            for l in 0..NL {
                notu[l] = !usable_rc[l] & MASK60;
            }
            let leftwall = lane_colmask(1 << 0);
            let rightwall = lane_colmask(1 << (COL_NB - 1));
            let mut bl = shift_const::<1, 0>(&notu);
            let mut br = shift_const::<-1, 0>(&notu);
            let bu = shift_const::<0, -1>(&notu);
            let mut bd = shift_const::<0, 1>(&notu);
            for l in 0..NL {
                bl[l] = (bl[l] | leftwall) & MASK60;
                br[l] = (br[l] | rightwall) & MASK60;
            }
            bd[0] |= COLS;
            (bl, br, bu, bd)
        }

        fn col_mask(cols_set: [bool; COL_NB]) -> Pb {
            let mut row = 0u64;
            for (x, &on) in cols_set.iter().enumerate() {
                if on {
                    row |= 1u64 << x;
                }
            }
            [lane_colmask(row); NL]
        }

        // T-spin corner maps (engine movegen.rs:786-818). corners[k](x,y) tells whether
        // the k-th diagonal neighbour of cell (x,y) is occupied; off-board columns and
        // the floor count as occupied (engine `!0` / `|1`). `spins` = the 3-corner mask
        // (>=3 of 4 diagonals filled with the standard front/back pairing); `face[r]`
        // further requires the two front corners for orientation r, splitting full vs
        // mini. `check_spin` gates whether T uses spin labeling at all: it is set iff
        // some landable 3-corner placement exists for an in-bounds rotation.
        struct TMaps {
            spins: Pb,
            face: [Pb; ROTATION_NB],
            check_spin: bool,
        }

        fn build_tspin_maps(rows: &[u16; PH], usable: &[Pb; ROTATION_NB]) -> TMaps {
            let occ = pack_rows(rows);
            let leftwall = [lane_colmask(1 << 0); NL];
            let rightwall = [lane_colmask(1 << (COL_NB - 1)); NL];
            let mut floor = [0u64; NL];
            floor[0] = COLS;

            let mut c = [[0u64; NL]; 4];
            let s10 = shift_const::<1, -1>(&occ);
            let sm1m1 = shift_const::<-1, -1>(&occ);
            let sm11 = shift_const::<-1, 1>(&occ);
            let s11 = shift_const::<1, 1>(&occ);
            for l in 0..NL {
                c[0][l] = (s10[l] | leftwall[l]) & MASK60;
                c[1][l] = (sm1m1[l] | rightwall[l]) & MASK60;
                c[2][l] = (sm11[l] | floor[l] | rightwall[l]) & MASK60;
                c[3][l] = (s11[l] | floor[l] | leftwall[l]) & MASK60;
            }

            let mut spins = [0u64; NL];
            for l in 0..NL {
                spins[l] = ((c[0][l] & c[1][l] & (c[2][l] | c[3][l]))
                    | (c[2][l] & c[3][l] & (c[0][l] | c[1][l])))
                    & MASK60;
            }

            let mut face = [[0u64; NL]; ROTATION_NB];
            let mut check_spin = false;
            for ri in 0..ROTATION_NB {
                let r = Rotation::from_u8(ri as u8);
                let mut inb = [false; COL_NB];
                for x in 0..COL_NB {
                    inb[x] = in_bounds(Piece::T, r, x as i32);
                }
                let inb_mask = col_mask(inb);
                let cw = rotate(Direction::Cw, r) as usize;
                let land = landable_of(&usable[ri]);
                for l in 0..NL {
                    face[ri][l] = spins[l] & c[ri][l] & c[cw][l] & inb_mask[l];
                    if spins[l] & usable[ri][l] & land[l] & inb_mask[l] != 0 {
                        check_spin = true;
                    }
                }
            }

            TMaps {
                spins,
                face,
                check_spin,
            }
        }

        #[inline]
        #[allow(clippy::too_many_arguments)]
fn apply_rotate(
            search: &mut [Pb; ROTATION_NB],
            usable: &[Pb; ROTATION_NB],
            mini: &mut [Pb; ROTATION_NB],
            done: &mut [bool; ROTATION_NB],
            p: Piece,
            ri: usize,
            d: Direction,
            kicks: &[Coordinates],
            label_allspin: bool,
        ) {
            let r = Rotation::from_u8(ri as u8);
            let r1 = rotate(d, r);
            let r1i = r1 as usize;
            let r1c = canonical_r(p, r1) as usize;
            let off = canonical_offset(p, r) - canonical_offset(p, r1);
            let result = rotate_into_const_dispatch(
                &search[ri],
                &usable[r1c],
                p,
                ri,
                d,
                ACTIVE_RULES.srs_plus,
            )
            .unwrap_or_else(|| rotate_into(&search[ri], &usable[r1c], kicks, off));
            let mut gained = false;
            let mut newc = [0u64; NL];
            for l in 0..NL {
                let nc = result[l] & usable[r1c][l] & !search[r1i][l];
                if nc != 0 {
                    newc[l] = nc;
                    gained = true;
                }
            }
            if gained {
                if label_allspin {
                    // Mini-spin = rotated-in cell that is immobile (stuck). Stuck cells
                    // are unreachable by flood (every neighbour is a collision), so this
                    // union is order-independent vs the engine's worklist labeling.
                    let (bl, br, bu, bd) = blocked_dirs(&usable[r1c]);
                    for l in 0..NL {
                        mini[r1i][l] |= newc[l] & bl[l] & br[l] & bu[l] & bd[l];
                    }
                }
                or_into(&mut search[r1i], &newc);
                done[r1i] = false;
            }
        }

        // T-spin labeling needs the kick index (kicks `i >= 4` are always full spins),
        // so this mirrors the engine's per-kick `do_rotate` loop instead of the
        // aggregate whole-board rotate. For T, canonical_r is the identity and
        // canonical_offset is zero, so the target rotation r1 indexes usable directly.
        #[inline]
        #[allow(clippy::too_many_arguments)]
fn apply_rotate_t(
            search: &mut [Pb; ROTATION_NB],
            usable: &[Pb; ROTATION_NB],
            mini: &mut [Pb; ROTATION_NB],
            full: &mut [Pb; ROTATION_NB],
            done: &mut [bool; ROTATION_NB],
            tmaps: &TMaps,
            ri: usize,
            d: Direction,
            kicks: &[Coordinates],
        ) {
            let r = Rotation::from_u8(ri as u8);
            let r1 = rotate(d, r);
            let r1i = r1 as usize;
            let mut temp = search[ri];
            let mut total_new = [0u64; NL];
            let mut gained = false;
            for (i, kick) in kicks.iter().enumerate() {
                if !any(&temp) {
                    break;
                }
                let kx = kick.x as i32;
                let ky = kick.y as i32;
                let kicked = shift(&temp, kx, ky);
                let mut m = [0u64; NL];
                for l in 0..NL {
                    m[l] = kicked[l] & usable[r1i][l];
                }
                let back = shift(&m, -kx, -ky);
                for l in 0..NL {
                    temp[l] &= !back[l];
                    m[l] &= !search[r1i][l] & !total_new[l];
                }
                if !any(&m) {
                    continue;
                }
                let mut spins_any = false;
                let mut spins = [0u64; NL];
                for l in 0..NL {
                    spins[l] = m[l] & tmaps.spins[l];
                    if spins[l] != 0 {
                        spins_any = true;
                    }
                }
                if spins_any {
                    if i >= 4 {
                        for l in 0..NL {
                            full[r1i][l] |= spins[l];
                        }
                    } else {
                        for l in 0..NL {
                            mini[r1i][l] |= spins[l] & !tmaps.face[r1i][l];
                            full[r1i][l] |= spins[l] & tmaps.face[r1i][l];
                        }
                    }
                }
                for l in 0..NL {
                    total_new[l] |= m[l];
                }
                gained = true;
            }
            if gained {
                or_into(&mut search[r1i], &total_new);
                done[r1i] = false;
            }
        }

        fn compute_reach(
            rows: &[u16; PH],
            p: Piece,
        ) -> (
            [Pb; ROTATION_NB],
            [Pb; ROTATION_NB],
            [Pb; ROTATION_NB],
            [Pb; ROTATION_NB],
        ) {
            compute_reach_with_force(rows, p, false)
        }

        fn compute_reach_with_force(
            rows: &[u16; PH],
            p: Piece,
            force: bool,
        ) -> (
            [Pb; ROTATION_NB],
            [Pb; ROTATION_NB],
            [Pb; ROTATION_NB],
            [Pb; ROTATION_NB],
        ) {
            let e = empty_from_rows(rows);
            let usable = usable_map(&e, p);
            let cs = canonical_size(p);
            let ssize = if p == Piece::O { 1 } else { ROTATION_NB };
            let allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
            let tmaps = if p == Piece::T && ACTIVE_RULES.enable_tspin {
                let t = build_tspin_maps(rows, &usable);
                if t.check_spin {
                    Some(t)
                } else {
                    None
                }
            } else {
                None
            };
            let cap = spawn_cap();
            let mut search = [[0u64; NL]; ROTATION_NB];
            let mut mini = [[0u64; NL]; ROTATION_NB];
            let mut full = [[0u64; NL]; ROTATION_NB];

            let mut h = 0usize;
            for y in (0..PH).rev() {
                if rows[y] & (COLS as u16) != 0 {
                    h = y + 1;
                    break;
                }
            }
            // Slow seed-from-spawn-row only applies when the spawn row is inside this
            // band (PH > spawn_row); short bands never contain it and are always fast,
            // which also keeps the lane=spawn_row/6 index below NL. Mirrors upstream's
            // `if constexpr (BoardT::H > SPAWN_Y)`.
            let slow = PH as i32 > ACTIVE_RULES.spawn_row && h as i32 > ACTIVE_RULES.spawn_row - 3;

            if slow {
                let sr = ACTIVE_RULES.spawn_row as usize;
                let n = Rotation::North as usize;
                if force {
                    for y in sr..PH {
                        let lane = y / 6;
                        let bit = ((y % 6) * 10 + SPAWN_COL) as u32;
                        if usable[n][lane] & (1u64 << bit) != 0 {
                            search[n][lane] = 1u64 << bit;
                            break;
                        }
                    }
                } else {
                    let lane = sr / 6;
                    let bit = ((sr % 6) * 10 + SPAWN_COL) as u32;
                    if usable[n][lane] & (1u64 << bit) != 0 {
                        search[n][lane] = 1u64 << bit;
                    }
                }
            } else {
                for ri in 0..cs {
                    let u = &usable[ri];
                    let mut surface = [0u64; NL];
                    for l in 0..NL {
                        surface[l] = !u[l] & MASK60;
                    }
                    // Headroom above the spawn row contains no real obstacles; masking it
                    // off prevents the top-row footprint-overflow artifact (usable=0 where
                    // the piece extends past the board ceiling) from cascading down empty
                    // columns during the downward smear and killing the free-fall seed.
                    and_into(&mut surface, &cap);
                    let s1 = shift_const::<0, -1>(&surface);
                    or_into(&mut surface, &s1);
                    let s2 = shift_const::<0, -2>(&surface);
                    or_into(&mut surface, &s2);
                    let s4 = shift_const::<0, -4>(&surface);
                    or_into(&mut surface, &s4);
                    let s8 = shift_const::<0, -8>(&surface);
                    or_into(&mut surface, &s8);
                    let s16 = shift_const::<0, -16>(&surface);
                    or_into(&mut surface, &s16);
                    let mut s = [0u64; NL];
                    for l in 0..NL {
                        s[l] = !surface[l] & MASK60;
                    }
                    for _ in 0..2 {
                        let l = shift_const::<-1, 0>(&s);
                        let r = shift_const::<1, 0>(&s);
                        for i in 0..NL {
                            s[i] |= (l[i] | r[i]) & u[i];
                        }
                    }
                    and_into(&mut s, &cap);
                    search[ri] = s;

                    if group2(p) {
                        let r1 = rotate(Direction::Flip, Rotation::from_u8(ri as u8));
                        let mut seed = s;
                        if r1 == Rotation::South {
                            let down = shift_const::<0, -1>(&s);
                            and_into(&mut seed, &down);
                        }
                        and_into(&mut seed, &cap);
                        search[r1 as usize] = seed;
                    }
                }
            }

            let ki = kick_index(p, ACTIVE_RULES.srs_plus);
            let ki180 = kick_180_index(p);
            let mut done = [true; ROTATION_NB];
            for ri in 0..ssize {
                done[ri] = !any(&search[ri]);
            }

            loop {
                let mut all_done = true;
                for ri in 0..ssize {
                    if done[ri] {
                        continue;
                    }
                    done[ri] = true;
                    let r = Rotation::from_u8(ri as u8);
                    let rc = canonical_r(p, r);
                    if group2(p) && ri >= cs {
                        flood_source(&mut search[ri], &usable[rc as usize]);
                    } else {
                        flood(&mut search[ri], &usable[rc as usize]);
                    }

                    if let Some(tm) = &tmaps {
                        apply_rotate_t(
                            &mut search,
                            &usable,
                            &mut mini,
                            &mut full,
                            &mut done,
                            tm,
                            ri,
                            Direction::Cw,
                            &KICKS[ki][Direction::Cw as usize][ri],
                        );
                        apply_rotate_t(
                            &mut search,
                            &usable,
                            &mut mini,
                            &mut full,
                            &mut done,
                            tm,
                            ri,
                            Direction::Ccw,
                            &KICKS[ki][Direction::Ccw as usize][ri],
                        );
                        if ACTIVE_RULES.enable_180 {
                            let flip_n = if ACTIVE_RULES.srs_plus { 6 } else { 2 };
                            apply_rotate_t(
                                &mut search,
                                &usable,
                                &mut mini,
                                &mut full,
                                &mut done,
                                tm,
                                ri,
                                Direction::Flip,
                                &KICKS_180[ki180][ri][..flip_n],
                            );
                        }
                    } else {
                        apply_rotate(
                            &mut search,
                            &usable,
                            &mut mini,
                            &mut done,
                            p,
                            ri,
                            Direction::Cw,
                            &KICKS[ki][Direction::Cw as usize][ri],
                            allspin,
                        );
                        apply_rotate(
                            &mut search,
                            &usable,
                            &mut mini,
                            &mut done,
                            p,
                            ri,
                            Direction::Ccw,
                            &KICKS[ki][Direction::Ccw as usize][ri],
                            allspin,
                        );
                        if ACTIVE_RULES.enable_180 {
                            let flip_n = if ACTIVE_RULES.srs_plus { 6 } else { 2 };
                            apply_rotate(
                                &mut search,
                                &usable,
                                &mut mini,
                                &mut done,
                                p,
                                ri,
                                Direction::Flip,
                                &KICKS_180[ki180][ri][..flip_n],
                                allspin,
                            );
                        }
                    }
                }
                for ri in 0..ssize {
                    if !done[ri] {
                        all_done = false;
                    }
                }
                if all_done {
                    break;
                }
            }

            let _ = cs;
            (search, usable, mini, full)
        }

        fn emit_boards(rows: &[u16; PH], p: Piece) -> [Pb; ROTATION_NB] {
            let (search, usable, _mini, _full) = compute_reach(rows, p);
            let cs = canonical_size(p);
            let mut emit = [[0u64; NL]; ROTATION_NB];
            for ri in 0..cs {
                let land = landable_of(&usable[ri]);
                for l in 0..NL {
                    emit[ri][l] |= search[ri][l] & land[l];
                }
            }
            emit
        }

        /// Sorted `Move::raw()` u16 values with spin bits, matching the engine's
        /// full > mini > nospin emit partition: non-T uses all-spin mini labels, T
        /// uses the T-spin sentinel for full/mini, O and non-spin cells stay nospin.
        pub fn labeled_placements(rows: &[u16; PH], p: Piece) -> Vec<u16> {
            let (search, usable, mini, full) = compute_reach(rows, p);
            let cs = canonical_size(p);
            let allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
            let is_t = p == Piece::T;
            let mut out = Vec::with_capacity(64);
            let mut emit_bits = |mut bits: u64, l: usize, mk: &dyn Fn(i32, i32) -> u16| {
                while bits != 0 {
                    let b = bits.trailing_zeros() as usize;
                    let y = (l * 6 + b / 10) as i32;
                    let x = (b % 10) as i32;
                    out.push(mk(x, y));
                    bits &= bits - 1;
                }
            };
            for ri in 0..cs {
                let r = Rotation::from_u8(ri as u8);
                let land = landable_of(&usable[ri]);
                for l in 0..NL {
                    let legal = search[ri][l] & land[l];
                    let full_l = if is_t { legal & full[ri][l] } else { 0 };
                    let mini_l = if is_t {
                        legal & mini[ri][l] & !full_l
                    } else if allspin {
                        legal & mini[ri][l]
                    } else {
                        0
                    };
                    let nospin_l = legal & !mini_l & !full_l;
                    emit_bits(nospin_l, l, &|x, y| Move::new(p, r, x, y, false).raw());
                    if is_t {
                        emit_bits(mini_l, l, &|x, y| Move::new_tspin(r, x, y, false).raw());
                        emit_bits(full_l, l, &|x, y| Move::new_tspin(r, x, y, true).raw());
                    } else {
                        emit_bits(mini_l, l, &|x, y| Move::new_allspin_mini(p, r, x, y).raw());
                    }
                }
            }
            out.sort_unstable();
            out
        }

        pub fn generate_packed(
            rows: &[u16; PH],
            p: Piece,
            moves: &mut crate::move_buffer::MoveBuffer,
        ) {
            generate_packed_with_force(rows, p, false, moves);
        }

        /// Hot-path drop-in generator: whole-board packed reachability + spin labeling,
        /// pushing `Move`s straight into the engine `MoveBuffer` (no `Vec`, no sort).
        pub fn generate_packed_with_force(
            rows: &[u16; PH],
            p: Piece,
            force: bool,
            moves: &mut crate::move_buffer::MoveBuffer,
        ) {
            let (search, usable, mini, full) = compute_reach_with_force(rows, p, force);
            let cs = canonical_size(p);
            let allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
            let is_t = p == Piece::T;
            // emit each set bit as a Move with the constructor spliced inline (no dyn
            // dispatch): this is the engine hot path, an indirect call per move costs.
            macro_rules! emit {
                ($bits:expr, $l:expr, $mk:expr) => {{
                    let mut bits = $bits;
                    while bits != 0 {
                        let b = bits.trailing_zeros() as usize;
                        let y = ($l * 6 + b / 10) as i32;
                        let x = (b % 10) as i32;
                        moves.push($mk(x, y));
                        bits &= bits - 1;
                    }
                }};
            }
            for ri in 0..cs {
                let r = Rotation::from_u8(ri as u8);
                let land = landable_of(&usable[ri]);
                for l in 0..NL {
                    let legal = search[ri][l] & land[l];
                    let full_l = if is_t { legal & full[ri][l] } else { 0 };
                    let mini_l = if is_t {
                        legal & mini[ri][l] & !full_l
                    } else if allspin {
                        legal & mini[ri][l]
                    } else {
                        0
                    };
                    let nospin_l = legal & !mini_l & !full_l;
                    emit!(nospin_l, l, |x, y| Move::new(p, r, x, y, false));
                    if is_t {
                        emit!(mini_l, l, |x, y| Move::new_tspin(r, x, y, false));
                        emit!(full_l, l, |x, y| Move::new_tspin(r, x, y, true));
                    } else {
                        emit!(mini_l, l, |x, y| Move::new_allspin_mini(p, r, x, y));
                    }
                }
            }
        }

        /// Reachable+landable placement set, sorted as (rotation<<12)|(y<<6)|x.
        pub fn reach_placements(rows: &[u16; PH], p: Piece) -> Vec<u16> {
            let emit = emit_boards(rows, p);
            let mut out = Vec::with_capacity(64);
            for (rc, board) in emit.iter().enumerate() {
                for l in 0..NL {
                    let mut bits = board[l];
                    while bits != 0 {
                        let b = bits.trailing_zeros() as usize;
                        let y = (l * 6 + b / 10) as u16;
                        let x = (b % 10) as u16;
                        out.push(((rc as u16) << 12) | (y << 6) | x);
                        bits &= bits - 1;
                    }
                }
            }
            out.sort_unstable();
            out
        }

        /// Placement count only (allocation-free), for fair benchmarking.
        pub fn reach_count(rows: &[u16; PH], p: Piece) -> u32 {
            let emit = emit_boards(rows, p);
            let mut total = 0u32;
            for board in emit.iter() {
                for l in 0..NL {
                    total += board[l].count_ones();
                }
            }
            total
        }

        /// Full labeled generation cost (reach + spin partition), allocation-free, for
        /// an apples-to-apples benchmark against the engine's spin-labeling generate().
        pub fn labeled_count(rows: &[u16; PH], p: Piece) -> u32 {
            let (search, usable, mini, full) = compute_reach(rows, p);
            let cs = canonical_size(p);
            let allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
            let is_t = p == Piece::T;
            let mut total = 0u32;
            for ri in 0..cs {
                let land = landable_of(&usable[ri]);
                for l in 0..NL {
                    let legal = search[ri][l] & land[l];
                    let full_l = if is_t { legal & full[ri][l] } else { 0 };
                    let mini_l = if is_t {
                        legal & mini[ri][l] & !full_l
                    } else if allspin {
                        legal & mini[ri][l]
                    } else {
                        0
                    };
                    let nospin_l = legal & !mini_l & !full_l;
                    total += full_l.count_ones() + mini_l.count_ones() + nospin_l.count_ones();
                }
            }
            total
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            type Ref = [u16; PH];

            fn to_ref(p: &Pb) -> Ref {
                let mut r = [0u16; PH];
                for y in 0..PH {
                    let lane = y / 6;
                    let off = (y % 6) * 10;
                    r[y] = ((p[lane] >> off) as u16) & (COLS as u16);
                }
                r
            }

            fn shift_ref(src: &Ref, dx: i32, dy: i32) -> Ref {
                let mut out = [0u16; PH];
                for y in 0..PH as i32 {
                    for x in 0..COL_NB as i32 {
                        let sx = x - dx;
                        let sy = y - dy;
                        if sx >= 0
                            && sx < COL_NB as i32
                            && sy >= 0
                            && sy < PH as i32
                            && src[sy as usize] & (1u16 << sx) != 0
                        {
                            out[y as usize] |= 1u16 << x;
                        }
                    }
                }
                out
            }

            fn xorshift(s: &mut u64) -> u64 {
                let mut x = *s;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *s = x;
                x
            }

            #[test]
            fn packed_shift_matches_scalar_reference() {
                let mut st = 0xABCD_1234_5678_9999u64;
                for _ in 0..3000 {
                    let mut rows = [0u16; PH];
                    for y in 0..PH {
                        rows[y] = (xorshift(&mut st) as u16) & (COLS as u16);
                    }
                    let pb = pack_rows(&rows);
                    assert_eq!(to_ref(&pb), rows, "pack/unpack roundtrip");
                    for dx in -3i32..=3 {
                        for dy in -18i32..=18 {
                            let got = to_ref(&shift(&pb, dx, dy));
                            let want = shift_ref(&rows, dx, dy);
                            assert_eq!(got, want, "shift dx={dx} dy={dy}");
                        }
                    }
                }
            }
        }
    };
}

pub mod p6 {
    impl_packed!(6);
}
pub mod p12 {
    impl_packed!(12);
}
pub mod p18 {
    impl_packed!(18);
}
pub mod p24 {
    impl_packed!(24);
}
pub mod p30 {
    impl_packed!(30);
}

pub use p30::PH;

use crate::header::{Piece, COL_NB};
use crate::move_buffer::MoveBuffer;

#[inline]
fn board_height(rows: &[u16; PH]) -> usize {
    let full = (1u16 << COL_NB) - 1;
    for y in (0..PH).rev() {
        if rows[y] & full != 0 {
            return y + 1;
        }
    }
    0
}

#[inline]
fn shrink<const N: usize>(rows: &[u16; PH]) -> [u16; N] {
    let mut o = [0u16; N];
    o.copy_from_slice(&rows[..N]);
    o
}

// The packed seed (compute_reach non-slow branch) relies on headroom ABOVE the
// stack to absorb the ceiling footprint-overflow artifact and to free-fall the
// seed. The original p24/h<=18 path proved a SUFFICIENT band margin of
// `band - h >= 6` (24 - 18). Routing every board to the smallest band in
// {6,12,18,24} that keeps that proven margin sends the common near-empty perft
// boards to 1-2 lane bands (vs the old fixed 4), cutting every whole-board op
// proportionally. The piece's own clearance (h_gen) only ever needs <= 2 rows,
// already inside the margin, so the routing stays piece-independent. Tall
// boards (margin would exceed 24) keep the 5-lane PH30 slow-capable band.
#[inline]
fn pick_band(h: usize, _p: Piece) -> u8 {
    let need = h + 6;
    if need <= 6 {
        6
    } else if need <= 12 {
        12
    } else if need <= 18 {
        18
    } else if need <= 24 {
        24
    } else {
        30
    }
}

pub fn generate_packed(rows: &[u16; PH], p: Piece, moves: &mut MoveBuffer) {
    match pick_band(board_height(rows), p) {
        6 => p6::generate_packed(&shrink::<6>(rows), p, moves),
        12 => p12::generate_packed(&shrink::<12>(rows), p, moves),
        18 => p18::generate_packed(&shrink::<18>(rows), p, moves),
        24 => p24::generate_packed(&shrink::<24>(rows), p, moves),
        _ => p30::generate_packed(rows, p, moves),
    }
}

pub fn generate_packed_with_force(rows: &[u16; PH], p: Piece, force: bool, moves: &mut MoveBuffer) {
    match pick_band(board_height(rows), p) {
        6 => p6::generate_packed_with_force(&shrink::<6>(rows), p, force, moves),
        12 => p12::generate_packed_with_force(&shrink::<12>(rows), p, force, moves),
        18 => p18::generate_packed_with_force(&shrink::<18>(rows), p, force, moves),
        24 => p24::generate_packed_with_force(&shrink::<24>(rows), p, force, moves),
        _ => p30::generate_packed_with_force(rows, p, force, moves),
    }
}

pub fn reach_placements(rows: &[u16; PH], p: Piece) -> Vec<u16> {
    match pick_band(board_height(rows), p) {
        6 => p6::reach_placements(&shrink::<6>(rows), p),
        12 => p12::reach_placements(&shrink::<12>(rows), p),
        18 => p18::reach_placements(&shrink::<18>(rows), p),
        24 => p24::reach_placements(&shrink::<24>(rows), p),
        _ => p30::reach_placements(rows, p),
    }
}

pub fn labeled_placements(rows: &[u16; PH], p: Piece) -> Vec<u16> {
    match pick_band(board_height(rows), p) {
        6 => p6::labeled_placements(&shrink::<6>(rows), p),
        12 => p12::labeled_placements(&shrink::<12>(rows), p),
        18 => p18::labeled_placements(&shrink::<18>(rows), p),
        24 => p24::labeled_placements(&shrink::<24>(rows), p),
        _ => p30::labeled_placements(rows, p),
    }
}

pub fn reach_count(rows: &[u16; PH], p: Piece) -> u32 {
    match pick_band(board_height(rows), p) {
        6 => p6::reach_count(&shrink::<6>(rows), p),
        12 => p12::reach_count(&shrink::<12>(rows), p),
        18 => p18::reach_count(&shrink::<18>(rows), p),
        24 => p24::reach_count(&shrink::<24>(rows), p),
        _ => p30::reach_count(rows, p),
    }
}

pub fn labeled_count(rows: &[u16; PH], p: Piece) -> u32 {
    match pick_band(board_height(rows), p) {
        6 => p6::labeled_count(&shrink::<6>(rows), p),
        12 => p12::labeled_count(&shrink::<12>(rows), p),
        18 => p18::labeled_count(&shrink::<18>(rows), p),
        24 => p24::labeled_count(&shrink::<24>(rows), p),
        _ => p30::labeled_count(rows, p),
    }
}

#[cfg(test)]
mod force_seed_tests {
    use super::*;
    use crate::board::{Board, BOARD_HEIGHT, FULL_ROW};
    use crate::default_ruleset::ACTIVE_RULES;
    use crate::gen::SPAWN_COL;
    use crate::movegen::{
        generate_engine, generate_with_request, MovegenConsumer, MovegenOrder, MovegenRequest,
    };

    fn sync_cols(board: &mut Board) {
        for x in 0..COL_NB {
            let mask = 1u16 << x;
            let mut col = 0u64;
            for y in 0..BOARD_HEIGHT {
                if board.rows[y] & mask != 0 {
                    col |= 1u64 << y;
                }
            }
            board.cols[x] = col;
        }
    }

    fn spawn_row_open() -> Board {
        let mut board = Board::new();
        for y in 0..ACTIVE_RULES.spawn_row as usize {
            board.rows[y] = FULL_ROW & !(1u16 << SPAWN_COL);
        }
        sync_cols(&mut board);
        board
    }

    fn spawn_row_blocked_higher_open() -> Board {
        let mut board = Board::new();
        for y in 0..=ACTIVE_RULES.spawn_row as usize {
            board.rows[y] = FULL_ROW;
        }
        board.rows[ACTIVE_RULES.spawn_row as usize + 1] = FULL_ROW & !(1u16 << SPAWN_COL);
        sync_cols(&mut board);
        board
    }

    fn no_open_north_seed() -> Board {
        let mut board = Board::new();
        for y in 0..BOARD_HEIGHT {
            board.rows[y] = if y < ACTIVE_RULES.spawn_row as usize {
                FULL_ROW & !(1u16 << SPAWN_COL)
            } else {
                FULL_ROW
            };
        }
        sync_cols(&mut board);
        board
    }

    fn high_stack_garbage_like() -> Board {
        let mut board = Board::new();
        for y in 0..28 {
            board.rows[y] = match y % 4 {
                0 => FULL_ROW & !(1u16 << SPAWN_COL),
                1 => FULL_ROW & !(1u16 << 2),
                2 => FULL_ROW & !(1u16 << 7),
                _ => FULL_ROW & !(1u16 << 4),
            };
        }
        board.rows[ACTIVE_RULES.spawn_row as usize] &= !(1u16 << SPAWN_COL);
        sync_cols(&mut board);
        board
    }

    fn request_raw(board: &Board, piece: Piece) -> Vec<u16> {
        let request = MovegenRequest::new(piece)
            .with_force(true)
            .with_consumer(MovegenConsumer::Search)
            .with_order(MovegenOrder::ScalarCompatible);
        let mut moves = MoveBuffer::new();
        generate_with_request(board, &mut moves, request);
        moves.as_slice().iter().map(|mv| mv.raw()).collect()
    }

    fn engine_raw(board: &Board, piece: Piece) -> Vec<u16> {
        let mut moves = MoveBuffer::new();
        generate_engine(board, &mut moves, piece, true);
        moves.as_slice().iter().map(|mv| mv.raw()).collect()
    }

    fn packed_force_raw(board: &Board, piece: Piece) -> Vec<u16> {
        let rows: &[u16; PH] = board.rows[..PH].try_into().unwrap();
        let mut moves = MoveBuffer::new();
        generate_packed_with_force(rows, piece, true, &mut moves);
        let mut raw: Vec<u16> = moves.as_slice().iter().map(|mv| mv.raw()).collect();
        raw.sort_unstable();
        raw
    }

    #[test]
    fn movegen_force_true_search_request_matches_scalar_seed_cases() {
        let boards = [
            spawn_row_open(),
            spawn_row_blocked_higher_open(),
            no_open_north_seed(),
            high_stack_garbage_like(),
        ];
        for board in boards {
            for piece in [Piece::I, Piece::S, Piece::Z, Piece::L, Piece::J] {
                assert_eq!(
                    request_raw(&board, piece),
                    engine_raw(&board, piece),
                    "force=true request differs from scalar seed behavior for {piece:?} height={}",
                    board.height()
                );
            }
        }
    }

    #[test]
    fn movegen_packed_force_true_matches_scalar_seed_cases_as_set() {
        let boards = [spawn_row_open(), spawn_row_blocked_higher_open()];
        for board in boards {
            for piece in [Piece::I, Piece::S, Piece::Z, Piece::L, Piece::J] {
                let mut engine = engine_raw(&board, piece);
                engine.sort_unstable();
                assert_eq!(
                    packed_force_raw(&board, piece),
                    engine,
                    "packed force=true set differs from scalar seed behavior for {piece:?} height={}",
                    board.height()
                );
            }
        }
    }
}
