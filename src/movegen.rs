// movegen.rs -- 1:1 port of movegen.hpp + movegen.cpp
// const generics mirror C++ template<Piece p1> specialization
use crate::board::Board;
use crate::default_ruleset::ACTIVE_RULES;
use crate::gen::{
    canonical_offset, canonical_r, canonical_size, group2, in_bounds, kick_180_index, kick_index,
    rotate, CollisionMap, CollisionMap16, Direction, KICKS, KICKS_180, SPAWN_COL,
};
use crate::header::*;

pub use crate::move_buffer::{MoveBuffer, MoveList};

// compile-time piece from const generic index — must match Piece enum discriminants
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

// const-generic generate_inner — compiler specializes per piece + spin mode
#[inline(never)]
fn generate_inner<const P: usize, const CHECK_SPIN: bool>(
    cm: &CollisionMap,
    moves: &mut MoveBuffer,
    slow: bool,
    force: bool,
    spin_map: Option<&[[Bitboard; 5]; COL_NB]>,
) {
    let p = piece_from_index(P);
    let canonical_sz = canonical_size(p);
    let is_group2 = group2(p);

    let mut total: i32 = 0;
    let mut remaining: Bitboard = 0;
    let mut to_search = [[0u64; ROTATION_NB]; COL_NB];
    let mut searched = [[0u64; ROTATION_NB]; COL_NB];
    let mut move_set = [[0u64; ROTATION_NB]; COL_NB];
    // skip zeroing spin_set when CHECK_SPIN=false — all access is behind `if CHECK_SPIN` guards
    // matches Cobra's zero-size `spinSet[COL_NB][ROTATION_NB][checkSpin ? SPIN_NB : 0]`
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
        let spawn: Bitboard = if force {
            let s = !cm.get(SPAWN_COL, Rotation::North) & (!0u64 << ACTIVE_RULES.spawn_row);
            s & s.wrapping_neg()
        } else {
            !cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row)
        };
        if spawn == 0 {
            return;
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
                    moves.push(Move::new(p, r, x as i32, y as i32, false));
                    total += popcount(!cm.get(x, r) & ((cm.get(x, r) << 1) | 1)) as i32 - 1;
                }
            }
        }

        if !CHECK_SPIN && total == 0 {
            return;
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
                let mut bits = m;
                while bits != 0 {
                    moves.push(Move::new(p, r1, x as i32, ctz(bits) as i32, false));
                    bits &= bits - 1;
                }
                if total == 0 {
                    return;
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
            // P != O
            let do_rotate =
                |kicks_rot: &[[Coordinates; 5]; ROTATION_NB],
                 d: Direction,
                 to_search: &mut [[Bitboard; ROTATION_NB]; COL_NB],
                 searched: &[[Bitboard; ROTATION_NB]; COL_NB],
                 remaining: &mut Bitboard,
                 spin_set: &mut [[[Bitboard; SPIN_NB]; ROTATION_NB]; COL_NB],
                 cm: &CollisionMap,
                 spin_map: Option<&[[Bitboard; 5]; COL_NB]>| {
                    let kicks = &kicks_rot[ri];
                    let r1 = rotate(d, r);
                    let rc = canonical_r(p, r1);
                    let off = canonical_offset(p, r) - canonical_offset(p, r1);
                    let n = if !ACTIVE_RULES.srs_plus && kicks.len() == 6 {
                        2
                    } else {
                        kicks.len()
                    };

                    let mut current = to_search[x][ri];

                    for (i, kick) in kicks.iter().enumerate().take(n) {
                        if current == 0 {
                            break;
                        }
                        let x1 = x as i32 + kick.x as i32 + off.x as i32;
                        if !is_ok_x(x1) {
                            continue;
                        }
                        let x1u = x1 as usize;

                        let threshold: i32 = 3;
                        let y1 = threshold + kick.y as i32 + off.y as i32;

                        let mut m = ((current << y1) >> threshold) & !cm.get(x1u, rc);
                        current ^= (m << threshold) >> y1;

                        m &= !searched[x1u][r1 as usize];
                        if m == 0 {
                            continue;
                        }

                        if CHECK_SPIN {
                            let r1i = r1 as usize;
                            if let Some(smap) = spin_map {
                                let spins = m & smap[x1u][0];
                                spin_set[x1u][r1i][SpinType::NoSpin as usize] &= !spins;
                                spin_set[x1u][r1i][SpinType::NoSpin as usize] |= m ^ spins;
                                if spins != 0 {
                                    if i >= 4 {
                                        spin_set[x1u][r1i][SpinType::Full as usize] |= spins;
                                    } else {
                                        spin_set[x1u][r1i][SpinType::Mini as usize] |=
                                            spins & !smap[x1u][1 + r1i];
                                        spin_set[x1u][r1i][SpinType::Full as usize] |=
                                            spins & smap[x1u][1 + r1i];
                                    }
                                }
                            } else {
                                let blocked_left = if x1u > 0 { cm.get(x1u - 1, rc) } else { !0u64 };
                                let blocked_right = if x1u < COL_NB - 1 { cm.get(x1u + 1, rc) } else { !0u64 };
                                let same_col = cm.get(x1u, rc);
                                let blocked_up = same_col >> 1;
                                let blocked_down = (same_col << 1) | 1;
                                let stuck = m & blocked_left & blocked_right & blocked_down & blocked_up;
                                spin_set[x1u][r1i][SpinType::NoSpin as usize] &= !stuck;
                                spin_set[x1u][r1i][SpinType::Mini as usize] |= stuck;
                                spin_set[x1u][r1i][SpinType::NoSpin as usize] |= m ^ stuck;
                            }
                        }

                        to_search[x1u][r1 as usize] |= m;
                        *remaining |= remaining_index(x1, r1);
                    }
                };

            let ki = kick_index(p, ACTIVE_RULES.srs_plus);
            do_rotate(
                &KICKS[ki][Direction::Cw as usize],
                Direction::Cw,
                &mut to_search,
                &searched,
                &mut remaining,
                &mut spin_set,
                cm,
                spin_map,
            );
            do_rotate(
                &KICKS[ki][Direction::Ccw as usize],
                Direction::Ccw,
                &mut to_search,
                &searched,
                &mut remaining,
                &mut spin_set,
                cm,
                spin_map,
            );

            if ACTIVE_RULES.enable_180 {
                let ki180 = kick_180_index(p);
                do_rotate_180::<P, CHECK_SPIN>(&mut RotateContext {
                    kicks_rot: &KICKS_180[ki180],
                    current_search: to_search[x][ri],
                    x,
                    r,
                    to_search: &mut to_search,
                    searched: &searched,
                    remaining: &mut remaining,
                    spin_set: &mut spin_set,
                    cm,
                    spin_map,
                });
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
                    let mini = raw_mini & !full;
                    let nospin = raw_nospin & !mini & !full;
                    (full, mini, nospin)
                } else {
                    let mini = raw_mini;
                    let nospin = raw_nospin & !mini;
                    (0, mini, nospin)
                };

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
            }
        }
    }
}

struct RotateContext<'a> {
    kicks_rot: &'a [[Coordinates; 6]; ROTATION_NB],
    current_search: Bitboard,
    x: usize,
    r: Rotation,
    to_search: &'a mut [[Bitboard; ROTATION_NB]; COL_NB],
    searched: &'a [[Bitboard; ROTATION_NB]; COL_NB],
    remaining: &'a mut Bitboard,
    spin_set: &'a mut [[[Bitboard; SPIN_NB]; ROTATION_NB]; COL_NB],
    cm: &'a CollisionMap,
    spin_map: Option<&'a [[Bitboard; 5]; COL_NB]>,
}

fn do_rotate_180<const P: usize, const CHECK_SPIN: bool>(ctx: &mut RotateContext<'_>) {
    let p = piece_from_index(P);
    let ri = ctx.r as usize;
    let r1 = rotate(Direction::Flip, ctx.r);
    let rc = canonical_r(p, r1);
    let off = canonical_offset(p, ctx.r) - canonical_offset(p, r1);
    let kicks = &ctx.kicks_rot[ri];
    let n = if !ACTIVE_RULES.srs_plus && kicks.len() == 6 {
        2
    } else {
        kicks.len()
    };

    let remaining_index =
        |x: i32, r: Rotation| -> Bitboard { bb(x * ROTATION_NB as i32 + r as i32) };

    let mut current = ctx.current_search;

    for (i, kick) in kicks.iter().enumerate().take(n) {
        if current == 0 {
            break;
        }
        let x1 = ctx.x as i32 + kick.x as i32 + off.x as i32;
        if !is_ok_x(x1) {
            continue;
        }
        let x1u = x1 as usize;

        let threshold: i32 = 3;
        let y1 = threshold + kick.y as i32 + off.y as i32;

        let mut m = ((current << y1) >> threshold) & !ctx.cm.get(x1u, rc);
        current ^= (m << threshold) >> y1;

        m &= !ctx.searched[x1u][r1 as usize];
        if m == 0 {
            continue;
        }

        if CHECK_SPIN {
            let r1i = r1 as usize;
            if let Some(smap) = ctx.spin_map {
                let spins = m & smap[x1u][0];
                ctx.spin_set[x1u][r1i][SpinType::NoSpin as usize] &= !spins;
                ctx.spin_set[x1u][r1i][SpinType::NoSpin as usize] |= m ^ spins;
                if spins != 0 {
                    if i >= 4 {
                        ctx.spin_set[x1u][r1i][SpinType::Full as usize] |= spins;
                    } else {
                        ctx.spin_set[x1u][r1i][SpinType::Mini as usize] |=
                            spins & !smap[x1u][1 + r1i];
                        ctx.spin_set[x1u][r1i][SpinType::Full as usize] |=
                            spins & smap[x1u][1 + r1i];
                    }
                }
            } else {
                let blocked_left = if x1u > 0 { ctx.cm.get(x1u - 1, rc) } else { !0u64 };
                let blocked_right = if x1u < COL_NB - 1 { ctx.cm.get(x1u + 1, rc) } else { !0u64 };
                let same_col = ctx.cm.get(x1u, rc);
                let blocked_up = same_col >> 1;
                let blocked_down = (same_col << 1) | 1;
                let stuck = m & blocked_left & blocked_right & blocked_down & blocked_up;
                ctx.spin_set[x1u][r1i][SpinType::NoSpin as usize] &= !stuck;
                ctx.spin_set[x1u][r1i][SpinType::Mini as usize] |= stuck;
                ctx.spin_set[x1u][r1i][SpinType::NoSpin as usize] |= m ^ stuck;
            }
        }

        ctx.to_search[x1u][r1 as usize] |= m;
        *ctx.remaining |= remaining_index(x1, r1);
    }
}

fn generate16<const P: usize>(cols: &[Bitboard; COL_NB], moves: &mut MoveBuffer) {
    let p = piece_from_index(P);
    // all const — compiler resolves at monomorphization
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

        let mut m = move_set[x];
        total += popcount(!cm.get(x) & ((cm.get(x) << 1) | f_mask) & canonical_mask) as i32
            - popcount(m) as i32;

        while m != 0 {
            let y = ctz(m);
            let r: Rotation = Rotation::from_u8((y / 16) as u8);
            moves.push(Move::new(p, r, x as i32, (y % 16) as i32, false));
            m &= m - 1;
        }
    }

    if total == 0 {
        return;
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

                let mut bits = m;
                while bits != 0 {
                    let y = ctz(bits);
                    let r: Rotation = Rotation::from_u8((y / 16) as u8);
                    moves.push(Move::new(p, r, x as i32, (y % 16) as i32, false));
                    bits &= bits - 1;
                }

                if total == 0 {
                    return;
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
        if p != Piece::O {
            let do_process = |kicks_rot: &[[Coordinates; 5]; ROTATION_NB],
                              d: Direction,
                              current: &mut Bitboard,
                              to_search: &mut [Bitboard; COL_NB],
                              searched: &[Bitboard; COL_NB],
                              remaining: &mut u32,
                              cm16: &CollisionMap16,
                              x: usize| {
                for (ri, kicks) in kicks_rot.iter().enumerate() {
                    let r: Rotation = Rotation::from_u8(ri as u8);
                    let shift_src = ri * 16;
                    let src_bits = (*current >> shift_src) & 0xFFFFu64;
                    if src_bits == 0 {
                        continue;
                    }

                    let r1 = rotate(d, r);
                    let shift_dest = (r1 as usize) * 16;
                    let off = canonical_offset(p, r) - canonical_offset(p, r1);
                    let n = if !ACTIVE_RULES.srs_plus && kicks.len() == 6 {
                        2
                    } else {
                        kicks.len()
                    };

                    let mut src = src_bits;
                    for kick in kicks.iter().take(n) {
                        if src == 0 {
                            break;
                        }
                        let x1 = x as i32 + kick.x as i32 + off.x as i32;
                        if !is_ok_x(x1) {
                            continue;
                        }
                        let x1u = x1 as usize;

                        let threshold: i32 = 3;
                        let shift_val = threshold + kick.y as i32 + off.y as i32;

                        let mut m = (src << shift_val) >> threshold;
                        m &= !(cm16.get(x1u) >> shift_dest) & 0xFFFFu64;
                        src ^= (m << threshold) >> shift_val;

                        let mut visited = searched[x1u];
                        if x1u == x {
                            visited |= *current;
                        }
                        m &= !(visited >> shift_dest);

                        if m != 0 {
                            to_search[x1u] |= m << shift_dest;
                            *remaining |= 1 << x1u;
                        }
                    }
                }
            };

            let ki = kick_index(p, ACTIVE_RULES.srs_plus);
            do_process(
                &KICKS[ki][Direction::Cw as usize],
                Direction::Cw,
                &mut current,
                &mut to_search,
                &searched,
                &mut remaining,
                &cm,
                x,
            );
            do_process(
                &KICKS[ki][Direction::Ccw as usize],
                Direction::Ccw,
                &mut current,
                &mut to_search,
                &searched,
                &mut remaining,
                &cm,
                x,
            );

            if ACTIVE_RULES.enable_180 {
                let ki180 = kick_180_index(p);
                do_process_180::<P>(&mut ProcessContext {
                    kicks_rot: &KICKS_180[ki180],
                    d: Direction::Flip,
                    current: &mut current,
                    to_search: &mut to_search,
                    searched: &searched,
                    remaining: &mut remaining,
                    cm16: &cm,
                    x,
                });
            }
        }

        searched[x] |= current;
    }
}

struct ProcessContext<'a> {
    kicks_rot: &'a [[Coordinates; 6]; ROTATION_NB],
    d: Direction,
    current: &'a mut Bitboard,
    to_search: &'a mut [Bitboard; COL_NB],
    searched: &'a [Bitboard; COL_NB],
    remaining: &'a mut u32,
    cm16: &'a CollisionMap16,
    x: usize,
}

fn do_process_180<const P: usize>(ctx: &mut ProcessContext<'_>) {
    let p = piece_from_index(P);
    for (ri, kicks) in ctx.kicks_rot.iter().enumerate() {
        let r: Rotation = Rotation::from_u8(ri as u8);
        let shift_src = ri * 16;
        let src_bits = (*ctx.current >> shift_src) & 0xFFFFu64;
        if src_bits == 0 {
            continue;
        }

        let r1 = rotate(ctx.d, r);
        let shift_dest = (r1 as usize) * 16;
        let off = canonical_offset(p, r) - canonical_offset(p, r1);
        let n = if !ACTIVE_RULES.srs_plus && kicks.len() == 6 {
            2
        } else {
            kicks.len()
        };

        let mut src = src_bits;
        for kick in kicks.iter().take(n) {
            if src == 0 {
                break;
            }
            let x1 = ctx.x as i32 + kick.x as i32 + off.x as i32;
            if !is_ok_x(x1) {
                continue;
            }
            let x1u = x1 as usize;

            let threshold: i32 = 3;
            let shift_val = threshold + kick.y as i32 + off.y as i32;

            let mut m = (src << shift_val) >> threshold;
            m &= !(ctx.cm16.get(x1u) >> shift_dest) & 0xFFFFu64;
            src ^= (m << threshold) >> shift_val;

            let mut visited = ctx.searched[x1u];
            if x1u == ctx.x {
                visited |= *ctx.current;
            }
            m &= !(visited >> shift_dest);

            if m != 0 {
                ctx.to_search[x1u] |= m << shift_dest;
                *ctx.remaining |= 1 << x1u;
            }
        }
    }
}

// -- generate: 1:1 port of generate() dispatch --
pub fn generate(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
    debug_assert!(ACTIVE_RULES.spawn_row > 0);

    // precompute columns once — avoids repeated 40-row iteration in col()
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
        match p {
            Piece::I => generate16::<{ Piece::I as usize }>(&cols, moves),
            Piece::O => generate16::<{ Piece::O as usize }>(&cols, moves),
            Piece::T => generate16::<{ Piece::T as usize }>(&cols, moves),
            Piece::L => generate16::<{ Piece::L as usize }>(&cols, moves),
            Piece::J => generate16::<{ Piece::J as usize }>(&cols, moves),
            Piece::S => generate16::<{ Piece::S as usize }>(&cols, moves),
            Piece::Z => generate16::<{ Piece::Z as usize }>(&cols, moves),
        }
        return;
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
                if spins != 0 {
                    for ri in 0..ROTATION_NB {
                        let r: Rotation = Rotation::from_u8(ri as u8);
                        if in_bounds(Piece::T, r, x as i32) {
                            let cw_r = rotate(Direction::Cw, r);
                            spin_map[x][1 + ri] = spins & corners[ri] & corners[cw_r as usize];
                            check_spin |= (spins & !cm.get(x, r) & ((cm.get(x, r) << 1) | 1)) != 0;
                        }
                    }
                }
            }

            if check_spin {
                generate_inner::<{ Piece::T as usize }, true>(
                    &cm,
                    moves,
                    slow,
                    force,
                    Some(&spin_map),
                );
            } else if low {
                match p {
                    Piece::I => generate16::<{ Piece::I as usize }>(&cols, moves),
                    Piece::O => generate16::<{ Piece::O as usize }>(&cols, moves),
                    Piece::T => generate16::<{ Piece::T as usize }>(&cols, moves),
                    Piece::L => generate16::<{ Piece::L as usize }>(&cols, moves),
                    Piece::J => generate16::<{ Piece::J as usize }>(&cols, moves),
                    Piece::S => generate16::<{ Piece::S as usize }>(&cols, moves),
                    Piece::Z => generate16::<{ Piece::Z as usize }>(&cols, moves),
                }
            } else {
                generate_inner::<{ Piece::T as usize }, false>(&cm, moves, slow, force, None);
            }
        }
        _ => {
            let cm = CollisionMap::new(&cols, p);
            if allspin_eligible {
                match p {
                    Piece::I => {
                        generate_inner::<{ Piece::I as usize }, true>(&cm, moves, slow, force, None)
                    }
                    Piece::L => {
                        generate_inner::<{ Piece::L as usize }, true>(&cm, moves, slow, force, None)
                    }
                    Piece::J => {
                        generate_inner::<{ Piece::J as usize }, true>(&cm, moves, slow, force, None)
                    }
                    Piece::S => {
                        generate_inner::<{ Piece::S as usize }, true>(&cm, moves, slow, force, None)
                    }
                    Piece::Z => {
                        generate_inner::<{ Piece::Z as usize }, true>(&cm, moves, slow, force, None)
                    }
                    _ => {
                        generate_inner::<{ Piece::T as usize }, false>(&cm, moves, slow, force, None)
                    }
                }
            } else {
                match p {
                    Piece::I => {
                        generate_inner::<{ Piece::I as usize }, false>(&cm, moves, slow, force, None)
                    }
                    Piece::O => {
                        generate_inner::<{ Piece::O as usize }, false>(&cm, moves, slow, force, None)
                    }
                    Piece::L => {
                        generate_inner::<{ Piece::L as usize }, false>(&cm, moves, slow, force, None)
                    }
                    Piece::J => {
                        generate_inner::<{ Piece::J as usize }, false>(&cm, moves, slow, force, None)
                    }
                    Piece::S => {
                        generate_inner::<{ Piece::S as usize }, false>(&cm, moves, slow, force, None)
                    }
                    Piece::Z => {
                        generate_inner::<{ Piece::Z as usize }, false>(&cm, moves, slow, force, None)
                    }
                    Piece::T => {
                        generate_inner::<{ Piece::T as usize }, false>(&cm, moves, slow, force, None)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
