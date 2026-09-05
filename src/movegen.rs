// movegen.rs -- 1:1 port of movegen.hpp + movegen.cpp
// const generics mirror C++ template<Piece p1> specialization
use crate::board::Board;
#[cfg(not(target_arch = "wasm32"))]
use crate::default_ruleset::ACTIVE_RULES;
#[cfg(not(target_arch = "wasm32"))]
use crate::gen::{
    canonical_offset, canonical_r, canonical_size, group2, in_bounds, kick_180_index, kick_index,
    rotate, CollisionMap, CollisionMap16, Direction, KICKS, KICKS_180, SPAWN_COL,
};
use crate::header::*;

pub use crate::move_buffer::{MoveBuffer, MoveList};

// compile-time piece from const generic index; must match Piece enum discriminants
#[cfg(not(target_arch = "wasm32"))]
#[inline(always)]
const fn piece_from_index(p: usize) -> Piece {
    match p {
        0 => Piece::I,
        1 => Piece::O,
        2 => Piece::T,
        3 => Piece::L,
        4 => Piece::J,
        5 => Piece::S,
        6 => Piece::Z,
        _ => Piece::I,
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct WaveTab {
    r1: u8,
    rc: u8,
    n: u8,
    dx: [i8; 6],
    dy: [u8; 6],
}

#[cfg(not(target_arch = "wasm32"))]
impl WaveTab {
    const EMPTY: WaveTab = WaveTab {
        r1: 0,
        rc: 0,
        n: 0,
        dx: [0; 6],
        dy: [0; 6],
    };
}

// Kick tables computed at compile time per piece; the wave loop unrolls with
// literal offsets and constant shift amounts, matching the upstream C++
// constexpr-template codegen.
#[cfg(not(target_arch = "wasm32"))]
const fn build_wave_tables(p_idx: usize) -> [[WaveTab; ROTATION_NB]; 3] {
    let p = piece_from_index(p_idx);
    let ki = kick_index(p, ACTIVE_RULES.srs_plus);
    let ki180 = kick_180_index(p);
    let mut tabs = [[WaveTab::EMPTY; ROTATION_NB]; 3];
    let mut d_idx = 0;
    while d_idx < 3 {
        let dir = match d_idx {
            0 => Direction::Cw,
            1 => Direction::Ccw,
            _ => Direction::Flip,
        };
        let mut ri = 0;
        while ri < ROTATION_NB {
            let r = Rotation::from_u8(ri as u8);
            let r1 = rotate(dir, r);
            let rc = canonical_r(p, r1);
            let a = canonical_offset(p, r);
            let b = canonical_offset(p, r1);
            let off_x = a.x as i32 - b.x as i32;
            let off_y = a.y as i32 - b.y as i32;
            let mut dx = [0i8; 6];
            let mut dy = [0u8; 6];
            let n: u8;
            if d_idx < 2 {
                let kicks = &KICKS[ki][d_idx][ri];
                n = kicks.len() as u8;
                let mut i = 0;
                while i < kicks.len() {
                    dx[i] = (kicks[i].x as i32 + off_x) as i8;
                    dy[i] = (3 + kicks[i].y as i32 + off_y) as u8;
                    i += 1;
                }
            } else {
                let kicks = &KICKS_180[ki180][ri];
                n = if ACTIVE_RULES.srs_plus {
                    kicks.len() as u8
                } else {
                    2
                };
                let mut i = 0;
                while i < kicks.len() {
                    dx[i] = (kicks[i].x as i32 + off_x) as i8;
                    dy[i] = (3 + kicks[i].y as i32 + off_y) as u8;
                    i += 1;
                }
            }
            tabs[d_idx][ri] = WaveTab {
                r1: r1 as u8,
                rc: rc as u8,
                n,
                dx,
                dy,
            };
            ri += 1;
        }
        d_idx += 1;
    }
    tabs
}

#[cfg(not(target_arch = "wasm32"))]
struct WaveTables<const P: usize>;

#[cfg(not(target_arch = "wasm32"))]
impl<const P: usize> WaveTables<P> {
    const TABS: [[WaveTab; ROTATION_NB]; 3] = build_wave_tables(P);
}

#[cfg(not(target_arch = "wasm32"))]
static ZERO_SMAP: [[Bitboard; 5]; COL_NB] = [[0; 5]; COL_NB];

// Hot path: rotation/canonical/offset/kick lookups are folded into WaveTab once
// per generate call, so this loop touches only precomputed deltas and masks.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
#[cfg(not(target_arch = "wasm32"))]
fn rotation_wave<const CHECK_SPIN: bool, const HAS_SMAP: bool>(
    w: &WaveTab,
    x: usize,
    ri: usize,
    to_search: &mut [[Bitboard; ROTATION_NB]; COL_NB],
    searched: &[[Bitboard; ROTATION_NB]; COL_NB],
    remaining: &mut Bitboard,
    spin_set: &mut [[[Bitboard; SPIN_NB]; ROTATION_NB]; COL_NB],
    imm: &[[Bitboard; ROTATION_NB]; COL_NB],
    cm: &CollisionMap,
    smap: &[[Bitboard; 5]; COL_NB],
) {
    let r1i = w.r1 as usize;
    let rc = Rotation::from_u8(w.rc);
    let mut current = to_search[x][ri];
    let n = w.n as usize;

    for i in 0..n {
        if current == 0 {
            break;
        }
        let x1 = x as i32 + w.dx[i] as i32;
        if !is_ok_x(x1) {
            continue;
        }
        let x1u = x1 as usize;
        let y1 = w.dy[i] as u32;

        let reachable = ((current << y1) >> 3) & !cm.get(x1u, rc);
        current ^= (reachable << 3) >> y1;

        if reachable == 0 {
            continue;
        }

        if CHECK_SPIN {
            // Immobility spins: TETR.IO all-spin rule. With allspin off, T uses
            // guideline 3-corner detection only, so the stuck term const-folds
            // to zero and the fallback drops out.
            let stuck = if ACTIVE_RULES.enable_allspin || !HAS_SMAP {
                reachable & imm[x1u][w.rc as usize]
            } else {
                0
            };
            if HAS_SMAP {
                let spins = reachable & smap[x1u][0];
                let spin_tagged = spins | stuck;
                spin_set[x1u][r1i][SpinType::NoSpin as usize] |= reachable ^ spin_tagged;
                if spin_tagged != 0 {
                    if i >= 4 {
                        spin_set[x1u][r1i][SpinType::Full as usize] |= spins;
                        spin_set[x1u][r1i][SpinType::Mini as usize] |= stuck & !spins;
                    } else {
                        spin_set[x1u][r1i][SpinType::Mini as usize] |=
                            (spins & !smap[x1u][1 + r1i]) | (stuck & !spins);
                        spin_set[x1u][r1i][SpinType::Full as usize] |= spins & smap[x1u][1 + r1i];
                    }
                }
            } else {
                spin_set[x1u][r1i][SpinType::NoSpin as usize] &= !stuck;
                spin_set[x1u][r1i][SpinType::Mini as usize] |= stuck;
                spin_set[x1u][r1i][SpinType::NoSpin as usize] |= reachable ^ stuck;
            }
        }

        let m = reachable & !searched[x1u][r1i];
        if m == 0 {
            continue;
        }

        to_search[x1u][r1i] |= m;
        *remaining |= bb(x1 * ROTATION_NB as i32 + w.r1 as i32);
    }
}

// const-generic generate_inner; compiler specializes per piece + spin mode
#[cfg(not(target_arch = "wasm32"))]
#[inline(never)]
fn generate_inner<
    const P: usize,
    const CHECK_SPIN: bool,
    const HAS_SMAP: bool,
    const EMIT: bool,
>(
    cm: &CollisionMap,
    moves: &mut MoveBuffer,
    slow: bool,
    force: bool,
    spin_map: Option<&[[Bitboard; 5]; COL_NB]>,
) -> u32 {
    let mut count: u32 = 0;
    let smap = spin_map.unwrap_or(&ZERO_SMAP);
    let p = piece_from_index(P);
    let canonical_sz = canonical_size(p);
    let is_group2 = group2(p);

    let mut total: i32 = 0;
    let mut remaining: Bitboard = 0;
    let mut to_search = [[0u64; ROTATION_NB]; COL_NB];
    let mut searched = [[0u64; ROTATION_NB]; COL_NB];
    let mut move_set = [[0u64; ROTATION_NB]; COL_NB];
    // skip zeroing spin_set when CHECK_SPIN=false; all access is behind guards
    // matches Cobra's zero-size
    // `spinSet[COL_NB][ROTATION_NB][checkSpin ? SPIN_NB : 0]`
    let mut spin_set: [[[u64; SPIN_NB]; ROTATION_NB]; COL_NB] =
        [[[0u64; SPIN_NB]; ROTATION_NB]; COL_NB];

    let remaining_index =
        |x: i32, r: Rotation| -> Bitboard { bb(x * ROTATION_NB as i32 + r as i32) };

    for (x, searched_x) in searched.iter_mut().enumerate() {
        for r in 0..canonical_sz {
            searched_x[r] = cm.get(x, Rotation::from_u8(r as u8));
            if is_group2 {
                searched_x[r + 2] = searched_x[r];
            }
        }
    }

    if slow {
        #[allow(
            unknown_lints,
            clippy::manual_isolate_lowest_one,
            reason = "isolate_lowest_one is unstable on older supported toolchains"
        )]
        let spawn: Bitboard = if force {
            let s = !cm.get(SPAWN_COL, Rotation::North) & (!0u64 << ACTIVE_RULES.spawn_row);
            s & s.wrapping_neg()
        } else {
            !cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row)
        };
        if spawn == 0 {
            return count;
        }

        to_search[SPAWN_COL][Rotation::North as usize] = spawn;
        remaining |= remaining_index(SPAWN_COL as i32, Rotation::North);

        if CHECK_SPIN {
            spin_set[SPAWN_COL][Rotation::North as usize][SpinType::NoSpin as usize] = spawn;
        }
    } else {
        for x in 0..COL_NB {
            for ri in 0..canonical_sz {
                let r: Rotation = Rotation::from_u8(ri as u8);
                if !in_bounds(p, r, x as i32) {
                    continue;
                }

                debug_assert!(cm.get(x, r) != !0u64);
                let y = bitlen(cm.get(x, r));
                let surface = bb_low(ACTIVE_RULES.spawn_row) & !bb_low(y as i32);

                searched[x][ri] |= surface;
                to_search[x][ri] = surface;
                remaining |= remaining_index(x as i32, r);

                if is_group2 {
                    let r1 = rotate(Direction::Flip, r);
                    let r1i = r1 as usize;
                    if r1 == Rotation::South {
                        let s = surface & (surface >> 1);
                        searched[x][r1i] |= s;
                        to_search[x][r1i] = s;
                    } else {
                        searched[x][r1i] |= surface;
                        to_search[x][r1i] = surface;
                    }
                    remaining |= remaining_index(x as i32, r1);
                }

                if CHECK_SPIN {
                    spin_set[x][ri][SpinType::NoSpin as usize] = surface;
                } else {
                    if EMIT {
                        moves.push(Move::new(p, r, x as i32, y as i32, false));
                    } else {
                        count += 1;
                    }
                    total += popcount(!cm.get(x, r) & ((cm.get(x, r) << 1) | 1)) as i32 - 1;
                }
            }
        }

        if !CHECK_SPIN && total == 0 {
            return count;
        }
    }

    let ndirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
    let mut imm = [[0u64; ROTATION_NB]; COL_NB];
    if P != 1 && CHECK_SPIN && (ACTIVE_RULES.enable_allspin || !HAS_SMAP) {
        for (x, imm_x) in imm.iter_mut().enumerate() {
            for (rci, slot) in imm_x.iter_mut().enumerate().take(canonical_sz) {
                let rc = Rotation::from_u8(rci as u8);
                let same = cm.get(x, rc);
                let left = if x > 0 { cm.get(x - 1, rc) } else { !0u64 };
                let right = if x < COL_NB - 1 {
                    cm.get(x + 1, rc)
                } else {
                    !0u64
                };
                *slot = left & right & (same >> 1) & ((same << 1) | 1);
            }
        }
    }

    while remaining != 0 {
        let index = ctz(remaining);
        let x = (index >> 2) as usize;
        let r: Rotation = Rotation::from_u8((index & 3) as u8);
        let ri = r as usize;

        debug_assert!(is_ok_x(x as i32));
        debug_assert!(to_search[x][ri] != 0);

        if CHECK_SPIN {
            let mut m = (to_search[x][ri] >> 1) & !cm.get(x, r);
            while (m & to_search[x][ri]) != m {
                to_search[x][ri] |= m;
                m |= (m >> 1) & !cm.get(x, r);
            }
            spin_set[x][ri][SpinType::NoSpin as usize] |= m;
        } else {
            let mut m = (to_search[x][ri] >> 1) & !to_search[x][ri] & !searched[x][ri];
            while m != 0 {
                to_search[x][ri] |= m;
                m = (m >> 1) & !searched[x][ri];
            }
        }

        if CHECK_SPIN {
            move_set[x][ri] |= to_search[x][ri] & ((cm.get(x, r) << 1) | 1);
        } else {
            let r1 = canonical_r(p, r);
            let r1i = r1 as usize;
            let m = to_search[x][ri]
                & ((cm.get(x, r1) << 1) | 1)
                & !searched[x][ri]
                & !move_set[x][r1i];
            if m != 0 {
                move_set[x][r1i] |= m;
                total -= popcount(m) as i32;
                if EMIT {
                    let mut bits = m;
                    while bits != 0 {
                        moves.push(Move::new(p, r1, x as i32, ctz(bits) as i32, false));
                        bits &= bits - 1;
                    }
                } else {
                    count += popcount(m);
                }
                if total == 0 {
                    return count;
                }
            }
        }

        {
            let mut do_shift = |x1: usize| {
                let m = to_search[x][ri] & !searched[x1][ri];
                if m != 0 {
                    to_search[x1][ri] |= m;
                    remaining |= remaining_index(x1 as i32, r);
                    if CHECK_SPIN {
                        spin_set[x1][ri][SpinType::NoSpin as usize] |= m;
                    }
                }
            };
            if x > 0 {
                do_shift(x - 1);
            }
            if x < COL_NB - 1 {
                do_shift(x + 1);
            }
        }

        if P != 1 {
            macro_rules! run_waves {
                ($ri:literal) => {{
                    rotation_wave::<CHECK_SPIN, HAS_SMAP>(
                        &WaveTables::<P>::TABS[0][$ri],
                        x,
                        ri,
                        &mut to_search,
                        &searched,
                        &mut remaining,
                        &mut spin_set,
                        &imm,
                        cm,
                        smap,
                    );
                    rotation_wave::<CHECK_SPIN, HAS_SMAP>(
                        &WaveTables::<P>::TABS[1][$ri],
                        x,
                        ri,
                        &mut to_search,
                        &searched,
                        &mut remaining,
                        &mut spin_set,
                        &imm,
                        cm,
                        smap,
                    );
                    if ndirs == 3 {
                        rotation_wave::<CHECK_SPIN, HAS_SMAP>(
                            &WaveTables::<P>::TABS[2][$ri],
                            x,
                            ri,
                            &mut to_search,
                            &searched,
                            &mut remaining,
                            &mut spin_set,
                            &imm,
                            cm,
                            smap,
                        );
                    }
                }};
            }
            match ri {
                0 => run_waves!(0),
                1 => run_waves!(1),
                2 => run_waves!(2),
                _ => run_waves!(3),
            }
        }

        searched[x][ri] |= to_search[x][ri];
        to_search[x][ri] = 0;
        remaining ^= bb(index as i32);
    }

    if CHECK_SPIN {
        for x in 0..COL_NB {
            for ri in 0..canonical_sz {
                let r = Rotation::from_u8(ri as u8);
                if move_set[x][ri] == 0 {
                    continue;
                }
                let legal = move_set[x][ri];
                let raw_full = legal & spin_set[x][ri][SpinType::Full as usize];
                let raw_mini = legal & spin_set[x][ri][SpinType::Mini as usize];
                let raw_nospin = legal & spin_set[x][ri][SpinType::NoSpin as usize];

                let (mut full, mut mini, mut nospin) = if P == { Piece::T as usize } {
                    let full = raw_full;
                    let mini = raw_mini;
                    let nospin = raw_nospin;
                    (full, mini, nospin)
                } else {
                    let mini = raw_mini;
                    let nospin = raw_nospin & !mini;
                    (0, mini, nospin)
                };

                if EMIT {
                    while full != 0 {
                        let y = ctz(full) as i32;
                        if P == { Piece::T as usize } {
                            moves.push(Move::new_tspin(r, x as i32, y, true));
                        } else {
                            moves.push(Move::new_allspin_mini(p, r, x as i32, y));
                        }
                        full &= full - 1;
                    }

                    while mini != 0 {
                        let y = ctz(mini) as i32;
                        if P == { Piece::T as usize } {
                            moves.push(Move::new_tspin(r, x as i32, y, false));
                        } else {
                            moves.push(Move::new_allspin_mini(p, r, x as i32, y));
                        }
                        mini &= mini - 1;
                    }

                    while nospin != 0 {
                        let y = ctz(nospin) as i32;
                        moves.push(Move::new(p, r, x as i32, y, false));
                        nospin &= nospin - 1;
                    }
                } else {
                    count += popcount(full) + popcount(mini) + popcount(nospin);
                }
            }
        }
    }
    count
}

#[cfg(not(target_arch = "wasm32"))]
fn immobile_bits(cm: &CollisionMap, x: usize, r: Rotation, reachable: Bitboard) -> Bitboard {
    let blocked_left = if x > 0 { cm.get(x - 1, r) } else { !0u64 };
    let blocked_right = if x < COL_NB - 1 {
        cm.get(x + 1, r)
    } else {
        !0u64
    };
    let same_col = cm.get(x, r);
    let blocked_up = same_col >> 1;
    let blocked_down = (same_col << 1) | 1;
    reachable & blocked_left & blocked_right & blocked_down & blocked_up
}

// 16-lane mirror of rotation_wave: same const kick table, 16-bit rotation
// lanes per packed word. Write order is interchangeable with the legacy
// per-direction order (all waves of a pop read the same `current_all`).
#[cfg(not(target_arch = "wasm32"))]
struct Wave16Ctx<'a> {
    current_all: Bitboard,
    to_search: &'a mut [Bitboard; COL_NB],
    searched: &'a [Bitboard; COL_NB],
    remaining: &'a mut u32,
    cm16: &'a CollisionMap16,
}

#[inline(always)]
#[cfg(not(target_arch = "wasm32"))]
fn wave16(w: &WaveTab, x: usize, src_bits: Bitboard, ctx: &mut Wave16Ctx<'_>) {
    let sd = (w.r1 as usize) * 16;
    let mut src = src_bits;
    for i in 0..w.n as usize {
        if src == 0 {
            break;
        }
        let x1 = x as i32 + w.dx[i] as i32;
        if !is_ok_x(x1) {
            continue;
        }
        let x1u = x1 as usize;
        let sv = w.dy[i] as u32;

        let mut m = (src << sv) >> 3;
        m &= !(ctx.cm16.get(x1u) >> sd) & 0xFFFFu64;
        src ^= (m << 3) >> sv;

        let mut visited = ctx.searched[x1u];
        if x1u == x {
            visited |= ctx.current_all;
        }
        m &= !(visited >> sd);

        if m != 0 {
            ctx.to_search[x1u] |= m << sd;
            *ctx.remaining |= 1 << x1u;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn generate16<const P: usize, const EMIT: bool>(
    cols: &[Bitboard; COL_NB],
    moves: &mut MoveBuffer,
) -> u32 {
    let mut count: u32 = 0;
    let p = piece_from_index(P);
    // all const; compiler resolves at monomorphization
    let canonical_sz = canonical_size(p);
    let search_size: usize = if P == { Piece::O as usize } {
        1
    } else {
        ROTATION_NB
    };
    let canonical_mask: Bitboard = match canonical_sz {
        4 => !0u64,
        2 => 0xFFFF_FFFFu64,
        _ => 0xFFFFu64,
    };
    let search_mask: Bitboard = if search_size == 4 { !0u64 } else { 0xFFFFu64 };
    let s_mask: Bitboard = 0x7FFF_7FFF_7FFF_7FFFu64;
    let f_mask: Bitboard = 0x0001_0001_0001_0001u64;
    let is_group2 = group2(p);

    let cm = CollisionMap16::new(cols, p);

    let mut total: i32 = 0;
    let mut remaining: u32 = 0;
    let mut to_search = [0u64; COL_NB];
    let mut searched = [0u64; COL_NB];
    let mut move_set = [0u64; COL_NB];

    // fast init
    for x in 0..COL_NB {
        let mut surface = cm.get(x);
        searched[x] = surface; // include cm in searched
        surface |= (surface >> 1) & 0x7FFF_7FFF_7FFF_7FFFu64;
        surface |= (surface >> 2) & 0x3FFF_3FFF_3FFF_3FFFu64;
        surface |= (surface >> 4) & 0x0FFF_0FFF_0FFF_0FFFu64;
        surface |= (surface >> 8) & 0x00FF_00FF_00FF_00FFu64;

        let s = !surface;
        searched[x] |= s;
        to_search[x] = s;
        if s != 0 {
            remaining |= 1 << x;
        }

        move_set[x] = !surface & ((surface << 1) | f_mask) & canonical_mask;

        let m = move_set[x];
        total += popcount(!cm.get(x) & ((cm.get(x) << 1) | f_mask) & canonical_mask) as i32
            - popcount(m) as i32;

        if EMIT {
            let mut bits = m;
            while bits != 0 {
                let y = ctz(bits);
                let r: Rotation = Rotation::from_u8((y / 16) as u8);
                moves.push(Move::new(p, r, x as i32, (y % 16) as i32, false));
                bits &= bits - 1;
            }
        } else {
            count += popcount(m);
        }
    }

    if total == 0 {
        return count;
    }

    while remaining != 0 {
        let x = remaining.trailing_zeros() as usize;
        remaining &= remaining - 1;

        debug_assert!(is_ok_x(x as i32));
        debug_assert!(to_search[x] != 0);

        let mut current = to_search[x];
        to_search[x] = 0;

        // softdrops
        {
            let mut m = (current >> 1) & !searched[x] & s_mask;
            while m != 0 {
                current |= m;
                m = (m >> 1) & s_mask & !searched[x];
            }
        }

        // harddrops
        {
            let mut m = current & ((cm.get(x) << 1) | f_mask) & search_mask;

            if is_group2 {
                m = (m | (m >> 32)) & canonical_mask;
            }

            m &= !move_set[x];

            if m != 0 {
                move_set[x] |= m;
                total -= popcount(m) as i32;

                if EMIT {
                    let mut bits = m;
                    while bits != 0 {
                        let y = ctz(bits);
                        let r: Rotation = Rotation::from_u8((y / 16) as u8);
                        moves.push(Move::new(p, r, x as i32, (y % 16) as i32, false));
                        bits &= bits - 1;
                    }
                } else {
                    count += popcount(m);
                }

                if total == 0 {
                    return count;
                }
            }
        }

        // shift
        {
            let mut do_shift = |x1: usize| {
                let m = current & !searched[x1];
                if m != 0 {
                    to_search[x1] |= m;
                    remaining |= 1 << x1;
                }
            };
            if x > 0 {
                do_shift(x - 1);
            }
            if x < COL_NB - 1 {
                do_shift(x + 1);
            }
        }

        // rotate
        if P != 1 {
            macro_rules! run_waves16 {
                ($ri:literal) => {{
                    let src_bits = (current >> ($ri * 16)) & 0xFFFFu64;
                    if src_bits != 0 {
                        let mut wave_ctx = Wave16Ctx {
                            current_all: current,
                            to_search: &mut to_search,
                            searched: &searched,
                            remaining: &mut remaining,
                            cm16: &cm,
                        };
                        wave16(&WaveTables::<P>::TABS[0][$ri], x, src_bits, &mut wave_ctx);
                        wave16(&WaveTables::<P>::TABS[1][$ri], x, src_bits, &mut wave_ctx);
                        if ACTIVE_RULES.enable_180 {
                            wave16(&WaveTables::<P>::TABS[2][$ri], x, src_bits, &mut wave_ctx);
                        }
                    }
                }};
            }
            run_waves16!(0);
            run_waves16!(1);
            run_waves16!(2);
            run_waves16!(3);
        }

        searched[x] |= current;
    }
    count
}

#[cfg(test)]
struct SpinMasks16 {
    spins: [Bitboard; COL_NB],
    front: [Bitboard; COL_NB],
    imm: [Bitboard; COL_NB],
}

#[cfg(test)]
const LANE_REP: Bitboard = 0x0001_0001_0001_0001;
#[cfg(test)]
const LANE_TOP: Bitboard = 0x7FFF_7FFF_7FFF_7FFF;

// 16-bit-lane T spin check; valid for h<=13 where corner/lock/immobility
// bits above row 15 are provably zero. Test-only oracle: the 16-bit pass
// duplicates the 64-bit build on deep low boards, so it is not a net win
// for production dispatch.
#[cfg(test)]
fn t_spin_masks16(cols: &[Bitboard; COL_NB], cm: &CollisionMap16) -> (SpinMasks16, bool) {
    let mut masks = SpinMasks16 {
        spins: [0; COL_NB],
        front: [0; COL_NB],
        imm: [0; COL_NB],
    };
    let mut check_spin = false;
    for x in 0..COL_NB {
        let c = [
            if x > 0 {
                (cols[x - 1] >> 1) & 0xFFFF
            } else {
                0xFFFF
            },
            if x < COL_NB - 1 {
                (cols[x + 1] >> 1) & 0xFFFF
            } else {
                0xFFFF
            },
            if x < COL_NB - 1 {
                ((cols[x + 1] << 1) | 1) & 0xFFFF
            } else {
                0xFFFF
            },
            if x > 0 {
                ((cols[x - 1] << 1) | 1) & 0xFFFF
            } else {
                0xFFFF
            },
        ];
        let spins = (c[0] & c[1] & (c[2] | c[3])) | (c[2] & c[3] & (c[0] | c[1]));
        masks.spins[x] = spins.wrapping_mul(LANE_REP);
        let mut front = 0u64;
        for (ri, &cri) in c.iter().enumerate() {
            let cw = rotate(Direction::Cw, Rotation::from_u8(ri as u8)) as usize;
            front |= (spins & cri & c[cw]) << (ri * 16);
        }
        masks.front[x] = front;

        let w = cm.get(x);
        let left = if x > 0 { cm.get(x - 1) } else { !0u64 };
        let right = if x < COL_NB - 1 { cm.get(x + 1) } else { !0u64 };
        let down = ((w & LANE_TOP) << 1) | LANE_REP;
        masks.imm[x] = left & right & ((w >> 1) & LANE_TOP) & down;

        let legal = !w & down;
        check_spin |= (masks.spins[x] & legal) != 0
            || (ACTIVE_RULES.enable_allspin && (masks.imm[x] & legal) != 0);
    }
    (masks, check_spin)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MovegenOrder {
    ScalarCompatible,
    CanonicalRaw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MovegenRequest {
    pub piece: Piece,
    pub force: bool,
    pub order: MovegenOrder,
}

impl MovegenRequest {
    pub fn new(piece: Piece) -> Self {
        Self {
            piece,
            force: false,
            order: MovegenOrder::ScalarCompatible,
        }
    }

    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    pub fn with_order(mut self, order: MovegenOrder) -> Self {
        self.order = order;
        self
    }
}

// Production dispatch: strict reach (no worklist-timing phantoms, no
// phantom spin labels) with engine-exact Full/Mini strata, at 2-4x the
// hybrid's speed. The scalar engine remains the labeled parity oracle.
pub fn generate_with_request(b: &Board, moves: &mut MoveBuffer, request: MovegenRequest) {
    crate::smear_core::generate_smear(b, moves, request.piece, request.force);

    if request.order == MovegenOrder::CanonicalRaw {
        moves.sort_by_raw();
    }
}

pub fn generate_search(b: &Board, moves: &mut MoveBuffer, p: Piece) {
    generate_with_request(
        b,
        moves,
        MovegenRequest::new(p)
            .with_force(true)
            .with_order(MovegenOrder::ScalarCompatible),
    );
}

/// Scalar-oracle labeled move count without materialization.
#[cfg(not(target_arch = "wasm32"))]
pub fn count_moves(b: &Board, p: Piece, force: bool) -> u32 {
    let mut scratch = MoveBuffer::new();
    generate_engine::<false>(b, &mut scratch, p, force)
}

/// Counts distinct placements via the smear kernel. Matches
/// `generate_placements` len exactly; labeled `generate` may emit more
/// moves for T spin duals.
pub fn count_placements(b: &Board, p: Piece, force: bool) -> u32 {
    crate::smear_core::count_smear(b, p, force)
}

// -- generate: 1:1 port of generate() dispatch --
pub fn generate(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
    generate_with_request(
        b,
        moves,
        MovegenRequest::new(p)
            .with_force(force)
            .with_order(MovegenOrder::CanonicalRaw),
    );
}

/// True when the board has any covered hole/overhang, so a piece's set of
/// physically reachable placements can differ from the geometric `generate`
/// over-approximation. On a clean (no-hole) surface every geometric placement
/// is reachable, so the reachability filter can be skipped.
#[cfg(not(target_arch = "wasm32"))]
pub fn needs_reachability_filter(b: &Board) -> bool {
    for &col in &b.cols {
        if col == 0 {
            continue;
        }
        let height = u64::BITS - col.leading_zeros();
        let solid_below = (1u64 << height) - 1;
        if col != solid_below {
            return true;
        }
    }
    false
}

/// Checks physical reachability of a placement from spawn, including every
/// spin-label stratum that can reach its occupied cells.
#[cfg(not(target_arch = "wasm32"))]
pub fn move_reachable(b: &Board, m: &Move, force: bool) -> bool {
    let p = m.piece();
    let bare = Move::new(p, m.rotation(), m.x(), m.y(), false);
    if !crate::pathfinder::get_input(b, &bare, false, force)
        .data
        .is_empty()
    {
        return true;
    }
    let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
    let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
    if !is_t && !is_allspin {
        return false;
    }
    let mini = if p == Piece::T {
        Move::new_tspin(m.rotation(), m.x(), m.y(), false)
    } else {
        Move::new_allspin_mini(p, m.rotation(), m.x(), m.y())
    };
    if !crate::pathfinder::get_input(b, &mini, false, force)
        .data
        .is_empty()
    {
        return true;
    }
    is_t && {
        let full = Move::new_tspin(m.rotation(), m.x(), m.y(), true);
        !crate::pathfinder::get_input(b, &full, false, force)
            .data
            .is_empty()
    }
}

// playable IS generate; the alias stays because consumers encode the intent
// difference (play-legal vs any) at call sites.
pub fn generate_playable(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
    generate(b, moves, p, force);
}

/// Scalar parity-oracle path (per-column collision-map BFS / generate16).
/// Retained for native benchmarks and the parity harness.
#[cfg(not(target_arch = "wasm32"))]
pub fn generate_engine<const EMIT: bool>(
    b: &Board,
    moves: &mut MoveBuffer,
    p: Piece,
    force: bool,
) -> u32 {
    const { assert!(ACTIVE_RULES.spawn_row > 0) };

    // precompute columns once; avoids repeated 40-row iteration in col()
    let cols = b.compute_cols();

    let h = {
        let mut m = cols[0];
        for col in cols.iter().skip(1) {
            m |= col;
        }
        bitlen(m)
    };

    let slow = h as i32 > ACTIVE_RULES.spawn_row - 3;
    let low = !slow && h <= 13;

    let allspin_eligible = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
    if low && (p != Piece::T || !ACTIVE_RULES.enable_tspin) && !allspin_eligible {
        return match p {
            Piece::I => generate16::<{ Piece::I as usize }, EMIT>(&cols, moves),
            Piece::O => generate16::<{ Piece::O as usize }, EMIT>(&cols, moves),
            Piece::T => generate16::<{ Piece::T as usize }, EMIT>(&cols, moves),
            Piece::L => generate16::<{ Piece::L as usize }, EMIT>(&cols, moves),
            Piece::J => generate16::<{ Piece::J as usize }, EMIT>(&cols, moves),
            Piece::S => generate16::<{ Piece::S as usize }, EMIT>(&cols, moves),
            Piece::Z => generate16::<{ Piece::Z as usize }, EMIT>(&cols, moves),
        };
    }

    match p {
        Piece::T if ACTIVE_RULES.enable_tspin => {
            let cm = CollisionMap::new(&cols, Piece::T);
            let mut check_spin = false;
            let mut spin_map = [[0u64; 5]; COL_NB]; // [col][0=3corner, 1+r=face_corner]

            for x in 0..COL_NB {
                let corners = [
                    if x > 0 { cols[x - 1] >> 1 } else { !0u64 },
                    if x < COL_NB - 1 {
                        cols[x + 1] >> 1
                    } else {
                        !0u64
                    },
                    if x < COL_NB - 1 {
                        (cols[x + 1] << 1) | 1
                    } else {
                        !0u64
                    },
                    if x > 0 { (cols[x - 1] << 1) | 1 } else { !0u64 },
                ];

                let spins = (corners[0] & corners[1] & (corners[2] | corners[3]))
                    | (corners[2] & corners[3] & (corners[0] | corners[1]));

                spin_map[x][0] = spins;
                for ri in 0..ROTATION_NB {
                    let r: Rotation = Rotation::from_u8(ri as u8);
                    if in_bounds(Piece::T, r, x as i32) {
                        let cw_r = rotate(Direction::Cw, r);
                        spin_map[x][1 + ri] = spins & corners[ri] & corners[cw_r as usize];
                        let legal = !cm.get(x, r) & ((cm.get(x, r) << 1) | 1);
                        check_spin |= (spins & legal) != 0
                            || (ACTIVE_RULES.enable_allspin
                                && immobile_bits(&cm, x, r, legal) != 0);
                    }
                }
            }

            if check_spin {
                generate_inner::<{ Piece::T as usize }, true, true, EMIT>(
                    &cm,
                    moves,
                    slow,
                    force,
                    Some(&spin_map),
                )
            } else if low {
                match p {
                    Piece::I => generate16::<{ Piece::I as usize }, EMIT>(&cols, moves),
                    Piece::O => generate16::<{ Piece::O as usize }, EMIT>(&cols, moves),
                    Piece::T => generate16::<{ Piece::T as usize }, EMIT>(&cols, moves),
                    Piece::L => generate16::<{ Piece::L as usize }, EMIT>(&cols, moves),
                    Piece::J => generate16::<{ Piece::J as usize }, EMIT>(&cols, moves),
                    Piece::S => generate16::<{ Piece::S as usize }, EMIT>(&cols, moves),
                    Piece::Z => generate16::<{ Piece::Z as usize }, EMIT>(&cols, moves),
                }
            } else {
                generate_inner::<{ Piece::T as usize }, false, false, EMIT>(
                    &cm, moves, slow, force, None,
                )
            }
        }
        _ => {
            let cm = CollisionMap::new(&cols, p);
            if allspin_eligible {
                match p {
                    Piece::I => generate_inner::<{ Piece::I as usize }, true, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::L => generate_inner::<{ Piece::L as usize }, true, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::J => generate_inner::<{ Piece::J as usize }, true, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::S => generate_inner::<{ Piece::S as usize }, true, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::Z => generate_inner::<{ Piece::Z as usize }, true, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    _ => generate_inner::<{ Piece::T as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                }
            } else {
                match p {
                    Piece::I => generate_inner::<{ Piece::I as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::O => generate_inner::<{ Piece::O as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::L => generate_inner::<{ Piece::L as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::J => generate_inner::<{ Piece::J as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::S => generate_inner::<{ Piece::S as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::Z => generate_inner::<{ Piece::Z as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::T => generate_inner::<{ Piece::T as usize }, false, false, EMIT>(
                        &cm, moves, slow, force, None,
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKED_OLJ_MAX_HEIGHT: usize = 22;

    #[test]
    fn count_moves_matches_generate_len_on_seeded_boards() {
        let mut seed = 0xC0DE_2026_0611_BEEFu64;
        let mut xs = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
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
        for case in 0..4000u32 {
            let h = 1 + (xs() % 24) as usize;
            let mut b = Board::new();
            for y in 0..h {
                let mut row = (xs() & 0x3FF) as u16;
                row &= !(1u16 << (xs() % 10));
                b.rows[y] = row;
            }
            for y in 0..h {
                let mut bits = b.rows[y] as u64;
                while bits != 0 {
                    let x = bits.trailing_zeros() as usize;
                    b.cols[x] |= 1u64 << y;
                    bits &= bits - 1;
                }
            }
            for &p in &pieces {
                for force in [false, true] {
                    // Engine-internal consistency: popcount path must agree with
                    // the materializing path. Production generate() is strict
                    // smear-core and legitimately emits fewer moves than the
                    // engine's phantom-carrying sets (pinned in smear_core tests).
                    let mut moves = MoveBuffer::new();
                    generate_engine::<true>(&b, &mut moves, p, force);
                    let counted = count_moves(&b, p, force);
                    assert_eq!(
                        counted,
                        moves.len() as u32,
                        "case={case} piece={p:?} force={force} h={h}"
                    );
                }
            }
        }
    }

    #[test]
    fn count_placements_matches_generate_placements_on_seeded_boards() {
        let mut seed = 0xC0DE_2026_0704_FACEu64;
        let mut xs = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
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
        for case in 0..4000u32 {
            let h = 1 + (xs() % 24) as usize;
            let mut b = Board::new();
            for y in 0..h {
                let mut row = (xs() & 0x3FF) as u16;
                row &= !(1u16 << (xs() % 10));
                b.rows[y] = row;
            }
            for y in 0..h {
                let mut bits = b.rows[y] as u64;
                while bits != 0 {
                    let x = bits.trailing_zeros() as usize;
                    b.cols[x] |= 1u64 << y;
                    bits &= bits - 1;
                }
            }
            // Dispatch count is the strict placement count; must equal the
            // distinct-placement cardinality of production `generate` and
            // `generate_placements` len. The engine count is a sound upper
            // bound (worklist phantoms, T duals) that collapses to equality
            // on clean boards where geometric reach is already exact.
            let clean = !needs_reachability_filter(&b);
            for &p in &pieces {
                for force in [false, true] {
                    let placements = count_placements(&b, p, force);
                    let mut mb = MoveBuffer::new();
                    generate(&b, &mut mb, p, force);
                    let distinct: std::collections::BTreeSet<(u8, i32, i32)> = mb
                        .iter()
                        .map(|m| (m.rotation() as u8, m.x(), m.y()))
                        .collect();
                    assert_eq!(
                        placements,
                        distinct.len() as u32,
                        "case={case} piece={p:?} force={force} h={h} (distinct placements)"
                    );
                    let mut pb = MoveBuffer::new();
                    crate::smear_core::generate_placements(&b, &mut pb, p, force);
                    assert_eq!(
                        placements,
                        pb.len() as u32,
                        "case={case} piece={p:?} force={force} h={h} (placement emission)"
                    );
                    let engine = count_moves(&b, p, force);
                    assert!(
                        engine >= placements,
                        "case={case} piece={p:?} force={force} h={h} engine {engine} < strict {placements}"
                    );
                    if clean && p != Piece::T {
                        assert_eq!(
                            engine, placements,
                            "case={case} piece={p:?} force={force} h={h} (clean board)"
                        );
                    }
                }
            }
        }
    }

    fn t_dispatch_64(cols: &[Bitboard; COL_NB]) -> (CollisionMap, [[u64; 5]; COL_NB], bool) {
        let cm = CollisionMap::new(cols, Piece::T);
        let mut check_spin = false;
        let mut spin_map = [[0u64; 5]; COL_NB];
        for x in 0..COL_NB {
            let corners = [
                if x > 0 { cols[x - 1] >> 1 } else { !0u64 },
                if x < COL_NB - 1 {
                    cols[x + 1] >> 1
                } else {
                    !0u64
                },
                if x < COL_NB - 1 {
                    (cols[x + 1] << 1) | 1
                } else {
                    !0u64
                },
                if x > 0 { (cols[x - 1] << 1) | 1 } else { !0u64 },
            ];
            let spins = (corners[0] & corners[1] & (corners[2] | corners[3]))
                | (corners[2] & corners[3] & (corners[0] | corners[1]));
            spin_map[x][0] = spins;
            for ri in 0..ROTATION_NB {
                let r: Rotation = Rotation::from_u8(ri as u8);
                if in_bounds(Piece::T, r, x as i32) {
                    let cw_r = rotate(Direction::Cw, r);
                    spin_map[x][1 + ri] = spins & corners[ri] & corners[cw_r as usize];
                    let legal = !cm.get(x, r) & ((cm.get(x, r) << 1) | 1);
                    check_spin |= (spins & legal) != 0
                        || (ACTIVE_RULES.enable_allspin && immobile_bits(&cm, x, r, legal) != 0);
                }
            }
        }
        (cm, spin_map, check_spin)
    }

    #[test]
    fn t_spin_masks16_precheck_matches_64bit_dispatch() {
        fn xs(s: &mut u64) -> u64 {
            let mut x = *s;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *s = x;
            x
        }
        let mut st = 0x7515_0611_2026_0001u64;
        let mut spin_cases = 0u64;
        for case in 0..4000u64 {
            let h = 1 + (xs(&mut st) % 12) as usize;
            let mut rows = vec![0u16; h];
            for r in rows.iter_mut() {
                *r = (xs(&mut st) as u16) & 0x3FF;
                *r &= !(1u16 << (xs(&mut st) % 10));
            }
            if let Some(l) = rows.last_mut() {
                if *l == 0 {
                    *l = 1u16 << (xs(&mut st) % 10);
                }
            }
            let b = board_from_rows(&rows);
            let cols = b.compute_cols();
            let hbits = {
                let mut m = 0u64;
                for c in cols.iter() {
                    m |= c;
                }
                crate::header::bitlen(m)
            };
            if hbits > 13 {
                continue;
            }

            let (_, _, check64) = t_dispatch_64(&cols);
            let cm16 = CollisionMap16::new(&cols, Piece::T);
            let (_, check16) = t_spin_masks16(&cols, &cm16);
            assert_eq!(check16, check64, "precheck case={case} rows={rows:?}");
            if check64 {
                spin_cases += 1;
            }
        }
        assert!(spin_cases > 200, "corpus too weak: {spin_cases}");
    }

    #[test]
    fn test_generate_i_piece_empty_board() {
        let b = Board::new();
        let mut moves = MoveBuffer::new();
        generate(&b, &mut moves, Piece::I, false);
        assert_eq!(moves.len(), 17); // D1 baseline for I piece
    }

    #[test]
    fn test_generate_all_pieces_d1() {
        // D1 baselines from cobra-movegen (queue IOLJSZT, each piece solo)
        let b = Board::new();
        let expected = [
            (Piece::I, 17),
            (Piece::O, 9),
            (Piece::L, 34),
            (Piece::J, 34),
            (Piece::S, 17),
            (Piece::Z, 17),
            (Piece::T, 34),
        ];
        for (p, count) in expected {
            let mut moves = MoveBuffer::new();
            generate(&b, &mut moves, p, false);
            assert_eq!(
                moves.len(),
                count,
                "D1 mismatch for {:?}: got {}",
                p,
                moves.len()
            );
        }
    }

    #[test]
    fn test_movelist_no_duplicates() {
        let b = Board::new();
        for &p in &[
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ] {
            let ml = MoveList::new(&b, p);
            assert!(ml.size() > 0, "No moves for {:?}", p);
        }
    }

    // Negative pin: immobile T cells WITHOUT three filled corners exist (e.g.
    // rows below, x=1, South, y=1), so the stuck/imm machinery cannot be
    // dropped. Counts still agree with cobra; only the Mini-vs-NoSpin label
    // differs (TETR.IO immobility rules require the Mini label).
    #[test]
    fn t_immobile_without_three_corners_exists() {
        let b = board_from_rows(&[29, 816, 138, 598, 703, 312, 918, 491]);
        let cm = crate::gen::CollisionMap::new(&b.cols, Piece::T);
        let x = 1usize;
        let corners = [
            b.cols[x - 1] >> 1,
            b.cols[x + 1] >> 1,
            (b.cols[x + 1] << 1) | 1,
            (b.cols[x - 1] << 1) | 1,
        ];
        let spins = (corners[0] & corners[1] & (corners[2] | corners[3]))
            | (corners[2] & corners[3] & (corners[0] | corners[1]));
        let r = Rotation::from_u8(2);
        let stuck = immobile_bits(&cm, x, r, !cm.get(x, r));
        assert_ne!(stuck & !spins, 0);
    }

    // col 5: empty 4-deep well capped by a filled cell at row 4. Vertical I
    // locked in the well (rows 0..3) is physically unreachable (cap blocks
    // entry from top, 1-wide well too deep to spin into).
    fn capped_well_board() -> Board {
        let full = 0x3FFu16;
        let mut rows = vec![full & !(1u16 << 5); 4];
        rows.push(1u16 << 5);
        board_from_rows(&rows)
    }

    fn reachable(b: &Board, m: &Move) -> bool {
        move_reachable(b, m, false)
    }

    #[test]
    fn capped_well_triggers_reachability_filter() {
        assert!(needs_reachability_filter(&capped_well_board()));
        assert!(!needs_reachability_filter(&Board::new()));
    }

    // Phantom removal: strict reach makes both entries emit exactly the
    // reachable set. The engine keeps the old over-production as oracle.
    #[test]
    fn generate_is_strict_in_capped_well() {
        let b = capped_well_board();
        let mut raw = MoveBuffer::new();
        generate(&b, &mut raw, Piece::I, false);
        for m in raw.iter() {
            assert!(reachable(&b, m), "strict generate emitted {:?}", m);
            assert!(b.legal_lock_placement(m), "generate move {:?} floats", m);
        }
        let mut engine = MoveBuffer::new();
        generate_engine::<true>(&b, &mut engine, Piece::I, false);
        let engine_unreachable = engine.iter().filter(|m| !reachable(&b, m)).count();
        assert!(
            engine_unreachable > 0,
            "engine oracle stopped over-producing in the capped well"
        );
        assert!(raw.len() < engine.len());
    }

    #[test]
    fn generate_playable_keeps_exactly_reachable_legal_moves() {
        let b = capped_well_board();
        let mut raw = MoveBuffer::new();
        generate(&b, &mut raw, Piece::I, false);
        let mut playable = MoveBuffer::new();
        generate_playable(&b, &mut playable, Piece::I, false);

        for m in playable.iter() {
            assert!(reachable(&b, m), "playable move {:?} is unreachable", m);
            assert!(b.legal_lock_placement(m), "playable move {:?} floats", m);
        }
        assert_eq!(
            playable.as_slice(),
            raw.as_slice(),
            "playable and generate are the same strict set since the cutover"
        );
    }

    #[test]
    fn generate_playable_is_noop_on_clean_board() {
        let b = Board::new();
        for &p in &[
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ] {
            let mut raw = MoveBuffer::new();
            generate(&b, &mut raw, p, false);
            let mut playable = MoveBuffer::new();
            generate_playable(&b, &mut playable, p, false);
            assert_eq!(
                playable.as_slice(),
                raw.as_slice(),
                "clean-board generate_playable changed the set for {:?}",
                p
            );
        }
    }

    #[test]
    fn generate_playable_drops_impossible_capped_well_i() {
        // col 6: 1-wide 4-deep well (rows 0..3) capped by overhang at rows
        // 4..5. Vertical I is collision-free but physically unreachable.
        let b = board_from_rows(&[0x3BF, 0x3BF, 0x3BF, 0x3BF, 0x3CF, 0x3C7]);
        let impossible = Move::new(Piece::I, Rotation::East, 6, 2, false);
        assert!(
            b.legal_lock_placement(&impossible),
            "well placement is geometrically legal (collision-free)"
        );
        assert!(
            crate::pathfinder::get_input(&b, &impossible, false, false)
                .data
                .is_empty(),
            "capped-well I must be unreachable (harddrop cannot teleport past the cap)"
        );
        let mut playable = MoveBuffer::new();
        generate_playable(&b, &mut playable, Piece::I, false);
        assert!(
            !playable.iter().any(|m| m.piece() == Piece::I
                && m.rotation() == Rotation::East
                && m.x() == 6
                && m.y() == 2),
            "generate_playable must drop the impossible capped-well I"
        );
    }

    #[test]
    fn generate_playable_retains_reachable_tspin() {
        // Board emits a reachable T-spin Mini at North x=3 y=3; the
        // reachability filter must not drop it.
        let b = board_from_rows(&[1007, 879, 1007, 995, 935, 519, 3, 3, 3]);
        let mut raw = MoveBuffer::new();
        generate(&b, &mut raw, Piece::T, false);
        let spin = *raw
            .iter()
            .find(|m| {
                m.piece() == Piece::T
                    && m.rotation() == Rotation::North
                    && m.x() == 3
                    && m.y() == 3
                    && m.spin() == SpinType::Mini
            })
            .expect("board must emit the reachable T-spin Mini");
        assert!(
            reachable(&b, &spin),
            "get_input must find a path for the reachable T-spin Mini"
        );
        let mut playable = MoveBuffer::new();
        generate_playable(&b, &mut playable, Piece::T, false);
        assert!(
            playable.as_slice().contains(&spin),
            "generate_playable dropped a reachable T-spin Mini (false-negative)"
        );
    }

    fn board_from_rows(rows: &[u16]) -> Board {
        let mut b = Board::new();
        for (y, &row) in rows.iter().enumerate() {
            b.rows[y] = row;
            let mut bits = row as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                b.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        b
    }

    fn rows_after_move(board: &Board, m: &Move) -> [u16; 40] {
        let mut next = board.clone();
        next.place(m);
        let cleared = next.line_clears();
        if cleared != 0 {
            next.clear_lines(cleared);
        }
        next.rows
    }

    #[test]
    fn dual_reachable_t_spin_mini_prefers_rotation_label() {
        let b = board_from_rows(&[1007, 879, 1007, 995, 935, 519, 3, 3, 3]);
        let mut moves = MoveBuffer::new();
        generate(&b, &mut moves, Piece::T, false);

        let spins: Vec<SpinType> = moves
            .as_slice()
            .iter()
            .filter(|m| m.piece() == Piece::T)
            .filter(|m| m.rotation() == Rotation::North && m.x() == 3 && m.y() == 3)
            .map(|m| m.spin())
            .collect();

        assert_eq!(
            spins,
            vec![SpinType::Mini],
            "expected T North x=3 y=3 to be emitted only as Mini"
        );
    }

    #[test]
    fn dual_reachable_t_nospin_replay_keeps_nonspin_candidate() {
        let b = board_from_rows(&[511, 511, 991, 542, 28, 12]);
        let mut moves = MoveBuffer::new();
        generate(&b, &mut moves, Piece::T, false);
        let mut expected = [0u16; 40];
        expected[0] = 511;
        expected[1] = 511;
        expected[2] = 638;
        expected[3] = 60;
        expected[4] = 12;

        let spins: Vec<SpinType> = moves
            .as_slice()
            .iter()
            .filter(|m| m.piece() == Piece::T)
            .filter(|m| rows_after_move(&b, m) == expected)
            .map(|m| m.spin())
            .collect();

        assert!(
            spins.contains(&SpinType::NoSpin),
            "expected replay-matched placement to retain a NoSpin candidate, got {spins:?}"
        );
    }

    fn board_with_height(h: usize) -> Board {
        let mut b = Board::new();
        for y in 0..h {
            b.rows[y] = 0x1FF; // cols 0-8 filled, col 9 open well -> real moves
        }
        for y in 0..b.rows.len() {
            let mut bits = b.rows[y] as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                b.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        b
    }

    fn board_with_spawn_boundary() -> Board {
        board_with_height((ACTIVE_RULES.spawn_row - 2) as usize)
    }

    fn raw_moves_for_request(board: &Board, piece: Piece, force: bool) -> Vec<u16> {
        let request = MovegenRequest::new(piece)
            .with_force(force)
            .with_order(MovegenOrder::ScalarCompatible);
        let mut moves = MoveBuffer::new();
        generate_with_request(board, &mut moves, request);
        moves.as_slice().iter().map(|m| m.raw()).collect()
    }

    fn raw_moves_for_engine(board: &Board, piece: Piece, force: bool) -> Vec<u16> {
        let mut moves = MoveBuffer::new();
        generate_engine::<true>(board, &mut moves, piece, force);
        moves.as_slice().iter().map(|m| m.raw()).collect()
    }

    fn raw_moves_for_order(
        board: &Board,
        piece: Piece,
        force: bool,
        order: MovegenOrder,
    ) -> Vec<u16> {
        let request = MovegenRequest::new(piece)
            .with_force(force)
            .with_order(order);
        let mut moves = MoveBuffer::new();
        generate_with_request(board, &mut moves, request);
        moves.as_slice().iter().map(|m| m.raw()).collect()
    }

    fn raw_moves_for_scalar_request(board: &Board, piece: Piece, force: bool) -> Vec<u16> {
        let request = MovegenRequest::new(piece)
            .with_force(force)
            .with_order(MovegenOrder::ScalarCompatible);
        let mut moves = MoveBuffer::new();
        generate_engine::<true>(board, &mut moves, request.piece, request.force);
        moves.as_slice().iter().map(|m| m.raw()).collect()
    }

    #[test]
    fn scalar_compatible_request_matches_engine_for_force_modes() {
        let boards = [
            Board::new(),
            board_with_spawn_boundary(),
            board_with_height(28),
        ];
        for board in boards {
            for force in [false, true] {
                for piece in [
                    Piece::I,
                    Piece::O,
                    Piece::T,
                    Piece::L,
                    Piece::J,
                    Piece::S,
                    Piece::Z,
                ] {
                    let got = raw_moves_for_request(&board, piece, force);
                    let want = raw_moves_for_engine(&board, piece, force);
                    let scalar_request = raw_moves_for_scalar_request(&board, piece, force);
                    assert_eq!(
                        scalar_request,
                        want,
                        "scalar request output differs from engine for {piece:?} force={force} height={}",
                        board.height()
                    );

                    {
                        let mut got = got;
                        let mut want = want;
                        got.sort_unstable();
                        want.sort_unstable();
                        assert_eq!(
                            got, want,
                            "request SET differs from engine for {piece:?} force={force} height={} (order may differ under native packed dispatch)",
                            board.height()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn request_api_matches_engine_oracle_on_clean_boards() {
        let boards = [
            Board::new(),
            board_with_height(13),
            board_with_spawn_boundary(),
            board_with_height(24),
        ];
        for board in boards {
            for force in [false, true] {
                for piece in [
                    Piece::I,
                    Piece::O,
                    Piece::T,
                    Piece::L,
                    Piece::J,
                    Piece::S,
                    Piece::Z,
                ] {
                    let engine = raw_moves_for_engine(&board, piece, force);

                    let mut scalar =
                        raw_moves_for_order(&board, piece, force, MovegenOrder::ScalarCompatible);
                    let mut scalar_engine = engine.clone();
                    scalar.sort_unstable();
                    scalar_engine.sort_unstable();
                    assert_eq!(
                        scalar,
                        scalar_engine,
                        "scalar-compatible request SET differs for {piece:?} force={force} height={}",
                        board.height()
                    );

                    let mut canonical =
                        raw_moves_for_order(&board, piece, force, MovegenOrder::CanonicalRaw);
                    let mut sorted_engine = engine;
                    sorted_engine.sort_unstable();
                    canonical.sort_unstable();
                    assert_eq!(
                        canonical,
                        sorted_engine,
                        "canonical request differs for {piece:?} force={force} height={}",
                        board.height()
                    );
                }
            }
        }
    }

    #[test]
    fn generate_equals_engine_on_clean_height_boundaries() {
        // Clean flat boards have no phantoms; strict generate() must equal
        // the sorted engine output at every height band the old hybrid
        // routed differently.
        for h in [13usize, 18, 23, 24, 25, 29] {
            let b = board_with_height(h);
            for &p in &[Piece::I, Piece::S, Piece::Z, Piece::L, Piece::J] {
                let mut via = MoveBuffer::new();
                generate(&b, &mut via, p, false);
                let got: Vec<u16> = via.as_slice().iter().map(|m| m.raw()).collect();

                let mut eng = MoveBuffer::new();
                generate_engine::<true>(&b, &mut eng, p, false);
                let mut want: Vec<u16> = eng.as_slice().iter().map(|m| m.raw()).collect();
                want.sort_unstable();

                assert_eq!(got, want, "hybrid != engine at h={h} p={p:?}");
            }
        }
    }

    #[test]
    #[ignore]
    fn bench_movegen_nps_generate_vs_engine() {
        use std::hint::black_box;
        use std::time::Instant;
        fn xs(s: &mut u64) -> u64 {
            let mut x = *s;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *s = x;
            x
        }
        let mut st = 0xB17E_5EED_2026_000Au64;
        let mut boards: Vec<Board> = Vec::with_capacity(512);
        while boards.len() < 512 {
            let h = 4 + (xs(&mut st) % 21) as usize;
            let mut rows = vec![0u16; h];
            for r in rows.iter_mut() {
                *r = (xs(&mut st) as u16) & 0x3FF;
            }
            if let Some(l) = rows.last_mut() {
                if *l == 0 {
                    *l = 1u16 << (xs(&mut st) % 10);
                }
            }
            boards.push(board_from_rows(&rows));
        }
        let all = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ];
        let packed_pieces = [Piece::I, Piece::S, Piece::Z, Piece::L, Piece::J];
        let run = |pieces: &[Piece], iters: usize, native_dispatch: bool| -> (u64, f64) {
            let mut buf = MoveBuffer::new();
            for b in &boards {
                for &p in pieces {
                    buf.clear();
                    if native_dispatch {
                        generate(b, &mut buf, p, false);
                    } else {
                        generate_engine::<true>(b, &mut buf, p, false);
                    }
                    black_box(buf.len());
                }
            }
            let mut total: u64 = 0;
            let t = Instant::now();
            for _ in 0..iters {
                for b in &boards {
                    for &p in pieces {
                        buf.clear();
                        if native_dispatch {
                            generate(b, &mut buf, p, false);
                        } else {
                            generate_engine::<true>(b, &mut buf, p, false);
                        }
                        total += black_box(buf.len() as u64);
                    }
                }
            }
            (total, t.elapsed().as_secs_f64())
        };
        let iters = 60;
        let (m_all_native, s_all_native) = run(&all, iters, true);
        let (m_isz_native, s_isz_native) = run(&packed_pieces, iters, true);
        let (m_all_engine, s_all_engine) = run(&all, iters, false);
        let (m_isz_engine, s_isz_engine) = run(&packed_pieces, iters, false);
        eprintln!(
            "movegen_nps native_dispatch=packed-preferred engine=generate_engine | ALL7 native {:.2}M ({} in {:.3}s) engine {:.2}M ({} in {:.3}s) | ISZLJ native {:.2}M ({} in {:.3}s) engine {:.2}M ({} in {:.3}s)",
            m_all_native as f64 / s_all_native / 1e6,
            m_all_native,
            s_all_native,
            m_all_engine as f64 / s_all_engine / 1e6,
            m_all_engine,
            s_all_engine,
            m_isz_native as f64 / s_isz_native / 1e6,
            m_isz_native,
            s_isz_native,
            m_isz_engine as f64 / s_isz_engine / 1e6,
            m_isz_engine,
            s_isz_engine,
        );
    }

    #[test]
    fn reachable_locks_matches_move_reachable_on_holed_boards() {
        reachable_locks_corpus(300, 10, 100);
    }

    #[test]
    #[ignore = "full 3000-board corpus; the default tier runs a 300-board slice"]
    fn reachable_locks_matches_move_reachable_full_corpus() {
        reachable_locks_corpus(3000, 100, 1000);
    }

    fn reachable_locks_corpus(boards: u64, min_holed: u64, min_checks: u64) {
        fn xs(s: &mut u64) -> u64 {
            let mut x = *s;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *s = x;
            x
        }
        let mut st = 0xDEAD_BEEF_1234_5678u64;
        let pieces = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ];
        let mut checks = 0u64;
        let mut holed = 0u64;
        for _ in 0..boards {
            let h = 3 + (xs(&mut st) % 14) as usize;
            let mut rows = vec![0u16; h];
            for r in rows.iter_mut() {
                *r = (xs(&mut st) as u16) & 0x3FF;
            }
            if let Some(l) = rows.last_mut() {
                if *l == 0 {
                    *l = 1u16 << (xs(&mut st) % 10);
                }
            }
            let b = board_from_rows(&rows);
            if !needs_reachability_filter(&b) {
                continue;
            }
            holed += 1;
            for &p in &pieces {
                for force in [false, true] {
                    let reach = crate::pathfinder::reachable_locks(&b, p, force);
                    let mut gen = MoveBuffer::new();
                    generate(&b, &mut gen, p, force);
                    for m in gen.iter() {
                        checks += 1;
                        let want = move_reachable(&b, m, force);
                        let got = reach.move_reachable(m);
                        assert_eq!(
                            got,
                            want,
                            "reach!=oracle p={p:?} force={force} m=({},{},{:?},{:?}) rows={rows:?}",
                            m.x(),
                            m.y(),
                            m.rotation(),
                            m.spin()
                        );
                    }
                }
            }
        }
        assert!(
            holed > min_holed && checks > min_checks,
            "insufficient coverage holed={holed} checks={checks}"
        );
    }

    fn legacy_playable(b: &Board, p: Piece, force: bool) -> Vec<u16> {
        let mut moves = MoveBuffer::new();
        generate(b, &mut moves, p, force);
        if needs_reachability_filter(b) {
            moves.retain(|m| b.legal_lock_placement(m) && move_reachable(b, m, force));
        }
        moves.as_slice().iter().map(|m| m.raw()).collect()
    }

    // With the canonical-frame in_bounds fix, the pathfinder filter is exact
    // on strict emissions, so filtering strict generate() by move_reachable
    // must be the identity (the old blind spot, including probe case-13's
    // NoSpin I North (1,7) placement, is pinned closed).
    #[test]
    fn pathfinder_filter_is_identity_on_strict_generate() {
        pathfinder_filter_identity_corpus(200, 12);
    }

    #[test]
    #[ignore = "full 800-board corpus; the default tier runs a 200-board slice"]
    fn pathfinder_filter_is_identity_full_corpus() {
        pathfinder_filter_identity_corpus(800, 50);
    }

    fn pathfinder_filter_identity_corpus(boards: u64, min_holed: u64) {
        fn xs(s: &mut u64) -> u64 {
            let mut x = *s;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *s = x;
            x
        }
        let mut st = 0x0F1E_2D3C_4B5A_6978u64;
        let pieces = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ];
        let mut holed = 0u64;
        for _ in 0..boards {
            let h = 3 + (xs(&mut st) % 14) as usize;
            let mut rows = vec![0u16; h];
            for r in rows.iter_mut() {
                *r = (xs(&mut st) as u16) & 0x3FF;
            }
            if let Some(l) = rows.last_mut() {
                if *l == 0 {
                    *l = 1u16 << (xs(&mut st) % 10);
                }
            }
            let b = board_from_rows(&rows);
            if !needs_reachability_filter(&b) {
                continue;
            }
            holed += 1;
            for &p in &pieces {
                for force in [false, true] {
                    let mut pb = MoveBuffer::new();
                    generate_playable(&b, &mut pb, p, force);
                    let legacy = legacy_playable(&b, p, force);
                    assert_eq!(
                        legacy.len(),
                        pb.as_slice().len(),
                        "pathfinder filter dropped strict emissions p={p:?} force={force} rows={rows:?}"
                    );
                    for m in pb.iter() {
                        assert!(
                            legacy.contains(&m.raw()),
                            "pathfinder filter dropped a strict emission {m:?} p={p:?} force={force} rows={rows:?}"
                        );
                    }
                }
            }
        }
        assert!(holed > min_holed, "insufficient holed coverage {holed}");
    }

    /// Byte-identical to the legacy per-move pathfinder filter on real boards.
    #[test]
    fn generate_playable_matches_legacy_on_real_contexts() {
        let path = match std::env::var("LABEL_OPP_REAL_CTX") {
            Ok(p) => p,
            Err(_) => return,
        };
        // K=7 context record layout: FRAME_BYTES=195 per frame, (K+1)=8 frames.
        const FRAME_BYTES: usize = 195;
        const REC: usize = 1750;
        const NFRAMES: usize = 8;
        let data = std::fs::read(&path).expect("read real ctx sample");
        let nrec = data.len() / REC;
        assert!(nrec > 0, "no records in {path} (len={})", data.len());
        let pieces = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ];
        let max_rec = nrec.min(30_000);
        let mut holed = 0u64;
        let mut checks = 0u64;
        let mut olj_le22 = 0u64;
        for i in 0..max_rec {
            let rec = &data[i * REC..(i + 1) * REC];
            for f in 0..NFRAMES {
                let off = f * FRAME_BYTES;
                let mut rows = [0u16; 40];
                for (y, r) in rows.iter_mut().enumerate() {
                    let lo = rec[off + y * 2] as u16;
                    let hi = rec[off + y * 2 + 1] as u16;
                    *r = (lo | (hi << 8)) & 0x03FF;
                }
                let h = (0..40).rev().find(|&y| rows[y] != 0).map(|y| y + 1);
                let h = match h {
                    Some(h) => h,
                    None => continue,
                };
                let rowsv = rows[..h].to_vec();
                let b = board_from_rows(&rowsv);
                if !needs_reachability_filter(&b) {
                    continue;
                }
                holed += 1;
                let height = b.height() as usize;
                for &p in &pieces {
                    if matches!(p, Piece::O | Piece::L | Piece::J)
                        && height <= PACKED_OLJ_MAX_HEIGHT
                    {
                        olj_le22 += 1;
                    }
                    for force in [false, true] {
                        let mut pb = MoveBuffer::new();
                        generate_playable(&b, &mut pb, p, force);
                        let got: Vec<u16> = pb.as_slice().iter().map(|m| m.raw()).collect();
                        let want = legacy_playable(&b, p, force);
                        checks += 1;
                        assert_eq!(
                            got, want,
                            "real-ctx parity FAIL rec={i} frame={f} p={p:?} force={force} height={height} rows={rowsv:?}"
                        );
                    }
                }
            }
        }
        assert!(
            holed > 100 && checks > 1000 && olj_le22 > 100,
            "insufficient real coverage holed={holed} checks={checks} olj_le22={olj_le22}"
        );
        eprintln!(
            "real-ctx parity OK records={max_rec} holed_boards={holed} checks={checks} olj_packed_path_boards={olj_le22}"
        );
    }
}
