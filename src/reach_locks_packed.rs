//! Bit-parallel reachable-locks flood with strict first-valid-kick SRS.
//!
//! Under strict kick resolution every state's transitions are a deterministic
//! function of its physical position, so the reachable set is the least fixed
//! point of {spawn} closed under movement and rotation images. That makes the
//! reach order-independent and lets us compute it with per-column u64 y-bitsets
//! instead of the scalar BFS in `pathfinder::reachable_locks`.
//!
//! Spin labels are tracked as three layers mirroring the scalar searched sets:
//! movement images and spawn land in the no-spin layer; each rotation kick wave
//! classifies its arrivals (same rules as `generate()`'s do_rotate) and unions
//! them into the matching layer. Locks follow the scalar projection rule: a
//! resting state locks under its layer's label, a falling state locks as
//! no-spin at its landing cell.

use crate::board::Board;
use crate::default_ruleset::ACTIVE_RULES;
use crate::gen::*;
use crate::header::*;
use crate::pathfinder::ReachLocks;

type ColSet = [u64; COL_NB];

const LOW_ROWS: u64 = bb_low(ROW_NB as i32);

#[inline(always)]
fn sh(v: u64, dy: i32) -> u64 {
    if dy >= 0 {
        v << dy
    } else {
        v >> -dy
    }
}

#[inline(always)]
fn sh_back(v: u64, dy: i32) -> u64 {
    if dy >= 0 {
        v >> dy
    } else {
        v << -dy
    }
}

/// Spin-classification masks, one bit per candidate target cell. Encodes the
/// same predicates as `classify_rotation_spin` so a whole kick wave can be
/// labelled with a handful of mask operations.
struct SpinMasks {
    /// 4-direction immobility per nominal rotation and column.
    stuck: [ColSet; ROTATION_NB],
    /// T only: 3-corner rule satisfied (diagonal corners).
    spins3: ColSet,
    /// T only: both front corners filled, per nominal rotation.
    front: [ColSet; ROTATION_NB],
}

impl SpinMasks {
    fn new(p: Piece, cols: &[u64; COL_NB], cm: &CollisionMap, is_t: bool) -> Self {
        let mut stuck = [[0u64; COL_NB]; ROTATION_NB];
        for (r_idx, stuck_r) in stuck.iter_mut().enumerate() {
            let rc = canonical_r(p, Rotation::from_u8(r_idx as u8));
            for (x, slot) in stuck_r.iter_mut().enumerate() {
                let left = if x == 0 { !0u64 } else { cm.get(x - 1, rc) };
                let right = if x >= COL_NB - 1 { !0u64 } else { cm.get(x + 1, rc) };
                let here = cm.get(x, rc);
                let down = (here << 1) | 1;
                let up = here >> 1;
                *slot = left & right & down & up;
            }
        }

        let mut spins3 = [0u64; COL_NB];
        let mut front = [[0u64; COL_NB]; ROTATION_NB];
        if is_t {
            for x in 0..COL_NB {
                // Diagonal corners; off-board and floor count filled, above the
                // stack counts empty (cols carry no bits at or above ROW_NB).
                let nw = if x == 0 { !0u64 } else { cols[x - 1] >> 1 };
                let ne = if x >= COL_NB - 1 { !0u64 } else { cols[x + 1] >> 1 };
                let se = if x >= COL_NB - 1 { !0u64 } else { (cols[x + 1] << 1) | 1 };
                let sw = if x == 0 { !0u64 } else { (cols[x - 1] << 1) | 1 };
                spins3[x] = (nw & ne & (se | sw)) | (se & sw & (nw | ne));
                front[Rotation::North as usize][x] = nw & ne;
                front[Rotation::East as usize][x] = ne & se;
                front[Rotation::South as usize][x] = se & sw;
                front[Rotation::West as usize][x] = sw & nw;
            }
        }

        SpinMasks { stuck, spins3, front }
    }

    /// Splits a wave's arrival bits into (full, mini, nospin) following the
    /// scalar classifier: T uses 3-corner + front-corner + kick>=4 override
    /// with the immobility fallback; all-spin pieces label immobile cells Mini.
    #[inline]
    fn split(&self, is_t: bool, rt_idx: usize, x: usize, k: usize, t: u64) -> (u64, u64, u64) {
        if is_t {
            let tagged = self.spins3[x] | self.stuck[rt_idx][x];
            let full_mask = if k >= 4 {
                self.spins3[x]
            } else {
                self.spins3[x] & self.front[rt_idx][x]
            };
            let full = t & full_mask;
            let mini = t & tagged & !full_mask;
            (full, mini, t & !tagged)
        } else {
            let mini = t & self.stuck[rt_idx][x];
            (0, mini, t & !self.stuck[rt_idx][x])
        }
    }
}

/// Bit-parallel equivalent of `pathfinder::reachable_locks` (strict SRS).
/// Produces the identical lock cube; parity is pinned against the scalar
/// oracle over seeded corpora in the tests below.
pub(crate) fn reachable_locks_packed(board: &Board, p: Piece, force: bool) -> ReachLocks {
    let cols = board.compute_cols();
    let cm = CollisionMap::new(&cols, p);
    let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
    let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
    let can_spin = is_t || is_allspin;

    let empty = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];

    let spawn_y = if force {
        let blocked = cm.get(SPAWN_COL, Rotation::North);
        let valid = !blocked & !bb_low(ACTIVE_RULES.spawn_row);
        if valid == 0 {
            return ReachLocks::from_locks(p, empty);
        }
        ctz(valid) as i32
    } else {
        if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
            return ReachLocks::from_locks(p, empty);
        }
        ACTIVE_RULES.spawn_row
    };

    // Physical reach per nominal rotation, plus per-label layers. Movement and
    // rotation transitions ignore the source label, so the physical union
    // drives expansion while the layers record how each cell was labelled.
    // `pending` holds physical states not yet rotation-expanded: transitions
    // are deterministic per state, so each state needs exactly one expansion.
    let mut reach = [[0u64; COL_NB]; ROTATION_NB];
    let mut layers = [[[0u64; COL_NB]; ROTATION_NB]; SPIN_NB];
    let mut pending = [[0u64; COL_NB]; ROTATION_NB];
    let mut move_seen = [[0u64; COL_NB]; ROTATION_NB];

    let seed = bb(spawn_y);
    reach[Rotation::North as usize][SPAWN_COL] |= seed;
    layers[0][Rotation::North as usize][SPAWN_COL] |= seed;
    pending[Rotation::North as usize][SPAWN_COL] |= seed;

    let masks = if can_spin {
        Some(SpinMasks::new(p, &cols, &cm, is_t))
    } else {
        None
    };

    let mut col_ok = [[false; COL_NB]; ROTATION_NB];
    for (r_idx, row) in col_ok.iter_mut().enumerate() {
        let r = Rotation::from_u8(r_idx as u8);
        for (x, ok) in row.iter_mut().enumerate() {
            *ok = in_bounds(p, r, x as i32);
        }
    }

    let mut dirty = [true, false, false, false];

    loop {
        // Movement closure: gravity falls plus horizontal passes until stable.
        // Images always land in the no-spin layer, mirroring the scalar BFS.
        // Movement never changes rotation, so each rotation closes on its own;
        // only rotations that received new states since their last closure run.
        // Vertical falls are memoized per column: a column whose physical set
        // did not change since its last fall closure cannot produce new bits.
        for r_idx in 0..ROTATION_NB {
            if !dirty[r_idx] {
                continue;
            }
            dirty[r_idx] = false;
            let rc = canonical_r(p, Rotation::from_u8(r_idx as u8));
            loop {
                let mut moved = false;
                for x in 0..COL_NB {
                    let src = reach[r_idx][x];
                    if src == 0 || src == move_seen[r_idx][x] {
                        continue;
                    }
                    let free = !cm.get(x, rc);
                    let mut img = (src >> 1) & free;
                    loop {
                        let nxt = img | ((img >> 1) & free);
                        if nxt == img {
                            break;
                        }
                        img = nxt;
                    }
                    layers[0][r_idx][x] |= img;
                    let new_phys = img & !src;
                    if new_phys != 0 {
                        reach[r_idx][x] |= new_phys;
                        pending[r_idx][x] |= new_phys;
                        moved = true;
                    }
                    move_seen[r_idx][x] = reach[r_idx][x];
                }
                for x in 1..COL_NB {
                    if !col_ok[r_idx][x] {
                        continue;
                    }
                    let img = reach[r_idx][x - 1] & !cm.get(x, rc);
                    if img & !layers[0][r_idx][x] != 0 {
                        layers[0][r_idx][x] |= img;
                        let new_phys = img & !reach[r_idx][x];
                        if new_phys != 0 {
                            reach[r_idx][x] |= new_phys;
                            pending[r_idx][x] |= new_phys;
                            moved = true;
                        }
                    }
                }
                for x in (0..COL_NB - 1).rev() {
                    if !col_ok[r_idx][x] {
                        continue;
                    }
                    let img = reach[r_idx][x + 1] & !cm.get(x, rc);
                    if img & !layers[0][r_idx][x] != 0 {
                        layers[0][r_idx][x] |= img;
                        let new_phys = img & !reach[r_idx][x];
                        if new_phys != 0 {
                            reach[r_idx][x] |= new_phys;
                            pending[r_idx][x] |= new_phys;
                            moved = true;
                        }
                    }
                }
                if !moved {
                    break;
                }
            }
        }

        if p == Piece::O {
            break;
        }

        let mut frontier = [[0u64; COL_NB]; ROTATION_NB];
        std::mem::swap(&mut frontier, &mut pending);
        if frontier.iter().all(|cols| cols.iter().all(|&b| b == 0)) {
            break;
        }

        // Rotation kick waves. Each wave resolves the first non-colliding kick
        // for every remaining source in one mask step; resolved sources leave
        // the wave so later kicks only see sources the earlier kicks rejected.
        let dirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
        for r_idx in 0..ROTATION_NB {
            if frontier[r_idx].iter().all(|&b| b == 0) {
                continue;
            }
            let r = Rotation::from_u8(r_idx as u8);
            for d_idx in 0..dirs {
                let d = match d_idx {
                    0 => Direction::Cw,
                    1 => Direction::Ccw,
                    _ => Direction::Flip,
                };
                let rt = rotate(d, r);
                let rt_idx = rt as usize;
                let rt_c = canonical_r(p, rt);
                let off = canonical_offset(p, r) - canonical_offset(p, rt);

                let kicks: &[Coordinates] = if d == Direction::Flip {
                    let arr = &KICKS_180[kick_180_index(p)][r_idx];
                    let n = if ACTIVE_RULES.srs_plus { arr.len() } else { 2 };
                    &arr[..n]
                } else {
                    &KICKS[kick_index(p, ACTIVE_RULES.srs_plus)][d as usize][r_idx]
                };

                let mut remaining = frontier[r_idx];
                for (k, kick) in kicks.iter().enumerate() {
                    let dx = kick.x as i32 + off.x as i32;
                    let dy = kick.y as i32 + off.y as i32;
                    for (x, remaining_x) in remaining.iter_mut().enumerate().take(COL_NB) {
                        let src = *remaining_x;
                        if src == 0 {
                            continue;
                        }
                        let x1 = x as i32 + dx;
                        if x1 < 0 || x1 >= COL_NB as i32 || !col_ok[rt_idx][x1 as usize] {
                            continue;
                        }
                        let x1u = x1 as usize;
                        let t = sh(src, dy) & !cm.get(x1u, rt_c) & LOW_ROWS;
                        if t == 0 {
                            continue;
                        }
                        *remaining_x &= !sh_back(t, dy);

                        let (full, mini, nospin) = match &masks {
                            Some(m) => m.split(is_t, rt_idx, x1u, k, t),
                            None => (0, 0, t),
                        };
                        layers[0][rt_idx][x1u] |= nospin;
                        layers[1][rt_idx][x1u] |= mini;
                        layers[2][rt_idx][x1u] |= full;
                        let new_phys = t & !reach[rt_idx][x1u];
                        if new_phys != 0 {
                            reach[rt_idx][x1u] |= new_phys;
                            pending[rt_idx][x1u] |= new_phys;
                            dirty[rt_idx] = true;
                        }
                    }
                }
            }
        }
    }

    // Lock projection: resting states keep their layer's label; falling states
    // land as no-spin. Locks are recorded under the canonical rotation, same
    // as the scalar.
    let mut locks = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];
    for r_idx in 0..ROTATION_NB {
        let rc = canonical_r(p, Rotation::from_u8(r_idx as u8));
        let rc_i = rc as usize;
        for x in 0..COL_NB {
            let pset = reach[r_idx][x];
            if pset == 0 {
                continue;
            }
            let here = cm.get(x, rc);
            let free = !here;
            let grounded = (here << 1) | 1;
            let mut fall = pset & !grounded;
            loop {
                let nxt = fall | ((fall >> 1) & free);
                if nxt == fall {
                    break;
                }
                fall = nxt;
            }
            locks[0][x][rc_i] |= (fall & grounded) | (layers[0][r_idx][x] & grounded);
            if can_spin {
                locks[1][x][rc_i] |= layers[1][r_idx][x] & grounded;
                locks[2][x][rc_i] |= layers[2][r_idx][x] & grounded;
            }
        }
    }

    ReachLocks::from_locks(p, locks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::BOARD_HEIGHT;
    use crate::default_ruleset::ACTIVE_RULES;
    use crate::gen::{CollisionMap, SPAWN_COL};

    fn board_from_rows(rows: &[u16]) -> Board {
        let mut board = Board::new();
        for (y, &row) in rows.iter().enumerate().take(BOARD_HEIGHT) {
            let mut bits = (row & 0x03FF) as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                board.rows[y] |= 1u16 << x;
                board.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        board
    }

    fn spawn_blocked_board(piece: Piece) -> Board {
        let mut board = Board::new();
        let mut row = ACTIVE_RULES.spawn_row as usize;
        loop {
            board.rows[row] = 0x03FF;
            for x in 0..COL_NB {
                board.cols[x] |= 1u64 << row;
            }
            let cm = CollisionMap::new(&board.compute_cols(), piece);
            if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
                return board;
            }
            row += 1;
            assert!(row < BOARD_HEIGHT, "could not block spawn for {piece:?}");
        }
    }

    fn xs(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn random_profile_board(state: &mut u64, case: usize) -> Board {
        let height = match case % 5 {
            0 => (xs(state) % 6) as usize,
            1 => 6 + (xs(state) % 8) as usize,
            2 => 14 + (xs(state) % 10) as usize,
            3 => 24 + (xs(state) % 12) as usize,
            _ => ACTIVE_RULES.spawn_row as usize + 1 + (xs(state) % 8) as usize,
        }
        .min(BOARD_HEIGHT);

        let mut rows = vec![0u16; height];
        for (y, row) in rows.iter_mut().enumerate() {
            let r = xs(state);
            let density_mask = match case % 5 {
                0 => 0x003F,
                1 => 0x00FF,
                2 => 0x01FF,
                3 => 0x03FF,
                _ => 0x03FF,
            };
            *row = (r as u16) & density_mask & 0x03FF;
            if y < height.saturating_sub(2) && *row == 0 {
                *row = 1u16 << (xs(state) % COL_NB as u64);
            }
            if case % 7 == 0 {
                let well = (xs(state) % COL_NB as u64) as u16;
                *row &= !(1u16 << well);
            }
        }
        board_from_rows(&rows)
    }

    fn first_lock_diff(
        left: &[[[u64; ROTATION_NB]; COL_NB]; SPIN_NB],
        right: &[[[u64; ROTATION_NB]; COL_NB]; SPIN_NB],
    ) -> Option<(usize, usize, usize, u64, u64)> {
        for spin in 0..SPIN_NB {
            for x in 0..COL_NB {
                for rot in 0..ROTATION_NB {
                    if left[spin][x][rot] != right[spin][x][rot] {
                        return Some((spin, x, rot, left[spin][x][rot], right[spin][x][rot]));
                    }
                }
            }
        }
        None
    }

    fn assert_cube_matches_scalar(board: &Board, piece: Piece, force: bool, tag: &str) {
        let got = reachable_locks_packed(board, piece, force);
        let expected = crate::pathfinder::reachable_locks(board, piece, force);
        if got.locks != expected.locks {
            let diff = first_lock_diff(&got.locks, &expected.locks).unwrap();
            panic!(
                "{tag} piece={piece:?} force={force} diff(spin,x,rot,got,want)={diff:?} rows={:?}",
                board.rows
            );
        }
    }

    #[test]
    fn fixtures_match_strict_scalar_lock_cube() {
        let empty = Board::new();
        let holey = board_from_rows(&[0x03BF, 0x036F, 0x03BF, 0x03CF, 0x03C7, 0x0207]);
        let tall = board_from_rows(&[
            0x03FF, 0x03DF, 0x03EF, 0x03BF, 0x037F, 0x01FF, 0x03FE, 0x03FB, 0x03F7,
            0x03DF, 0x03EF, 0x03BF, 0x037F, 0x01FF, 0x03FE, 0x03FB, 0x03F7, 0x03DF,
            0x03EF, 0x03BF, 0x037F, 0x01FF,
        ]);

        for &piece in &ALL_PIECES {
            for board in [&empty, &holey, &tall] {
                for force in [false, true] {
                    assert_cube_matches_scalar(board, piece, force, "fixture");
                }
            }
        }
    }

    #[test]
    fn spawn_blocked_force_false_returns_empty_lock_cube() {
        for &piece in &ALL_PIECES {
            let board = spawn_blocked_board(piece);
            assert_cube_matches_scalar(&board, piece, false, "spawn_blocked");
        }
    }

    #[test]
    fn seeded_random_full_cube_parity_matches_strict_scalar() {
        const CASES: usize = 2_000;
        let mut state = 0xB17B_0A7D_5EED_2026u64;
        let boards: Vec<Board> = (0..CASES)
            .map(|case| random_profile_board(&mut state, case))
            .collect();

        for &piece in &ALL_PIECES {
            for (case, board) in boards.iter().enumerate() {
                for force in [false, true] {
                    let tag = format!("case={case}");
                    assert_cube_matches_scalar(board, piece, force, &tag);
                }
            }
        }
    }

    // Release-mode proof over a large corpus; takes minutes. Run with:
    // cargo test --release --lib reach_locks_packed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn seeded_random_million_full_cube_parity_release_proof() {
        const CASES: usize = 1_000_000;
        for &piece in &ALL_PIECES {
            let mut state = 0xB17B_0A7D_1000_2026u64 ^ (piece as u64) << 32;
            for case in 0..CASES {
                let board = random_profile_board(&mut state, case);
                for force in [false, true] {
                    let tag = format!("release case={case}");
                    assert_cube_matches_scalar(&board, piece, force, &tag);
                }
            }
            eprintln!("reach_locks_packed 1M release proof: piece={piece:?} status=pass");
        }
    }

    #[test]
    #[ignore]
    fn micro_timing_scalar_vs_packed_holey_200x7() {
        use std::hint::black_box;
        use std::time::Instant;

        const CASES: usize = 200;
        const ITERS: usize = 20;
        let mut state = 0xCAFE_BABE_2026_0611u64;
        let boards: Vec<Board> = (0..CASES)
            .map(|case| random_profile_board(&mut state, case + 2))
            .collect();

        let mut calls = 0usize;
        let start = Instant::now();
        for _ in 0..ITERS {
            for board in &boards {
                for &piece in &ALL_PIECES {
                    let reach =
                        crate::pathfinder::reachable_locks(black_box(board), black_box(piece), false);
                    black_box(reach.locks[0][SPAWN_COL][0]);
                    calls += 1;
                }
            }
        }
        let scalar_ns = start.elapsed().as_nanos() / calls as u128;

        let start = Instant::now();
        for _ in 0..ITERS {
            for board in &boards {
                for &piece in &ALL_PIECES {
                    let reach = reachable_locks_packed(black_box(board), black_box(piece), false);
                    black_box(reach.locks[0][SPAWN_COL][0]);
                }
            }
        }
        let packed_ns = start.elapsed().as_nanos() / calls as u128;

        eprintln!(
            "reach_locks_packed micro timing: cases={CASES} pieces=7 iters={ITERS} scalar_ns_per_call={scalar_ns} packed_ns_per_call={packed_ns}"
        );
    }
}
