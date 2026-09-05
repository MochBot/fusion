// pathfinder.rs -- 1:1 port of pathfinder.hpp + pathfinder.cpp
#![allow(dead_code)]
#![allow(clippy::enum_variant_names)] // NoInput variant triggers this

use std::collections::VecDeque;

use crate::board::Board;
use crate::default_ruleset::ACTIVE_RULES;
use crate::gen::*;
use crate::header::*;

// -- Input --

pub(crate) const MAX_INPUTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Input {
    NoInput = 0,
    ShiftLeft,
    ShiftRight,
    DasLeft,
    DasRight,
    RotateCw,
    RotateCcw,
    RotateFlip,
    SoftDrop,
    HardDrop,
}

// -- Inputs --

#[derive(Clone, Debug)]
pub(crate) struct Inputs {
    pub(crate) data: Vec<Input>,
}

impl Inputs {
    pub(crate) fn new() -> Self {
        Inputs { data: Vec::new() }
    }

    pub(crate) fn push(&mut self, input: Input) {
        self.data.push(input);
    }

    pub(crate) fn reverse(&mut self) {
        self.data.reverse();
    }

    pub(crate) fn size(&self) -> usize {
        self.data.len()
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::new()
    }
}

// -- PathNode --

struct PathNode {
    input: Input,
    prev: u16,
}

// -- GhostMove --

#[derive(Clone, Copy)]
struct GhostMove {
    r: Rotation,
    x: i8,
    y: i8,
    i: u16,
    s: SpinType,
}

impl GhostMove {
    fn root_index() -> u16 {
        u16::MAX
    }
}

// -- rotation spin classification --
//
// Mirrors the per-kick-wave labeling in generate() (movegen.rs do_rotate):
// T uses the diagonal-corner 3-corner rule with the front-corner Full/Mini
// split, the kick-index>=4 Full override, and the immobility fallback; other
// spin-eligible pieces label immobile arrivals as Mini.
/// All four unit shifts collide (canonical frame).
fn immobile_at(cm: &CollisionMap, x1: i32, y1: i32, rc: Rotation) -> bool {
    let x1u = x1 as usize;
    let blocked_left = x1u == 0 || cm.get(x1u - 1, rc) & bb(y1) != 0;
    let blocked_right = x1u >= COL_NB - 1 || cm.get(x1u + 1, rc) & bb(y1) != 0;
    let blocked_down = y1 == 0 || cm.get(x1u, rc) & bb(y1 - 1) != 0;
    let blocked_up = cm.get(x1u, rc) & bb(y1 + 1) != 0;
    blocked_left && blocked_right && blocked_down && blocked_up
}

#[allow(clippy::too_many_arguments)]
fn classify_rotation_spin(
    board: &Board,
    cm: &CollisionMap,
    is_t: bool,
    is_allspin: bool,
    x1: i32,
    y1: i32,
    rt: Rotation,
    rt_c: Rotation,
    kick_idx: usize,
) -> SpinType {
    let stuck = immobile_at(cm, x1, y1, rt_c);
    let (has_three_corners, has_front_corners) = if is_t {
        let corner = |dx: i32, dy: i32| -> bool {
            let cx = x1 + dx;
            let cy = y1 + dy;
            cx < 0 || cx >= COL_NB as i32 || cy < 0 || board.occupied(cx, cy)
        };
        let nw = corner(-1, 1);
        let ne = corner(1, 1);
        let se = corner(1, -1);
        let sw = corner(-1, -1);
        let has_three_corners = (nw && ne && (se || sw)) || (se && sw && (nw || ne));
        let has_front_corners = match rt {
            Rotation::North => nw && ne,
            Rotation::East => ne && se,
            Rotation::South => se && sw,
            Rotation::West => sw && nw,
        };
        (has_three_corners, has_front_corners)
    } else {
        (false, false)
    };
    rotation_spin_type(
        is_t,
        is_allspin,
        has_three_corners,
        has_front_corners,
        stuck,
        kick_idx,
    )
}

// -- get_input --

pub(crate) fn get_input(board: &Board, target: &Move, use_finesse: bool, force: bool) -> Inputs {
    get_input_inner(board, target, use_finesse, force, target.piece())
}

fn get_input_inner(
    board: &Board,
    target: &Move,
    use_finesse: bool,
    force: bool,
    p: Piece,
) -> Inputs {
    let cols = board.compute_cols();
    let cm = CollisionMap::new(&cols, p);
    let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
    let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
    let can_spin = is_t || is_allspin;
    let spin_nb = if can_spin { SPIN_NB } else { 1 };

    // searched[spin][col][rot] bitboard
    let mut searched = vec![vec![vec![0u64; ROTATION_NB]; COL_NB]; spin_nb];

    let mut vec: Vec<PathNode> = Vec::new();
    let mut queue: VecDeque<GhostMove> = VecDeque::new();

    // Strict emission dedup: non-T NoSpin yields to Mini (NoSpin &= !Mini).
    // A later stuck rotation arrival claims the cell as Mini, invalidating
    let defer_nospin = is_allspin && target.spin() == SpinType::NoSpin;
    let mut deferred: Option<Inputs> = None;
    let mut mini_locks = [[0u64; ROTATION_NB]; COL_NB];

    // spawn
    let spawn_y = if force {
        // find lowest valid row >= spawn_row
        let blocked = cm.get(SPAWN_COL, Rotation::North);
        let above_spawn = !bb_low(ACTIVE_RULES.spawn_row);
        let valid = !blocked & above_spawn;
        if valid == 0 {
            return Inputs::new();
        }
        ctz(valid) as i8
    } else {
        if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
            return Inputs::new();
        }
        ACTIVE_RULES.spawn_row as i8
    };

    searched[0][SPAWN_COL][Rotation::North as usize] |= bb(spawn_y as i32);
    queue.push_back(GhostMove {
        r: Rotation::North,
        x: SPAWN_COL as i8,
        y: spawn_y,
        i: GhostMove::root_index(),
        s: SpinType::NoSpin,
    });

    while let Some(m) = queue.pop_front() {
        let x = m.x as usize;
        let r = m.r;
        let y = m.y;
        let rc = canonical_r(p, r);

        // harddrop: contiguous gravity descent. Steps one row at a time
        // so the piece cannot teleport into a disconnected pocket.
        let mut drop_y = y;
        while drop_y > 0 && (cm.get(x, rc) & bb((drop_y - 1) as i32)) == 0 {
            drop_y -= 1;
        }

        if drop_y >= 0 {
            // Lock label: only when piece is already resting (drop_y == y);
            // falling after rotation locks NoSpin (mirrors movegen).
            let sc = if can_spin && drop_y == y {
                m.s as usize
            } else {
                0
            };
            if is_allspin && sc == SpinType::Mini as usize {
                mini_locks[x][rc as usize] |= bb(drop_y as i32);
            }

            // check if this harddrop position == target
            let target_r = target.rotation();
            let target_rc = canonical_r(p, target_r);
            if x as i32 == target.x() && drop_y as i32 == target.y() && rc == target_rc {
                // check spin match
                let target_spin = target.spin();
                if !can_spin || sc == target_spin as usize {
                    // trace back path
                    let mut result = Inputs::new();
                    result.push(Input::HardDrop);
                    let mut idx = m.i;
                    while idx != GhostMove::root_index() {
                        result.push(vec[idx as usize].input);
                        idx = vec[idx as usize].prev;
                    }
                    result.reverse();
                    if defer_nospin {
                        if deferred.is_none() {
                            deferred = Some(result);
                        }
                    } else {
                        return result;
                    }
                }
            }
        }

        // T-piece: reset spin after harddrop check
        // (C++ resets queue.front().s but we popped it, so s is local)

        // rotate
        if p != Piece::O {
            let dirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
            for d_idx in 0..dirs {
                let d = match d_idx {
                    0 => Direction::Cw,
                    1 => Direction::Ccw,
                    _ => Direction::Flip,
                };
                let input = match d {
                    Direction::Cw => Input::RotateCw,
                    Direction::Ccw => Input::RotateCcw,
                    Direction::Flip => Input::RotateFlip,
                };

                let rt = rotate(d, r);
                let off = canonical_offset(p, r) - canonical_offset(p, rt);

                let mut kick_buf = [Coordinates::new(0, 0); 6];
                let kick_count = if d == Direction::Flip {
                    let ki = kick_180_index(p);
                    let arr = &KICKS_180[ki][r as usize];
                    let n = if !ACTIVE_RULES.srs_plus { 2 } else { arr.len() };
                    kick_buf[..n].copy_from_slice(&arr[..n]);
                    n
                } else {
                    let ki = kick_index(p, ACTIVE_RULES.srs_plus);
                    let arr = &KICKS[ki][d as usize][r as usize];
                    kick_buf[..arr.len()].copy_from_slice(arr);
                    arr.len()
                };

                let rt_c = canonical_r(p, rt);
                for (k, &kick) in kick_buf.iter().enumerate().take(kick_count) {
                    let x1 = m.x as i32 + kick.x as i32 + off.x as i32;
                    let y1 = y as i32 + kick.y as i32 + off.y as i32;

                    if x1 < 0 || y1 < 0 {
                        continue;
                    }
                    let x1u = x1 as usize;
                    // State coords are canonical-frame; column check must use
                    // canonical rotation cells, not true-rotation table.
                    if !in_bounds(p, rt_c, x1) {
                        continue;
                    }
                    if y1 >= ROW_NB as i32 {
                        continue;
                    }

                    if cm.get(x1u, rt_c) & bb(y1) != 0 {
                        continue;
                    }

                    // First non-colliding kick resolves the rotation (SRS);
                    // later kicks exist only for sources this kick rejected.
                    let s = if can_spin {
                        classify_rotation_spin(board, &cm, is_t, is_allspin, x1, y1, rt, rt_c, k)
                    } else {
                        SpinType::NoSpin
                    };

                    let s_idx = if can_spin { s as usize } else { 0 };
                    let rt_idx = rt as usize;

                    if searched[s_idx][x1u][rt_idx] & bb(y1) == 0 {
                        searched[s_idx][x1u][rt_idx] |= bb(y1);
                        let node_idx = vec.len() as u16;
                        vec.push(PathNode { input, prev: m.i });
                        queue.push_back(GhostMove {
                            r: rt,
                            x: x1 as i8,
                            y: y1 as i8,
                            i: node_idx,
                            s,
                        });
                    }
                    break;
                }
            }
        }

        // shift
        for dx in [-1i8, 1i8] {
            let x1 = m.x as i32 + dx as i32;
            if x1 < 0 {
                continue;
            }
            let x1u = x1 as usize;
            let rc = canonical_r(p, r);
            if !in_bounds(p, rc, x1) {
                continue;
            }
            if cm.get(x1u, rc) & bb(y as i32) != 0 {
                continue;
            }

            let s_idx = if can_spin {
                SpinType::NoSpin as usize
            } else {
                0
            };
            let r_idx = r as usize;

            if searched[s_idx][x1u][r_idx] & bb(y as i32) != 0 {
                continue;
            }
            searched[s_idx][x1u][r_idx] |= bb(y as i32);

            let input = if dx < 0 {
                Input::ShiftLeft
            } else {
                Input::ShiftRight
            };
            let node_idx = vec.len() as u16;
            vec.push(PathNode { input, prev: m.i });
            queue.push_back(GhostMove {
                r,
                x: x1 as i8,
                y,
                i: node_idx,
                s: SpinType::NoSpin,
            });
        }

        // DAS (finesse)
        if use_finesse {
            for dx in [-1i8, 1i8] {
                let mut x1 = m.x as i32 + dx as i32;
                let rc = canonical_r(p, r);
                // slide to wall
                loop {
                    if x1 < 0 || !in_bounds(p, rc, x1) {
                        break;
                    }
                    if cm.get(x1 as usize, rc) & bb(y as i32) != 0 {
                        break;
                    }
                    x1 += dx as i32;
                }
                x1 -= dx as i32; // back to last valid

                if x1 == m.x as i32 {
                    continue;
                }
                let x1u = x1 as usize;
                let s_idx = if can_spin {
                    SpinType::NoSpin as usize
                } else {
                    0
                };
                let r_idx = r as usize;

                if searched[s_idx][x1u][r_idx] & bb(y as i32) != 0 {
                    continue;
                }
                searched[s_idx][x1u][r_idx] |= bb(y as i32);

                let input = if dx < 0 {
                    Input::DasLeft
                } else {
                    Input::DasRight
                };
                let node_idx = vec.len() as u16;
                vec.push(PathNode { input, prev: m.i });
                queue.push_back(GhostMove {
                    r,
                    x: x1 as i8,
                    y,
                    i: node_idx,
                    s: SpinType::NoSpin,
                });
            }
        }

        // softdrop
        let y1 = y - 1;
        if y1 >= 0 {
            let rc = canonical_r(p, r);
            if cm.get(x, rc) & bb(y1 as i32) == 0 {
                let s_idx = if can_spin {
                    SpinType::NoSpin as usize
                } else {
                    0
                };
                let r_idx = r as usize;
                if searched[s_idx][x][r_idx] & bb(y1 as i32) == 0 {
                    searched[s_idx][x][r_idx] |= bb(y1 as i32);
                    let node_idx = vec.len() as u16;
                    vec.push(PathNode {
                        input: Input::SoftDrop,
                        prev: m.i,
                    });
                    queue.push_back(GhostMove {
                        r,
                        x: m.x,
                        y: y1,
                        i: node_idx,
                        s: SpinType::NoSpin,
                    });
                }
            }
        }
    }

    if let Some(path) = deferred {
        let xu = target.x() as usize;
        let rc = canonical_r(p, target.rotation()) as usize;
        if mini_locks[xu][rc] & bb(target.y()) == 0 {
            return path;
        }
    }

    // target not found (or the NoSpin cell is Mini-claimed by strict dedup)
    Inputs::new()
}

pub(crate) struct ReachLocks {
    pub(crate) piece: Piece,
    pub(crate) locks: [[[u64; ROTATION_NB]; COL_NB]; SPIN_NB],
}

impl ReachLocks {
    #[inline]
    pub(crate) fn from_locks(piece: Piece, locks: [[[u64; ROTATION_NB]; COL_NB]; SPIN_NB]) -> Self {
        Self { piece, locks }
    }

    /// True when any label stratum locks at the cell.
    #[inline]
    pub(crate) fn move_reachable(&self, m: &Move) -> bool {
        let x = m.x();
        let y = m.y();
        if x < 0 || x >= COL_NB as i32 || y < 0 {
            return false;
        }
        let xu = x as usize;
        let rc = canonical_r(self.piece, m.rotation()) as usize;
        let phys = self.locks[0][xu][rc] | self.locks[1][xu][rc] | self.locks[2][xu][rc];
        phys & bb(y) != 0
    }
}

thread_local! {
    static RL_QUEUE: std::cell::RefCell<Vec<GhostMove>> =
        std::cell::RefCell::new(Vec::with_capacity(256));
}

pub(crate) fn reachable_locks(board: &Board, p: Piece, force: bool) -> ReachLocks {
    let cols = board.compute_cols();
    let cm = CollisionMap::new(&cols, p);
    let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
    let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
    let can_spin = is_t || is_allspin;

    RL_QUEUE.with(|qcell| {
        let mut queue = qcell.borrow_mut();
        queue.clear();
        let mut searched = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];
        let mut locks = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];

        let spawn_y = if force {
            let blocked = cm.get(SPAWN_COL, Rotation::North);
            let above_spawn = !bb_low(ACTIVE_RULES.spawn_row);
            let valid = !blocked & above_spawn;
            if valid == 0 {
                return ReachLocks { piece: p, locks };
            }
            ctz(valid) as i8
        } else {
            if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
                return ReachLocks { piece: p, locks };
            }
            ACTIVE_RULES.spawn_row as i8
        };

        searched[0][SPAWN_COL][Rotation::North as usize] |= bb(spawn_y as i32);
        queue.push(GhostMove {
            r: Rotation::North,
            x: SPAWN_COL as i8,
            y: spawn_y,
            i: GhostMove::root_index(),
            s: SpinType::NoSpin,
        });

        let mut head = 0usize;
        while head < queue.len() {
            let m = queue[head];
            head += 1;
            let x = m.x as usize;
            let r = m.r;
            let y = m.y;
            let rc = canonical_r(p, r);

            let mut drop_y = y;
            while drop_y > 0 && (cm.get(x, rc) & bb((drop_y - 1) as i32)) == 0 {
                drop_y -= 1;
            }
            if drop_y >= 0 {
                // Arrival-based lock label while resting (mirrors get_input).
                let sc = if can_spin && drop_y == y {
                    m.s as usize
                } else {
                    0
                };
                locks[sc][x][rc as usize] |= bb(drop_y as i32);
            }

            if p != Piece::O {
                let dirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
                for d_idx in 0..dirs {
                    let d = match d_idx {
                        0 => Direction::Cw,
                        1 => Direction::Ccw,
                        _ => Direction::Flip,
                    };

                    let rt = rotate(d, r);
                    let off = canonical_offset(p, r) - canonical_offset(p, rt);

                    let mut kick_buf = [Coordinates::new(0, 0); 6];
                    let kick_count = if d == Direction::Flip {
                        let ki = kick_180_index(p);
                        let arr = &KICKS_180[ki][r as usize];
                        let n = if !ACTIVE_RULES.srs_plus { 2 } else { arr.len() };
                        kick_buf[..n].copy_from_slice(&arr[..n]);
                        n
                    } else {
                        let ki = kick_index(p, ACTIVE_RULES.srs_plus);
                        let arr = &KICKS[ki][d as usize][r as usize];
                        kick_buf[..arr.len()].copy_from_slice(arr);
                        arr.len()
                    };

                    let rt_c = canonical_r(p, rt);
                    for (k, &kick) in kick_buf.iter().enumerate().take(kick_count) {
                        let x1 = m.x as i32 + kick.x as i32 + off.x as i32;
                        let y1 = y as i32 + kick.y as i32 + off.y as i32;

                        if x1 < 0 || y1 < 0 {
                            continue;
                        }
                        let x1u = x1 as usize;
                        // Canonical-frame pivot -> canonical-rotation column check
                        if !in_bounds(p, rt_c, x1) {
                            continue;
                        }
                        if y1 >= ROW_NB as i32 {
                            continue;
                        }

                        if cm.get(x1u, rt_c) & bb(y1) != 0 {
                            continue;
                        }

                        // First non-colliding kick resolves the rotation (SRS);
                        // later kicks exist only for sources this kick rejected.
                        let s = if can_spin {
                            classify_rotation_spin(
                                board, &cm, is_t, is_allspin, x1, y1, rt, rt_c, k,
                            )
                        } else {
                            SpinType::NoSpin
                        };

                        let s_idx = if can_spin { s as usize } else { 0 };
                        let rt_idx = rt as usize;

                        if searched[s_idx][x1u][rt_idx] & bb(y1) == 0 {
                            searched[s_idx][x1u][rt_idx] |= bb(y1);
                            queue.push(GhostMove {
                                r: rt,
                                x: x1 as i8,
                                y: y1 as i8,
                                i: GhostMove::root_index(),
                                s,
                            });
                        }
                        break;
                    }
                }
            }

            for dx in [-1i8, 1i8] {
                let x1 = m.x as i32 + dx as i32;
                if x1 < 0 {
                    continue;
                }
                let x1u = x1 as usize;
                let rc = canonical_r(p, r);
                if !in_bounds(p, rc, x1) {
                    continue;
                }
                if cm.get(x1u, rc) & bb(y as i32) != 0 {
                    continue;
                }

                let s_idx = if can_spin {
                    SpinType::NoSpin as usize
                } else {
                    0
                };
                let r_idx = r as usize;

                if searched[s_idx][x1u][r_idx] & bb(y as i32) != 0 {
                    continue;
                }
                searched[s_idx][x1u][r_idx] |= bb(y as i32);

                queue.push(GhostMove {
                    r,
                    x: x1 as i8,
                    y,
                    i: GhostMove::root_index(),
                    s: SpinType::NoSpin,
                });
            }

            let y1 = y - 1;
            if y1 >= 0 {
                let rc = canonical_r(p, r);
                if cm.get(x, rc) & bb(y1 as i32) == 0 {
                    let s_idx = if can_spin {
                        SpinType::NoSpin as usize
                    } else {
                        0
                    };
                    let r_idx = r as usize;
                    if searched[s_idx][x][r_idx] & bb(y1 as i32) == 0 {
                        searched[s_idx][x][r_idx] |= bb(y1 as i32);
                        queue.push(GhostMove {
                            r,
                            x: m.x,
                            y: y1,
                            i: GhostMove::root_index(),
                            s: SpinType::NoSpin,
                        });
                    }
                }
            }
        }

        if is_allspin {
            // Strict emission dedup: Mini wins the cell for non-T.
            let mini = locks[SpinType::Mini as usize];
            for (ns_col, mini_col) in locks[SpinType::NoSpin as usize].iter_mut().zip(mini.iter()) {
                for (b, m) in ns_col.iter_mut().zip(mini_col.iter()) {
                    *b &= !m;
                }
            }
        }

        ReachLocks { piece: p, locks }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_from_rows(rows: &[u16]) -> Board {
        let mut b = Board::new();
        for (y, &row) in rows.iter().enumerate().take(crate::board::BOARD_HEIGHT) {
            b.rows[y] = row & 0x03FF;
        }
        let rows = b.rows;
        b.clear();
        for y in 0..crate::board::BOARD_HEIGHT {
            let mut bits = rows[y] as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as i32;
                b.rows[y] |= 1u16 << x;
                b.cols[x as usize] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        b
    }

    // Frozen pre-strict semantics (later-kick phantom states, face-cell T
    // labels, no T immobility fallback). Kept verbatim for the
    // strict-vs-legacy diff harness; do not "fix" this copy.
    fn reachable_locks_legacy(
        board: &Board,
        p: Piece,
        force: bool,
    ) -> [[[u64; ROTATION_NB]; COL_NB]; SPIN_NB] {
        let cols = board.compute_cols();
        let cm = CollisionMap::new(&cols, p);
        let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
        let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
        let can_spin = is_t || is_allspin;
        let mut queue: VecDeque<GhostMove> = VecDeque::new();
        let mut searched = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];
        let mut locks = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];

        let spawn_y = if force {
            let blocked = cm.get(SPAWN_COL, Rotation::North);
            let above_spawn = !bb_low(ACTIVE_RULES.spawn_row);
            let valid = !blocked & above_spawn;
            if valid == 0 {
                return locks;
            }
            ctz(valid) as i8
        } else {
            if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
                return locks;
            }
            ACTIVE_RULES.spawn_row as i8
        };

        searched[0][SPAWN_COL][Rotation::North as usize] |= bb(spawn_y as i32);
        queue.push_back(GhostMove {
            r: Rotation::North,
            x: SPAWN_COL as i8,
            y: spawn_y,
            i: GhostMove::root_index(),
            s: SpinType::NoSpin,
        });

        while let Some(m) = queue.pop_front() {
            let x = m.x as usize;
            let r = m.r;
            let y = m.y;
            let rc = canonical_r(p, r);

            let mut drop_y = y;
            while drop_y > 0 && (cm.get(x, rc) & bb((drop_y - 1) as i32)) == 0 {
                drop_y -= 1;
            }
            if drop_y >= 0 {
                let sc = if can_spin && drop_y == y {
                    m.s as usize
                } else {
                    0
                };
                locks[sc][x][rc as usize] |= bb(drop_y as i32);
            }

            if p != Piece::O {
                let dirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
                for d_idx in 0..dirs {
                    let d = match d_idx {
                        0 => Direction::Cw,
                        1 => Direction::Ccw,
                        _ => Direction::Flip,
                    };

                    let rt = rotate(d, r);
                    let off = canonical_offset(p, r) - canonical_offset(p, rt);

                    let mut kick_buf = [Coordinates::new(0, 0); 6];
                    let kick_count = if d == Direction::Flip {
                        let ki = kick_180_index(p);
                        let arr = &KICKS_180[ki][r as usize];
                        let n = if !ACTIVE_RULES.srs_plus { 2 } else { arr.len() };
                        kick_buf[..n].copy_from_slice(&arr[..n]);
                        n
                    } else {
                        let ki = kick_index(p, ACTIVE_RULES.srs_plus);
                        let arr = &KICKS[ki][d as usize][r as usize];
                        kick_buf[..arr.len()].copy_from_slice(arr);
                        arr.len()
                    };

                    for (k, &kick) in kick_buf.iter().enumerate().take(kick_count) {
                        let x1 = m.x as i32 + kick.x as i32 + off.x as i32;
                        let y1 = y as i32 + kick.y as i32 + off.y as i32;

                        if x1 < 0 || y1 < 0 {
                            continue;
                        }
                        let x1u = x1 as usize;
                        if !in_bounds(p, rt, x1) || y1 >= ROW_NB as i32 {
                            continue;
                        }

                        let rt_c = canonical_r(p, rt);
                        if cm.get(x1u, rt_c) & bb(y1) != 0 {
                            continue;
                        }

                        let mut s = SpinType::NoSpin;
                        if is_t {
                            let mut corners = 0u32;
                            for &(dx, dy) in &[(-1i32, -1i32), (1, -1), (-1, 1), (1, 1)] {
                                let cx = x1 + dx;
                                let cy = y1 + dy;
                                if cx < 0 || cx >= COL_NB as i32 || cy < 0 || board.occupied(cx, cy)
                                {
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
                                    let fx = x1 + dx;
                                    let fy = y1 + dy;
                                    if fx < 0
                                        || fx >= COL_NB as i32
                                        || fy < 0
                                        || board.occupied(fx, fy)
                                    {
                                        face_filled += 1;
                                    }
                                }
                                s = if face_filled >= 2 || k >= 4 {
                                    SpinType::Full
                                } else {
                                    SpinType::Mini
                                };
                            }
                        } else if is_allspin {
                            let rt_c = canonical_r(p, rt);
                            let blocked_left = x1u == 0 || cm.get(x1u - 1, rt_c) & bb(y1) != 0;
                            let blocked_right =
                                x1u >= COL_NB - 1 || cm.get(x1u + 1, rt_c) & bb(y1) != 0;
                            let blocked_down = y1 <= 0 || cm.get(x1u, rt_c) & bb(y1 - 1) != 0;
                            let blocked_up =
                                y1 >= ROW_NB as i32 - 1 || cm.get(x1u, rt_c) & bb(y1 + 1) != 0;
                            if blocked_left && blocked_right && blocked_down && blocked_up {
                                s = SpinType::Mini;
                            }
                        }

                        let s_idx = if can_spin { s as usize } else { 0 };
                        let rt_c_idx = canonical_r(p, rt) as usize;

                        if searched[s_idx][x1u][rt_c_idx] & bb(y1) != 0 {
                            continue;
                        }
                        searched[s_idx][x1u][rt_c_idx] |= bb(y1);

                        queue.push_back(GhostMove {
                            r: rt,
                            x: x1 as i8,
                            y: y1 as i8,
                            i: GhostMove::root_index(),
                            s,
                        });
                        break;
                    }
                }
            }

            for dx in [-1i8, 1i8] {
                let x1 = m.x as i32 + dx as i32;
                if x1 < 0 {
                    continue;
                }
                let x1u = x1 as usize;
                if !in_bounds(p, r, x1) {
                    continue;
                }
                let rc = canonical_r(p, r);
                if cm.get(x1u, rc) & bb(y as i32) != 0 {
                    continue;
                }

                let s_idx = if can_spin {
                    SpinType::NoSpin as usize
                } else {
                    0
                };
                let rc_idx = canonical_r(p, r) as usize;

                if searched[s_idx][x1u][rc_idx] & bb(y as i32) != 0 {
                    continue;
                }
                searched[s_idx][x1u][rc_idx] |= bb(y as i32);

                queue.push_back(GhostMove {
                    r,
                    x: x1 as i8,
                    y,
                    i: GhostMove::root_index(),
                    s: SpinType::NoSpin,
                });
            }

            let y1 = y - 1;
            if y1 >= 0 {
                let rc = canonical_r(p, r);
                if cm.get(x, rc) & bb(y1 as i32) == 0 {
                    let s_idx = if can_spin {
                        SpinType::NoSpin as usize
                    } else {
                        0
                    };
                    let rc_idx = rc as usize;
                    if searched[s_idx][x][rc_idx] & bb(y1 as i32) == 0 {
                        searched[s_idx][x][rc_idx] |= bb(y1 as i32);
                        queue.push_back(GhostMove {
                            r,
                            x: m.x,
                            y: y1,
                            i: GhostMove::root_index(),
                            s: SpinType::NoSpin,
                        });
                    }
                }
            }
        }

        locks
    }

    fn xs(s: &mut u64) -> u64 {
        let mut x = *s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *s = x;
        x
    }

    fn reachable_locks_strict_reference(
        board: &Board,
        p: Piece,
        force: bool,
    ) -> [[[u64; ROTATION_NB]; COL_NB]; SPIN_NB] {
        let cols = board.compute_cols();
        let cm = CollisionMap::new(&cols, p);
        let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
        let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
        let can_spin = is_t || is_allspin;
        let mut queue: VecDeque<GhostMove> = VecDeque::new();
        let mut searched = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];
        let mut locks = [[[0u64; ROTATION_NB]; COL_NB]; SPIN_NB];

        let spawn_y = if force {
            let blocked = cm.get(SPAWN_COL, Rotation::North);
            let above_spawn = !bb_low(ACTIVE_RULES.spawn_row);
            let valid = !blocked & above_spawn;
            if valid == 0 {
                return locks;
            }
            ctz(valid) as i8
        } else {
            if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
                return locks;
            }
            ACTIVE_RULES.spawn_row as i8
        };

        searched[0][SPAWN_COL][Rotation::North as usize] |= bb(spawn_y as i32);
        queue.push_back(GhostMove {
            r: Rotation::North,
            x: SPAWN_COL as i8,
            y: spawn_y,
            i: GhostMove::root_index(),
            s: SpinType::NoSpin,
        });

        while let Some(m) = queue.pop_front() {
            let x = m.x as usize;
            let r = m.r;
            let y = m.y;
            let rc = canonical_r(p, r);

            let mut drop_y = y;
            while drop_y > 0 && (cm.get(x, rc) & bb((drop_y - 1) as i32)) == 0 {
                drop_y -= 1;
            }
            if drop_y >= 0 {
                let sc = if can_spin && drop_y == y {
                    m.s as usize
                } else {
                    0
                };
                locks[sc][x][rc as usize] |= bb(drop_y as i32);
            }

            if p != Piece::O {
                let dirs = if ACTIVE_RULES.enable_180 { 3 } else { 2 };
                for d_idx in 0..dirs {
                    let d = match d_idx {
                        0 => Direction::Cw,
                        1 => Direction::Ccw,
                        _ => Direction::Flip,
                    };

                    let rt = rotate(d, r);
                    let off = canonical_offset(p, r) - canonical_offset(p, rt);

                    let mut kick_buf = [Coordinates::new(0, 0); 6];
                    let kick_count = if d == Direction::Flip {
                        let ki = kick_180_index(p);
                        let arr = &KICKS_180[ki][r as usize];
                        let n = if !ACTIVE_RULES.srs_plus { 2 } else { arr.len() };
                        kick_buf[..n].copy_from_slice(&arr[..n]);
                        n
                    } else {
                        let ki = kick_index(p, ACTIVE_RULES.srs_plus);
                        let arr = &KICKS[ki][d as usize][r as usize];
                        kick_buf[..arr.len()].copy_from_slice(arr);
                        arr.len()
                    };

                    for (k, &kick) in kick_buf.iter().enumerate().take(kick_count) {
                        let x1 = m.x as i32 + kick.x as i32 + off.x as i32;
                        let y1 = y as i32 + kick.y as i32 + off.y as i32;

                        if x1 < 0 || y1 < 0 {
                            continue;
                        }
                        let x1u = x1 as usize;
                        let rt_c = canonical_r(p, rt);
                        if !in_bounds(p, rt_c, x1) || y1 >= ROW_NB as i32 {
                            continue;
                        }

                        if cm.get(x1u, rt_c) & bb(y1) != 0 {
                            continue;
                        }

                        let s = if can_spin {
                            classify_rotation_spin(
                                board, &cm, is_t, is_allspin, x1, y1, rt, rt_c, k,
                            )
                        } else {
                            SpinType::NoSpin
                        };

                        let s_idx = if can_spin { s as usize } else { 0 };
                        let rt_idx = rt as usize;

                        if searched[s_idx][x1u][rt_idx] & bb(y1) == 0 {
                            searched[s_idx][x1u][rt_idx] |= bb(y1);
                            queue.push_back(GhostMove {
                                r: rt,
                                x: x1 as i8,
                                y: y1 as i8,
                                i: GhostMove::root_index(),
                                s,
                            });
                        }
                        break;
                    }
                }
            }

            for dx in [-1i8, 1i8] {
                let x1 = m.x as i32 + dx as i32;
                if x1 < 0 {
                    continue;
                }
                let x1u = x1 as usize;
                let rc = canonical_r(p, r);
                if !in_bounds(p, rc, x1) {
                    continue;
                }
                if cm.get(x1u, rc) & bb(y as i32) != 0 {
                    continue;
                }

                let s_idx = if can_spin {
                    SpinType::NoSpin as usize
                } else {
                    0
                };
                let r_idx = r as usize;

                if searched[s_idx][x1u][r_idx] & bb(y as i32) != 0 {
                    continue;
                }
                searched[s_idx][x1u][r_idx] |= bb(y as i32);

                queue.push_back(GhostMove {
                    r,
                    x: x1 as i8,
                    y,
                    i: GhostMove::root_index(),
                    s: SpinType::NoSpin,
                });
            }

            let y1 = y - 1;
            if y1 >= 0 {
                let rc = canonical_r(p, r);
                if cm.get(x, rc) & bb(y1 as i32) == 0 {
                    let s_idx = if can_spin {
                        SpinType::NoSpin as usize
                    } else {
                        0
                    };
                    let r_idx = r as usize;
                    if searched[s_idx][x][r_idx] & bb(y1 as i32) == 0 {
                        searched[s_idx][x][r_idx] |= bb(y1 as i32);
                        queue.push_back(GhostMove {
                            r,
                            x: m.x,
                            y: y1,
                            i: GhostMove::root_index(),
                            s: SpinType::NoSpin,
                        });
                    }
                }
            }
        }

        if is_allspin {
            let mini = locks[SpinType::Mini as usize];
            for (ns_col, mini_col) in locks[SpinType::NoSpin as usize].iter_mut().zip(mini.iter()) {
                for (b, m) in ns_col.iter_mut().zip(mini_col.iter()) {
                    *b &= !m;
                }
            }
        }

        locks
    }

    const ALL_PIECES: [Piece; 7] = [
        Piece::I,
        Piece::O,
        Piece::T,
        Piece::L,
        Piece::J,
        Piece::S,
        Piece::Z,
    ];

    fn seeded_holey_rows(st: &mut u64) -> Option<Vec<u16>> {
        let h = 2 + (xs(st) % 21) as usize;
        let mut rows = vec![0u16; h];
        for r in rows.iter_mut() {
            *r = (xs(st) as u16) & 0x03FF;
        }
        if let Some(last) = rows.last_mut() {
            if *last == 0 {
                *last = 1u16 << (xs(st) % 10);
            }
        }
        let b = board_from_rows(&rows);
        if crate::movegen::needs_reachability_filter(&b) {
            Some(rows)
        } else {
            None
        }
    }

    #[test]
    fn strict_scalar_matches_strict_reference_on_seeded_boards() {
        let mut st = 0xA11C_E5E5_2026_0610u64;
        let mut boards = 0u64;
        while boards < 2_000 {
            let Some(rows) = seeded_holey_rows(&mut st) else {
                continue;
            };
            let b = board_from_rows(&rows);
            boards += 1;
            for &p in &ALL_PIECES {
                for force in [false, true] {
                    let got = reachable_locks(&b, p, force);
                    let want = reachable_locks_strict_reference(&b, p, force);
                    assert_eq!(got.locks, want, "p={p:?} force={force} rows={rows:?}");
                }
            }
        }
        assert_eq!(boards, 2_000);
    }

    #[test]
    #[ignore]
    fn reachable_locks_matches_strict_reference_on_100k_holey_boards() {
        let mut st = 0xA11C_E5E5_2026_0610u64;
        let mut boards = 0u64;
        while boards < 100_000 {
            let Some(rows) = seeded_holey_rows(&mut st) else {
                continue;
            };
            let b = board_from_rows(&rows);
            boards += 1;
            for &p in &ALL_PIECES {
                for force in [false, true] {
                    let got = reachable_locks(&b, p, force);
                    let want = reachable_locks_strict_reference(&b, p, force);
                    assert_eq!(got.locks, want, "p={p:?} force={force} rows={rows:?}");
                }
            }
        }
        assert_eq!(boards, 100_000);
    }

    #[test]
    fn legacy_semantics_diverge_from_strict_on_seeded_boards() {
        let mut st = 0xA11C_E5E5_2026_0610u64;
        let mut boards = 0u64;
        let mut diffs = 0u64;
        let mut first: Option<String> = None;
        while boards < 2_000 {
            let Some(rows) = seeded_holey_rows(&mut st) else {
                continue;
            };
            let b = board_from_rows(&rows);
            boards += 1;
            for &p in &ALL_PIECES {
                for force in [false, true] {
                    let legacy = reachable_locks_legacy(&b, p, force);
                    let strict = reachable_locks(&b, p, force).locks;
                    if legacy != strict {
                        diffs += 1;
                        if first.is_none() {
                            first = Some(format!("p={p:?} force={force} rows={rows:?}"));
                        }
                    }
                }
            }
        }
        assert!(
            diffs > 0,
            "expected the strict kick fix to change at least one cube on the corpus"
        );
        println!(
            "legacy-vs-strict cubes differing: {diffs} (first: {})",
            first.unwrap()
        );
    }

    #[derive(Default)]
    struct DiffCounts {
        cubes_compared: u64,
        cubes_differing: u64,
        phantom_removed: u64,
        label_loss: u64,
        label_gain: u64,
        reach_gain: u64,
        bug: u64,
        bug_examples: Vec<String>,
    }

    // Diff classification: every changed lock bit must be a phantom removal,
    // label loss whose physical placement survives, or label gain backed by
    // a strict input path; anything else is a regression.
    fn classify_diffs_on_corpus(n_boards: u64, seed: u64) -> DiffCounts {
        let mut st = seed;
        let mut counts = DiffCounts::default();
        let mut boards = 0u64;
        while boards < n_boards {
            let Some(rows) = seeded_holey_rows(&mut st) else {
                continue;
            };
            let b = board_from_rows(&rows);
            boards += 1;
            for &p in &ALL_PIECES {
                for force in [false, true] {
                    let legacy = reachable_locks_legacy(&b, p, force);
                    let strict = reachable_locks(&b, p, force).locks;
                    counts.cubes_compared += 1;
                    if legacy == strict {
                        continue;
                    }
                    counts.cubes_differing += 1;
                    for s_idx in 0..SPIN_NB {
                        for x in 0..COL_NB {
                            for ri in 0..ROTATION_NB {
                                let l = legacy[s_idx][x][ri];
                                let s = strict[s_idx][x][ri];
                                if l == s {
                                    continue;
                                }
                                let phys_strict =
                                    strict[0][x][ri] | strict[1][x][ri] | strict[2][x][ri];
                                let phys_legacy =
                                    legacy[0][x][ri] | legacy[1][x][ri] | legacy[2][x][ri];

                                let mut gone = l & !s;
                                while gone != 0 {
                                    let y = ctz(gone) as i32;
                                    gone &= gone - 1;
                                    let has_path = move_for_lock(p, s_idx, x as i32, y, ri)
                                        .map(|mv| !get_input(&b, &mv, false, force).data.is_empty())
                                        .unwrap_or(false);
                                    if has_path {
                                        counts.bug += 1;
                                        if counts.bug_examples.len() < 5 {
                                            counts.bug_examples.push(format!(
                                                "dropped-but-reachable p={p:?} force={force} \
                                                 s={s_idx} x={x} y={y} ri={ri} rows={rows:?}"
                                            ));
                                        }
                                    } else if phys_strict & bb(y) != 0 {
                                        counts.label_loss += 1;
                                    } else {
                                        counts.phantom_removed += 1;
                                    }
                                }

                                let mut gained = s & !l;
                                while gained != 0 {
                                    let y = ctz(gained) as i32;
                                    gained &= gained - 1;
                                    let replays = move_for_lock(p, s_idx, x as i32, y, ri)
                                        .map(|mv| {
                                            let inputs = get_input(&b, &mv, false, force);
                                            !inputs.data.is_empty()
                                                && simulate_inputs(&b, p, force, &inputs)
                                                    == Some((
                                                        x as i32,
                                                        y,
                                                        Rotation::from_u8(ri as u8),
                                                        SpinType::from_u8(s_idx as u8),
                                                    ))
                                        })
                                        .unwrap_or(false);
                                    if !replays {
                                        counts.bug += 1;
                                        if counts.bug_examples.len() < 5 {
                                            counts.bug_examples.push(format!(
                                                "unexplained-gain p={p:?} force={force} \
                                                 s={s_idx} x={x} y={y} ri={ri} rows={rows:?}"
                                            ));
                                        }
                                    } else if phys_legacy & bb(y) != 0 {
                                        counts.label_gain += 1;
                                    } else {
                                        counts.reach_gain += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        counts
    }

    fn report_diff_counts(counts: &DiffCounts) {
        println!(
            "cubes_compared={} cubes_differing={} phantom_removed={} label_loss={} \
             label_gain={} reach_gain={} bug={}",
            counts.cubes_compared,
            counts.cubes_differing,
            counts.phantom_removed,
            counts.label_loss,
            counts.label_gain,
            counts.reach_gain,
            counts.bug
        );
        for e in &counts.bug_examples {
            println!("BUG: {e}");
        }
    }

    #[test]
    fn diff_classification_zero_bugs_on_seeded_boards() {
        let counts = classify_diffs_on_corpus(2_000, 0xA11C_E5E5_2026_0610u64);
        report_diff_counts(&counts);
        assert_eq!(counts.bug, 0, "{:?}", counts.bug_examples);
        assert!(counts.cubes_differing > 0);
    }

    #[test]
    #[ignore]
    fn diff_classification_zero_bugs_on_100k_boards() {
        let counts = classify_diffs_on_corpus(100_000, 0xA11C_E5E5_2026_0610u64);
        report_diff_counts(&counts);
        assert_eq!(counts.bug, 0, "{:?}", counts.bug_examples);
        assert!(counts.cubes_differing > 0);
    }

    fn simulate_inputs(
        board: &Board,
        p: Piece,
        force: bool,
        inputs: &Inputs,
    ) -> Option<(i32, i32, Rotation, SpinType)> {
        let cols = board.compute_cols();
        let cm = CollisionMap::new(&cols, p);
        let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
        let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
        let can_spin = is_t || is_allspin;

        let mut x = SPAWN_COL as i32;
        let mut y: i32 = if force {
            let blocked = cm.get(SPAWN_COL, Rotation::North);
            let valid = !blocked & !bb_low(ACTIVE_RULES.spawn_row);
            if valid == 0 {
                return None;
            }
            ctz(valid) as i32
        } else {
            if cm.get(SPAWN_COL, Rotation::North) & bb(ACTIVE_RULES.spawn_row) != 0 {
                return None;
            }
            ACTIVE_RULES.spawn_row
        };
        let mut r = Rotation::North;
        let mut s = SpinType::NoSpin;

        for (idx, &inp) in inputs.data.iter().enumerate() {
            match inp {
                Input::HardDrop => {
                    if idx + 1 != inputs.data.len() {
                        return None;
                    }
                    let rc = canonical_r(p, r);
                    let mut drop_y = y;
                    while drop_y > 0 && cm.get(x as usize, rc) & bb(drop_y - 1) == 0 {
                        drop_y -= 1;
                    }
                    let label = if can_spin && drop_y == y {
                        s
                    } else {
                        SpinType::NoSpin
                    };
                    return Some((x, drop_y, rc, label));
                }
                Input::ShiftLeft | Input::ShiftRight => {
                    let dx = if inp == Input::ShiftLeft { -1 } else { 1 };
                    let x1 = x + dx;
                    let rc = canonical_r(p, r);
                    if x1 < 0 || !in_bounds(p, rc, x1) || cm.get(x1 as usize, rc) & bb(y) != 0 {
                        return None;
                    }
                    x = x1;
                    s = SpinType::NoSpin;
                }
                Input::DasLeft | Input::DasRight => {
                    let dx = if inp == Input::DasLeft { -1 } else { 1 };
                    let rc = canonical_r(p, r);
                    let mut x1 = x;
                    loop {
                        let nx = x1 + dx;
                        if nx < 0 || !in_bounds(p, rc, nx) || cm.get(nx as usize, rc) & bb(y) != 0 {
                            break;
                        }
                        x1 = nx;
                    }
                    if x1 == x {
                        return None;
                    }
                    x = x1;
                    s = SpinType::NoSpin;
                }
                Input::SoftDrop => {
                    let rc = canonical_r(p, r);
                    if y == 0 || cm.get(x as usize, rc) & bb(y - 1) != 0 {
                        return None;
                    }
                    y -= 1;
                    s = SpinType::NoSpin;
                }
                Input::RotateCw | Input::RotateCcw | Input::RotateFlip => {
                    if p == Piece::O {
                        return None;
                    }
                    let d = match inp {
                        Input::RotateCw => Direction::Cw,
                        Input::RotateCcw => Direction::Ccw,
                        _ => Direction::Flip,
                    };
                    if d == Direction::Flip && !ACTIVE_RULES.enable_180 {
                        return None;
                    }
                    let rt = rotate(d, r);
                    let off = canonical_offset(p, r) - canonical_offset(p, rt);
                    let mut kick_buf = [Coordinates::new(0, 0); 6];
                    let kick_count = if d == Direction::Flip {
                        let ki = kick_180_index(p);
                        let arr = &KICKS_180[ki][r as usize];
                        let n = if !ACTIVE_RULES.srs_plus { 2 } else { arr.len() };
                        kick_buf[..n].copy_from_slice(&arr[..n]);
                        n
                    } else {
                        let ki = kick_index(p, ACTIVE_RULES.srs_plus);
                        let arr = &KICKS[ki][d as usize][r as usize];
                        kick_buf[..arr.len()].copy_from_slice(arr);
                        arr.len()
                    };
                    let mut applied = false;
                    let rt_c = canonical_r(p, rt);
                    for (k, &kick) in kick_buf.iter().enumerate().take(kick_count) {
                        let x1 = x + kick.x as i32 + off.x as i32;
                        let y1 = y + kick.y as i32 + off.y as i32;
                        if x1 < 0 || y1 < 0 || !in_bounds(p, rt_c, x1) || y1 >= ROW_NB as i32 {
                            continue;
                        }
                        if cm.get(x1 as usize, rt_c) & bb(y1) != 0 {
                            continue;
                        }
                        s = if can_spin {
                            classify_rotation_spin(
                                board, &cm, is_t, is_allspin, x1, y1, rt, rt_c, k,
                            )
                        } else {
                            SpinType::NoSpin
                        };
                        x = x1;
                        y = y1;
                        r = rt;
                        applied = true;
                        break;
                    }
                    if !applied {
                        return None;
                    }
                }
                Input::NoInput => return None,
            }
        }
        None
    }

    fn move_for_lock(p: Piece, s_idx: usize, x: i32, y: i32, ri: usize) -> Option<Move> {
        let r = Rotation::from_u8(ri as u8);
        match s_idx {
            0 => Some(Move::new(p, r, x, y, false)),
            1 => {
                if p == Piece::T {
                    Some(Move::new_tspin(r, x, y, false))
                } else {
                    Some(Move::new_allspin_mini(p, r, x, y))
                }
            }
            2 => {
                if p == Piece::T {
                    Some(Move::new_tspin(r, x, y, true))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[test]
    fn strict_locks_roundtrip_get_input_and_simulator() {
        let mut st = 0x5EED_50F7_D40B_2026u64;
        let mut boards = 0u64;
        let mut positives = 0u64;
        while boards < 150 {
            let Some(rows) = seeded_holey_rows(&mut st) else {
                continue;
            };
            let b = board_from_rows(&rows);
            boards += 1;
            for &p in &ALL_PIECES {
                for force in [false, true] {
                    let reach = reachable_locks(&b, p, force);
                    for s_idx in 0..SPIN_NB {
                        for x in 0..COL_NB {
                            for ri in 0..ROTATION_NB {
                                let mut bits = reach.locks[s_idx][x][ri];
                                if s_idx == 2 && p != Piece::T {
                                    assert_eq!(
                                        bits, 0,
                                        "non-T Full lock p={p:?} force={force} rows={rows:?}"
                                    );
                                }
                                while bits != 0 {
                                    let y = ctz(bits) as i32;
                                    bits &= bits - 1;
                                    let Some(target) = move_for_lock(p, s_idx, x as i32, y, ri)
                                    else {
                                        continue;
                                    };
                                    let inputs = get_input(&b, &target, false, force);
                                    assert!(
                                        !inputs.data.is_empty(),
                                        "lock without path p={p:?} force={force} s={s_idx} \
                                         x={x} y={y} ri={ri} rows={rows:?}"
                                    );
                                    let sim = simulate_inputs(&b, p, force, &inputs);
                                    let Some((fx, fy, frc, fs)) = sim else {
                                        panic!(
                                            "path does not replay p={p:?} force={force} \
                                             s={s_idx} x={x} y={y} ri={ri} rows={rows:?}"
                                        );
                                    };
                                    assert_eq!(
                                        (fx, fy, frc as usize, fs as usize),
                                        (x as i32, y, ri, s_idx),
                                        "path lands elsewhere p={p:?} force={force} \
                                         rows={rows:?} inputs={:?}",
                                        inputs.data
                                    );
                                    positives += 1;
                                }
                            }
                        }
                    }

                    let mut buf = crate::move_buffer::MoveBuffer::new();
                    crate::movegen::generate(&b, &mut buf, p, force);
                    for m in buf.as_slice() {
                        if !b.legal_lock_placement(m) {
                            continue;
                        }
                        let retained = reach.move_reachable(m);
                        let path_based = crate::movegen::move_reachable(&b, m, force);
                        assert_eq!(
                            retained, path_based,
                            "cube vs get_input disagree m={m:?} p={p:?} force={force} \
                             rows={rows:?}"
                        );
                        // Shared left-edge canonical-frame blind spot is
                        // gone; every strict emission must be retained.
                        assert!(
                            retained,
                            "strict emission not reachable m={m:?} p={p:?} force={force} \
                             rows={rows:?}"
                        );
                    }
                }
            }
        }
        assert!(positives > 10_000, "positives={positives}");
    }

    // Arbiter = production movegen (smear-core strict; reference-BFS-exact
    // on the 42k corpus). Contract per labeled lock key (spin, x, y, r):
    //   completeness - get_input finds a path for every key generate() emits;
    //   soundness    - get_input finds no path for any key generate() omits;
    //   validity     - every returned path replays on the true-physics
    //                  simulator to exactly its target key.
    fn assert_pathfinder_matches_strict(b: &Board, p: Piece, force: bool, ctx: &str) {
        use std::collections::BTreeSet;

        let is_t = p == Piece::T && ACTIVE_RULES.enable_tspin;
        let is_allspin = p != Piece::T && p != Piece::O && ACTIVE_RULES.enable_allspin;
        let can_spin = is_t || is_allspin;

        let mut buf = crate::move_buffer::MoveBuffer::new();
        crate::movegen::generate(b, &mut buf, p, force);
        let mut emitted: BTreeSet<(usize, i32, i32, usize)> = BTreeSet::new();
        for m in buf.as_slice() {
            emitted.insert((
                m.spin() as usize,
                m.x(),
                m.y(),
                canonical_r(p, m.rotation()) as usize,
            ));
        }

        let h = b
            .rows
            .iter()
            .rposition(|&r| r != 0)
            .map(|y| y + 1)
            .unwrap_or(0) as i32;
        let y_hi = (h + 3).min(ROW_NB as i32 - 1);
        for &(_, _, y, _) in &emitted {
            assert!(
                y <= y_hi,
                "emission above sweep box y={y} y_hi={y_hi} {ctx}"
            );
        }

        for s_idx in 0..SPIN_NB {
            if s_idx > 0 && !can_spin {
                continue;
            }
            for x in 0..COL_NB as i32 {
                for ri in 0..canonical_size(p) {
                    for y in 0..=y_hi {
                        let Some(target) = move_for_lock(p, s_idx, x, y, ri) else {
                            continue;
                        };
                        let inputs = get_input(b, &target, false, force);
                        let in_emission = emitted.contains(&(s_idx, x, y, ri));
                        if inputs.data.is_empty() {
                            assert!(
                                !in_emission,
                                "MISSING PATH s={s_idx} x={x} y={y} ri={ri} p={p:?} \
                                 force={force} {ctx}"
                            );
                        } else {
                            assert!(
                                in_emission,
                                "PHANTOM PATH s={s_idx} x={x} y={y} ri={ri} p={p:?} \
                                 force={force} inputs={:?} {ctx}",
                                inputs.data
                            );
                            let sim = simulate_inputs(b, p, force, &inputs);
                            assert_eq!(
                                sim,
                                Some((
                                    x,
                                    y,
                                    Rotation::from_u8(ri as u8),
                                    SpinType::from_u8(s_idx as u8)
                                )),
                                "WRONG-TARGET path s={s_idx} x={x} y={y} ri={ri} p={p:?} \
                                 force={force} inputs={:?} {ctx}",
                                inputs.data
                            );
                        }
                    }
                }
            }
        }

        // The retained-cube lane must agree with strict emission label-exactly.
        let reach = reachable_locks(b, p, force);
        let mut cube: BTreeSet<(usize, i32, i32, usize)> = BTreeSet::new();
        for (s_idx, sl) in reach.locks.iter().enumerate() {
            if s_idx > 0 && !can_spin {
                continue;
            }
            for (x, xl) in sl.iter().enumerate() {
                for (ri, &bits) in xl.iter().enumerate().take(canonical_size(p)) {
                    let mut bits = bits;
                    while bits != 0 {
                        let y = ctz(bits) as i32;
                        bits &= bits - 1;
                        cube.insert((s_idx, x as i32, y, ri));
                    }
                }
            }
        }
        assert_eq!(
            cube.difference(&emitted).collect::<Vec<_>>(),
            Vec::<&(usize, i32, i32, usize)>::new(),
            "cube-only locks p={p:?} force={force} {ctx}"
        );
        assert_eq!(
            emitted.difference(&cube).collect::<Vec<_>>(),
            Vec::<&(usize, i32, i32, usize)>::new(),
            "emission-only locks p={p:?} force={force} {ctx}"
        );
    }

    // Pinned boards from smear_core disqualification probes (P1) and the
    // T3b diagnosis. case-13/case-4: strict-reachable left-edge locks;
    // case-353: force lane whose old kick resolution was wrong-target;
    // case-1: raw engine over-produces 9 phantoms (soundness side).
    #[test]
    fn group2_fold_probe_boards_pin() {
        // case13: I North (1,1) in a left-edge slot
        let b13 = board_from_rows(&[634, 208, 972]);
        assert_pathfinder_matches_strict(&b13, Piece::I, false, "case13 rows=[634,208,972]");
        let bare13 = Move::new(Piece::I, Rotation::North, 1, 1, false);
        assert!(b13.legal_lock_placement(&bare13));
        assert!(
            crate::movegen::move_reachable(&b13, &bare13, false),
            "case13 placement-level reachability (move_reachable strata union)"
        );

        // case4: S East (1,15), an isolated pocket (P1's original
        // finding; T3b table mis-read it as pathfinder miss).
        // Strict arbiter omits it; pin is soundness direction.
        let b4 = board_from_rows(&[
            531, 182, 32, 710, 608, 683, 985, 727, 402, 562, 545, 977, 414, 691, 779, 97, 468, 708,
        ]);
        assert_pathfinder_matches_strict(&b4, Piece::S, false, "case4 pocket board");
        let bare4 = Move::new(Piece::S, Rotation::East, 1, 15, false);
        assert!(b4.legal_lock_placement(&bare4));
        let mut s4 = crate::move_buffer::MoveBuffer::new();
        crate::movegen::generate(&b4, &mut s4, Piece::S, false);
        assert!(
            !s4.iter()
                .any(|m| (m.x(), m.y(), canonical_r(Piece::S, m.rotation()))
                    == (1, 15, Rotation::East)),
            "case4 pocket entered the strict emission set; re-audit"
        );
        assert!(
            !crate::movegen::move_reachable(&b4, &bare4, false),
            "case4 isolated pocket must stay unreachable (soundness)"
        );

        // case353: I force lane on a tall board; old path to (4,25,East)
        // was [Ccw,Ccw,Cw,HardDrop] which true physics locks elsewhere.
        let b353 = board_from_rows(&[
            532, 186, 142, 497, 630, 526, 937, 37, 190, 398, 549, 82, 398, 355, 18, 23, 45, 404,
            278, 651, 818, 276, 16,
        ]);
        assert_pathfinder_matches_strict(&b353, Piece::I, true, "case353 force tall board");

        // case1: raw engine emits 9 phantom I locks; strict omits them.
        let b1 = board_from_rows(&[527, 263, 30, 60, 898, 821, 581, 746]);
        assert_pathfinder_matches_strict(&b1, Piece::I, false, "case1 engine-phantom board");
    }

    #[test]
    fn get_input_matches_strict_generate_on_seeded_boards() {
        let mut st = 0x6F1D_F01D_2026_0707u64;
        let mut boards = 0u64;
        while boards < 40 {
            let h = 2 + (xs(&mut st) % 25) as usize;
            let mut rows = vec![0u16; h];
            for r in rows.iter_mut() {
                *r = (xs(&mut st) as u16) & 0x03FF;
            }
            if let Some(last) = rows.last_mut() {
                if *last == 0 {
                    *last = 1u16 << (xs(&mut st) % 10);
                }
            }
            let b = board_from_rows(&rows);
            if !crate::movegen::needs_reachability_filter(&b) {
                continue;
            }
            boards += 1;
            for &p in &ALL_PIECES {
                for force in [false, true] {
                    assert_pathfinder_matches_strict(
                        &b,
                        p,
                        force,
                        &format!("seeded rows={rows:?}"),
                    );
                }
            }
        }
        assert_eq!(boards, 40);
    }

    #[test]
    #[ignore]
    fn get_input_matches_strict_generate_on_2k_boards() {
        let mut st = 0x6F1D_F01D_2026_0707u64;
        let mut boards = 0u64;
        while boards < 2_000 {
            let h = 2 + (xs(&mut st) % 25) as usize;
            let mut rows = vec![0u16; h];
            for r in rows.iter_mut() {
                *r = (xs(&mut st) as u16) & 0x03FF;
            }
            if let Some(last) = rows.last_mut() {
                if *last == 0 {
                    *last = 1u16 << (xs(&mut st) % 10);
                }
            }
            let b = board_from_rows(&rows);
            if !crate::movegen::needs_reachability_filter(&b) {
                continue;
            }
            boards += 1;
            for &p in &ALL_PIECES {
                for force in [false, true] {
                    assert_pathfinder_matches_strict(
                        &b,
                        p,
                        force,
                        &format!("seeded rows={rows:?}"),
                    );
                }
            }
        }
        assert_eq!(boards, 2_000);
    }

    #[test]
    fn test_get_input_simple_i_drop() {
        let board = Board::new();
        let target = Move::new(Piece::I, Rotation::North, SPAWN_COL as i32, 0, false);
        let inputs = get_input(&board, &target, false, false);
        assert!(!inputs.data.is_empty());
        assert_eq!(*inputs.data.last().unwrap(), Input::HardDrop);
    }

    #[test]
    fn test_get_input_t_piece() {
        let board = Board::new();
        let target = Move::new(Piece::T, Rotation::North, SPAWN_COL as i32, 0, false);
        let inputs = get_input(&board, &target, false, false);
        assert!(!inputs.data.is_empty());
        assert_eq!(*inputs.data.last().unwrap(), Input::HardDrop);
    }
}
