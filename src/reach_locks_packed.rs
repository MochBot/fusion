use crate::board::Board;
use crate::default_ruleset::ACTIVE_RULES;
use crate::gen::*;
use crate::header::*;
use crate::pathfinder::ReachLocks;

#[allow(dead_code)]
pub(crate) fn try_reachable_locks_packed(
    board: &Board,
    piece: Piece,
    force: bool,
) -> Option<ReachLocks> {
    if piece != Piece::O {
        return None;
    }

    let cols = board.compute_cols();
    let cm = CollisionMap::new(&cols, piece);
    let is_t = piece == Piece::T && ACTIVE_RULES.enable_tspin;
    let is_allspin = piece != Piece::T && piece != Piece::O && ACTIVE_RULES.enable_allspin;
    let can_spin = is_t || is_allspin;
    let spin_layers = if can_spin { SPIN_NB } else { 1 };
    let mut searched = [[[0u64; COL_NB]; ROTATION_NB]; SPIN_NB];
    let mut frontier = [[[0u64; COL_NB]; ROTATION_NB]; SPIN_NB];
    let locks_empty = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];

    let spawn_y = if force {
        let blocked = cm.get(SPAWN_COL, Rotation::North);
        let above_spawn = !bb_low(ACTIVE_RULES.spawn_row) & row_mask();
        let valid = !blocked & above_spawn & row_mask();
        if valid == 0 {
            return Some(ReachLocks::from_locks(piece, locks_empty));
        }
        ctz(valid) as i32
    } else {
        if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
            return Some(ReachLocks::from_locks(piece, locks_empty));
        }
        ACTIVE_RULES.spawn_row
    };

    searched[0][Rotation::North as usize][SPAWN_COL] |= bb(spawn_y);
    frontier[0][Rotation::North as usize][SPAWN_COL] |= bb(spawn_y);

    while frontier_has_bits(&frontier) {
        let mut next = [[[0u64; COL_NB]; ROTATION_NB]; SPIN_NB];

        for spin in 0..spin_layers {
            for rot_idx in 0..ROTATION_NB {
                let r = Rotation::from_u8(rot_idx as u8);
                let rc = canonical_r(piece, r);

                for x in 0..COL_NB {
                    let bits = frontier[spin][rot_idx][x];
                    if bits == 0 {
                        continue;
                    }

                    if x > 0 && in_bounds(piece, r, x as i32 - 1) {
                        let targets = bits
                            & !cm.get(x - 1, rc)
                            & !searched[SpinType::NoSpin as usize][rot_idx][x - 1]
                            & row_mask();
                        if targets != 0 {
                            searched[SpinType::NoSpin as usize][rot_idx][x - 1] |= targets;
                            next[SpinType::NoSpin as usize][rot_idx][x - 1] |= targets;
                        }
                    }

                    if x + 1 < COL_NB && in_bounds(piece, r, x as i32 + 1) {
                        let targets = bits
                            & !cm.get(x + 1, rc)
                            & !searched[SpinType::NoSpin as usize][rot_idx][x + 1]
                            & row_mask();
                        if targets != 0 {
                            searched[SpinType::NoSpin as usize][rot_idx][x + 1] |= targets;
                            next[SpinType::NoSpin as usize][rot_idx][x + 1] |= targets;
                        }
                    }

                    let targets = (bits >> 1)
                        & !cm.get(x, rc)
                        & !searched[SpinType::NoSpin as usize][rot_idx][x]
                        & row_mask();
                    if targets != 0 {
                        searched[SpinType::NoSpin as usize][rot_idx][x] |= targets;
                        next[SpinType::NoSpin as usize][rot_idx][x] |= targets;
                    }

                    if piece != Piece::O {
                        rotate_frontier_cells(
                            board,
                            &cm,
                            piece,
                            is_t,
                            is_allspin,
                            r,
                            x,
                            bits,
                            &mut searched,
                            &mut next,
                        );
                    }
                }
            }
        }

        frontier = next;
    }

    let mut locks = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];
    for spin in 0..spin_layers {
        for rot_idx in 0..ROTATION_NB {
            let r = Rotation::from_u8(rot_idx as u8);
            let rc = canonical_r(piece, r);
            let rc_idx = rc as usize;
            for x in 0..COL_NB {
                let mut bits = searched[spin][rot_idx][x];
                while bits != 0 {
                    let y = bits.trailing_zeros() as i32;
                    let mut drop_y = y;
                    while drop_y > 0 && (cm.get(x, rc) & bb(drop_y - 1)) == 0 {
                        drop_y -= 1;
                    }
                    let lock_spin = if can_spin && drop_y == y { spin } else { 0 };
                    locks[lock_spin][x][rc_idx] |= bb(drop_y);
                    bits &= bits - 1;
                }
            }
        }
    }

    Some(ReachLocks::from_locks(piece, locks))
}

#[inline(always)]
fn row_mask() -> u64 {
    (1u64 << ROW_NB) - 1
}

fn frontier_has_bits(frontier: &[[[u64; COL_NB]; ROTATION_NB]; SPIN_NB]) -> bool {
    frontier
        .iter()
        .flat_map(|rotations| rotations.iter())
        .flat_map(|cols| cols.iter())
        .any(|&bits| bits != 0)
}

fn rotate_frontier_cells(
    board: &Board,
    cm: &CollisionMap,
    piece: Piece,
    is_t: bool,
    is_allspin: bool,
    r: Rotation,
    x: usize,
    mut bits: u64,
    searched: &mut [[[u64; COL_NB]; ROTATION_NB]; SPIN_NB],
    next: &mut [[[u64; COL_NB]; ROTATION_NB]; SPIN_NB],
) {
    let dirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
    while bits != 0 {
        let y = bits.trailing_zeros() as i32;
        bits &= bits - 1;

        for d_idx in 0..dirs {
            let d = match d_idx {
                0 => Direction::Cw,
                1 => Direction::Ccw,
                _ => Direction::Flip,
            };
            let rt = rotate(d, r);
            let off = canonical_offset(piece, r) - canonical_offset(piece, rt);

            let mut kick_buf = [Coordinates::new(0, 0); 6];
            let kick_count = if d == Direction::Flip {
                let ki = kick_180_index(piece);
                let arr = &KICKS_180[ki][r as usize];
                let n = if !ACTIVE_RULES.srs_plus { 2 } else { arr.len() };
                kick_buf[..n].copy_from_slice(&arr[..n]);
                n
            } else {
                let ki = kick_index(piece, ACTIVE_RULES.srs_plus);
                let arr = &KICKS[ki][d as usize][r as usize];
                kick_buf[..arr.len()].copy_from_slice(arr);
                arr.len()
            };

            for (k, &kick) in kick_buf.iter().enumerate().take(kick_count) {
                let x1 = x as i32 + kick.x as i32 + off.x as i32;
                let y1 = y + kick.y as i32 + off.y as i32;
                if x1 < 0 || y1 < 0 || y1 >= ROW_NB as i32 || !in_bounds(piece, rt, x1) {
                    continue;
                }
                let x1u = x1 as usize;
                let rt_c = canonical_r(piece, rt);
                if cm.get(x1u, rt_c) & bb(y1) != 0 {
                    continue;
                }

                let spin = classify_rotation_spin(board, cm, piece, is_t, is_allspin, rt, x1u, y1, k);
                let spin_idx = if is_t || is_allspin { spin as usize } else { 0 };
                let rt_idx = rt as usize;
                if searched[spin_idx][rt_idx][x1u] & bb(y1) != 0 {
                    continue;
                }
                searched[spin_idx][rt_idx][x1u] |= bb(y1);
                next[spin_idx][rt_idx][x1u] |= bb(y1);
                break;
            }
        }
    }
}

fn classify_rotation_spin(
    board: &Board,
    cm: &CollisionMap,
    piece: Piece,
    is_t: bool,
    is_allspin: bool,
    rt: Rotation,
    x: usize,
    y: i32,
    kick_idx: usize,
) -> SpinType {
    if is_t {
        let tx = x as i32;
        let mut corners = 0u32;
        for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
            let cx = tx + dx;
            let cy = y + dy;
            if cx < 0 || cx >= COL_NB as i32 || cy < 0 || board.occupied(cx, cy) {
                corners += 1;
            }
        }
        if corners >= 3 {
            let face = match rt {
                Rotation::North => [(0i32, -1i32), (0, 1)],
                Rotation::East => [(-1, 0), (1, 0)],
                Rotation::South => [(0, -1), (0, 1)],
                Rotation::West => [(-1, 0), (1, 0)],
            };
            let mut face_filled = 0u32;
            for &(dx, dy) in &face {
                let fx = tx + dx;
                let fy = y + dy;
                if fx < 0 || fx >= COL_NB as i32 || fy < 0 || board.occupied(fx, fy) {
                    face_filled += 1;
                }
            }
            return if face_filled >= 2 || kick_idx >= 4 {
                SpinType::Full
            } else {
                SpinType::Mini
            };
        }
    } else if is_allspin {
        let rt_c = canonical_r(piece, rt);
        let blocked_left = x == 0 || cm.get(x - 1, rt_c) & bb(y) != 0;
        let blocked_right = x >= COL_NB - 1 || cm.get(x + 1, rt_c) & bb(y) != 0;
        let blocked_down = y <= 0 || cm.get(x, rt_c) & bb(y - 1) != 0;
        let blocked_up = y >= ROW_NB as i32 - 1 || cm.get(x, rt_c) & bb(y + 1) != 0;
        if blocked_left && blocked_right && blocked_down && blocked_up {
            return SpinType::Mini;
        }
    }
    SpinType::NoSpin
}

#[cfg(test)]
mod reference {
    use super::*;

    #[derive(Clone, Copy)]
    struct PackedState {
        r: Rotation,
        x: i8,
        y: i8,
        s: SpinType,
    }

    pub(crate) fn reachable_locks_reference(board: &Board, piece: Piece, force: bool) -> ReachLocks {
        let cols = board.compute_cols();
        let cm = CollisionMap::new(&cols, piece);
        let is_t = piece == Piece::T && ACTIVE_RULES.enable_tspin;
        let is_allspin = piece != Piece::T && piece != Piece::O && ACTIVE_RULES.enable_allspin;
        let can_spin = is_t || is_allspin;
        let mut searched = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];
        let mut locks = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];

        let spawn_y = if force {
            let blocked = cm.get(SPAWN_COL, Rotation::North);
            let above_spawn = !bb_low(ACTIVE_RULES.spawn_row);
            let valid = !blocked & above_spawn;
            if valid == 0 {
                return ReachLocks::from_locks(piece, locks);
            }
            ctz(valid) as i8
        } else {
            if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
                return ReachLocks::from_locks(piece, locks);
            }
            ACTIVE_RULES.spawn_row as i8
        };

        searched[0][SPAWN_COL][Rotation::North as usize] |= bb(spawn_y as i32);
        let mut queue = Vec::with_capacity(256);
        queue.push(PackedState {
            r: Rotation::North,
            x: SPAWN_COL as i8,
            y: spawn_y,
            s: SpinType::NoSpin,
        });

        let mut head = 0usize;
        while head < queue.len() {
            let m = queue[head];
            head += 1;
            let x = m.x as usize;
            let y = m.y;
            let r = m.r;
            let rc = canonical_r(piece, r);

            let mut drop_y = y;
            while drop_y > 0 && (cm.get(x, rc) & bb((drop_y - 1) as i32)) == 0 {
                drop_y -= 1;
            }
            let sc = if can_spin && drop_y == y {
                m.s as usize
            } else {
                SpinType::NoSpin as usize
            };
            locks[sc][x][rc as usize] |= bb(drop_y as i32);

            if piece != Piece::O {
                let dirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
                for d_idx in 0..dirs {
                    let d = match d_idx {
                        0 => Direction::Cw,
                        1 => Direction::Ccw,
                        _ => Direction::Flip,
                    };
                    let rt = rotate(d, r);
                    let off = canonical_offset(piece, r) - canonical_offset(piece, rt);

                    let mut kick_buf = [Coordinates::new(0, 0); 6];
                    let kick_count = if d == Direction::Flip {
                        let arr = &KICKS_180[kick_180_index(piece)][r as usize];
                        let n = if ACTIVE_RULES.srs_plus { arr.len() } else { 2 };
                        kick_buf[..n].copy_from_slice(&arr[..n]);
                        n
                    } else {
                        let arr = &KICKS[kick_index(piece, ACTIVE_RULES.srs_plus)][d as usize]
                            [r as usize];
                        kick_buf[..arr.len()].copy_from_slice(arr);
                        arr.len()
                    };

                    for (k, &kick) in kick_buf.iter().enumerate().take(kick_count) {
                        let x1 = m.x as i32 + kick.x as i32 + off.x as i32;
                        let y1 = y as i32 + kick.y as i32 + off.y as i32;
                        if x1 < 0 || y1 < 0 || y1 >= ROW_NB as i32 {
                            continue;
                        }
                        if !in_bounds(piece, rt, x1) {
                            continue;
                        }
                        let x1u = x1 as usize;
                        let rt_c = canonical_r(piece, rt);
                        if cm.get(x1u, rt_c) & bb(y1) != 0 {
                            continue;
                        }

                        let spin = classify_rotation_spin(board, &cm, piece, is_t, is_allspin, rt, x1u, y1, k);
                        let spin_idx = if can_spin { spin as usize } else { 0 };
                        let rt_c_idx = rt_c as usize;
                        if searched[spin_idx][x1u][rt_c_idx] & bb(y1) != 0 {
                            continue;
                        }
                        searched[spin_idx][x1u][rt_c_idx] |= bb(y1);
                        queue.push(PackedState {
                            r: rt,
                            x: x1 as i8,
                            y: y1 as i8,
                            s: spin,
                        });
                        break;
                    }
                }
            }

            for dx in [-1i8, 1i8] {
                let x1 = m.x as i32 + dx as i32;
                if x1 < 0 || !in_bounds(piece, r, x1) {
                    continue;
                }
                let x1u = x1 as usize;
                let rc_idx = rc as usize;
                if cm.get(x1u, rc) & bb(y as i32) != 0 {
                    continue;
                }
                if searched[0][x1u][rc_idx] & bb(y as i32) != 0 {
                    continue;
                }
                searched[0][x1u][rc_idx] |= bb(y as i32);
                queue.push(PackedState {
                    r,
                    x: x1 as i8,
                    y,
                    s: SpinType::NoSpin,
                });
            }

            let y1 = y - 1;
            if y1 >= 0 && cm.get(x, rc) & bb(y1 as i32) == 0 {
                let rc_idx = rc as usize;
                if searched[0][x][rc_idx] & bb(y1 as i32) == 0 {
                    searched[0][x][rc_idx] |= bb(y1 as i32);
                    queue.push(PackedState {
                        r,
                        x: m.x,
                        y: y1,
                        s: SpinType::NoSpin,
                    });
                }
            }
        }

        ReachLocks::from_locks(piece, locks)
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn reachable_locks_o_north_reference(board: &Board, piece: Piece, force: bool) -> Option<ReachLocks> {
    if piece != Piece::O {
        return None;
    }

    let cols = board.compute_cols();
    let cm = CollisionMap::new(&cols, piece);
    let mut locks = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];
    let mut searched = [0u64; COL_NB];

    let spawn_y = if force {
        let blocked = cm.get(SPAWN_COL, Rotation::North);
        let above_spawn = !bb_low(ACTIVE_RULES.spawn_row);
        let valid = !blocked & above_spawn;
        if valid == 0 {
            return Some(ReachLocks::from_locks(piece, locks));
        }
        ctz(valid) as i8
    } else {
        if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
            return Some(ReachLocks::from_locks(piece, locks));
        }
        ACTIVE_RULES.spawn_row as i8
    };

    searched[SPAWN_COL] |= bb(spawn_y as i32);
    let mut frontier = [0u64; COL_NB];
    frontier[SPAWN_COL] = bb(spawn_y as i32);

    while frontier.iter().any(|&bits| bits != 0) {
        let mut next = [0u64; COL_NB];

        for x in 0..COL_NB {
            let bits = frontier[x];
            if bits == 0 {
                continue;
            }

            if x > 0 && in_bounds(piece, Rotation::North, x as i32 - 1) {
                let targets = bits & !cm.get(x - 1, Rotation::North) & !searched[x - 1];
                searched[x - 1] |= targets;
                next[x - 1] |= targets;
            }
            if x + 1 < COL_NB && in_bounds(piece, Rotation::North, x as i32 + 1) {
                let targets = bits & !cm.get(x + 1, Rotation::North) & !searched[x + 1];
                searched[x + 1] |= targets;
                next[x + 1] |= targets;
            }

            let targets = (bits >> 1) & !cm.get(x, Rotation::North) & !searched[x];
            searched[x] |= targets;
            next[x] |= targets;
        }

        frontier = next;
    }

    for x in 0..COL_NB {
        let mut bits = searched[x];
        while bits != 0 {
            let y = bits.trailing_zeros() as i32;
            let mut drop_y = y;
            while drop_y > 0 && (cm.get(x, Rotation::North) & bb(drop_y - 1)) == 0 {
                drop_y -= 1;
            }
            locks[SpinType::NoSpin as usize][x][Rotation::North as usize] |= bb(drop_y);
            bits &= bits - 1;
        }
    }

    Some(ReachLocks::from_locks(piece, locks))
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

    fn assert_lock_cube_matches_pathfinder(board: &Board, piece: Piece, force: bool) {
        if let Some(got) = try_reachable_locks_packed(board, piece, force) {
            let expected = reference::reachable_locks_reference(board, piece, force);
            assert_eq!(got.locks, expected.locks, "piece={piece:?} force={force}");
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

    #[test]
    fn j_case_88_force_true_documents_order_sensitive_gap() {
        let board = board_from_rows(&[
            880, 622, 514, 910, 73, 665, 804, 298, 592, 692, 441, 442, 113, 433, 58, 490,
            417, 930, 765, 688, 477, 174, 407, 508, 39, 159, 646, 707, 126, 830, 200, 697,
        ]);
        let expected = reference::reachable_locks_reference(&board, Piece::J, true);
        assert_ne!(expected.locks[0][3][Rotation::South as usize] & bb(27), 0);
        assert!(try_reachable_locks_packed(&board, Piece::J, true).is_none());
    }

    #[test]
    fn fixtures_match_pathfinder_lock_cube() {
        let empty = Board::new();
        let holey = board_from_rows(&[0x03BF, 0x036F, 0x03BF, 0x03CF, 0x03C7, 0x0207]);
        let tall = board_from_rows(&[
            0x03FF, 0x03DF, 0x03EF, 0x03BF, 0x037F, 0x01FF, 0x03FE, 0x03FB, 0x03F7,
            0x03DF, 0x03EF, 0x03BF, 0x037F, 0x01FF, 0x03FE, 0x03FB, 0x03F7, 0x03DF,
            0x03EF, 0x03BF, 0x037F, 0x01FF,
        ]);
        let boards = [&empty, &holey, &tall];

        for &piece in &ALL_PIECES {
            for board in boards {
                for force in [false, true] {
                    assert_lock_cube_matches_pathfinder(board, piece, force);
                }
            }
        }
    }

    #[test]
    fn spawn_blocked_force_false_returns_empty_lock_cube() {
        for &piece in &ALL_PIECES {
            let board = spawn_blocked_board(piece);
            if let Some(got) = try_reachable_locks_packed(&board, piece, false) {
                let expected = crate::pathfinder::reachable_locks(&board, piece, false);
                assert_eq!(got.locks, expected.locks, "piece={piece:?}");
            }
        }
    }


    #[test]
    #[ignore]
    fn micro_timing_pathfinder_vs_current_packed_try_holey_200x7() {
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
                    let reach = crate::pathfinder::reachable_locks(black_box(board), black_box(piece), false);
                    black_box(reach.locks[0][SPAWN_COL][0]);
                    calls += 1;
                }
            }
        }
        let pathfinder_ns = start.elapsed().as_nanos() / calls as u128;

        let mut packed_some = 0usize;
        let start = Instant::now();
        for _ in 0..ITERS {
            for board in &boards {
                for &piece in &ALL_PIECES {
                    let reach = try_reachable_locks_packed(black_box(board), black_box(piece), false);
                    if let Some(reach) = reach {
                        packed_some += 1;
                        black_box(reach.locks[0][SPAWN_COL][0]);
                    }
                }
            }
        }
        let packed_ns = start.elapsed().as_nanos() / (ITERS * CASES * ALL_PIECES.len()) as u128;

        let start = Instant::now();
        for _ in 0..ITERS {
            for board in &boards {
                for &piece in &ALL_PIECES {
                    let reach = try_reachable_locks_packed(black_box(board), black_box(piece), false)
                        .unwrap_or_else(|| crate::pathfinder::reachable_locks(black_box(board), black_box(piece), false));
                    black_box(reach.locks[0][SPAWN_COL][0]);
                }
            }
        }
        let packed_or_fallback_ns = start.elapsed().as_nanos()
            / (ITERS * CASES * ALL_PIECES.len()) as u128;

        eprintln!(
            "reach_locks_packed micro timing: cases={CASES} pieces=7 iters={ITERS} pathfinder_ns_per_call={pathfinder_ns} try_only_ns_per_call={packed_ns} try_some={packed_some} packed_or_fallback_ns_per_call={packed_or_fallback_ns}"
        );
    }


    #[test]
    #[ignore]
    fn seeded_random_million_o_piece_parity_matches_pathfinder_release_proof() {
        const CASES: usize = 1_000_000;
        let mut state = 0xB17B_0A7D_1000_2026u64;
        for case in 0..CASES {
            let board = random_profile_board(&mut state, case);
            for force in [false, true] {
                let expected = reference::reachable_locks_reference(&board, Piece::O, force);
                let got = try_reachable_locks_packed(&board, Piece::O, force).unwrap();
                if got.locks != expected.locks {
                    let diff = first_lock_diff(&got.locks, &expected.locks).unwrap();
                    panic!(
                        "piece=O case={case} force={force} diff={diff:?} rows={:?}",
                        board.rows
                    );
                }
            }
        }
        eprintln!("reach_locks_packed 1M release proof: piece=O cases={CASES} force_cases={} status=pass", CASES * 2);
    }

    #[test]
    fn seeded_random_full_cube_parity_matches_pathfinder() {
        const CASES_PER_PIECE: usize = 100_000;
        let mut state = 0xB17B_0A7D_5EED_2026u64;
        let boards: Vec<Board> = (0..CASES_PER_PIECE)
            .map(|case| random_profile_board(&mut state, case))
            .collect();

        let mut packed_counts = [0usize; PIECE_NB];
        let mut gated_counts = [0usize; PIECE_NB];

        for &piece in &ALL_PIECES {
            for (case, board) in boards.iter().enumerate() {
                for force in [false, true] {
                    if let Some(got) = try_reachable_locks_packed(board, piece, force) {
                        let expected = reference::reachable_locks_reference(board, piece, force);
                        packed_counts[piece as usize] += 1;
                        if got.locks != expected.locks {
                            let diff = first_lock_diff(&got.locks, &expected.locks).unwrap();
                            panic!(
                                "piece={piece:?} case={case} force={force} diff={diff:?} rows={:?}",
                                board.rows
                            );
                        }
                    } else {
                        gated_counts[piece as usize] += 1;
                    }
                }
            }
            eprintln!(
                "reach_locks_packed parity: {piece:?} packed={} gated={}",
                packed_counts[piece as usize],
                gated_counts[piece as usize]
            );
        }

        for &piece in &[Piece::O] {
            assert_eq!(packed_counts[piece as usize], CASES_PER_PIECE * 2, "packed count for {piece:?}");
            assert_eq!(gated_counts[piece as usize], 0, "gated count for {piece:?}");
        }
        // Non-O rotation reachability is still order-sensitive. Known specimens:
        // I case=4 force=true; J case=852 force=true; L case=248 force=true; S case=1 force=false.
        for &piece in &[Piece::I, Piece::T, Piece::L, Piece::J, Piece::S, Piece::Z] {
            assert_eq!(packed_counts[piece as usize], 0, "packed count for gated {piece:?}");
            assert_eq!(gated_counts[piece as usize], CASES_PER_PIECE * 2, "gated count for {piece:?}");
        }
    }
}
