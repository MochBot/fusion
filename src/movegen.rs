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

                        let reachable = ((current << y1) >> threshold) & !cm.get(x1u, rc);
                        current ^= (reachable << threshold) >> y1;

                        if reachable == 0 {
                            continue;
                        }

                        if CHECK_SPIN {
                            let r1i = r1 as usize;
                            if let Some(smap) = spin_map {
                                let stuck = immobile_bits(cm, x1u, rc, reachable);
                                let spins = reachable & smap[x1u][0];
                                let spin_tagged = spins | stuck;
                                spin_set[x1u][r1i][SpinType::NoSpin as usize] |=
                                    reachable ^ spin_tagged;
                                if spin_tagged != 0 {
                                    if i >= 4 {
                                        spin_set[x1u][r1i][SpinType::Full as usize] |= spins;
                                        spin_set[x1u][r1i][SpinType::Mini as usize] |=
                                            stuck & !spins;
                                    } else {
                                        spin_set[x1u][r1i][SpinType::Mini as usize] |=
                                            (spins & !smap[x1u][1 + r1i]) | (stuck & !spins);
                                        spin_set[x1u][r1i][SpinType::Full as usize] |=
                                            spins & smap[x1u][1 + r1i];
                                    }
                                }
                            } else {
                                let stuck = immobile_bits(cm, x1u, rc, reachable);
                                spin_set[x1u][r1i][SpinType::NoSpin as usize] &= !stuck;
                                spin_set[x1u][r1i][SpinType::Mini as usize] |= stuck;
                                spin_set[x1u][r1i][SpinType::NoSpin as usize] |= reachable ^ stuck;
                            }
                        }

                        let m = reachable & !searched[x1u][r1 as usize];
                        if m == 0 {
                            continue;
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
                    let mini = raw_mini;
                    let nospin = raw_nospin;
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

        let reachable = ((current << y1) >> threshold) & !ctx.cm.get(x1u, rc);
        current ^= (reachable << threshold) >> y1;

        if reachable == 0 {
            continue;
        }

        if CHECK_SPIN {
            let r1i = r1 as usize;
            if let Some(smap) = ctx.spin_map {
                let stuck = immobile_bits(ctx.cm, x1u, rc, reachable);
                let spins = reachable & smap[x1u][0];
                let spin_tagged = spins | stuck;
                ctx.spin_set[x1u][r1i][SpinType::NoSpin as usize] |= reachable ^ spin_tagged;
                if spin_tagged != 0 {
                    if i >= 4 {
                        ctx.spin_set[x1u][r1i][SpinType::Full as usize] |= spins;
                        ctx.spin_set[x1u][r1i][SpinType::Mini as usize] |= stuck & !spins;
                    } else {
                        ctx.spin_set[x1u][r1i][SpinType::Mini as usize] |=
                            (spins & !smap[x1u][1 + r1i]) | (stuck & !spins);
                        ctx.spin_set[x1u][r1i][SpinType::Full as usize] |=
                            spins & smap[x1u][1 + r1i];
                    }
                }
            } else {
                let stuck = immobile_bits(ctx.cm, x1u, rc, reachable);
                ctx.spin_set[x1u][r1i][SpinType::NoSpin as usize] &= !stuck;
                ctx.spin_set[x1u][r1i][SpinType::Mini as usize] |= stuck;
                ctx.spin_set[x1u][r1i][SpinType::NoSpin as usize] |= reachable ^ stuck;
            }
        }

        let m = reachable & !ctx.searched[x1u][r1 as usize];
        if m == 0 {
            continue;
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

// Packed fast path is opt-in via the `packed_movegen` feature (off by default,
// absent from the wasm build) because its emission order degrades beam coaching.
// When the feature is on, FUSION_NO_PACKED still forces the pure engine path.
#[inline]
#[cfg(feature = "packed_movegen")]
fn packed_path_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUSION_NO_PACKED").is_none())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MovegenConsumer {
    General,
    Search,
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
    pub consumer: MovegenConsumer,
    pub order: MovegenOrder,
}

impl MovegenRequest {
    pub fn new(piece: Piece) -> Self {
        Self {
            piece,
            force: false,
            consumer: MovegenConsumer::General,
            order: MovegenOrder::ScalarCompatible,
        }
    }

    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    pub fn with_consumer(mut self, consumer: MovegenConsumer) -> Self {
        self.consumer = consumer;
        self
    }

    pub fn with_order(mut self, order: MovegenOrder) -> Self {
        self.order = order;
        self
    }
}

#[inline]
#[cfg(feature = "packed_movegen")]
fn board_height_rows(b: &Board) -> usize {
    for y in (0..b.rows.len()).rev() {
        if b.rows[y] != 0 {
            return y + 1;
        }
    }
    0
}

#[inline]
#[cfg(feature = "packed_movegen")]
fn scalar_uses_slow_seed(b: &Board) -> bool {
    let cols = b.compute_cols();
    let mut h_bits = cols[0];
    for col in cols.iter().skip(1) {
        h_bits |= col;
    }
    bitlen(h_bits) as i32 > ACTIVE_RULES.spawn_row - 3
}

#[inline]
#[cfg(feature = "packed_movegen")]
fn request_allows_packed(b: &Board, request: MovegenRequest) -> bool {
    if !packed_path_enabled() {
        return false;
    }
    if !matches!(
        request.piece,
        Piece::I | Piece::S | Piece::Z | Piece::L | Piece::J
    ) {
        return false;
    }
    if request.consumer == MovegenConsumer::Search
        || request.order == MovegenOrder::ScalarCompatible
    {
        return false;
    }
    if board_height_rows(b) > 24 {
        return false;
    }
    if request.force && scalar_uses_slow_seed(b) {
        return false;
    }
    true
}

pub fn generate_with_request(b: &Board, moves: &mut MoveBuffer, request: MovegenRequest) {
    #[cfg(not(feature = "packed_movegen"))]
    {
        generate_engine(b, moves, request.piece, request.force);
    }

    #[cfg(feature = "packed_movegen")]
    {
        let mut used_packed = false;
        if request_allows_packed(b, request) {
            let rows: &[u16; crate::reach_packed::PH] =
                b.rows[..crate::reach_packed::PH].try_into().unwrap();
            crate::reach_packed::generate_packed_with_force(
                rows,
                request.piece,
                request.force,
                moves,
            );
            used_packed = true;
        }

        if !used_packed {
            generate_engine(b, moves, request.piece, request.force);
        }

        if request.order == MovegenOrder::CanonicalRaw {
            moves.sort_by_raw();
        }
    }
}

pub fn generate_search(b: &Board, moves: &mut MoveBuffer, p: Piece) {
    generate_with_request(
        b,
        moves,
        MovegenRequest::new(p)
            .with_force(true)
            .with_consumer(MovegenConsumer::Search)
            .with_order(MovegenOrder::ScalarCompatible),
    );
}

// -- generate: 1:1 port of generate() dispatch --
pub fn generate(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
    generate_with_request(
        b,
        moves,
        MovegenRequest::new(p)
            .with_force(force)
            .with_consumer(MovegenConsumer::General)
            .with_order(MovegenOrder::CanonicalRaw),
    );
}

/// True when the board has any covered hole/overhang, so a piece's set of
/// physically reachable placements can differ from the geometric `generate`
/// over-approximation. On a clean (no-hole) surface every geometric placement
/// is reachable, so the reachability filter can be skipped.
pub fn needs_reachability_filter(b: &Board) -> bool {
    for col in b.compute_cols() {
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

/// `generate`, but restricted to placements that are actually reachable by a
/// legal move sequence from spawn (no piece teleported into a capped well).
/// Order-preserving so consumers that pair this with `expand_all_gm` stay
/// index-aligned. Skips the (expensive) per-move pathfinder check when the board
/// has no holes, where `generate` is already exact.
/// True when placement `m`'s final cells are physically reachable by some legal
/// input sequence. A spin-labelled move counts as reachable if EITHER the bare
/// position is reachable (movegen may over-label an openly-droppable placement
/// as a spin) OR the spin tuck itself is reachable. The spin label is attack
/// metadata; physical reachability is about the occupied cells.
pub fn move_reachable(b: &Board, m: &Move, force: bool) -> bool {
    let bare = Move::new(m.piece(), m.rotation(), m.x(), m.y(), false);
    if !crate::pathfinder::get_input(b, &bare, false, force).data.is_empty() {
        return true;
    }
    m.spin() != SpinType::NoSpin
        && !crate::pathfinder::get_input(b, m, false, force).data.is_empty()
}

pub(crate) const PACKED_OLJ_MAX_HEIGHT: usize = 22;

pub fn generate_playable(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
    generate(b, moves, p, force);
    if !needs_reachability_filter(b) {
        return;
    }
    // Packed whole-board reachability is byte-exact vs the scalar BFS for O/L/J up
    // to PACKED_OLJ_MAX_HEIGHT (proven in packed_oljfilter_matches_scalar_generate_playable);
    // it diverges above that and for I/T/S/Z, which keep the exact scalar BFS.
    if matches!(p, Piece::O | Piece::L | Piece::J)
        && (b.height() as usize) <= PACKED_OLJ_MAX_HEIGHT
    {
        let rows30: &[u16; crate::reach_packed::PH] =
            b.rows[..crate::reach_packed::PH].try_into().unwrap();
        let mut packed = MoveBuffer::new();
        crate::reach_packed::generate_packed_with_force(rows30, p, force, &mut packed);
        packed.sort_by_raw();
        let praws = packed.as_slice();
        moves.retain(|m| {
            b.legal_lock_placement(m)
                && praws.binary_search_by(|x| x.raw().cmp(&m.raw())).is_ok()
        });
        return;
    }
    let reach = crate::pathfinder::reachable_locks(b, p, force);
    moves.retain(|m| b.legal_lock_placement(m) && reach.move_reachable(m));
}

/// GPU-parity hook (non-production): internal collision map board[x][r] as y-bitsets.
fn cols_from_rows(rows: &[u16; crate::board::BOARD_HEIGHT]) -> [Bitboard; COL_NB] {
    let mut cols = [0u64; COL_NB];
    for (y, &row) in rows.iter().enumerate() {
        let mut bits = row as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }
    cols
}

pub fn debug_collision_map(rows: &[u16; crate::board::BOARD_HEIGHT], p: Piece) -> [[Bitboard; 4]; COL_NB] {
    crate::gen::CollisionMap::new(&cols_from_rows(rows), p).board
}

/// GPU-parity hook. Flat dims: KICKS[3][2][4][5][2], KICKS_180[2][4][6][2], canon_r[7][4], canon_off[7][4][2].
pub fn debug_movegen_tables() -> (Vec<i32>, Vec<i32>, Vec<u32>, Vec<i32>) {
    use crate::gen::{canonical_offset, canonical_r, KICKS, KICKS_180};
    let pieces = [Piece::I, Piece::O, Piece::T, Piece::L, Piece::J, Piece::S, Piece::Z];
    let mut kicks = Vec::new();
    for set in KICKS.iter() {
        for dir in set.iter() {
            for rot in dir.iter() {
                for c in rot.iter() {
                    kicks.push(c.x as i32);
                    kicks.push(c.y as i32);
                }
            }
        }
    }
    let mut kicks180 = Vec::new();
    for set in KICKS_180.iter() {
        for rot in set.iter() {
            for c in rot.iter() {
                kicks180.push(c.x as i32);
                kicks180.push(c.y as i32);
            }
        }
    }
    let mut canon_r = Vec::new();
    let mut canon_off = Vec::new();
    for &p in pieces.iter() {
        for r in 0..4u8 {
            let rr = Rotation::from_u8(r);
            canon_r.push(canonical_r(p, rr) as u32);
            let off = canonical_offset(p, rr);
            canon_off.push(off.x as i32);
            canon_off.push(off.y as i32);
        }
    }
    (kicks, kicks180, canon_r, canon_off)
}

/// GPU-parity hook (non-production): engine move set as (x, y, rotation_u8, spin_u8).
pub fn debug_move_set(rows: &[u16; crate::board::BOARD_HEIGHT], p: Piece) -> Vec<(i32, i32, u8, u8)> {
    let mut b = Board::new();
    b.rows = *rows;
    b.cols = cols_from_rows(rows);
    let mut moves = MoveBuffer::new();
    generate(&b, &mut moves, p, false);
    moves
        .as_slice()
        .iter()
        .map(|m| (m.x(), m.y(), m.rotation() as u8, m.spin() as u8))
        .collect()
}

/// GPU-parity hook (non-production): authoritative 5-ply beam `best` label via the
/// real engine (generate+place+attack, gm=0, mult=1.0), mirroring bench_beam::beam_opt2
/// (stable sort by acc desc, dedup by rows keep-first, top `bw`). Reference for GPU Gate 6.
pub fn debug_beam_best(rows0: &[u16; crate::board::BOARD_HEIGHT], pieces: &[u8], bw: usize) -> i64 {
    #[derive(Clone)]
    struct N {
        rows: [u16; 40],
        acc: i64,
        b2b: i32,
        combo: i32,
        pending: i32,
    }
    let mut cur: Vec<N> = vec![N { rows: *rows0, acc: 0, b2b: 0, combo: 0, pending: 0 }];
    for &pe in pieces {
        let p = piece_from_index(pe as usize);
        let mut nxt: Vec<N> = Vec::new();
        for node in &cur {
            let mut b = Board::new();
            b.rows = node.rows;
            b.cols = cols_from_rows(&node.rows);
            let mut moves = MoveBuffer::new();
            generate(&b, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut cr = node.rows;
                let x = m.x();
                let y = m.y();
                if (x as usize) < 10 && (y as usize) < 40 {
                    cr[y as usize] |= 1u16 << x;
                }
                let pc = m.cells();
                for i in 0..3 {
                    let cx = (pc[i].x as i32 + x) as usize;
                    let cy = (pc[i].y as i32 + y) as usize;
                    if cx < 10 && cy < 40 {
                        cr[cy] |= 1u16 << cx;
                    }
                }
                let mut cleared = 0u64;
                for (yy, &cell) in cr.iter().enumerate() {
                    if cell == 0x3FF {
                        cleared |= 1u64 << yy;
                    }
                }
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    let mut w = 0usize;
                    for read in 0..40 {
                        if cleared & (1u64 << read) == 0 {
                            cr[w] = cr[read];
                            w += 1;
                        }
                    }
                    for cell in cr.iter_mut().take(40).skip(w) {
                        *cell = 0;
                    }
                }
                let is_empty = cr.iter().all(|&r| r == 0);
                let at = crate::attack::calculate_attack_s2_tl_with_multiplier(
                    lines,
                    m.spin(),
                    node.b2b,
                    node.combo,
                    is_empty,
                    0,
                    1.0,
                );
                nxt.push(N {
                    rows: cr,
                    acc: node.acc + at.attack as i64,
                    b2b: at.b2b_after,
                    combo: at.combo_after,
                    pending: (node.pending - lines as i32).max(0),
                });
            }
        }
        if nxt.is_empty() {
            cur.clear();
            break;
        }
        let mut idx: Vec<usize> = (0..nxt.len()).collect();
        idx.sort_by(|&a, &b| nxt[b].acc.cmp(&nxt[a].acc));
        let mut seen: std::collections::HashSet<[u16; 40]> = std::collections::HashSet::new();
        let mut kept: Vec<N> = Vec::with_capacity(bw);
        for &ci in &idx {
            if !seen.insert(nxt[ci].rows) {
                continue;
            }
            kept.push(nxt[ci].clone());
            if kept.len() >= bw {
                break;
            }
        }
        cur = kept;
    }
    cur.iter().map(|n| n.acc).max().unwrap_or(0)
}

/// GPU-parity hook (non-production): the beam frontier boards after running `pieces`.
pub fn debug_beam_frontier(
    rows0: &[u16; crate::board::BOARD_HEIGHT],
    pieces: &[u8],
    bw: usize,
) -> Vec<[u16; 40]> {
    #[derive(Clone)]
    struct N {
        rows: [u16; 40],
        acc: i64,
        b2b: i32,
        combo: i32,
        pending: i32,
    }
    let mut cur: Vec<N> = vec![N { rows: *rows0, acc: 0, b2b: 0, combo: 0, pending: 0 }];
    for &pe in pieces {
        let p = piece_from_index(pe as usize);
        let mut nxt: Vec<N> = Vec::new();
        for node in &cur {
            let mut b = Board::new();
            b.rows = node.rows;
            b.cols = cols_from_rows(&node.rows);
            let mut moves = MoveBuffer::new();
            generate(&b, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut cr = node.rows;
                let x = m.x();
                let y = m.y();
                if (x as usize) < 10 && (y as usize) < 40 {
                    cr[y as usize] |= 1u16 << x;
                }
                let pc = m.cells();
                for i in 0..3 {
                    let cx = (pc[i].x as i32 + x) as usize;
                    let cy = (pc[i].y as i32 + y) as usize;
                    if cx < 10 && cy < 40 {
                        cr[cy] |= 1u16 << cx;
                    }
                }
                let mut cleared = 0u64;
                for (yy, &cell) in cr.iter().enumerate() {
                    if cell == 0x3FF {
                        cleared |= 1u64 << yy;
                    }
                }
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    let mut w = 0usize;
                    for read in 0..40 {
                        if cleared & (1u64 << read) == 0 {
                            cr[w] = cr[read];
                            w += 1;
                        }
                    }
                    for cell in cr.iter_mut().take(40).skip(w) {
                        *cell = 0;
                    }
                }
                let is_empty = cr.iter().all(|&r| r == 0);
                let at = crate::attack::calculate_attack_s2_tl_with_multiplier(
                    lines, m.spin(), node.b2b, node.combo, is_empty, 0, 1.0,
                );
                nxt.push(N {
                    rows: cr,
                    acc: node.acc + at.attack as i64,
                    b2b: at.b2b_after,
                    combo: at.combo_after,
                    pending: (node.pending - lines as i32).max(0),
                });
            }
        }
        if nxt.is_empty() {
            cur.clear();
            break;
        }
        let mut idx: Vec<usize> = (0..nxt.len()).collect();
        idx.sort_by(|&a, &b| nxt[b].acc.cmp(&nxt[a].acc));
        let mut seen: std::collections::HashSet<[u16; 40]> = std::collections::HashSet::new();
        let mut kept: Vec<N> = Vec::with_capacity(bw);
        for &ci in &idx {
            if !seen.insert(nxt[ci].rows) {
                continue;
            }
            kept.push(nxt[ci].clone());
            if kept.len() >= bw {
                break;
            }
        }
        cur = kept;
    }
    cur.iter().map(|n| n.rows).collect()
}

/// GPU-parity hook (non-production): per-ply (frontier_count, max_acc) trace of the
/// beam, to localize where a GPU beam divergence first occurs.
pub fn debug_beam_best_trace(
    rows0: &[u16; crate::board::BOARD_HEIGHT],
    pieces: &[u8],
    bw: usize,
) -> Vec<(usize, i64)> {
    #[derive(Clone)]
    struct N {
        rows: [u16; 40],
        acc: i64,
        b2b: i32,
        combo: i32,
        pending: i32,
    }
    let mut cur: Vec<N> = vec![N { rows: *rows0, acc: 0, b2b: 0, combo: 0, pending: 0 }];
    let mut trace = Vec::new();
    for &pe in pieces {
        let p = piece_from_index(pe as usize);
        let mut nxt: Vec<N> = Vec::new();
        for node in &cur {
            let mut b = Board::new();
            b.rows = node.rows;
            b.cols = cols_from_rows(&node.rows);
            let mut moves = MoveBuffer::new();
            generate(&b, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut cr = node.rows;
                let x = m.x();
                let y = m.y();
                if (x as usize) < 10 && (y as usize) < 40 {
                    cr[y as usize] |= 1u16 << x;
                }
                let pc = m.cells();
                for i in 0..3 {
                    let cx = (pc[i].x as i32 + x) as usize;
                    let cy = (pc[i].y as i32 + y) as usize;
                    if cx < 10 && cy < 40 {
                        cr[cy] |= 1u16 << cx;
                    }
                }
                let mut cleared = 0u64;
                for (yy, &cell) in cr.iter().enumerate() {
                    if cell == 0x3FF {
                        cleared |= 1u64 << yy;
                    }
                }
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    let mut w = 0usize;
                    for read in 0..40 {
                        if cleared & (1u64 << read) == 0 {
                            cr[w] = cr[read];
                            w += 1;
                        }
                    }
                    for cell in cr.iter_mut().take(40).skip(w) {
                        *cell = 0;
                    }
                }
                let is_empty = cr.iter().all(|&r| r == 0);
                let at = crate::attack::calculate_attack_s2_tl_with_multiplier(
                    lines, m.spin(), node.b2b, node.combo, is_empty, 0, 1.0,
                );
                nxt.push(N {
                    rows: cr,
                    acc: node.acc + at.attack as i64,
                    b2b: at.b2b_after,
                    combo: at.combo_after,
                    pending: (node.pending - lines as i32).max(0),
                });
            }
        }
        if nxt.is_empty() {
            trace.push((0, 0));
            cur.clear();
            break;
        }
        let mut idx: Vec<usize> = (0..nxt.len()).collect();
        idx.sort_by(|&a, &b| nxt[b].acc.cmp(&nxt[a].acc));
        let mut seen: std::collections::HashSet<[u16; 40]> = std::collections::HashSet::new();
        let mut kept: Vec<N> = Vec::with_capacity(bw);
        for &ci in &idx {
            if !seen.insert(nxt[ci].rows) {
                continue;
            }
            kept.push(nxt[ci].clone());
            if kept.len() >= bw {
                break;
            }
        }
        cur = kept;
        let mx = cur.iter().map(|n| n.acc).max().unwrap_or(0);
        trace.push((cur.len(), mx));
    }
    trace
}

/// Pure engine path (per-column collision-map BFS / generate16), bypassing the
/// packed hybrid. `generate` dispatches here for O/T/force/tall boards; also
/// exposed so benchmarks and the parity harness can compare engine vs packed.
pub fn generate_engine(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
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
                for ri in 0..ROTATION_NB {
                    let r: Rotation = Rotation::from_u8(ri as u8);
                    if in_bounds(Piece::T, r, x as i32) {
                        let cw_r = rotate(Direction::Cw, r);
                        spin_map[x][1 + ri] = spins & corners[ri] & corners[cw_r as usize];
                        let legal = !cm.get(x, r) & ((cm.get(x, r) << 1) | 1);
                        check_spin |= (spins & legal) != 0 || immobile_bits(&cm, x, r, legal) != 0;
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
                    _ => generate_inner::<{ Piece::T as usize }, false>(
                        &cm, moves, slow, force, None,
                    ),
                }
            } else {
                match p {
                    Piece::I => generate_inner::<{ Piece::I as usize }, false>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::O => generate_inner::<{ Piece::O as usize }, false>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::L => generate_inner::<{ Piece::L as usize }, false>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::J => generate_inner::<{ Piece::J as usize }, false>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::S => generate_inner::<{ Piece::S as usize }, false>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::Z => generate_inner::<{ Piece::Z as usize }, false>(
                        &cm, moves, slow, force, None,
                    ),
                    Piece::T => generate_inner::<{ Piece::T as usize }, false>(
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

    // col 5 is an empty 4-deep well capped by a single filled cell at row 4; a
    // vertical I locked in the well (rows 0..3) is physically unreachable (the cap
    // blocks entry from the top and the 1-wide well is too deep to spin into).
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

    #[test]
    fn raw_generate_overproduces_unreachable_in_capped_well() {
        let b = capped_well_board();
        let mut raw = MoveBuffer::new();
        generate(&b, &mut raw, Piece::I, false);
        let unreachable = raw.iter().filter(|m| !reachable(&b, m)).count();
        assert!(
            unreachable > 0,
            "expected raw generate to over-produce unreachable I placements, got {} moves all reachable",
            raw.len()
        );
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
        let expected: Vec<Move> = raw
            .iter()
            .copied()
            .filter(|m| b.legal_lock_placement(m) && reachable(&b, m))
            .collect();
        assert_eq!(
            playable.as_slice(),
            expected.as_slice(),
            "generate_playable must equal the reachable+legal subset of generate, in order"
        );
        assert!(
            playable.len() < raw.len(),
            "filter removed nothing on a capped-well board"
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
        // col 6 is a 1-wide 4-deep well (rows 0..3) capped by an overhang at
        // rows 4..5. A vertical I in the well is collision-free but physically
        // unreachable: no input sequence can pass the cap.
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
        // This board emits a reachable T-spin Mini at North x=3 y=3 (see
        // dual_reachable_t_spin_mini_prefers_rotation_label). The reachability
        // filter must NOT drop it.
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
            .with_consumer(MovegenConsumer::Search)
            .with_order(MovegenOrder::ScalarCompatible);
        let mut moves = MoveBuffer::new();
        generate_with_request(board, &mut moves, request);
        moves.as_slice().iter().map(|m| m.raw()).collect()
    }

    fn raw_moves_for_engine(board: &Board, piece: Piece, force: bool) -> Vec<u16> {
        let mut moves = MoveBuffer::new();
        generate_engine(board, &mut moves, piece, force);
        moves.as_slice().iter().map(|m| m.raw()).collect()
    }

    #[cfg(feature = "packed_movegen")]
    fn raw_moves_for_order(
        board: &Board,
        piece: Piece,
        force: bool,
        order: MovegenOrder,
    ) -> Vec<u16> {
        let request = MovegenRequest::new(piece)
            .with_force(force)
            .with_consumer(MovegenConsumer::General)
            .with_order(order);
        let mut moves = MoveBuffer::new();
        generate_with_request(board, &mut moves, request);
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
                    assert_eq!(
                        raw_moves_for_request(&board, piece, force),
                        raw_moves_for_engine(&board, piece, force),
                        "request output differs from engine for {piece:?} force={force} height={}",
                        board.height()
                    );
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "packed_movegen")]
    fn packed_feature_request_api_matches_engine_oracle() {
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
                    let scalar =
                        raw_moves_for_order(&board, piece, force, MovegenOrder::ScalarCompatible);
                    let engine = raw_moves_for_engine(&board, piece, force);
                    assert_eq!(
                        scalar,
                        engine,
                        "scalar-compatible request differs for {piece:?} force={force} height={}",
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
    #[cfg(feature = "packed_movegen")]
    fn hybrid_equals_engine_across_gate_boundaries() {
        // h<=24 routes to packed; h>24 falls back to engine. Production generate()
        // canonical-sorts, so its output must equal the sorted engine output at
        // every boundary, including the h=24/25 routing edge.
        for h in [13usize, 18, 23, 24, 25, 29] {
            let b = board_with_height(h);
            for &p in &[Piece::I, Piece::S, Piece::Z, Piece::L, Piece::J] {
                let mut via = MoveBuffer::new();
                generate(&b, &mut via, p, false);
                let got: Vec<u16> = via.as_slice().iter().map(|m| m.raw()).collect();

                let mut eng = MoveBuffer::new();
                generate_engine(&b, &mut eng, p, false);
                let mut want: Vec<u16> = eng.as_slice().iter().map(|m| m.raw()).collect();
                want.sort_unstable();

                assert_eq!(got, want, "hybrid != engine at h={h} p={p:?}");
            }
        }
    }

    #[test]
    fn reachable_locks_matches_move_reachable_on_holed_boards() {
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
        for _ in 0..3000 {
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
                            got, want,
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
            holed > 100 && checks > 1000,
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

    #[test]
    fn generate_playable_matches_legacy_per_move_filter() {
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
        for _ in 0..800 {
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
                    let got: Vec<u16> = pb.as_slice().iter().map(|m| m.raw()).collect();
                    let want = legacy_playable(&b, p, force);
                    assert_eq!(
                        got, want,
                        "generate_playable != legacy filter p={p:?} force={force} rows={rows:?}"
                    );
                }
            }
        }
        assert!(holed > 50, "insufficient holed coverage {holed}");
    }

    #[test]
    fn packed_oljfilter_matches_scalar_generate_playable() {
        fn xs(s: &mut u64) -> u64 {
            let mut x = *s;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *s = x;
            x
        }
        let mut st = 0xC0FF_EE13_3700_1234u64;
        let pieces = [Piece::O, Piece::L, Piece::J];
        let mut per_height_holed = [0u64; 64];
        let mut per_height_mismatch = [0u64; 64];
        let mut first: Option<String> = None;
        for _ in 0..6000 {
            let h = 3 + (xs(&mut st) % 24) as usize;
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
            if b.height() as usize > PACKED_OLJ_MAX_HEIGHT {
                continue;
            }
            per_height_holed[h] += 1;
            let rows30: &[u16; crate::reach_packed::PH] =
                b.rows[..crate::reach_packed::PH].try_into().unwrap();
            for &p in &pieces {
                for force in [false, true] {
                    let mut sb = MoveBuffer::new();
                    generate_playable(&b, &mut sb, p, force);
                    let scalar: Vec<u16> = sb.as_slice().iter().map(|m| m.raw()).collect();

                    let mut pk = MoveBuffer::new();
                    crate::reach_packed::generate_packed_with_force(rows30, p, force, &mut pk);
                    let mut pset: Vec<u16> = pk.as_slice().iter().map(|m| m.raw()).collect();
                    pset.sort_unstable();
                    pset.dedup();
                    let mut hb = MoveBuffer::new();
                    generate(&b, &mut hb, p, force);
                    hb.retain(|m| b.legal_lock_placement(m) && pset.binary_search(&m.raw()).is_ok());
                    let hybrid: Vec<u16> = hb.as_slice().iter().map(|m| m.raw()).collect();

                    if hybrid != scalar {
                        per_height_mismatch[h] += 1;
                        if first.is_none() {
                            first = Some(format!("h={h} p={p:?} force={force} rows={rows:?}"));
                        }
                    }
                }
            }
        }
        let total: u64 = per_height_mismatch.iter().sum();
        let maxh = (0..64).rev().find(|&h| per_height_holed[h] > 0).unwrap_or(0);
        eprintln!("OLJ packed parity: max_holed_height={maxh} total_mismatch={total}");
        for h in 0..=maxh {
            if per_height_mismatch[h] > 0 {
                eprintln!("  height {h}: {} mismatches / {} holed", per_height_mismatch[h], per_height_holed[h]);
            }
        }
        if let Some(f) = &first {
            eprintln!("  first: {f}");
        }
        assert_eq!(total, 0, "packed OLJ filter diverges from scalar (see per-height above)");
    }

    /// Real-distribution parity: decode boards from an actual production `.ctx`
    /// shard (path via env `LABEL_OPP_REAL_CTX`) and assert the hybrid
    /// `generate_playable` is byte-identical to the legacy per-move pathfinder
    /// filter on every real board. Hermetic skip when the env is absent.
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
                    if matches!(p, Piece::O | Piece::L | Piece::J) && height <= PACKED_OLJ_MAX_HEIGHT
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
