//! Production `Move` emission from the SBoard racer kernel.
//!
//! `generate_engine` stays the differential oracle; the two share piece
//! tables, the canonical anchor convention (`canon_off` == the engine's
//! `searched[r+2]` folding), and the `(x, y, rotation)` move key space,
//! so placement-set parity is directly assertable (pinned by the
//! seeded-corpus test in this module). Emission order differs (mask scan
//! vs BFS discovery order) by design.

use crate::board::{Board, BOARD_HEIGHT};
use crate::header::{Move, Piece, Rotation};
use crate::move_buffer::MoveBuffer;
use crate::smear::{
    band_words, count_locks_rules, count_locks_rules_pair, csize, generate_labeled_rules,
    generate_rules, h_gen, SBoard, PI_T,
};

/// Pack the production row masks (10 bits per row, y-up) into an `N`-word
/// band: word `i` holds rows `6i..6i+6` at 10 bits each. Rows above the
/// 40-row board stay zero (the 8-word band spans 48 rows).
#[inline(always)]
fn sboard_from_rows<const N: usize>(rows: &[u16; BOARD_HEIGHT]) -> SBoard<N> {
    let mut d = [0u64; N];
    let mut i = 0;
    while i < N {
        let base = i * 6;
        let mut w = 0u64;
        let mut j = 0;
        while j < 6 {
            let y = base + j;
            if y < BOARD_HEIGHT {
                w |= (rows[y] as u64) << (10 * j);
            }
            j += 1;
        }
        d[i] = w;
        i += 1;
    }
    SBoard { d }
}

#[inline(always)]
fn emit_band<const P: usize, const N: usize>(
    rows: &[u16; BOARD_HEIGHT],
    h: i32,
    force: i32,
    moves: &mut MoveBuffer,
) {
    let sb = sboard_from_rows::<N>(rows);
    debug_assert_eq!(h, sb.max_y());
    let ml = generate_rules::<P, N>(&sb, h, force);
    let p = Piece::from_u8(P as u8);
    let mut rc = 0;
    while rc < csize(P) {
        let r = Rotation::from_u8(rc as u8);
        ml.m[rc].for_each_set_bit(|x, y| {
            moves.push(Move::new(p, r, x, y, false));
        });
        rc += 1;
    }
}

/// Labeled emission with engine-exact spin strata. T uses the T-spin `Move`
/// encodings and emits duals (one placement, several strata) once per
/// stratum exactly like the engine's independent full/mini/nospin scans.
/// Other pieces follow the engine's all-spin emission: Mini wins overlaps
/// (`nospin & !mini`), Full never occurs, no duals.
#[inline(always)]
fn emit_labeled_band<const P: usize, const N: usize>(
    rows: &[u16; BOARD_HEIGHT],
    h: i32,
    force: i32,
    moves: &mut MoveBuffer,
) {
    let sb = sboard_from_rows::<N>(rows);
    debug_assert_eq!(h, sb.max_y());
    let ml = generate_labeled_rules::<P, N>(&sb, h, force);
    let p = Piece::from_u8(P as u8);
    let mut rc = 0;
    while rc < csize(P) {
        let r = Rotation::from_u8(rc as u8);
        if P == PI_T {
            ml.full[rc].for_each_set_bit(|x, y| {
                moves.push(Move::new_tspin(r, x, y, true));
            });
            ml.mini[rc].for_each_set_bit(|x, y| {
                moves.push(Move::new_tspin(r, x, y, false));
            });
            ml.nospin[rc].for_each_set_bit(|x, y| {
                moves.push(Move::new(p, r, x, y, false));
            });
        } else {
            ml.mini[rc].for_each_set_bit(|x, y| {
                moves.push(Move::new_allspin_mini(p, r, x, y));
            });
            ml.nospin[rc].andnot(&ml.mini[rc]).for_each_set_bit(|x, y| {
                moves.push(Move::new(p, r, x, y, false));
            });
        }
        rc += 1;
    }
}

#[inline(always)]
fn emit_labeled<const P: usize>(b: &Board, force: bool, moves: &mut MoveBuffer) {
    let mut occ = 0u64;
    let mut c = 0;
    while c < b.cols.len() {
        occ |= b.cols[c];
        c += 1;
    }
    let h = (64 - occ.leading_zeros()) as i32;
    let f: i32 = if force { 64 } else { 0 };
    match band_words(h + h_gen(P)) {
        1 => emit_labeled_band::<P, 1>(&b.rows, h, f, moves),
        2 => emit_labeled_band::<P, 2>(&b.rows, h, f, moves),
        3 => emit_labeled_band::<P, 3>(&b.rows, h, f, moves),
        4 => emit_labeled_band::<P, 4>(&b.rows, h, f, moves),
        _ => emit_labeled_band::<P, 8>(&b.rows, h, f, moves),
    }
}

#[inline(always)]
fn emit_piece<const P: usize>(b: &Board, force: bool, moves: &mut MoveBuffer) {
    // Stack height from the column bitboards (the `Board` invariant keeps
    // rows/cols in sync; `do_move` maintains both).
    let mut occ = 0u64;
    let mut c = 0;
    while c < b.cols.len() {
        occ |= b.cols[c];
        c += 1;
    }
    let h = (64 - occ.leading_zeros()) as i32;
    // `force` extends the spawn scan upward without bound; the kernel
    // clamps its scan threshold to the band top, so any value past the
    // tallest band expresses the same semantics.
    let f: i32 = if force { 64 } else { 0 };
    match band_words(h + h_gen(P)) {
        1 => emit_band::<P, 1>(&b.rows, h, f, moves),
        2 => emit_band::<P, 2>(&b.rows, h, f, moves),
        3 => emit_band::<P, 3>(&b.rows, h, f, moves),
        4 => emit_band::<P, 4>(&b.rows, h, f, moves),
        _ => emit_band::<P, 8>(&b.rows, h, f, moves),
    }
}

/// Reachable lock placements for `p` on `b` as canonical-rotation `Move`s
/// with engine-exact spin labels: T-spin Full/Mini strata (with duals) for
/// T, all-spin Minis for the other spinnable pieces, plain placements for O.
pub fn generate_smear(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
    match p {
        Piece::I => emit_labeled::<{ Piece::I as usize }>(b, force, moves),
        Piece::O => emit_piece::<{ Piece::O as usize }>(b, force, moves),
        Piece::T => emit_labeled::<{ Piece::T as usize }>(b, force, moves),
        Piece::L => emit_labeled::<{ Piece::L as usize }>(b, force, moves),
        Piece::J => emit_labeled::<{ Piece::J as usize }>(b, force, moves),
        Piece::S => emit_labeled::<{ Piece::S as usize }>(b, force, moves),
        Piece::Z => emit_labeled::<{ Piece::Z as usize }>(b, force, moves),
    }
}

/// Label-free placement emission: one `Move` per distinct reachable
/// `(rotation, x, y)`. Same strict placement sets as `generate_smear` (the
/// plain and labeled kernels share reach closure), without the spin-strata
/// work and without T's dual emissions. This is the perft/expansion entry:
/// tree consumers must visit each child board exactly once.
pub fn generate_placements(b: &Board, moves: &mut MoveBuffer, p: Piece, force: bool) {
    match p {
        Piece::I => emit_piece::<{ Piece::I as usize }>(b, force, moves),
        Piece::O => emit_piece::<{ Piece::O as usize }>(b, force, moves),
        Piece::T => emit_piece::<{ Piece::T as usize }>(b, force, moves),
        Piece::L => emit_piece::<{ Piece::L as usize }>(b, force, moves),
        Piece::J => emit_piece::<{ Piece::J as usize }>(b, force, moves),
        Piece::S => emit_piece::<{ Piece::S as usize }>(b, force, moves),
        Piece::Z => emit_piece::<{ Piece::Z as usize }>(b, force, moves),
    }
}

#[inline(always)]
fn count_band<const P: usize, const N: usize>(rows: &[u16; BOARD_HEIGHT], h: i32, f: i32) -> u32 {
    let sb = sboard_from_rows::<N>(rows);
    debug_assert_eq!(h, sb.max_y());
    count_locks_rules::<P, N>(&sb, h, f)
}

#[inline(always)]
fn count_piece<const P: usize>(rows: &[u16; BOARD_HEIGHT], h: i32, force: bool) -> u32 {
    let f: i32 = if force { 64 } else { 0 };
    match band_words(h + h_gen(P)) {
        1 => count_band::<P, 1>(rows, h, f),
        2 => count_band::<P, 2>(rows, h, f),
        3 => count_band::<P, 3>(rows, h, f),
        4 => count_band::<P, 4>(rows, h, f),
        _ => count_band::<P, 8>(rows, h, f),
    }
}

/// Distinct reachable placement count from row masks alone (no `Board`, no
/// cols): the bulk-counting twin of `generate_placements`. `h` must be the
/// stack height implied by `rows` (callers on the perft leaf path already
/// track it; `debug_assert` cross-checks against the packed band).
pub fn count_smear_rows(rows: &[u16; BOARD_HEIGHT], h: i32, p: Piece, force: bool) -> u32 {
    match p {
        Piece::I => count_piece::<{ Piece::I as usize }>(rows, h, force),
        Piece::O => count_piece::<{ Piece::O as usize }>(rows, h, force),
        Piece::T => count_piece::<{ Piece::T as usize }>(rows, h, force),
        Piece::L => count_piece::<{ Piece::L as usize }>(rows, h, force),
        Piece::J => count_piece::<{ Piece::J as usize }>(rows, h, force),
        Piece::S => count_piece::<{ Piece::S as usize }>(rows, h, force),
        Piece::Z => count_piece::<{ Piece::Z as usize }>(rows, h, force),
    }
}

/// Pack production row masks into an `N`-word band for callers that batch
/// many counts against one base board (perft's fused last level).
pub fn pack_band<const N: usize>(rows: &[u16; BOARD_HEIGHT]) -> SBoard<N> {
    sboard_from_rows::<N>(rows)
}

/// `count_smear_rows` on a caller-prepacked band: skips the per-call row
/// repack. `N` must satisfy `band_words(h + h_gen(p))` and `h` must be the
/// packed stack height (`debug_assert` cross-checks).
pub fn count_smear_band<const N: usize>(sb: &SBoard<N>, h: i32, p: Piece, force: bool) -> u32 {
    debug_assert_eq!(h, sb.max_y());
    debug_assert!(N >= band_words(h + h_gen(p as usize)));
    let f: i32 = if force { 64 } else { 0 };
    match p {
        Piece::I => count_locks_rules::<{ Piece::I as usize }, N>(sb, h, f),
        Piece::O => count_locks_rules::<{ Piece::O as usize }, N>(sb, h, f),
        Piece::T => count_locks_rules::<{ Piece::T as usize }, N>(sb, h, f),
        Piece::L => count_locks_rules::<{ Piece::L as usize }, N>(sb, h, f),
        Piece::J => count_locks_rules::<{ Piece::J as usize }, N>(sb, h, f),
        Piece::S => count_locks_rules::<{ Piece::S as usize }, N>(sb, h, f),
        Piece::Z => count_locks_rules::<{ Piece::Z as usize }, N>(sb, h, f),
    }
}

pub fn count_smear_band_pair<const N: usize>(
    a: &SBoard<N>,
    ha: i32,
    b: &SBoard<N>,
    hb: i32,
    p: Piece,
    force: bool,
) -> (u32, u32) {
    debug_assert_eq!(ha, a.max_y());
    debug_assert_eq!(hb, b.max_y());
    debug_assert!(N >= band_words(ha + h_gen(p as usize)));
    debug_assert!(N >= band_words(hb + h_gen(p as usize)));
    let f: i32 = if force { 64 } else { 0 };
    match p {
        Piece::I => count_locks_rules_pair::<{ Piece::I as usize }, N>(a, ha, b, hb, f),
        Piece::O => count_locks_rules_pair::<{ Piece::O as usize }, N>(a, ha, b, hb, f),
        Piece::T => count_locks_rules_pair::<{ Piece::T as usize }, N>(a, ha, b, hb, f),
        Piece::L => count_locks_rules_pair::<{ Piece::L as usize }, N>(a, ha, b, hb, f),
        Piece::J => count_locks_rules_pair::<{ Piece::J as usize }, N>(a, ha, b, hb, f),
        Piece::S => count_locks_rules_pair::<{ Piece::S as usize }, N>(a, ha, b, hb, f),
        Piece::Z => count_locks_rules_pair::<{ Piece::Z as usize }, N>(a, ha, b, hb, f),
    }
}

/// `count_smear_rows` over a full `Board` (height read from the column
/// bitboards, same invariant note as `emit_piece`).
pub fn count_smear(b: &Board, p: Piece, force: bool) -> u32 {
    let mut occ = 0u64;
    let mut c = 0;
    while c < b.cols.len() {
        occ |= b.cols[c];
        c += 1;
    }
    let h = (64 - occ.leading_zeros()) as i32;
    count_smear_rows(&b.rows, h, p, force)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::ALL_PIECES;
    use crate::movegen::generate_engine;
    use std::collections::BTreeSet;

    struct SplitMix64 {
        state: u64,
    }

    impl SplitMix64 {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
    }

    fn random_board(rng: &mut SplitMix64) -> Board {
        let h = 1 + (rng.next() % 24) as usize;
        let mut board = Board::new();
        for y in 0..h {
            let mut row = (rng.next() & 0x3FF) as u16;
            row &= !(1u16 << (rng.next() % 10));
            board.rows[y] = row;
            let mut bits = row as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                board.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        board
    }

    fn random_rows_for_height(rng: &mut SplitMix64, h: usize) -> [u16; BOARD_HEIGHT] {
        let mut rows = [0u16; BOARD_HEIGHT];
        for row in rows.iter_mut().take(h) {
            let occupied = 1u16 << (rng.next() % 10);
            let hole = 1u16 << (rng.next() % 10);
            *row = ((rng.next() & 0x3FF) as u16 | occupied) & !hole;
            if *row == 0 {
                *row = occupied;
            }
        }
        rows
    }

    fn pair_parity_for_piece<const P: usize, const N: usize>(
        a_rows: &[u16; BOARD_HEIGHT],
        ha: i32,
        b_rows: &[u16; BOARD_HEIGHT],
        hb: i32,
        force: i32,
    ) {
        let a = pack_band::<N>(a_rows);
        let b = pack_band::<N>(b_rows);
        let got = count_locks_rules_pair::<P, N>(&a, ha, &b, hb, force);
        let want = (
            count_locks_rules::<P, N>(&a, ha, force),
            count_locks_rules::<P, N>(&b, hb, force),
        );
        assert_eq!(got, want, "piece {P} N {N} ha {ha} hb {hb} force {force}");
    }

    fn pair_parity_all_pieces<const N: usize>(
        a_rows: &[u16; BOARD_HEIGHT],
        ha: i32,
        b_rows: &[u16; BOARD_HEIGHT],
        hb: i32,
        force: i32,
    ) {
        pair_parity_for_piece::<{ Piece::I as usize }, N>(a_rows, ha, b_rows, hb, force);
        pair_parity_for_piece::<{ Piece::O as usize }, N>(a_rows, ha, b_rows, hb, force);
        pair_parity_for_piece::<{ Piece::T as usize }, N>(a_rows, ha, b_rows, hb, force);
        pair_parity_for_piece::<{ Piece::L as usize }, N>(a_rows, ha, b_rows, hb, force);
        pair_parity_for_piece::<{ Piece::J as usize }, N>(a_rows, ha, b_rows, hb, force);
        pair_parity_for_piece::<{ Piece::S as usize }, N>(a_rows, ha, b_rows, hb, force);
        pair_parity_for_piece::<{ Piece::Z as usize }, N>(a_rows, ha, b_rows, hb, force);
    }

    #[test]
    fn count_locks_rules_pair_matches_single_counts() {
        let mut rng = SplitMix64::new(0xC0DE_CAFE_1234_5678);
        for i in 0..2_000usize {
            let h1 = [0, 1, 2, 3][i & 3];
            let h2 = 4 + (rng.next() % 6) as usize;
            let h3 = 10 + (rng.next() % 6) as usize;
            let h4 = 16 + (rng.next() % 6) as usize;
            let h8 = 22 + (rng.next() % 18) as usize;

            let a1 = random_rows_for_height(&mut rng, h1);
            let b1 = random_rows_for_height(&mut rng, h1);
            pair_parity_all_pieces::<1>(&a1, h1 as i32, &b1, h1 as i32, 0);

            let a2 = random_rows_for_height(&mut rng, h2);
            let b2 = random_rows_for_height(&mut rng, h2);
            pair_parity_all_pieces::<2>(&a2, h2 as i32, &b2, h2 as i32, 0);

            let a3 = random_rows_for_height(&mut rng, h3);
            let b3 = random_rows_for_height(&mut rng, h3);
            pair_parity_all_pieces::<3>(&a3, h3 as i32, &b3, h3 as i32, 0);

            let a4 = random_rows_for_height(&mut rng, h4);
            let b4 = random_rows_for_height(&mut rng, h4);
            pair_parity_all_pieces::<4>(&a4, h4 as i32, &b4, h4 as i32, 0);

            let a8 = random_rows_for_height(&mut rng, h8);
            let b8 = random_rows_for_height(&mut rng, h8);
            pair_parity_all_pieces::<8>(&a8, h8 as i32, &b8, h8 as i32, 0);
        }

        let empty = [0u16; BOARD_HEIGHT];
        pair_parity_all_pieces::<8>(&empty, 0, &empty, 0, 0);

        let same = random_rows_for_height(&mut rng, 18);
        pair_parity_all_pieces::<4>(&same, 18, &same, 18, 0);
        pair_parity_all_pieces::<4>(&same, 18, &same, 18, 64);
    }

    type Key = (i32, i32, u8);

    fn engine_set(b: &Board, p: Piece, force: bool) -> BTreeSet<Key> {
        let mut moves = MoveBuffer::new();
        generate_engine::<true>(b, &mut moves, p, force);
        moves
            .iter()
            .map(|m| (m.x(), m.y(), m.rotation() as u8))
            .collect()
    }

    // Engine BFS over-produces on holed boards (worklist-timing artifacts);
    // `generate_playable` retains only placements the strict
    // first-valid-kick reach admits (the semantics the racer kernel
    // implements).
    fn playable_set(b: &Board, p: Piece, force: bool) -> BTreeSet<Key> {
        let mut moves = MoveBuffer::new();
        crate::movegen::generate_playable(b, &mut moves, p, force);
        moves
            .iter()
            .map(|m| (m.x(), m.y(), m.rotation() as u8))
            .collect()
    }

    fn smear_set(b: &Board, p: Piece, force: bool) -> BTreeSet<Key> {
        let mut moves = MoveBuffer::new();
        generate_smear(b, &mut moves, p, force);
        moves
            .iter()
            .map(|m| (m.x(), m.y(), m.rotation() as u8))
            .collect()
    }

    #[derive(Default)]
    struct ParityStats {
        engine_timing_extras: usize,
        engine_underreach: usize,
    }

    // Smear must equal the strict SRS+/180 reference BFS exactly (both
    // directions, every board). Low boards use sky-hover seed; tall boards
    // use engine spawn-scan seed. Production engine sets are diagnostic only:
    // engine raw BFS over-produces (worklist timing). `move_reachable`'s
    // group2 fold misses (case-13 / case-353) were fixed in pathfinder.rs;
    // the probes stay as provenance.
    fn assert_parity(b: &Board, label: &str, stats: &mut ParityStats) {
        let h = b
            .rows
            .iter()
            .rposition(|&r| r != 0)
            .map(|y| y + 1)
            .unwrap_or(0);
        for &p in ALL_PIECES.iter() {
            for force in [false, true] {
                let s = smear_set(b, p, force);
                let n = naive_reach_impl(b, p, 3, true, h > 17, force);
                if s != n {
                    let ref_only: Vec<Key> = n.difference(&s).cloned().collect();
                    let smear_only: Vec<Key> = s.difference(&n).cloned().collect();
                    panic!(
                        "{label}: piece {p:?} force {force} h {h}:\n  reference-only {ref_only:?}\n  smear-only {smear_only:?}\n  rows {:?}",
                        &b.rows[..h.max(1)]
                    );
                }
                let e = engine_set(b, p, force);
                stats.engine_timing_extras += e.difference(&n).count();
                stats.engine_underreach += n.difference(&e).count();
            }
        }
    }

    fn cells_of(p: Piece, r: u8) -> [(i32, i32); 4] {
        let pc = crate::header::piece_table(p, crate::header::Rotation::from_u8(r));
        [
            (0, 0),
            (pc.coords[0].x as i32, pc.coords[0].y as i32),
            (pc.coords[1].x as i32, pc.coords[1].y as i32),
            (pc.coords[2].x as i32, pc.coords[2].y as i32),
        ]
    }

    fn fits(b: &Board, p: Piece, r: u8, x: i32, y: i32) -> bool {
        cells_of(p, r).iter().all(|&(dx, dy)| {
            let cx = x + dx;
            let cy = y + dy;
            (0..10).contains(&cx)
                && (0..64).contains(&cy)
                && (cy >= BOARD_HEIGHT as i32 || b.rows[cy as usize] & (1u16 << cx) == 0)
        })
    }

    fn naive_reach(b: &Board, p: Piece, ndirs: usize, srs_plus: bool) -> BTreeSet<Key> {
        naive_reach_impl(b, p, ndirs, srs_plus, false, false)
    }

    fn naive_reach_impl(
        b: &Board,
        p: Piece,
        ndirs: usize,
        srs_plus: bool,
        spawn_seed: bool,
        force: bool,
    ) -> BTreeSet<Key> {
        use crate::gen::{kick_180_index, kick_index, KICKS, KICKS_180};
        let ki = kick_index(p, srs_plus);
        let ki180 = kick_180_index(p);
        let rot_count: u8 = if p == Piece::O { 1 } else { 4 };
        let mut seen = [[[false; 4]; 64]; 10];
        let mut work: Vec<(i32, i32, u8)> = Vec::new();
        let h = b
            .rows
            .iter()
            .rposition(|&r| r != 0)
            .map(|y| y + 1)
            .unwrap_or(0) as i32;
        if spawn_seed {
            let sr = crate::default_ruleset::ACTIVE_RULES.spawn_row;
            let top = if force { 63 } else { sr };
            for s in sr..=top {
                if fits(b, p, 0, 4, s) {
                    seen[4][s as usize][0] = true;
                    work.push((4, s, 0));
                    break;
                }
            }
        } else {
            for r in 0..rot_count {
                for x in 0..10 {
                    for y in h..21 {
                        if fits(b, p, r, x, y) && !seen[x as usize][y as usize][r as usize] {
                            seen[x as usize][y as usize][r as usize] = true;
                            work.push((x, y, r));
                        }
                    }
                }
            }
        }
        while let Some((x, y, r)) = work.pop() {
            let mut push = |x1: i32, y1: i32, r1: u8| {
                if (0..64).contains(&y1) && !seen[x1 as usize][y1 as usize][r1 as usize] {
                    seen[x1 as usize][y1 as usize][r1 as usize] = true;
                    work.push((x1, y1, r1));
                }
            };
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1)] {
                if fits(b, p, r, x + dx, y + dy) {
                    push(x + dx, y + dy, r);
                }
            }
            for d in 0..if p == Piece::O { 0 } else { ndirs } {
                let (r1, kicks): (u8, &[crate::header::Coordinates]) = match d {
                    0 => ((r + 1) & 3, &KICKS[ki][0][r as usize][..]),
                    1 => ((r + 3) & 3, &KICKS[ki][1][r as usize][..]),
                    _ => ((r + 2) & 3, &KICKS_180[ki180][r as usize][..]),
                };
                for k in kicks {
                    let (x1, y1) = (x + k.x as i32, y + k.y as i32);
                    if fits(b, p, r1, x1, y1) {
                        push(x1, y1, r1);
                        break;
                    }
                }
            }
        }
        let mut out = BTreeSet::new();
        for x in 0..10i32 {
            for y in 0..40i32 {
                for r in 0..rot_count {
                    if seen[x as usize][y as usize][r as usize]
                        && fits(b, p, r, x, y)
                        && !fits(b, p, r, x, y - 1)
                    {
                        let rc = match p {
                            Piece::O => 0,
                            Piece::I | Piece::S | Piece::Z => r & 1,
                            _ => r,
                        };
                        let off = crate::gen::canonical_offset(p, Rotation::from_u8(r));
                        out.insert((x - off.x as i32, y - off.y as i32, rc));
                    }
                }
            }
        }
        out
    }

    #[test]
    #[ignore]
    fn probe_cutover_diffs() {
        use crate::default_ruleset::ACTIVE_RULES;
        use crate::gen::SPAWN_COL;
        const FULL_ROW: u16 = 0x3FF;
        let mut tall = Board::new();
        for y in 0..BOARD_HEIGHT {
            tall.rows[y] = match y % 4 {
                0 => FULL_ROW & !(1u16 << SPAWN_COL),
                1 => FULL_ROW & !(1u16 << 2),
                2 => FULL_ROW & !(1u16 << 7),
                _ => FULL_ROW & !(1u16 << 4),
            };
        }
        tall.rows[ACTIVE_RULES.spawn_row as usize] &= !(1u16 << SPAWN_COL);
        for y in 0..BOARD_HEIGHT {
            let mut bits = tall.rows[y] as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                tall.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        let e = engine_set(&tall, Piece::I, true);
        let s = smear_set(&tall, Piece::I, true);
        let n = naive_reach_impl(&tall, Piece::I, 3, true, true, true);
        for k in e.symmetric_difference(&s) {
            println!(
                "TALL diff {k:?} engine {} smear {} reference {}",
                e.contains(k),
                s.contains(k),
                n.contains(k)
            );
        }
        let el = labeled_set(&tall, Piece::I, true, false);
        let sl = labeled_set(&tall, Piece::I, true, true);
        for k in el.symmetric_difference(&sl) {
            println!(
                "TALL label diff {k:?} engine {} smear {} ref-placement {}",
                el.contains(k),
                sl.contains(k),
                n.contains(&(k.0, k.1, k.2))
            );
        }
        let mut em = MoveBuffer::new();
        generate_engine::<true>(&tall, &mut em, Piece::I, true);
        let mut sm = MoveBuffer::new();
        generate_smear(&tall, &mut sm, Piece::I, true);
        println!(
            "TALL raw lens: engine {} (unique {}) smear {} (unique {})",
            em.len(),
            em.iter().map(|m| m.raw()).collect::<BTreeSet<_>>().len(),
            sm.len(),
            sm.iter().map(|m| m.raw()).collect::<BTreeSet<_>>().len()
        );

        let mut nn = Board::new();
        for y in 0..BOARD_HEIGHT {
            nn.rows[y] = if y < ACTIVE_RULES.spawn_row as usize {
                FULL_ROW & !(1u16 << SPAWN_COL)
            } else {
                FULL_ROW
            };
            let mut bits = nn.rows[y] as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                nn.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        let mut en = MoveBuffer::new();
        generate_engine::<true>(&nn, &mut en, Piece::I, true);
        let mut sn = MoveBuffer::new();
        generate_smear(&nn, &mut sn, Piece::I, true);
        let er: BTreeSet<u16> = en.iter().map(|m| m.raw()).collect();
        let sr: BTreeSet<u16> = sn.iter().map(|m| m.raw()).collect();
        for m in en.iter() {
            if !sr.contains(&m.raw()) {
                println!(
                    "NN engine-only raw {} = x {} y {} r {:?} spin {:?}",
                    m.raw(),
                    m.x(),
                    m.y(),
                    m.rotation(),
                    m.spin()
                );
            }
        }
        for m in sn.iter() {
            if !er.contains(&m.raw()) {
                println!(
                    "NN smear-only raw {} = x {} y {} r {:?} spin {:?}",
                    m.raw(),
                    m.x(),
                    m.y(),
                    m.rotation(),
                    m.spin()
                );
            }
        }

        let rows: [u16; 9] = [489, 666, 543, 155, 2, 474, 249, 768, 510];
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
        let n2 = naive_reach_impl(&b, Piece::I, 3, true, false, false);
        let s2 = smear_set(&b, Piece::I, false);
        println!(
            "BLIND (1,7,North): reference {} smear {}",
            n2.contains(&(1, 7, 0)),
            s2.contains(&(1, 7, 0))
        );
        let mut stats = ParityStats::default();
        assert_parity(&b, "blind board", &mut stats);
        println!("BLIND assert_parity passed");
        for k in s2.symmetric_difference(&n2) {
            println!(
                "BLIND diff {k:?} smear {} ref {}",
                s2.contains(k),
                n2.contains(k)
            );
        }
    }

    #[test]
    #[ignore]
    fn probe_s_mini_case1() {
        use crate::smear::{generate_labeled_rules, imm_rot_probe, usable_map_probe};
        let rows: [u16; 4] = [906, 913, 524, 545];
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
        let mut em = MoveBuffer::new();
        generate_engine::<true>(&b, &mut em, Piece::S, false);
        for m in em.iter() {
            if m.spin() as u8 != 0 {
                println!(
                    "ENGINE spin move: x {} y {} r {:?} spin {:?}",
                    m.x(),
                    m.y(),
                    m.rotation(),
                    m.spin()
                );
            }
        }
        let sb = sboard_from_rows::<2>(&b.rows);
        let u = usable_map_probe::<{ Piece::S as usize }, 2>(&sb);
        for (rc, urc) in u.iter().enumerate().take(2) {
            let imm = imm_rot_probe(urc);
            for &(x, y) in &[(1i32, 1i32), (4i32, 2i32)] {
                println!(
                    "SMEAR rc {rc} at ({x},{y}): usable {} imm {}",
                    urc.get(x, y),
                    imm.get(x, y)
                );
            }
        }
        let ml = generate_labeled_rules::<{ Piece::S as usize }, 2>(&sb, 4, 0);
        for rc in 0..2 {
            for &(x, y) in &[(1i32, 1i32), (4i32, 2i32)] {
                println!(
                    "SMEAR rc {rc} at ({x},{y}): m {} full {} mini {} nospin {}",
                    ml.m[rc].get(x, y),
                    ml.full[rc].get(x, y),
                    ml.mini[rc].get(x, y),
                    ml.nospin[rc].get(x, y)
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn probe_force_h25_s_spawn_rest() {
        let rows: [u16; 25] = [
            638, 530, 590, 986, 533, 777, 39, 71, 87, 423, 708, 265, 515, 1007, 816, 166, 959, 40,
            584, 436, 399, 4, 100, 996, 587,
        ];
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
        let s = smear_set(&b, Piece::S, true);
        let n = naive_reach_impl(&b, Piece::S, 3, true, true, true);
        let e = engine_set(&b, Piece::S, true);
        println!(
            "smear {} naive {} engine {} | (4,23,0): smear {} naive {} engine {}",
            s.len(),
            n.len(),
            e.len(),
            s.contains(&(4, 23, 0)),
            n.contains(&(4, 23, 0)),
            e.contains(&(4, 23, 0))
        );
        println!(
            "naive-not-smear {:?} | smear-not-naive {:?}",
            n.difference(&s).collect::<Vec<_>>(),
            s.difference(&n).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore]
    fn probe_case1_i_piece() {
        use crate::smear::{generate, generate_rules, SBoard};
        let rows: [u16; 8] = [527, 263, 30, 60, 898, 821, 581, 746];
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
        let e = engine_set(&b, Piece::I, false);
        let sb: SBoard<2> = super::sboard_from_rows::<2>(&b.rows);
        let mut plain: BTreeSet<Key> = BTreeSet::new();
        let ml = generate::<{ Piece::I as usize }, 2>(&sb, 8, 0);
        for rc in 0..2 {
            ml.m[rc].for_each_set_bit(|x, y| {
                plain.insert((x, y, rc as u8));
            });
        }
        let mut rules: BTreeSet<Key> = BTreeSet::new();
        let mlr = generate_rules::<{ Piece::I as usize }, 2>(&sb, 8, 0);
        for rc in 0..2 {
            mlr.m[rc].for_each_set_bit(|x, y| {
                rules.insert((x, y, rc as u8));
            });
        }
        println!(
            "engine {} plain {} rules {}",
            e.len(),
            plain.len(),
            rules.len()
        );
        println!(
            "engine-not-plain {:?}",
            e.difference(&plain).collect::<Vec<_>>()
        );
        println!(
            "engine-not-rules {:?}",
            e.difference(&rules).collect::<Vec<_>>()
        );
        println!(
            "plain-not-engine {:?}",
            plain.difference(&e).collect::<Vec<_>>()
        );
        println!(
            "rules-not-engine {:?}",
            rules.difference(&e).collect::<Vec<_>>()
        );
        let mut pb = MoveBuffer::new();
        crate::movegen::generate_playable(&b, &mut pb, Piece::I, false);
        let playable: BTreeSet<Key> = pb
            .iter()
            .map(|m| (m.x(), m.y(), m.rotation() as u8))
            .collect();
        println!(
            "playable {} | playable-not-rules {:?} | rules-not-playable {:?}",
            playable.len(),
            playable.difference(&rules).collect::<Vec<_>>(),
            rules.difference(&playable).collect::<Vec<_>>()
        );
        for (ndirs, plus) in [(3, true), (2, true), (2, false), (3, false)] {
            let n = naive_reach(&b, Piece::I, ndirs, plus);
            println!(
                "naive ndirs={} srs_plus={}: {} | naive-not-engine {:?} | engine-not-naive {:?}",
                ndirs,
                plus,
                n.len(),
                n.difference(&e).collect::<Vec<_>>(),
                e.difference(&n).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    #[ignore]
    fn probe_case353_i_force_tall() {
        let rows: [u16; 23] = [
            532, 186, 142, 497, 630, 526, 937, 37, 190, 398, 549, 82, 398, 355, 18, 23, 45, 404,
            278, 651, 818, 276, 16,
        ];
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
        let s = smear_set(&b, Piece::I, true);
        let pl = playable_set(&b, Piece::I, true);
        let e = engine_set(&b, Piece::I, true);
        let nt = naive_reach_impl(&b, Piece::I, 3, true, true, true);
        println!(
            "smear {} playable {} engine {} naive-tall {}",
            s.len(),
            pl.len(),
            e.len(),
            nt.len()
        );
        println!(
            "playable-not-smear {:?}",
            pl.difference(&s).collect::<Vec<_>>()
        );
        println!(
            "naive-not-smear {:?}",
            nt.difference(&s).collect::<Vec<_>>()
        );
        println!(
            "smear-not-naive {:?}",
            s.difference(&nt).collect::<Vec<_>>()
        );
        println!(
            "naive-not-playable {:?}",
            nt.difference(&pl).collect::<Vec<_>>()
        );
        println!(
            "playable-not-naive {:?}",
            pl.difference(&nt).collect::<Vec<_>>()
        );
        let target = Move::new(Piece::I, Rotation::East, 4, 25, false);
        let inputs = crate::pathfinder::get_input(&b, &target, false, true);
        println!("input path to (4,25,East): {:?}", inputs.data);
    }

    #[test]
    #[ignore]
    fn probe_case13_i_left_slot() {
        let rows: [u16; 3] = [634, 208, 972];
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
        let m = Move::new(Piece::I, Rotation::North, 1, 1, false);
        let mut raw = MoveBuffer::new();
        generate_engine::<true>(&b, &mut raw, Piece::I, false);
        let raw_has = raw.iter().any(|mv| mv.raw() == m.raw());
        println!(
            "legal {} pathfinder-reachable {} engine-raw-has {}",
            b.legal_lock_placement(&m),
            crate::movegen::move_reachable(&b, &m, false),
            raw_has
        );
    }

    #[test]
    #[ignore]
    fn probe_case4_s_pocket() {
        let rows: [u16; 18] = [
            531, 182, 32, 710, 608, 683, 985, 727, 402, 562, 545, 977, 414, 691, 779, 97, 468, 708,
        ];
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
        let m = Move::new(Piece::S, Rotation::East, 1, 15, false);
        println!(
            "legal {} pathfinder-reachable {}",
            b.legal_lock_placement(&m),
            crate::movegen::move_reachable(&b, &m, false)
        );
        let e = engine_set(&b, Piece::S, false);
        let p = playable_set(&b, Piece::S, false);
        let s = smear_set(&b, Piece::S, false);
        println!(
            "engine {} playable {} smear {} | playable-not-smear {:?} | smear-not-playable {:?}",
            e.len(),
            p.len(),
            s.len(),
            p.difference(&s).collect::<Vec<_>>(),
            s.difference(&p).collect::<Vec<_>>()
        );
    }

    #[test]
    fn smear_core_placement_parity_seeded() {
        let mut rng = SplitMix64::new(0x53E4_C04E_2026_0705);
        let mut stats = ParityStats::default();
        assert_parity(&Board::new(), "empty", &mut stats);
        for i in 0..3000 {
            let b = random_board(&mut rng);
            assert_parity(&b, &format!("seeded case {i}"), &mut stats);
        }
        println!(
            "PARITY smear_core seeded: 3001 boards x 7 pieces x 2 force | engine raw extras {} | engine raw missing {}",
            stats.engine_timing_extras, stats.engine_underreach
        );
    }

    type LKey = (i32, i32, u8, u8);

    fn labeled_set(b: &Board, p: Piece, force: bool, smear: bool) -> BTreeSet<LKey> {
        let mut moves = MoveBuffer::new();
        if smear {
            generate_smear(b, &mut moves, p, force);
        } else {
            generate_engine::<true>(b, &mut moves, p, force);
        }
        moves
            .iter()
            .map(|m| (m.x(), m.y(), m.rotation() as u8, m.spin() as u8))
            .collect()
    }

    fn strata(s: &BTreeSet<LKey>, spin: u8) -> BTreeSet<LKey> {
        s.iter().filter(|k| k.3 == spin).cloned().collect()
    }

    // On strictly-reachable placements, Full and Mini must match the engine
    // exactly (pre-dedup rotation-arrival strata, order-independent). Engine
    // NoSpin must be covered; extras are tolerated only on phantom
    // placements (worklist-timing over-production). Smear-only spin labels
    // are never allowed. Applies to every spinnable piece.
    #[test]
    fn smear_core_labels_match_engine_seeded() {
        const PIECES: [Piece; 7] = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ];
        let mut rng = SplitMix64::new(0x71AB_E15C_2026_0705);
        let mut full_total = 0usize;
        let mut mini_total = [0usize; 7];
        let mut nospin_extras = 0usize;
        let mut phantom_labeled = [0usize; 3];
        for case in 0..3001 {
            let b = if case == 0 {
                Board::new()
            } else {
                random_board(&mut rng)
            };
            let h = b.rows.iter().rposition(|&r| r != 0).map_or(0, |y| y + 1);
            for p in PIECES {
                for force in [false, true] {
                    let e = labeled_set(&b, p, force, false);
                    let s = labeled_set(&b, p, force, true);
                    let reachable = naive_reach_impl(&b, p, 3, true, h > 17, force);
                    for spin in [2u8, 1u8] {
                        let es = strata(&e, spin);
                        let ss = strata(&s, spin);
                        let engine_only: Vec<&LKey> = es.difference(&ss).collect();
                        let smear_only: Vec<&LKey> = ss.difference(&es).collect();
                        let engine_only_real: Vec<&&LKey> = engine_only
                            .iter()
                            .filter(|k| reachable.contains(&(k.0, k.1, k.2)))
                            .collect();
                        if !engine_only_real.is_empty() || !smear_only.is_empty() {
                            panic!(
                                "case {case} {p:?} force {force} h {h} spin {spin}:\n  engine-only-real {engine_only_real:?}\n  smear-only {smear_only:?}\n  rows {:?}",
                                &b.rows[..h.max(1)]
                            );
                        }
                        phantom_labeled[spin as usize] += engine_only.len();
                    }
                    let en = strata(&e, 0);
                    let sn = strata(&s, 0);
                    let missing_real: Vec<&LKey> = en
                        .difference(&sn)
                        .filter(|k| reachable.contains(&(k.0, k.1, k.2)))
                        .collect();
                    if !missing_real.is_empty() {
                        panic!(
                            "case {case} {p:?} force {force} h {h}: engine NoSpin missing from smear {missing_real:?}\n  rows {:?}",
                            &b.rows[..h.max(1)]
                        );
                    }
                    phantom_labeled[0] += en
                        .difference(&sn)
                        .filter(|k| !reachable.contains(&(k.0, k.1, k.2)))
                        .count();
                    nospin_extras += sn.difference(&en).count();
                    if p != Piece::T {
                        assert!(
                            strata(&s, 2).is_empty() && strata(&e, 2).is_empty(),
                            "case {case} {p:?}: Full labels on a non-T piece"
                        );
                    }
                    if p == Piece::O {
                        assert!(
                            strata(&s, 1).is_empty(),
                            "case {case}: O piece carried spin labels"
                        );
                    }
                    full_total += strata(&s, 2).len();
                    mini_total[p as usize] += strata(&s, 1).len();
                }
            }
        }
        assert!(full_total > 0, "corpus never produced a Full label");
        for p in PIECES {
            if p != Piece::O {
                assert!(
                    mini_total[p as usize] > 0,
                    "corpus never produced a Mini label for {p:?}"
                );
            }
        }
        println!(
            "PARITY smear_core labels: 3001 boards x 7 pieces x 2 force | full {full_total} mini {mini_total:?} | nospin extras (order-free ⊇ engine) {nospin_extras} | engine phantom-labeled [nospin,mini,full] {phantom_labeled:?}"
        );
    }

    #[test]
    fn smear_core_placement_parity_edge_boards() {
        // Tall single-column well: exercises the 8-word band + slow init.
        let mut tall = Board::new();
        for y in 0..30 {
            let row: u16 = 0x3FF & !(1 << 9);
            tall.rows[y] = row;
            for x in 0..9 {
                tall.cols[x] |= 1u64 << y;
            }
        }
        let mut stats = ParityStats::default();
        assert_parity(&tall, "tall well", &mut stats);

        // Checkerboard-ish mid stack: dense tuck/kick territory.
        let mut mid = Board::new();
        let pattern: [u16; 8] = [0x155, 0x2AA, 0x155, 0x2AA, 0x175, 0x2EA, 0x1D5, 0x2BA];
        for (y, &row) in pattern.iter().enumerate() {
            mid.rows[y] = row;
            let mut bits = row as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                mid.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        assert_parity(&mid, "checker mid", &mut stats);
    }

    #[test]
    #[ignore]
    fn smear_core_interleaved_bench() {
        use std::hint::black_box;
        use std::time::Instant;

        let mut rng = SplitMix64::new(0xBE4C_1234_2026_0705);
        let boards: Vec<Board> = (0..2000).map(|_| random_board(&mut rng)).collect();

        fn smear_plain(b: &Board, moves: &mut MoveBuffer, p: Piece) {
            use crate::smear::generate;
            macro_rules! run {
                ($pi:expr, $n:literal) => {{
                    let sb = sboard_from_rows::<$n>(&b.rows);
                    let mut occ = 0u64;
                    for c in &b.cols {
                        occ |= *c;
                    }
                    let h = (64 - occ.leading_zeros()) as i32;
                    let ml = generate::<$pi, $n>(&sb, h, 0);
                    let mut rc = 0;
                    while rc < csize($pi) {
                        let r = Rotation::from_u8(rc as u8);
                        ml.m[rc].for_each_set_bit(|x, y| {
                            moves.push(Move::new(p, r, x, y, false));
                        });
                        rc += 1;
                    }
                }};
            }
            macro_rules! per_piece {
                ($pi:expr) => {{
                    let mut occ = 0u64;
                    for c in &b.cols {
                        occ |= *c;
                    }
                    let h = (64 - occ.leading_zeros()) as i32;
                    match crate::smear::band_words(h + crate::smear::h_gen($pi)) {
                        1 => run!($pi, 1),
                        2 => run!($pi, 2),
                        3 => run!($pi, 3),
                        4 => run!($pi, 4),
                        _ => run!($pi, 8),
                    }
                }};
            }
            match p {
                Piece::I => per_piece!(0),
                Piece::O => per_piece!(1),
                Piece::T => per_piece!(2),
                Piece::L => per_piece!(3),
                Piece::J => per_piece!(4),
                Piece::S => per_piece!(5),
                Piece::Z => per_piece!(6),
            }
        }

        for &p in ALL_PIECES.iter() {
            let mut best_prod = f64::MAX;
            let mut best_smear = f64::MAX;
            let mut best_plain = f64::MAX;
            for _ in 0..7 {
                let t0 = Instant::now();
                let mut sink = 0u64;
                for b in &boards {
                    let mut moves = MoveBuffer::new();
                    crate::movegen::generate(black_box(b), &mut moves, p, false);
                    sink += moves.len() as u64;
                }
                let prod = t0.elapsed().as_nanos() as f64 / boards.len() as f64;
                black_box(sink);

                let t1 = Instant::now();
                let mut sink2 = 0u64;
                for b in &boards {
                    let mut moves = MoveBuffer::new();
                    generate_smear(black_box(b), &mut moves, p, false);
                    sink2 += moves.len() as u64;
                }
                let smear = t1.elapsed().as_nanos() as f64 / boards.len() as f64;
                black_box(sink2);

                let t2 = Instant::now();
                let mut sink3 = 0u64;
                for b in &boards {
                    let mut moves = MoveBuffer::new();
                    smear_plain(black_box(b), &mut moves, p);
                    sink3 += moves.len() as u64;
                }
                let plain = t2.elapsed().as_nanos() as f64 / boards.len() as f64;
                black_box(sink3);

                best_prod = best_prod.min(prod);
                best_smear = best_smear.min(smear);
                best_plain = best_plain.min(plain);
            }
            // Since the cutover, `movegen::generate` routes through
            // `generate_smear`, so the first arm measures the dispatch
            // wrapper (MoveRequest plumbing + CanonicalRaw sort) on top of
            // the same kernel.
            println!(
                "BENCH smear_core {:?}: dispatch(sorted) {:.1}ns generate_smear {:.1}ns wrapper+sort {:.2}x | no-rules {:.1}ns (180+srs+ tax {:.1}ns)",
                p,
                best_prod,
                best_smear,
                best_prod / best_smear,
                best_plain,
                best_smear - best_plain
            );
        }

        // Conversion cost alone, for the record.
        let mut best_conv = f64::MAX;
        for _ in 0..7 {
            let t = Instant::now();
            let mut sink = 0u64;
            for b in &boards {
                let sb = sboard_from_rows::<3>(black_box(&b.rows));
                sink ^= sb.d[0];
            }
            best_conv = best_conv.min(t.elapsed().as_nanos() as f64 / boards.len() as f64);
            black_box(sink);
        }
        println!("BENCH smear_core rows->SBoard<3>: {:.1}ns", best_conv);
    }

    fn apply_key(parent: &Board, p: Piece, key: Key) -> Board {
        let (x, y, rc) = key;
        let mut rows = parent.rows;
        for &(dx, dy) in cells_of(p, rc).iter() {
            let (cx, cy) = ((x + dx) as usize, (y + dy) as usize);
            if cx < 10 && cy < BOARD_HEIGHT {
                rows[cy] |= 1u16 << cx;
            }
        }
        let mut out = [0u16; BOARD_HEIGHT];
        let mut w = 0;
        for row in rows.iter().take(BOARD_HEIGHT) {
            if *row != crate::board::FULL_ROW {
                out[w] = *row;
                w += 1;
            }
        }
        let mut child = Board::new();
        child.rows = out;
        for (yy, row) in out.iter().enumerate() {
            let mut bits = *row as u64;
            while bits != 0 {
                let cx = bits.trailing_zeros() as usize;
                child.cols[cx] |= 1u64 << yy;
                bits &= bits - 1;
            }
        }
        child
    }

    fn reference_perft(b: &Board, queue_offset: usize, depth: usize) -> u64 {
        const QUEUE: [Piece; 7] = [
            Piece::I,
            Piece::O,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
            Piece::T,
        ];
        let p = QUEUE[queue_offset % 7];
        let h = b.rows.iter().rposition(|&r| r != 0).map_or(0, |i| i + 1);
        let keys = naive_reach_impl(b, p, 3, true, h > 17, false);
        if depth == 1 {
            return keys.len() as u64;
        }
        let mut nodes = 0u64;
        for &key in &keys {
            let child = apply_key(b, p, key);
            nodes += reference_perft(&child, queue_offset + 1, depth - 1);
        }
        nodes
    }

    // Strict placement tree cross-validated by a second implementation:
    // reference-BFS expansion + reference leaf counts vs the smear kernel
    // tree (`perft` = generate_placements interiors + count_smear leaves).
    // Pins the semantics change from engine-emission trees (with phantoms)
    // to strict trees.
    #[test]
    fn perft_strict_matches_reference_bfs_shallow() {
        let b = Board::new();
        for depth in 1..=4 {
            assert_eq!(
                reference_perft(&b, 0, depth),
                crate::perft::perft(&b, 0, depth),
                "depth={depth}"
            );
        }
    }

    #[test]
    #[ignore]
    fn perft_strict_matches_reference_bfs_d5() {
        let b = Board::new();
        assert_eq!(reference_perft(&b, 0, 5), crate::perft::perft(&b, 0, 5));
    }
}
