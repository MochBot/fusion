//! Row-major banded-bitboard move generation for the perft benchmark domain.
//!
//! This mirrors the upstream cobra-movegen HEAD design: boards are stored six
//! rows per u64 word (low 60 bits used), monomorphized over word count so the
//! perft driver can route each node to the smallest band that fits the stack.
//! Move generation computes whole-board "usable" maps per canonical rotation,
//! seeds the reachable set with a sky-drop smear plus tuck/rotation seeds, and
//! only falls back to a masked kick-wave BFS when the seeds do not already
//! cover every landable candidate.
//!
//! The module is self-contained on purpose: it uses the upstream piece order,
//! coordinate conventions, and ruleset (SRS kicks, no 180, no spins, spawn at
//! x=4 y=19) so its node counts are directly comparable with the upstream
//! `bench` binary. Production movegen and `ACTIVE_RULES` are not involved.

const WIDTH: i32 = 10;
const TLINES: i32 = 6;
const TALL: u64 = (1u64 << 60) - 1;

pub const SPAWN_X: i32 = 4;
pub const SPAWN_Y: i32 = 19;

// Piece indices in upstream order.
pub const PI_I: usize = 0;
pub const PI_O: usize = 1;
pub const PI_T: usize = 2;
pub const PI_L: usize = 3;
pub const PI_J: usize = 4;
pub const PI_S: usize = 5;
pub const PI_Z: usize = 6;

const fn col_word(x: i32) -> u64 {
    let mut w = 0u64;
    let mut k = 0;
    while k < TLINES {
        w |= 1u64 << (k * WIDTH + x);
        k += 1;
    }
    w
}

const COL0: u64 = col_word(0);
const COL9: u64 = col_word(9);

const fn cols_below(n: i32) -> u64 {
    let mut w = 0u64;
    let mut c = 0;
    while c < n {
        w |= col_word(c);
        c += 1;
    }
    w
}

#[inline(always)]
const fn dx_mask(dx: i32) -> u64 {
    if dx > 0 {
        TALL & !cols_below(dx)
    } else if dx < 0 {
        TALL & !(cols_below(-dx) << ((WIDTH + dx) as u32))
    } else {
        TALL
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SBoard<const N: usize> {
    pub d: [u64; N],
}

impl<const N: usize> SBoard<N> {
    pub const EMPTY: Self = Self { d: [0; N] };
    pub const H: i32 = TLINES * N as i32;

    #[inline(always)]
    pub fn any(&self) -> bool {
        let mut t = 0u64;
        let mut i = 0;
        while i < N {
            t |= self.d[i];
            i += 1;
        }
        t != 0
    }

    #[inline(always)]
    pub fn popcount(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0;
        while i < N {
            t += self.d[i].count_ones();
            i += 1;
        }
        t
    }

    #[inline(always)]
    pub fn set(&mut self, x: i32, y: i32) {
        debug_assert!((0..WIDTH).contains(&x) && y >= 0 && y < Self::H);
        self.d[(y / TLINES) as usize] |= 1u64 << ((y % TLINES) * WIDTH + x);
    }

    #[inline(always)]
    pub fn get(&self, x: i32, y: i32) -> bool {
        debug_assert!((0..WIDTH).contains(&x) && y >= 0 && y < Self::H);
        self.d[(y / TLINES) as usize] & (1u64 << ((y % TLINES) * WIDTH + x)) != 0
    }

    #[inline(always)]
    pub fn not(&self) -> Self {
        let mut out = [0u64; N];
        let mut i = 0;
        while i < N {
            out[i] = TALL & !self.d[i];
            i += 1;
        }
        Self { d: out }
    }

    /// Translate every cell by (dx, dy). Bits leaving the 10-column band or
    /// the vertical extent are dropped; vacated cells are zero. Call sites
    /// pass compile-time-constant offsets so the shift amounts fold.
    #[inline(always)]
    pub fn shifted(&self, dx: i32, dy: i32) -> Self {
        let mut out = [0u64; N];
        if dy == 0 {
            out = self.d;
        } else if dy > 0 {
            let q = ((dy - 1) / TLINES) as usize;
            let hi = ((((dy - 1) % TLINES) + 1) * WIDTH) as u32; // 10..=60
            let lo = 60 - hi;
            let mut i = 0;
            while i < N {
                let mut w = 0u64;
                if i >= q {
                    // hi can reach 60; split the shift so it stays < 64.
                    w |= (self.d[i - q] << (hi - 1)) << 1;
                }
                if i >= q + 1 {
                    w |= self.d[i - q - 1] >> lo;
                }
                out[i] = w;
                i += 1;
            }
        } else {
            let q = ((-dy - 1) / TLINES) as usize;
            let lo = ((((-dy - 1) % TLINES) + 1) * WIDTH) as u32; // 10..=60
            let hi = 60 - lo;
            let mut i = 0;
            while i < N {
                let mut w = 0u64;
                if i + q < N {
                    w |= (self.d[i + q] >> (lo - 1)) >> 1;
                }
                if i + q + 1 < N {
                    w |= self.d[i + q + 1] << hi;
                }
                out[i] = w;
                i += 1;
            }
        }

        let m = dx_mask(dx);
        let mut i = 0;
        while i < N {
            let w = out[i];
            out[i] = if dx > 0 {
                w << dx
            } else if dx < 0 {
                w >> -dx
            } else {
                w
            } & m;
            i += 1;
        }
        Self { d: out }
    }

    #[inline(always)]
    pub fn and(&self, o: &Self) -> Self {
        let mut out = [0u64; N];
        let mut i = 0;
        while i < N {
            out[i] = self.d[i] & o.d[i];
            i += 1;
        }
        Self { d: out }
    }

    #[inline(always)]
    pub fn or(&self, o: &Self) -> Self {
        let mut out = [0u64; N];
        let mut i = 0;
        while i < N {
            out[i] = self.d[i] | o.d[i];
            i += 1;
        }
        Self { d: out }
    }

    #[inline(always)]
    pub fn xor(&self, o: &Self) -> Self {
        let mut out = [0u64; N];
        let mut i = 0;
        while i < N {
            out[i] = self.d[i] ^ o.d[i];
            i += 1;
        }
        Self { d: out }
    }

    #[inline(always)]
    pub fn andnot(&self, o: &Self) -> Self {
        let mut out = [0u64; N];
        let mut i = 0;
        while i < N {
            out[i] = self.d[i] & !o.d[i];
            i += 1;
        }
        Self { d: out }
    }

    /// Full rows, reported as the column-9 bit of each full row. The add
    /// carries through bits 0..8 of a row exactly when they are all set, so
    /// no carry ever crosses into the next row.
    #[inline(always)]
    pub fn line_clears(&self) -> Self {
        let mut out = [0u64; N];
        let mut i = 0;
        while i < N {
            let d = self.d[i];
            out[i] = d & ((d & !COL9) + COL0) & COL9;
            i += 1;
        }
        Self { d: out }
    }

    /// Remove the rows flagged in `lines` (column-9 bits) and pack everything
    /// above downwards, in-word first and then across words.
    pub fn clear_lines(&mut self, lines: &Self) {
        let mut prefix = [0i32; N];
        let mut i = 0;
        while i + 1 < N {
            prefix[i + 1] = prefix[i] + lines.d[i].count_ones() as i32;
            i += 1;
        }

        let mut packed = [0u64; N];
        let mut i = 0;
        while i < N {
            let ld = self.d[i];
            let ll = lines.d[i];
            let mut p = 0u64;
            let mut dest = 0u32;
            let mut row = 0;
            while row < TLINES {
                let src = (row * WIDTH) as u32;
                if (ll >> (src + 9)) & 1 == 0 {
                    p |= ((ld >> src) & 0x3FF) << dest;
                    dest += WIDTH as u32;
                }
                row += 1;
            }
            packed[i] = p;
            i += 1;
        }

        let mut dest = 0;
        while dest < N {
            let mut result = 0u64;
            let mut src = dest;
            while src < N {
                let relative = ((src - dest) as i32 * TLINES) - prefix[src];
                if relative >= 0 && relative < TLINES {
                    result |= packed[src] << (relative * WIDTH);
                } else if relative < 0 && relative > -TLINES {
                    result |= packed[src] >> (-relative * WIDTH);
                }
                src += 1;
            }
            self.d[dest] = result & TALL;
            dest += 1;
        }
    }

    /// Place a piece (canonical rotation cells) and resolve line clears.
    #[inline(always)]
    pub fn do_move(&mut self, p: usize, rc: usize, x: i32, y: i32) -> u32 {
        self.set(x, y);
        let cells = PCELLS[p][rc];
        self.set(x + cells[0].0 as i32, y + cells[0].1 as i32);
        self.set(x + cells[1].0 as i32, y + cells[1].1 as i32);
        self.set(x + cells[2].0 as i32, y + cells[2].1 as i32);
        let clears = self.line_clears();
        if clears.any() {
            let n = clears.popcount();
            self.clear_lines(&clears);
            n
        } else {
            0
        }
    }

    /// One past the highest occupied row, or 0 for an empty board.
    #[inline(always)]
    pub fn max_y(&self) -> i32 {
        let mut i = N;
        while i > 0 {
            i -= 1;
            let bits = self.d[i];
            if bits != 0 {
                let idx = 63 - bits.leading_zeros() as i32;
                return i as i32 * TLINES + idx / WIDTH + 1;
            }
        }
        0
    }

    #[inline(always)]
    pub fn cast<const M: usize>(&self) -> SBoard<M> {
        let mut out = [0u64; M];
        let mut i = 0;
        while i < M && i < N {
            out[i] = self.d[i];
            i += 1;
        }
        SBoard { d: out }
    }

    #[inline(always)]
    pub fn for_each_set_bit(&self, mut f: impl FnMut(i32, i32)) {
        let mut i = 0;
        while i < N {
            let mut bits = self.d[i];
            while bits != 0 {
                let idx = bits.trailing_zeros() as i32;
                f(idx % WIDTH, i as i32 * TLINES + idx / WIDTH);
                bits &= bits - 1;
            }
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Piece data (upstream conventions).

const fn base_cells(p: usize) -> [(i8, i8); 3] {
    match p {
        PI_I => [(-1, 0), (1, 0), (2, 0)],
        PI_O => [(1, 0), (0, 1), (1, 1)],
        PI_T => [(-1, 0), (1, 0), (0, 1)],
        PI_L => [(-1, 0), (1, 0), (1, 1)],
        PI_J => [(-1, 0), (1, 0), (-1, 1)],
        PI_S => [(-1, 0), (0, 1), (1, 1)],
        PI_Z => [(-1, 1), (0, 1), (1, 0)],
        _ => [(0, 0), (0, 0), (0, 0)],
    }
}

const fn rot_cell(c: (i8, i8), r: usize) -> (i8, i8) {
    match r {
        0 => c,
        1 => (c.1, -c.0),
        2 => (-c.0, -c.1),
        _ => (-c.1, c.0),
    }
}

const fn build_pcells() -> [[[(i8, i8); 3]; 4]; 7] {
    let mut out = [[[(0i8, 0i8); 3]; 4]; 7];
    let mut p = 0;
    while p < 7 {
        let base = base_cells(p);
        let mut r = 0;
        while r < 4 {
            let mut i = 0;
            while i < 3 {
                out[p][r][i] = rot_cell(base[i], r);
                i += 1;
            }
            r += 1;
        }
        p += 1;
    }
    out
}

pub const PCELLS: [[[(i8, i8); 3]; 4]; 7] = build_pcells();

pub const fn group2(p: usize) -> bool {
    matches!(p, PI_I | PI_S | PI_Z)
}

pub const fn group3(p: usize) -> bool {
    matches!(p, PI_T | PI_L | PI_J)
}

pub const fn csize(p: usize) -> usize {
    if p == PI_O {
        1
    } else if group2(p) {
        2
    } else {
        4
    }
}

pub const fn ssize(p: usize) -> usize {
    if p == PI_O {
        1
    } else {
        4
    }
}

pub const fn canon_r(p: usize, r: usize) -> usize {
    if p == PI_O {
        0
    } else if group2(p) {
        r & 1
    } else {
        r
    }
}

pub const fn canon_off(p: usize, r: usize) -> (i32, i32) {
    if p == PI_I {
        if r == 2 {
            return (1, 0);
        }
        if r == 3 {
            return (0, -1);
        }
    }
    if p == PI_S || p == PI_Z {
        if r == 2 {
            return (0, 1);
        }
        if r == 3 {
            return (1, 0);
        }
    }
    (0, 0)
}

pub const fn h_gen(p: usize) -> i32 {
    if p == PI_I || p == PI_T {
        2
    } else if p == PI_O {
        0
    } else {
        1
    }
}

pub const fn h_spawn(p: usize) -> i32 {
    if p == PI_I {
        2
    } else if p == PI_O {
        0
    } else {
        1
    }
}

pub const fn h_place(p: usize) -> i32 {
    2 + (p == PI_I) as i32 - (p == PI_O) as i32
}

type K5 = [(i8, i8); 5];

// SRS kick tables (CW, CCW), upstream layout: rows indexed by source rotation.
const KICKS_LJSZT: [[K5; 4]; 2] = [
    [
        [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
    ],
    [
        [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
    ],
];

const KICKS_I: [[K5; 4]; 2] = [
    [
        [(1, 0), (-1, 0), (2, 0), (-1, -1), (2, 2)],
        [(0, -1), (-1, -1), (2, -1), (-1, 1), (2, -2)],
        [(-1, 0), (1, 0), (-2, 0), (1, 1), (-2, -2)],
        [(0, 1), (1, 1), (-2, 1), (1, -1), (-2, 2)],
    ],
    [
        [(0, -1), (-1, -1), (2, -1), (-1, 1), (2, -2)],
        [(-1, 0), (1, 0), (-2, 0), (1, 1), (-2, -2)],
        [(0, 1), (1, 1), (-2, 1), (1, -1), (-2, 2)],
        [(1, 0), (-1, 0), (2, 0), (-1, -1), (2, 2)],
    ],
];

const fn kick_row_const(p: usize, d: usize, r: usize) -> K5 {
    if p == PI_I {
        KICKS_I[d][r]
    } else {
        KICKS_LJSZT[d][r]
    }
}

/// Per-(piece, direction, source-rotation) kick data as associated consts so
/// the BFS kick waves compile to constant-offset shifts.
struct KickTab<const P: usize, const D: usize, const R: usize>;

impl<const P: usize, const D: usize, const R: usize> KickTab<P, D, R> {
    const R1: usize = if D == 0 { (R + 1) & 3 } else { (R + 3) & 3 };
    const R1C: usize = canon_r(P, Self::R1);
    const OFF_X: i32 = canon_off(P, R).0 - canon_off(P, Self::R1).0;
    const OFF_Y: i32 = canon_off(P, R).1 - canon_off(P, Self::R1).1;
    const ROW: K5 = kick_row_const(P, D, R);
}

// ---------------------------------------------------------------------------
// Move generation.

pub struct SMoves<const N: usize> {
    pub m: [SBoard<N>; 4],
}

impl<const N: usize> SMoves<N> {
    pub const EMPTY: Self = Self {
        m: [SBoard::EMPTY; 4],
    };

    #[inline(always)]
    pub fn popcount(&self, cs: usize) -> u32 {
        let mut t = 0;
        let mut r = 0;
        while r < cs {
            t += self.m[r].popcount();
            r += 1;
        }
        t
    }
}

/// One non-anchor cell's contribution to the usable map. `R` and `I` are
/// const generics so the table lookups and shift offsets fold to immediates;
/// a runtime loop over the cells defeated that folding and dominated the
/// profile.
#[inline(always)]
fn usable_cell<const P: usize, const R: usize, const I: usize, const N: usize>(
    b: &SBoard<N>,
    nb: &SBoard<N>,
) -> SBoard<N> {
    let cx = PCELLS[P][R][I].0 as i32;
    let cy = PCELLS[P][R][I].1 as i32;
    if cy > 0 {
        b.shifted(0, -cy).not().shifted(-cx, 0)
    } else {
        nb.shifted(-cx, -cy)
    }
}

#[inline(always)]
fn usable_rot<const P: usize, const R: usize, const N: usize>(
    b: &SBoard<N>,
    nb: &SBoard<N>,
) -> SBoard<N> {
    nb.and(&usable_cell::<P, R, 0, N>(b, nb))
        .and(&usable_cell::<P, R, 1, N>(b, nb))
        .and(&usable_cell::<P, R, 2, N>(b, nb))
}

/// Anchor positions where the piece can collision-freely exist, per canonical
/// rotation. Cells above the band top do not collide ("don't kick against
/// the ceiling"); the routing layer guarantees the band is tall enough that
/// real placements stay inside it.
#[inline(always)]
fn usable_map<const P: usize, const N: usize>(b: &SBoard<N>) -> [SBoard<N>; 4] {
    let nb = b.not();
    let mut u = [SBoard::EMPTY; 4];
    u[0] = usable_rot::<P, 0, N>(b, &nb);
    if csize(P) > 1 {
        u[1] = usable_rot::<P, 1, N>(b, &nb);
    }
    if csize(P) > 2 {
        u[2] = usable_rot::<P, 2, N>(b, &nb);
        u[3] = usable_rot::<P, 3, N>(b, &nb);
    }
    u
}

#[inline(always)]
fn landable_map<const N: usize>(u: &[SBoard<N>; 4], cs: usize) -> [SBoard<N>; 4] {
    let mut c = [SBoard::EMPTY; 4];
    let mut r = 0;
    while r < cs {
        c[r] = u[r].andnot(&u[r].shifted(0, 1));
        r += 1;
    }
    c
}

/// One first-valid-kick wave step. `I` is a const generic for the same
/// reason as `usable_cell`: the kick offsets must fold to immediate shift
/// amounts, and a runtime 0..5 loop did not unroll.
#[inline(always)]
fn kick_step<const P: usize, const D: usize, const R: usize, const I: usize, const N: usize>(
    temp: &mut SBoard<N>,
    result: &mut SBoard<N>,
    usable_r1c: &SBoard<N>,
) {
    let kx = KickTab::<P, D, R>::ROW[I].0 as i32 + KickTab::<P, D, R>::OFF_X;
    let ky = KickTab::<P, D, R>::ROW[I].1 as i32 + KickTab::<P, D, R>::OFF_Y;
    *result = result.or(&temp.shifted(kx, ky));
    if I != 4 {
        *temp = temp.andnot(&usable_r1c.shifted(-kx, -ky));
    }
}

/// Generate all reachable lock positions for piece `P` on an `N`-word band.
/// `y` is the stack height (`max_y`), `force` extends the spawn scan upward.
pub fn generate<const P: usize, const N: usize>(b: &SBoard<N>, y: i32, force: i32) -> SMoves<N> {
    const { assert!(P < 7) };
    let h: i32 = TLINES * N as i32;
    let cs = csize(P);
    let ss = ssize(P);
    let all_done: u32 = (1u32 << ss) - 1;

    let usable = usable_map::<P, N>(b);
    let cands = landable_map(&usable, cs);

    let mut moves = [SBoard::<N>::EMPTY; 4];
    let mut search = [SBoard::<N>::EMPTY; 4];
    let mut remaining: u32 = 0;
    let mut done: u32;

    if h > SPAWN_Y && y > SPAWN_Y - h_spawn(P) {
        // Slow init: the stack reaches the spawn area, so reachability has to
        // start from the actual spawn cell (scanning upward by `force`).
        let threshold = (SPAWN_Y + force + 1).min(h);
        let mut s = SPAWN_Y;
        while s < threshold && !usable[0].get(SPAWN_X, s) {
            s += 1;
        }
        if s == threshold {
            return SMoves::EMPTY;
        }
        search[0].set(SPAWN_X, s);
        remaining = (1u32 << cs) - 1;
        done = all_done & !1;
    } else {
        // Fast init: smear the blocked map downward so `search` starts as the
        // sky-droppable set. Sky-drop reachability is a sound subset of full
        // reachability, so if it already covers every landable candidate the
        // tuck/seed/BFS phases cannot change the answer; most open boards
        // (the bulk of perft leaves) exit here before any tuck work.
        let ceiling = h - h_gen(P);
        let mut r = 0;
        while r < cs {
            let mut surface = usable[r].not();
            if ceiling >= 1 {
                surface = surface.or(&surface.shifted(0, -1));
            }
            if ceiling >= 2 {
                surface = surface.or(&surface.shifted(0, -2));
            }
            if ceiling >= 4 {
                surface = surface.or(&surface.shifted(0, -4));
            }
            if ceiling >= 8 {
                surface = surface.or(&surface.shifted(0, -8));
            }
            if ceiling >= 16 {
                surface = surface.or(&surface.shifted(0, -16));
            }
            search[r] = surface.not();
            moves[r] = search[r].and(&cands[r]);
            if moves[r] != cands[r] {
                remaining |= 1 << r;
            }
            r += 1;
        }
        if remaining == 0 {
            return SMoves { m: moves };
        }

        // Two rounds of horizontal tucks, then the kick-0 rotation seeds.
        let mut r = 0;
        while r < cs {
            let mut s = search[r];
            s = s.or(&s.shifted(-1, 0).or(&s.shifted(1, 0)).and(&usable[r]));
            s = s.or(&s.shifted(-1, 0).or(&s.shifted(1, 0)).and(&usable[r]));
            search[r] = s;
            r += 1;
        }

        if group3(P) {
            // Sequential on purpose, matching upstream: later rotations may
            // pick up seeds added to earlier ones. Any sound seed superset
            // yields the same closure.
            let mut r = 0;
            while r < 4 {
                let r1 = (r + 1) & 3;
                let r2 = (r + 3) & 3;
                search[r] = search[r].or(&search[r1].or(&search[r2]).and(&usable[r]));
                r += 1;
            }
        }

        remaining = 0;
        let mut r = 0;
        while r < cs {
            moves[r] = search[r].and(&cands[r]);
            if moves[r] != cands[r] {
                remaining |= 1 << r;
            }
            r += 1;
        }
        if remaining == 0 {
            return SMoves { m: moves };
        }
        if group2(P) {
            search[2] = search[0];
            search[3] = search[1];
        }
        done = 0;
    }

    // BFS over nominal rotations with masked first-valid-kick waves.
    let mut unsearched = [SBoard::<N>::EMPTY; 4];
    let mut rs = 0;
    while rs < ss {
        unsearched[rs] = search[rs].not().and(&usable[canon_r(P, rs)]);
        rs += 1;
    }

    macro_rules! rot_kick {
        ($r:literal, $d:literal) => {{
            let r1 = KickTab::<P, $d, $r>::R1;
            let r1c = KickTab::<P, $d, $r>::R1C;
            // The wave's entire effect is gated by `res = result & unsearched[r1]`,
            // so a fully-searched target rotation makes the five kick steps a no-op.
            // Measured on depth-7 IOLJSZT: 37% of waves hit this skip.
            if unsearched[r1].any() {
                let mut temp = search[$r];
                let mut result = SBoard::<N>::EMPTY;
                kick_step::<P, $d, $r, 0, N>(&mut temp, &mut result, &usable[r1c]);
                kick_step::<P, $d, $r, 1, N>(&mut temp, &mut result, &usable[r1c]);
                kick_step::<P, $d, $r, 2, N>(&mut temp, &mut result, &usable[r1c]);
                kick_step::<P, $d, $r, 3, N>(&mut temp, &mut result, &usable[r1c]);
                kick_step::<P, $d, $r, 4, N>(&mut temp, &mut result, &usable[r1c]);
                let res = result.and(&unsearched[r1]);
                if res.any() {
                    search[r1] = search[r1].or(&res);
                    unsearched[r1] = unsearched[r1].andnot(&res);
                    done &= !(1u32 << r1);
                    moves[r1c] = moves[r1c].or(&res.and(&cands[r1c]));
                    if moves[r1c] != cands[r1c] {
                        remaining |= 1 << r1c;
                    } else {
                        remaining &= !(1u32 << r1c);
                    }
                }
            }
        }};
    }

    macro_rules! process_rot {
        ($r:literal) => {
            if $r < ss && done & (1 << $r) == 0 {
                done |= 1 << $r;
                let rc = canon_r(P, $r);

                loop {
                    let temp = search[$r]
                        .shifted(-1, 0)
                        .or(&search[$r].shifted(1, 0))
                        .or(&search[$r].shifted(0, -1))
                        .and(&unsearched[$r]);
                    if !temp.any() {
                        break;
                    }
                    search[$r] = search[$r].or(&temp);
                    unsearched[$r] = unsearched[$r].xor(&temp);
                }

                moves[rc] = moves[rc].or(&search[$r].and(&cands[rc]));
                if moves[rc] != cands[rc] {
                    remaining |= 1 << rc;
                } else {
                    remaining &= !(1u32 << rc);
                }

                if remaining == 0 {
                    done = all_done;
                } else {
                    if P != PI_O {
                        rot_kick!($r, 0);
                        rot_kick!($r, 1);
                        if remaining == 0 {
                            done = all_done;
                        }
                    }
                    if done != all_done {
                        search[$r] = SBoard::EMPTY;
                    }
                }
            }
        };
    }

    while done != all_done {
        process_rot!(0);
        process_rot!(1);
        process_rot!(2);
        process_rot!(3);
    }

    SMoves { m: moves }
}

// ---------------------------------------------------------------------------
// Perft driver with band routing.

const fn band_words(h: i32) -> usize {
    if h < 6 {
        1
    } else if h < 12 {
        2
    } else if h < 18 {
        3
    } else if h < 24 {
        4
    } else {
        8
    }
}

fn leaf<const P: usize, const N: usize>(b: &SBoard<8>, h: i32) -> u64 {
    let b1: SBoard<N> = b.cast();
    generate::<P, N>(&b1, h, 0).popcount(csize(P)) as u64
}

fn step_cast<const P: usize, const N: usize, const M: usize>(
    b1: &SBoard<N>,
    rc: usize,
    x: i32,
    y: i32,
    q: &[usize],
    depth: usize,
) -> u64 {
    let mut b2: SBoard<M> = b1.cast();
    b2.do_move(P, rc, x, y);
    let nb: SBoard<8> = b2.cast();
    perft_rec(&nb, q, depth)
}

fn inner<const P: usize, const N: usize>(b: &SBoard<8>, q: &[usize], depth: usize, h: i32) -> u64 {
    let b1: SBoard<N> = b.cast();
    let ml = generate::<P, N>(&b1, h, 0);
    let h2w = band_words(h + h_place(P));
    let mut nodes = 0u64;
    let cs = csize(P);
    let rest = &q[1..];
    let mut rc = 0;
    while rc < cs {
        ml.m[rc].for_each_set_bit(|x, y| {
            nodes += if h2w == N {
                step_cast::<P, N, N>(&b1, rc, x, y, rest, depth - 1)
            } else {
                match h2w {
                    2 => step_cast::<P, N, 2>(&b1, rc, x, y, rest, depth - 1),
                    3 => step_cast::<P, N, 3>(&b1, rc, x, y, rest, depth - 1),
                    4 => step_cast::<P, N, 4>(&b1, rc, x, y, rest, depth - 1),
                    _ => step_cast::<P, N, 8>(&b1, rc, x, y, rest, depth - 1),
                }
            };
        });
        rc += 1;
    }
    nodes
}

fn with_piece<const P: usize>(b: &SBoard<8>, q: &[usize], depth: usize) -> u64 {
    let h = b.max_y();
    let h1w = band_words(h + h_gen(P));
    if depth == 1 {
        return match h1w {
            1 => leaf::<P, 1>(b, h),
            2 => leaf::<P, 2>(b, h),
            3 => leaf::<P, 3>(b, h),
            4 => leaf::<P, 4>(b, h),
            _ => leaf::<P, 8>(b, h),
        };
    }
    match h1w {
        1 => inner::<P, 1>(b, q, depth, h),
        2 => inner::<P, 2>(b, q, depth, h),
        3 => inner::<P, 3>(b, q, depth, h),
        4 => inner::<P, 4>(b, q, depth, h),
        _ => inner::<P, 8>(b, q, depth, h),
    }
}

fn perft_rec(b: &SBoard<8>, q: &[usize], depth: usize) -> u64 {
    match q[0] {
        0 => with_piece::<0>(b, q, depth),
        1 => with_piece::<1>(b, q, depth),
        2 => with_piece::<2>(b, q, depth),
        3 => with_piece::<3>(b, q, depth),
        4 => with_piece::<4>(b, q, depth),
        5 => with_piece::<5>(b, q, depth),
        _ => with_piece::<6>(b, q, depth),
    }
}

/// Perft from an empty board over the given piece queue.
pub fn perft(queue: &[usize]) -> u64 {
    assert!(!queue.is_empty() && queue.iter().all(|&p| p < 7));
    let b = SBoard::<8>::EMPTY;
    perft_rec(&b, queue, queue.len())
}

/// Parse an upstream-style queue string such as "IOLJSZT".
pub fn parse_queue(s: &str) -> Option<Vec<usize>> {
    s.chars()
        .map(|c| match c {
            'I' => Some(PI_I),
            'O' => Some(PI_O),
            'T' => Some(PI_T),
            'L' => Some(PI_L),
            'J' => Some(PI_J),
            'S' => Some(PI_S),
            'Z' => Some(PI_Z),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Naive reference model: plain bool grid, same coordinate conventions.
    #[derive(Clone)]
    struct Grid {
        h: i32,
        g: Vec<[bool; 10]>,
    }

    impl Grid {
        fn new(h: i32) -> Self {
            Self {
                h,
                g: vec![[false; 10]; h as usize],
            }
        }

        fn get(&self, x: i32, y: i32) -> bool {
            if !(0..10).contains(&x) || !(0..self.h).contains(&y) {
                return false;
            }
            self.g[y as usize][x as usize]
        }

        fn set(&mut self, x: i32, y: i32) {
            self.g[y as usize][x as usize] = true;
        }

        fn shifted(&self, dx: i32, dy: i32) -> Self {
            let mut out = Grid::new(self.h);
            for y in 0..self.h {
                for x in 0..10 {
                    if self.get(x - dx, y - dy) {
                        out.set(x, y);
                    }
                }
            }
            out
        }
    }

    fn to_grid<const N: usize>(b: &SBoard<N>) -> Grid {
        let mut g = Grid::new(SBoard::<N>::H);
        b.for_each_set_bit(|x, y| g.set(x, y));
        g
    }

    fn from_grid<const N: usize>(g: &Grid) -> SBoard<N> {
        let mut b = SBoard::<N>::EMPTY;
        for y in 0..g.h.min(SBoard::<N>::H) {
            for x in 0..10 {
                if g.get(x, y) {
                    b.set(x, y);
                }
            }
        }
        b
    }

    fn xs(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn random_board<const N: usize>(state: &mut u64, fill_rows: i32) -> SBoard<N> {
        let mut b = SBoard::<N>::EMPTY;
        for y in 0..fill_rows.min(SBoard::<N>::H) {
            let row = (xs(state) & 0x3FF) as u64 & !(1 << (xs(state) % 10));
            for x in 0..10 {
                if row & (1 << x) != 0 {
                    b.set(x, y);
                }
            }
        }
        b
    }

    fn shift_case<const N: usize>(state: &mut u64) {
        for _ in 0..40 {
            let b = random_board::<N>(state, SBoard::<N>::H);
            let g = to_grid(&b);
            for dx in -3i32..=3 {
                for dy in -17i32..=17 {
                    let want = from_grid::<N>(&g.shifted(dx, dy));
                    let got = b.shifted(dx, dy);
                    assert_eq!(got, want, "N={} dx={} dy={}", N, dx, dy);
                }
            }
        }
    }

    #[test]
    fn shifted_matches_naive_grid_model() {
        let mut st = 0x51EE_D00D_2026_0612u64;
        shift_case::<1>(&mut st);
        shift_case::<2>(&mut st);
        shift_case::<4>(&mut st);
        shift_case::<8>(&mut st);
    }

    #[test]
    fn line_clear_matches_naive_compaction() {
        let mut st = 0xC1EA_2026_0612_0001u64;
        for _ in 0..500 {
            let mut b = random_board::<4>(&mut st, 20);
            // Force a few full rows.
            let nfull = 1 + (xs(&mut st) % 3) as i32;
            for _ in 0..nfull {
                let y = (xs(&mut st) % 20) as i32;
                for x in 0..10 {
                    b.set(x, y);
                }
            }

            // Naive: collect non-full rows bottom-up.
            let g = to_grid(&b);
            let mut want = Grid::new(g.h);
            let mut dest = 0;
            let mut cleared = 0;
            for y in 0..g.h {
                if (0..10).all(|x| g.get(x, y)) {
                    cleared += 1;
                    continue;
                }
                for x in 0..10 {
                    if g.get(x, y) {
                        want.set(x, dest);
                    }
                }
                dest += 1;
            }

            let lines = b.line_clears();
            assert_eq!(lines.popcount(), cleared, "clear count");
            let mut got = b;
            got.clear_lines(&lines);
            assert_eq!(got, from_grid::<4>(&want));
        }
    }

    // Naive usable check: anchor + cells; above-top free, walls/floor solid.
    fn naive_usable(g: &Grid, p: usize, rc: usize, x: i32, y: i32) -> bool {
        if !(0..10).contains(&x) || y < 0 || y >= g.h {
            return false;
        }
        let mut cells = [(0i32, 0i32); 4];
        cells[0] = (x, y);
        for (i, c) in PCELLS[p][rc].iter().enumerate() {
            cells[i + 1] = (x + c.0 as i32, y + c.1 as i32);
        }
        cells
            .iter()
            .all(|&(cx, cy)| (0..10).contains(&cx) && cy >= 0 && (cy >= g.h || !g.get(cx, cy)))
    }

    #[test]
    fn usable_and_landable_match_naive_cell_checks() {
        let mut st = 0x0A11_2026_0612_0002u64;
        for _ in 0..200 {
            let b = random_board::<2>(&mut st, 8);
            let g = to_grid(&b);
            for p in 0..7 {
                let u = match p {
                    0 => usable_map::<0, 2>(&b),
                    1 => usable_map::<1, 2>(&b),
                    2 => usable_map::<2, 2>(&b),
                    3 => usable_map::<3, 2>(&b),
                    4 => usable_map::<4, 2>(&b),
                    5 => usable_map::<5, 2>(&b),
                    _ => usable_map::<6, 2>(&b),
                };
                let c = landable_map(&u, csize(p));
                for rc in 0..csize(p) {
                    for y in 0..12 {
                        for x in 0..10 {
                            let want = naive_usable(&g, p, rc, x, y);
                            assert_eq!(
                                u[rc].get(x, y),
                                want,
                                "usable p={} rc={} x={} y={}",
                                p,
                                rc,
                                x,
                                y
                            );
                            let want_land = want && !naive_usable(&g, p, rc, x, y - 1);
                            assert_eq!(
                                c[rc].get(x, y),
                                want_land,
                                "landable p={} rc={} x={} y={}",
                                p,
                                rc,
                                x,
                                y
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn perft_matches_upstream_golden_counts() {
        // Reference values from the upstream bench binary (HEAD 45ee318) under
        // the SRS / no-spin / no-180 / spawn-19 configuration.
        let cases: [(&str, u64); 6] = [
            ("I", 17),
            ("IO", 153),
            ("IOL", 5266),
            ("IOLJ", 188_374),
            ("IOLJS", 3_497_187),
            ("IIIIII", 33_325_345),
        ];
        for (q, want) in cases {
            let queue = parse_queue(q).unwrap();
            assert_eq!(perft(&queue), want, "queue {}", q);
        }
    }

    #[test]
    #[ignore] // release-only: ~70M and ~2.6B node runs
    fn perft_matches_upstream_golden_counts_deep() {
        let cases: [(&str, u64); 2] = [("IOLJSZ", 67_002_200), ("IOLJSZT", 2_647_076_135)];
        for (q, want) in cases {
            let queue = parse_queue(q).unwrap();
            assert_eq!(perft(&queue), want, "queue {}", q);
        }
    }
}
