// coach_beam.rs -- S2 coaching beam kernels (wasm.rs keeps only JS marshaling).
//
// Kernels (shared child-expansion shape):
// - beam_best_gm:      max accumulated S2 attack over a piece queue.
// - beam_best_gm_line: step+wellness variant matching the TS s2BestLine contract.
// - beam_best_gm_gi:    garbage-injecting variant keeping the player's line reachable.
//
// Parity contract: outputs are consumed by refrozen mosaic coaching goldens.
// Dedup keys are raw row arrays, sorts are stable, f64 accumulation order is
// part of the contract -- do not reorder operations.
//
// Beam nodes carry only `rows: [u16; 40]` plus chain counters (no Board, no
// cols cache). A Board is rebuilt once per surviving node for movegen.
// Placement/clearing is raw row arithmetic mirroring Board::place /
// line_clears / clear_lines bit-for-bit (see the `oracle` test module for
// the differential reference).

use crate::attack::calculate_attack_s2_tl_with_multiplier;
use crate::board::{Board, FULL_ROW};
use crate::eval::{evaluate, EvalWeights};
use crate::header::{piece_from_external, piece_to_external, Move};
use crate::move_buffer::MoveBuffer;
use crate::movegen::generate_playable;
use std::cmp::Ordering;

// Per-node garbage state as a u64 row-bitmask (bit y = row y has >=1 garbage
// cell). A garbage row only loses cells via a full-row clear, so the per-row
// predicate is exactly preserved without a full cell mask.
#[derive(Default)]
pub(crate) struct FxHasher64 {
    h: u64,
}
impl std::hash::Hasher for FxHasher64 {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        while bytes.len() >= 8 {
            let v = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            self.h = (self.h.rotate_left(5) ^ v).wrapping_mul(K);
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut b = [0u8; 8];
            b[..bytes.len()].copy_from_slice(bytes);
            self.h = (self.h.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(K);
        }
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.h
    }
}
pub(crate) type FxRowSet =
    std::collections::HashSet<[u16; 40], std::hash::BuildHasherDefault<FxHasher64>>;
pub(crate) type FxFullSet =
    std::collections::HashSet<([u16; 40], i32, i32), std::hash::BuildHasherDefault<FxHasher64>>;
pub(crate) type FxFullMap<V> =
    std::collections::HashMap<([u16; 40], i32, i32), V, std::hash::BuildHasherDefault<FxHasher64>>;

// Drop the bits of `gm` at cleared row positions and shift higher bits down,
// matching how `clear_lines` compacts the board (software pext on a single u64).
#[inline]
pub(crate) fn compact_gm_bits(gm: u64, cleared: u64) -> u64 {
    if cleared == 0 {
        return gm;
    }
    let mut out = 0u64;
    let mut w = 0u32;
    let mut k = !cleared;
    while k != 0 {
        let y = k.trailing_zeros();
        if gm & (1u64 << y) != 0 {
            out |= 1u64 << w;
        }
        w += 1;
        k &= k - 1;
    }
    out
}

// Bitmask of rows that still contain at least one cell (gm bits for emptied rows
// must be dropped, mirroring the per-cell `gm[y] &= rows[y]` step).
#[inline]
pub(crate) fn rows_nonempty_mask(rows: &[u16; 40]) -> u64 {
    let mut ne = 0u64;
    for (y, &row) in rows.iter().enumerate() {
        if row != 0 {
            ne |= 1u64 << y;
        }
    }
    ne
}

#[cfg(test)]
pub(crate) fn nonempty_row_mask(board: &Board) -> u64 {
    rows_nonempty_mask(&board.rows)
}

// Mirrors Board::place exactly, including the `as usize` wrap that skips
// negative coordinates -- placed cells drive dedup keys, so any bounds
// deviation changes beam outputs.
#[inline]
fn place_on_rows(rows: &mut [u16; 40], m: &Move) {
    let pc = m.cells();
    let x = m.x();
    let y = m.y();

    let xu = x as usize;
    let yu = y as usize;
    if xu < 10 && yu < 40 {
        rows[yu] |= 1 << x;
    }

    for i in 0..3 {
        let cx = (pc[i].x as i32 + x) as usize;
        let cy = (pc[i].y as i32 + y) as usize;
        if cx < 10 && cy < 40 {
            rows[cy] |= 1 << cx;
        }
    }
}

#[inline]
fn row_clear_mask(rows: &[u16; 40]) -> u64 {
    let mut cleared = 0u64;
    for (y, &row) in rows.iter().enumerate() {
        if row == FULL_ROW {
            cleared |= 1u64 << y;
        }
    }
    cleared
}

// Mirrors Board::clear_lines row compaction (cols cache does not exist here).
#[inline]
fn compact_rows(rows: &mut [u16; 40], cleared: u64) {
    let mut write = 0usize;
    for read in 0..40 {
        if cleared & (1u64 << read) == 0 {
            rows[write] = rows[read];
            write += 1;
        }
    }
    for row in rows.iter_mut().skip(write) {
        *row = 0;
    }
}

fn board_health_rows(rows: &[u16; 40]) -> (i32, i32) {
    let mut height = 0i32;
    for (y, row) in rows.iter().enumerate().rev() {
        if row & 0x3FF != 0 {
            height = y as i32 + 1;
            break;
        }
    }
    let mut seen: u16 = 0;
    let mut holes = 0i32;
    for row in rows.iter().take(height as usize).rev() {
        holes += i32::try_from((seen & !row & 0x3FF).count_ones()).unwrap_or(0);
        seen |= row;
    }
    (height, holes)
}

/// Build a Board (rows + cols cache) from pre-masked u16 rows. Equivalent to
/// wasm_board::board_from_row_bitmasks for inputs already reduced to 10 bits.
pub(crate) fn board_from_u16(rows: &[u16; 40]) -> Board {
    let mut b = Board::new();
    b.rows = *rows;
    b.cols = [0; 10];
    for (y, row) in b.rows.iter().enumerate() {
        let mut bits = *row as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            b.cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }
    b
}

#[derive(Clone, Copy)]
struct GmNode {
    rows: [u16; 40],
    gm: u64,
    acc: i64,
    b2b: i32,
    combo: i32,
    pending: i32,
}

#[inline]
fn sort_top_prefix_unstable_by<T, F>(values: &mut [T], keep: usize, mut cmp: F)
where
    F: FnMut(&T, &T) -> Ordering,
{
    if keep == 0 || values.len() <= 1 {
        return;
    }
    if keep >= values.len() {
        values.sort_unstable_by(cmp);
        return;
    }
    values.select_nth_unstable_by(keep - 1, &mut cmp);
    values[..keep].sort_unstable_by(cmp);
}

#[inline]
fn gm_idx_cmp(children: &[GmNode], a: usize, b: usize) -> Ordering {
    children[b].acc.cmp(&children[a].acc).then(a.cmp(&b))
}

#[inline]
fn gm_prune_prefix_len(total: usize, beam_width: usize) -> usize {
    total.min(beam_width.saturating_add(beam_width.max(1)))
}

#[cfg(test)]
static GM_PRUNE_FALLBACKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
#[inline]
fn record_gm_prune_fallback() {
    GM_PRUNE_FALLBACKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(test))]
#[inline]
fn record_gm_prune_fallback() {}

/// Max accumulated S2 attack over `pieces` from `rows0`, beam width `beam_width`.
/// `keep`: optional per-ply boards forced to survive pruning (player's line).
/// `surge_shaping`: value each placement as realized attack plus banked
///   surge-potential delta, deduping by (rows, b2b, combo).
#[allow(clippy::too_many_arguments)]
pub fn beam_best_gm(
    rows0: &[u16; 40],
    gm0: u64,
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep: Option<&[[u16; 40]]>,
    beam_width: u32,
    garbage_multiplier: f64,
    surge_shaping: bool,
) -> f64 {
    let bw = beam_width as usize;
    let k = pieces.len();

    let mut beam: Vec<GmNode> = vec![GmNode {
        rows: *rows0,
        gm: gm0,
        acc: 0,
        b2b,
        combo,
        pending,
    }];
    let mut children: Vec<GmNode> = Vec::new();
    let mut idx: Vec<usize> = Vec::new();
    let mut seen: FxRowSet = FxRowSet::default();
    let mut seen_full: FxFullSet = FxFullSet::default();
    let mut pruned: Vec<GmNode> = Vec::with_capacity(bw);
    let mut moves = MoveBuffer::new();

    for t in 0..k {
        let p = match piece_from_external(pieces[t]) {
            Some(p) => p,
            None => break,
        };
        children.clear();
        children.reserve(beam.len().saturating_mul(40));
        for node in &beam {
            let node_board = board_from_u16(&node.rows);
            moves.clear();
            generate_playable(&node_board, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut cr = node.rows;
                place_on_rows(&mut cr, m);
                let cleared = row_clear_mask(&cr);
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    compact_rows(&mut cr, cleared);
                }
                let spin = m.spin();
                let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                let attack = calculate_attack_s2_tl_with_multiplier(
                    lines,
                    spin,
                    node.b2b,
                    node.combo,
                    cr.iter().all(|&r| r == 0),
                    garbage_cleared,
                    garbage_multiplier,
                );
                let mut child_gm = compact_gm_bits(node.gm, cleared);
                if child_gm != 0 {
                    child_gm &= rows_nonempty_mask(&cr);
                }
                let shaped_delta = if surge_shaping {
                    crate::attack::surge_potential(attack.b2b_after, garbage_multiplier)
                        - crate::attack::surge_potential(node.b2b, garbage_multiplier)
                } else {
                    0
                };
                children.push(GmNode {
                    rows: cr,
                    gm: child_gm,
                    acc: node.acc + attack.attack as i64 + shaped_delta,
                    b2b: attack.b2b_after,
                    combo: attack.combo_after,
                    pending: (node.pending - lines as i32).max(0),
                });
            }
        }
        if children.is_empty() {
            break;
        }
        idx.clear();
        idx.extend(0..children.len());
        let sorted_prefix_len = gm_prune_prefix_len(idx.len(), bw);
        sort_top_prefix_unstable_by(&mut idx, sorted_prefix_len, |&a, &b| {
            gm_idx_cmp(&children, a, b)
        });
        let keepb: Option<[u16; 40]> = keep.map(|kb| kb[t]);
        seen.clear();
        seen.reserve(children.len());
        seen_full.clear();
        if surge_shaping {
            seen_full.reserve(children.len());
        }
        pruned.clear();
        pruned.reserve(bw);
        let mut kept = false;
        for &ci in &idx[..sorted_prefix_len] {
            let c = &children[ci];
            let is_dup = if surge_shaping {
                !seen_full.insert((c.rows, c.b2b, c.combo))
            } else {
                !seen.insert(c.rows)
            };
            if is_dup {
                continue;
            }
            if Some(c.rows) == keepb {
                kept = true;
            }
            pruned.push(*c);
            if pruned.len() >= bw {
                break;
            }
        }
        let needs_more = if bw == 0 {
            pruned.is_empty()
        } else {
            pruned.len() < bw
        };
        let mut remainder_sorted = false;
        if needs_more && sorted_prefix_len < idx.len() {
            record_gm_prune_fallback();
            idx[sorted_prefix_len..].sort_unstable_by(|&a, &b| gm_idx_cmp(&children, a, b));
            remainder_sorted = true;
            for &ci in &idx[sorted_prefix_len..] {
                let c = &children[ci];
                let is_dup = if surge_shaping {
                    !seen_full.insert((c.rows, c.b2b, c.combo))
                } else {
                    !seen.insert(c.rows)
                };
                if is_dup {
                    continue;
                }
                if Some(c.rows) == keepb {
                    kept = true;
                }
                pruned.push(*c);
                if pruned.len() >= bw {
                    break;
                }
            }
        }
        if let Some(kb) = keepb {
            if !kept {
                for &ci in &idx[..sorted_prefix_len] {
                    let c = &children[ci];
                    if c.rows == kb {
                        pruned.push(*c);
                        kept = true;
                        break;
                    }
                }
            }
            if !kept && sorted_prefix_len < idx.len() {
                if !remainder_sorted {
                    idx[sorted_prefix_len..].sort_unstable_by(|&a, &b| gm_idx_cmp(&children, a, b));
                }
                for &ci in &idx[sorted_prefix_len..] {
                    let c = &children[ci];
                    if c.rows == kb {
                        pruned.push(*c);
                        break;
                    }
                }
            }
        }
        std::mem::swap(&mut beam, &mut pruned);
    }
    let mut mx: i64 = 0;
    for node in &beam {
        if node.acc > mx {
            mx = node.acc;
        }
    }
    mx as f64
}

/// Move descriptor of one line step (external piece/rotation encoding).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoachLineMove {
    pub piece: u8,
    pub rotation: u8,
    pub x: i8,
    pub y: i8,
    pub spin: u8,
}

/// One placement of the chosen coaching line, with attack/chain breakdown.
#[derive(Clone, PartialEq, Debug)]
pub struct CoachLineStep {
    pub rows: [u16; 40],
    pub attack: f64,
    pub lines: u8,
    pub b2b: i32,
    pub combo: i32,
    pub spin: u8,
    pub b2b_before: i32,
    pub combo_before: i32,
    pub is_surge_release: bool,
    pub surge_potential_delta: f64,
    pub mv: CoachLineMove,
}

/// Chosen line of the step-emitting beam. `health_penalty` is
/// `attack - selection_score` (derived by the caller).
#[derive(Clone, PartialEq, Debug)]
pub struct CoachLineResult {
    pub attack: f64,
    pub selection_score: f64,
    pub steps: Vec<CoachLineStep>,
    pub final_rows: [u16; 40],
}

// Line-beam node: chosen line reconstructed via `parent` indices into the
// previous ply's survivors, avoiding per-child steps vector clone.
#[derive(Clone, Copy)]
struct LineNode {
    rows: [u16; 40],
    gm: u64,
    acc: f64,
    sel: f64,
    holes: i32,
    b2b: i32,
    combo: i32,
    pending: i32,
    parent: u32,
    attack: f64,
    lines: u8,
    spin: u8,
    b2b_before: i32,
    combo_before: i32,
    is_surge_release: bool,
    surge_delta: f64,
    mv: CoachLineMove,
}

#[inline]
fn line_idx_cmp(order: &[LineNode], ia: u32, ib: u32) -> Ordering {
    let a = &order[ia as usize];
    let b = &order[ib as usize];
    b.sel
        .partial_cmp(&a.sel)
        .unwrap_or(Ordering::Equal)
        .then(b.acc.partial_cmp(&a.acc).unwrap_or(Ordering::Equal))
        .then(a.holes.cmp(&b.holes))
        .then(ia.cmp(&ib))
}

/// Step+wellness-emitting beam. Selection score = acc - penalty (penalty vs
/// start board). With hole_w=height_w=0 the max accumulated attack is
/// identical to the surge-shaped beam_best_gm. Reconstructs the chosen
/// line's per-step breakdown matching the TS s2BestLine contract.
///
/// `terminal_lambda` re-ranks the final frontier by
/// `sel + terminal_lambda * eval(final board)`. `eval::evaluate` scores with
/// negative weights (clean empty board = 0, holes/height push the score
/// down), so adding the term rewards cleaner terminal boards and the chosen
/// line cannot dump its last piece for a marginal attack win. 0.0 = pick by
/// selection score, bit-identical to the pre-lambda kernel.
///
/// `dig_combo_w`/`dig_attack_w` shape selection IN-BEAM while the parent node
/// still has garbage rows (`gm != 0`): clearing steps earn
/// `dig_combo_w * combo_after` and every step earns
/// `dig_attack_w * step_attack`, added to `sel` only — `acc` (the reported
/// attack) is never touched, so the panel's attack arithmetic and the gap
/// numerator are unaffected. Both 0.0 = bit-identical to the pre-dig kernel.
///
/// `spin_w` prices spin clears IN-BEAM (rank-readability dial, retarget C):
/// every spin-clear step adds `spin_w` to `sel` — negative discourages
/// spin-hunting lines (low-rank readability), positive encourages them.
/// Like the dig weights it is sel-only; 0.0 = bit-identical.
#[allow(clippy::too_many_arguments)]
pub fn beam_best_gm_line(
    rows0: &[u16; 40],
    gm0: u64,
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    beam_width: u32,
    garbage_multiplier: f64,
    hole_w: f64,
    height_w: f64,
    height_grace: f64,
    terminal_lambda: f64,
    dig_combo_w: f64,
    dig_attack_w: f64,
    spin_w: f64,
) -> Option<CoachLineResult> {
    let bw = beam_width as usize;
    let wellness_on = hole_w != 0.0 || height_w != 0.0;

    let (start_height, start_holes) = board_health_rows(rows0);

    let penalty_of = |height: i32, holes: i32| -> f64 {
        if !wellness_on {
            return 0.0;
        }
        hole_w * ((holes - start_holes).max(0) as f64)
            + height_w * (((height - start_height) as f64 - height_grace).max(0.0))
    };

    let root = LineNode {
        rows: *rows0,
        gm: gm0,
        acc: 0.0,
        sel: 0.0,
        holes: start_holes,
        b2b,
        combo,
        pending,
        parent: u32::MAX,
        attack: 0.0,
        lines: 0,
        spin: 0,
        b2b_before: 0,
        combo_before: 0,
        is_surge_release: false,
        surge_delta: 0.0,
        mv: CoachLineMove {
            piece: 0,
            rotation: 0,
            x: 0,
            y: 0,
            spin: 0,
        },
    };
    let mut hist: Vec<Vec<LineNode>> = vec![vec![root]];
    let mut order: Vec<LineNode> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut index: FxFullMap<usize> = FxFullMap::default();
    let mut moves = MoveBuffer::new();

    for &piece_id in pieces {
        let p = match piece_from_external(piece_id) {
            Some(p) => p,
            None => break,
        };
        let beam = hist.last().expect("hist starts non-empty");
        order.clear();
        order.reserve(beam.len().saturating_mul(40));
        index.clear();
        index.reserve(beam.len().saturating_mul(40));
        for (pi, node) in beam.iter().enumerate() {
            let node_board = board_from_u16(&node.rows);
            moves.clear();
            generate_playable(&node_board, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut cr = node.rows;
                place_on_rows(&mut cr, m);
                let cleared = row_clear_mask(&cr);
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    compact_rows(&mut cr, cleared);
                }
                let spin_u8 = m.spin() as u8;
                let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                let attack = calculate_attack_s2_tl_with_multiplier(
                    lines,
                    m.spin(),
                    node.b2b,
                    node.combo,
                    cr.iter().all(|&r| r == 0),
                    garbage_cleared,
                    garbage_multiplier,
                );
                let mut child_gm = compact_gm_bits(node.gm, cleared);
                if child_gm != 0 {
                    child_gm &= rows_nonempty_mask(&cr);
                }
                let surge_delta =
                    crate::attack::surge_potential(attack.b2b_after, garbage_multiplier)
                        - crate::attack::surge_potential(node.b2b, garbage_multiplier);
                // scores sanitized against NaN totals
                let acc_new =
                    (node.acc + attack.attack as f64 + surge_delta as f64).max(f64::NEG_INFINITY);
                let (h_height, h_holes) = board_health_rows(&cr);
                let mut sel = (acc_new - penalty_of(h_height, h_holes)).max(f64::NEG_INFINITY);
                if (dig_combo_w != 0.0 || dig_attack_w != 0.0) && node.gm != 0 {
                    let combo_bonus = if lines > 0 {
                        dig_combo_w * attack.combo_after.max(0) as f64
                    } else {
                        0.0
                    };
                    sel = (sel + combo_bonus + dig_attack_w * attack.attack as f64)
                        .max(f64::NEG_INFINITY);
                }
                if spin_w != 0.0 && lines > 0 && spin_u8 != 0 {
                    sel = (sel + spin_w).max(f64::NEG_INFINITY);
                }
                let key = (cr, attack.b2b_after, attack.combo_after);
                let existing = index.get(&key).copied();
                if let Some(idx) = existing {
                    if order[idx].sel >= sel {
                        continue;
                    }
                }
                let is_surge_release = (1..4).contains(&lines) && spin_u8 == 0 && node.b2b >= 4;
                let lnode = LineNode {
                    rows: cr,
                    gm: child_gm,
                    acc: acc_new,
                    sel,
                    holes: h_holes,
                    b2b: attack.b2b_after,
                    combo: attack.combo_after,
                    pending: (node.pending - lines as i32).max(0),
                    parent: pi as u32,
                    attack: attack.attack as f64,
                    lines,
                    spin: spin_u8,
                    b2b_before: node.b2b,
                    combo_before: node.combo,
                    is_surge_release,
                    surge_delta: surge_delta as f64,
                    mv: CoachLineMove {
                        piece: piece_to_external(m.piece()),
                        rotation: m.rotation() as u8,
                        x: m.x() as i8,
                        y: m.y() as i8,
                        spin: spin_u8,
                    },
                };
                match existing {
                    Some(idx) => order[idx] = lnode,
                    None => {
                        index.insert(key, order.len());
                        order.push(lnode);
                    }
                }
            }
        }
        if order.is_empty() {
            break;
        }
        // Index sort + top-bw gather (avoids sorting the full struct array).
        idx.clear();
        idx.extend(0..order.len() as u32);
        sort_top_prefix_unstable_by(&mut idx, bw, |&ia, &ib| line_idx_cmp(&order, ia, ib));
        idx.truncate(bw);
        hist.push(idx.iter().map(|&i| order[i as usize]).collect());
    }

    let last_ply = hist.last().expect("hist starts non-empty");
    let best = if terminal_lambda != 0.0 && last_ply.len() > 1 {
        let weights = EvalWeights::default();
        let mut best_i = 0usize;
        let mut best_score = f64::NEG_INFINITY;
        for (i, node) in last_ply.iter().enumerate() {
            let quality = evaluate(&board_from_u16(&node.rows), &weights) as f64;
            let score = node.sel + terminal_lambda * quality;
            if score > best_score {
                best_score = score;
                best_i = i;
            }
        }
        last_ply[best_i]
    } else {
        *last_ply.first()?
    };
    let mut steps: Vec<CoachLineStep> = Vec::with_capacity(hist.len() - 1);
    let mut ply = hist.len() - 1;
    let mut cur = &best;
    while ply > 0 {
        steps.push(CoachLineStep {
            rows: cur.rows,
            attack: cur.attack,
            lines: cur.lines,
            b2b: cur.b2b,
            combo: cur.combo,
            spin: cur.spin,
            b2b_before: cur.b2b_before,
            combo_before: cur.combo_before,
            is_surge_release: cur.is_surge_release,
            surge_potential_delta: cur.surge_delta,
            mv: cur.mv,
        });
        let parent = cur.parent as usize;
        ply -= 1;
        cur = &hist[ply][parent];
    }
    steps.reverse();
    Some(CoachLineResult {
        attack: best.acc,
        selection_score: best.sel,
        steps,
        final_rows: best.rows,
    })
}

/// Garbage-injecting beam variant. After placing+clearing at step t,
/// inserts `garbage_counts[t]` garbage rows into every child board before
/// the keep comparison, so the player's real line stays reachable.
/// With all-zero `garbage_counts` this is identical to `beam_best_gm`.
#[allow(clippy::too_many_arguments)]
pub fn beam_best_gm_gi(
    rows0: &[u16; 40],
    gm0: u64,
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep: Option<&[[u16; 40]]>,
    beam_width: u32,
    multipliers: &[f64],
    garbage_rows: &[u16],
    garbage_counts: &[u32],
    max_gi: u32,
) -> f64 {
    let bw = beam_width as usize;
    let k = pieces.len();
    let stride = max_gi as usize;

    let mut beam: Vec<GmNode> = vec![GmNode {
        rows: *rows0,
        gm: gm0,
        acc: 0,
        b2b,
        combo,
        pending,
    }];
    let mut children: Vec<GmNode> = Vec::new();
    let mut idx: Vec<usize> = Vec::new();
    let mut seen: FxRowSet = FxRowSet::default();
    let mut pruned: Vec<GmNode> = Vec::with_capacity(bw);
    let mut moves = MoveBuffer::new();

    for t in 0..k {
        let p = match piece_from_external(pieces[t]) {
            Some(p) => p,
            None => break,
        };
        let mult = multipliers.get(t).copied().unwrap_or(1.0);
        let gc = garbage_counts.get(t).copied().unwrap_or(0) as usize;
        let gc = gc.min(stride);
        let mut garbage = [0u16; 40];
        for (i, g) in garbage.iter_mut().enumerate().take(gc) {
            *g = garbage_rows.get(t * stride + i).copied().unwrap_or(0);
        }
        // Bit i set iff inserted garbage row i actually has cells (marks it garbage).
        let inserted_bits: u64 = {
            let mut b = 0u64;
            for (i, &g) in garbage.iter().enumerate().take(gc) {
                if g != 0 {
                    b |= 1u64 << i;
                }
            }
            b
        };
        children.clear();
        children.reserve(beam.len().saturating_mul(40));
        for node in &beam {
            let node_board = board_from_u16(&node.rows);
            moves.clear();
            generate_playable(&node_board, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut cr = node.rows;
                place_on_rows(&mut cr, m);
                let cleared = row_clear_mask(&cr);
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    compact_rows(&mut cr, cleared);
                }
                let spin = m.spin();
                let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                let attack = calculate_attack_s2_tl_with_multiplier(
                    lines,
                    spin,
                    node.b2b,
                    node.combo,
                    cr.iter().all(|&r| r == 0),
                    garbage_cleared,
                    mult,
                );
                let mut child_gm = compact_gm_bits(node.gm, cleared);
                if child_gm != 0 {
                    child_gm &= rows_nonempty_mask(&cr);
                }
                let (crows, cgm) = if gc > 0 {
                    let mut nr = [0u16; 40];
                    nr[gc..40].copy_from_slice(&cr[..(40 - gc)]);
                    nr[..gc].copy_from_slice(&garbage[..gc]);
                    let ngm = ((child_gm << gc) | inserted_bits) & ((1u64 << 40) - 1);
                    (nr, ngm)
                } else {
                    (cr, child_gm)
                };
                children.push(GmNode {
                    rows: crows,
                    gm: cgm,
                    acc: node.acc + attack.attack as i64,
                    b2b: attack.b2b_after,
                    combo: attack.combo_after,
                    pending: (node.pending - lines as i32).max(0),
                });
            }
        }
        if children.is_empty() {
            break;
        }
        idx.clear();
        idx.extend(0..children.len());
        idx.sort_unstable_by(|&a, &b| children[b].acc.cmp(&children[a].acc).then(a.cmp(&b)));
        let keepb: Option<[u16; 40]> = keep.map(|kb| kb[t]);
        seen.clear();
        seen.reserve(children.len());
        pruned.clear();
        pruned.reserve(bw);
        let mut kept = false;
        for &ci in &idx {
            let c = &children[ci];
            if !seen.insert(c.rows) {
                continue;
            }
            if Some(c.rows) == keepb {
                kept = true;
            }
            pruned.push(*c);
            if pruned.len() >= bw {
                break;
            }
        }
        if let Some(kb) = keepb {
            if !kept {
                for &ci in &idx {
                    let c = &children[ci];
                    if c.rows == kb {
                        pruned.push(*c);
                        break;
                    }
                }
            }
        }
        std::mem::swap(&mut beam, &mut pruned);
    }
    let mut mx: i64 = 0;
    for node in &beam {
        if node.acc > mx {
            mx = node.acc;
        }
    }
    mx as f64
}

// The original Board-based kernels, kept verbatim as the differential parity
// reference for the rows-only rewrites above (same role as bench_beam's
// BASELINE replica). Do not "improve" this module.
#[cfg(test)]
mod oracle {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    pub fn beam_best_gm(
        rows0: &[u16; 40],
        gm0: u64,
        pieces: &[u8],
        b2b: i32,
        combo: i32,
        pending: i32,
        keep: Option<&[[u16; 40]]>,
        beam_width: u32,
        garbage_multiplier: f64,
        surge_shaping: bool,
    ) -> f64 {
        struct BNode {
            board: Board,
            gm: u64,
            acc: i64,
            b2b: i32,
            combo: i32,
            pending: i32,
        }
        struct Child {
            board: Board,
            acc: i64,
            b2b: i32,
            combo: i32,
            pending: i32,
            gm: u64,
        }
        let bw = beam_width as usize;
        let k = pieces.len();

        let mut beam: Vec<BNode> = vec![BNode {
            board: board_from_u16(rows0),
            gm: gm0,
            acc: 0,
            b2b,
            combo,
            pending,
        }];
        let mut children: Vec<Child> = Vec::new();
        let mut idx: Vec<usize> = Vec::new();
        let mut seen: FxRowSet = FxRowSet::default();
        let mut seen_full: FxFullSet = FxFullSet::default();
        let mut pruned: Vec<BNode> = Vec::with_capacity(bw);
        let mut moves = MoveBuffer::new();

        for t in 0..k {
            let p = match piece_from_external(pieces[t]) {
                Some(p) => p,
                None => break,
            };
            children.clear();
            children.reserve(beam.len().saturating_mul(40));
            for node in &beam {
                moves.clear();
                generate_playable(&node.board, &mut moves, p, false);
                for m in moves.as_slice() {
                    let mut nb = node.board.clone();
                    nb.place(m);
                    let cleared = nb.line_clears();
                    let lines = cleared.count_ones() as u8;
                    if cleared != 0 {
                        nb.clear_lines(cleared);
                    }
                    let spin = m.spin();
                    let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                    let attack = calculate_attack_s2_tl_with_multiplier(
                        lines,
                        spin,
                        node.b2b,
                        node.combo,
                        nb.is_empty(),
                        garbage_cleared,
                        garbage_multiplier,
                    );
                    let mut child_gm = compact_gm_bits(node.gm, cleared);
                    if child_gm != 0 {
                        child_gm &= nonempty_row_mask(&nb);
                    }
                    let shaped_delta = if surge_shaping {
                        crate::attack::surge_potential(attack.b2b_after, garbage_multiplier)
                            - crate::attack::surge_potential(node.b2b, garbage_multiplier)
                    } else {
                        0
                    };
                    children.push(Child {
                        board: nb,
                        acc: node.acc + attack.attack as i64 + shaped_delta,
                        b2b: attack.b2b_after,
                        combo: attack.combo_after,
                        pending: (node.pending - lines as i32).max(0),
                        gm: child_gm,
                    });
                }
            }
            if children.is_empty() {
                break;
            }
            idx.clear();
            idx.extend(0..children.len());
            idx.sort_unstable_by(|&a, &b| children[b].acc.cmp(&children[a].acc).then(a.cmp(&b)));
            let keepb: Option<[u16; 40]> = keep.map(|kb| kb[t]);
            seen.clear();
            seen.reserve(children.len());
            seen_full.clear();
            if surge_shaping {
                seen_full.reserve(children.len());
            }
            pruned.clear();
            pruned.reserve(bw);
            let mut kept = false;
            for &ci in &idx {
                let c = &children[ci];
                let rows = c.board.rows;
                let is_dup = if surge_shaping {
                    !seen_full.insert((rows, c.b2b, c.combo))
                } else {
                    !seen.insert(rows)
                };
                if is_dup {
                    continue;
                }
                if Some(rows) == keepb {
                    kept = true;
                }
                pruned.push(BNode {
                    board: c.board.clone(),
                    gm: c.gm,
                    acc: c.acc,
                    b2b: c.b2b,
                    combo: c.combo,
                    pending: c.pending,
                });
                if pruned.len() >= bw {
                    break;
                }
            }
            if let Some(kb) = keepb {
                if !kept {
                    for &ci in &idx {
                        let c = &children[ci];
                        let rows = c.board.rows;
                        if rows == kb {
                            pruned.push(BNode {
                                board: c.board.clone(),
                                gm: c.gm,
                                acc: c.acc,
                                b2b: c.b2b,
                                combo: c.combo,
                                pending: c.pending,
                            });
                            break;
                        }
                    }
                }
            }
            std::mem::swap(&mut beam, &mut pruned);
        }
        let mut mx: i64 = 0;
        for node in &beam {
            if node.acc > mx {
                mx = node.acc;
            }
        }
        mx as f64
    }

    #[allow(clippy::too_many_arguments)]
    pub fn beam_best_gm_line(
        rows0: &[u16; 40],
        gm0: u64,
        pieces: &[u8],
        b2b: i32,
        combo: i32,
        pending: i32,
        beam_width: u32,
        garbage_multiplier: f64,
        hole_w: f64,
        height_w: f64,
        height_grace: f64,
        terminal_lambda: f64,
        dig_combo_w: f64,
        dig_attack_w: f64,
        spin_w: f64,
    ) -> Option<CoachLineResult> {
        struct LNode {
            board: Board,
            gm: u64,
            acc: f64,
            sel: f64,
            holes: i32,
            b2b: i32,
            combo: i32,
            pending: i32,
            steps: Vec<CoachLineStep>,
        }

        let bw = beam_width as usize;
        let wellness_on = hole_w != 0.0 || height_w != 0.0;

        let start_b = board_from_u16(rows0);
        let (start_height, start_holes) = board_health_rows(&start_b.rows);

        let penalty_of = |height: i32, holes: i32| -> f64 {
            if !wellness_on {
                return 0.0;
            }
            hole_w * ((holes - start_holes).max(0) as f64)
                + height_w * (((height - start_height) as f64 - height_grace).max(0.0))
        };

        let mut beam: Vec<LNode> = vec![LNode {
            board: start_b,
            gm: gm0,
            acc: 0.0,
            sel: 0.0,
            holes: start_holes,
            b2b,
            combo,
            pending,
            steps: Vec::new(),
        }];
        let mut order: Vec<LNode> = Vec::new();
        let mut index: FxFullMap<usize> = FxFullMap::default();
        let mut moves = MoveBuffer::new();

        for &piece_id in pieces {
            let p = match piece_from_external(piece_id) {
                Some(p) => p,
                None => break,
            };
            order.clear();
            order.reserve(beam.len().saturating_mul(40));
            index.clear();
            index.reserve(beam.len().saturating_mul(40));
            for node in &beam {
                moves.clear();
                generate_playable(&node.board, &mut moves, p, false);
                for m in moves.as_slice() {
                    let mut nb = node.board.clone();
                    nb.place(m);
                    let cleared = nb.line_clears();
                    let lines = cleared.count_ones() as u8;
                    if cleared != 0 {
                        nb.clear_lines(cleared);
                    }
                    let spin_u8 = m.spin() as u8;
                    let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                    let attack = calculate_attack_s2_tl_with_multiplier(
                        lines,
                        m.spin(),
                        node.b2b,
                        node.combo,
                        nb.is_empty(),
                        garbage_cleared,
                        garbage_multiplier,
                    );
                    let mut child_gm = compact_gm_bits(node.gm, cleared);
                    if child_gm != 0 {
                        child_gm &= nonempty_row_mask(&nb);
                    }
                    let surge_delta =
                        crate::attack::surge_potential(attack.b2b_after, garbage_multiplier)
                            - crate::attack::surge_potential(node.b2b, garbage_multiplier);
                    let acc_new = node.acc + attack.attack as f64 + surge_delta as f64;
                    let (h_height, h_holes) = board_health_rows(&nb.rows);
                    let mut sel = acc_new - penalty_of(h_height, h_holes);
                    if (dig_combo_w != 0.0 || dig_attack_w != 0.0) && node.gm != 0 {
                        let combo_bonus = if lines > 0 {
                            dig_combo_w * attack.combo_after.max(0) as f64
                        } else {
                            0.0
                        };
                        sel += combo_bonus + dig_attack_w * attack.attack as f64;
                    }
                    if spin_w != 0.0 && lines > 0 && spin_u8 != 0 {
                        sel += spin_w;
                    }
                    let key = (nb.rows, attack.b2b_after, attack.combo_after);
                    let existing = index.get(&key).copied();
                    if let Some(idx) = existing {
                        if order[idx].sel >= sel {
                            continue;
                        }
                    }
                    let is_surge_release = (1..4).contains(&lines) && spin_u8 == 0 && node.b2b >= 4;
                    let mut steps = node.steps.clone();
                    steps.push(CoachLineStep {
                        rows: nb.rows,
                        attack: attack.attack as f64,
                        lines,
                        b2b: attack.b2b_after,
                        combo: attack.combo_after,
                        spin: spin_u8,
                        b2b_before: node.b2b,
                        combo_before: node.combo,
                        is_surge_release,
                        surge_potential_delta: surge_delta as f64,
                        mv: CoachLineMove {
                            piece: piece_to_external(m.piece()),
                            rotation: m.rotation() as u8,
                            x: m.x() as i8,
                            y: m.y() as i8,
                            spin: spin_u8,
                        },
                    });
                    let lnode = LNode {
                        board: nb,
                        gm: child_gm,
                        acc: acc_new,
                        sel,
                        holes: h_holes,
                        b2b: attack.b2b_after,
                        combo: attack.combo_after,
                        pending: (node.pending - lines as i32).max(0),
                        steps,
                    };
                    match existing {
                        Some(idx) => order[idx] = lnode,
                        None => {
                            index.insert(key, order.len());
                            order.push(lnode);
                        }
                    }
                }
            }
            if order.is_empty() {
                break;
            }
            order.sort_by(|a, b| {
                b.sel
                    .partial_cmp(&a.sel)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(
                        b.acc
                            .partial_cmp(&a.acc)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                    .then(a.holes.cmp(&b.holes))
            });
            order.truncate(bw);
            std::mem::swap(&mut beam, &mut order);
        }

        if terminal_lambda != 0.0 && beam.len() > 1 {
            let weights = crate::eval::EvalWeights::default();
            let mut best_i = 0usize;
            let mut best_score = f64::NEG_INFINITY;
            for (i, node) in beam.iter().enumerate() {
                let quality = crate::eval::evaluate(&node.board, &weights) as f64;
                let score = node.sel + terminal_lambda * quality;
                if score > best_score {
                    best_score = score;
                    best_i = i;
                }
            }
            let best = beam.swap_remove(best_i);
            return Some(CoachLineResult {
                attack: best.acc,
                selection_score: best.sel,
                steps: best.steps,
                final_rows: best.board.rows,
            });
        }
        beam.into_iter().next().map(|best| CoachLineResult {
            attack: best.acc,
            selection_score: best.sel,
            steps: best.steps,
            final_rows: best.board.rows,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn beam_best_gm_gi(
        rows0: &[u16; 40],
        gm0: u64,
        pieces: &[u8],
        b2b: i32,
        combo: i32,
        pending: i32,
        keep: Option<&[[u16; 40]]>,
        beam_width: u32,
        multipliers: &[f64],
        garbage_rows: &[u16],
        garbage_counts: &[u32],
        max_gi: u32,
    ) -> f64 {
        struct BNode {
            board: Board,
            gm: u64,
            acc: i64,
            b2b: i32,
            combo: i32,
            pending: i32,
        }
        struct Child {
            board: Board,
            acc: i64,
            b2b: i32,
            combo: i32,
            pending: i32,
            gm: u64,
        }
        let bw = beam_width as usize;
        let k = pieces.len();
        let stride = max_gi as usize;

        let mut beam: Vec<BNode> = vec![BNode {
            board: board_from_u16(rows0),
            gm: gm0,
            acc: 0,
            b2b,
            combo,
            pending,
        }];
        let mut children: Vec<Child> = Vec::new();
        let mut idx: Vec<usize> = Vec::new();
        let mut seen: FxRowSet = FxRowSet::default();
        let mut pruned: Vec<BNode> = Vec::with_capacity(bw);
        let mut moves = MoveBuffer::new();

        for t in 0..k {
            let p = match piece_from_external(pieces[t]) {
                Some(p) => p,
                None => break,
            };
            let mult = multipliers.get(t).copied().unwrap_or(1.0);
            let gc = garbage_counts.get(t).copied().unwrap_or(0) as usize;
            let gc = gc.min(stride);
            let mut garbage = [0u16; 40];
            for (i, g) in garbage.iter_mut().enumerate().take(gc) {
                *g = garbage_rows.get(t * stride + i).copied().unwrap_or(0);
            }
            let inserted_bits: u64 = {
                let mut b = 0u64;
                for (i, &g) in garbage.iter().enumerate().take(gc) {
                    if g != 0 {
                        b |= 1u64 << i;
                    }
                }
                b
            };
            children.clear();
            children.reserve(beam.len().saturating_mul(40));
            for node in &beam {
                moves.clear();
                generate_playable(&node.board, &mut moves, p, false);
                for m in moves.as_slice() {
                    let mut nb = node.board.clone();
                    nb.place(m);
                    let cleared = nb.line_clears();
                    let lines = cleared.count_ones() as u8;
                    if cleared != 0 {
                        nb.clear_lines(cleared);
                    }
                    let spin = m.spin();
                    let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                    let attack = calculate_attack_s2_tl_with_multiplier(
                        lines,
                        spin,
                        node.b2b,
                        node.combo,
                        nb.is_empty(),
                        garbage_cleared,
                        mult,
                    );
                    let mut child_gm = compact_gm_bits(node.gm, cleared);
                    if child_gm != 0 {
                        child_gm &= nonempty_row_mask(&nb);
                    }
                    let (cboard, cgm) = if gc > 0 {
                        let mut nr = [0u16; 40];
                        nr[gc..40].copy_from_slice(&nb.rows[..(40 - gc)]);
                        nr[..gc].copy_from_slice(&garbage[..gc]);
                        let ngm = ((child_gm << gc) | inserted_bits) & ((1u64 << 40) - 1);
                        (board_from_u16(&nr), ngm)
                    } else {
                        (nb, child_gm)
                    };
                    children.push(Child {
                        board: cboard,
                        acc: node.acc + attack.attack as i64,
                        b2b: attack.b2b_after,
                        combo: attack.combo_after,
                        pending: (node.pending - lines as i32).max(0),
                        gm: cgm,
                    });
                }
            }
            if children.is_empty() {
                break;
            }
            idx.clear();
            idx.extend(0..children.len());
            idx.sort_unstable_by(|&a, &b| children[b].acc.cmp(&children[a].acc).then(a.cmp(&b)));
            let keepb: Option<[u16; 40]> = keep.map(|kb| kb[t]);
            seen.clear();
            seen.reserve(children.len());
            pruned.clear();
            pruned.reserve(bw);
            let mut kept = false;
            for &ci in &idx {
                let c = &children[ci];
                let rows = c.board.rows;
                if !seen.insert(rows) {
                    continue;
                }
                if Some(rows) == keepb {
                    kept = true;
                }
                pruned.push(BNode {
                    board: c.board.clone(),
                    gm: c.gm,
                    acc: c.acc,
                    b2b: c.b2b,
                    combo: c.combo,
                    pending: c.pending,
                });
                if pruned.len() >= bw {
                    break;
                }
            }
            if let Some(kb) = keepb {
                if !kept {
                    for &ci in &idx {
                        let c = &children[ci];
                        let rows = c.board.rows;
                        if rows == kb {
                            pruned.push(BNode {
                                board: c.board.clone(),
                                gm: c.gm,
                                acc: c.acc,
                                b2b: c.b2b,
                                combo: c.combo,
                                pending: c.pending,
                            });
                            break;
                        }
                    }
                }
            }
            std::mem::swap(&mut beam, &mut pruned);
        }
        let mut mx: i64 = 0;
        for node in &beam {
            if node.acc > mx {
                mx = node.acc;
            }
        }
        mx as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::Piece;

    fn xs(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn random_stack(state: &mut u64) -> [u16; 40] {
        let mut rows = [0u16; 40];
        let h = (xs(state) % 13) as usize;
        for row in rows.iter_mut().take(h) {
            let mut r = (xs(state) & 0x3FF) as u16;
            if r == FULL_ROW {
                r &= !(1u16 << (xs(state) % 10));
            }
            *row = r;
        }
        rows
    }

    fn random_gm(state: &mut u64, rows: &[u16; 40]) -> u64 {
        rows_nonempty_mask(rows) & xs(state)
    }

    fn random_queue(state: &mut u64) -> Vec<u8> {
        let len = (xs(state) % 7) as usize;
        let mut q: Vec<u8> = (0..len).map(|_| (xs(state) % 7) as u8).collect();
        if len > 0 && xs(state) % 10 == 0 {
            let pos = (xs(state) as usize) % len;
            q[pos] = 7 + (xs(state) % 200) as u8;
        }
        q
    }

    // Plays a random playable move per ply (with per-ply garbage insertion for
    // the gi variant) so keep rows match real child dedup keys.
    fn playout_keep(
        state: &mut u64,
        rows0: &[u16; 40],
        pieces: &[u8],
        garbage: Option<(&[u16], &[u32], usize)>,
    ) -> Vec<[u16; 40]> {
        let mut rows = *rows0;
        let mut out = Vec::with_capacity(pieces.len());
        let mut moves = MoveBuffer::new();
        for (t, &piece_id) in pieces.iter().enumerate() {
            if let Some(p) = piece_from_external(piece_id) {
                let b = board_from_u16(&rows);
                moves.clear();
                generate_playable(&b, &mut moves, p, false);
                let n = moves.as_slice().len();
                if n > 0 {
                    let m = moves.as_slice()[(xs(state) as usize) % n];
                    place_on_rows(&mut rows, &m);
                    let cleared = row_clear_mask(&rows);
                    if cleared != 0 {
                        compact_rows(&mut rows, cleared);
                    }
                }
            }
            if let Some((grows, gcounts, stride)) = garbage {
                let gc = gcounts.get(t).copied().unwrap_or(0) as usize;
                let gc = gc.min(stride);
                if gc > 0 {
                    let mut nr = [0u16; 40];
                    nr[gc..40].copy_from_slice(&rows[..(40 - gc)]);
                    for (i, slot) in nr.iter_mut().enumerate().take(gc) {
                        *slot = grows.get(t * stride + i).copied().unwrap_or(0);
                    }
                    rows = nr;
                }
            }
            out.push(rows);
        }
        out
    }

    fn random_keep(
        state: &mut u64,
        rows0: &[u16; 40],
        pieces: &[u8],
        garbage: Option<(&[u16], &[u32], usize)>,
    ) -> Option<Vec<[u16; 40]>> {
        match xs(state) % 5 {
            0 | 1 | 2 => None,
            3 => Some(playout_keep(state, rows0, pieces, garbage)),
            _ => Some(
                (0..pieces.len())
                    .map(|_| random_stack(state))
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn beam_width_for(case: usize, state: &mut u64) -> u32 {
        if case % 25 == 0 {
            300
        } else {
            [1, 2, 3, 5, 8, 13, 40][(xs(state) as usize) % 7]
        }
    }

    const MULTS: [f64; 4] = [0.5, 1.0, 1.027, 2.0];

    #[test]
    fn sort_top_prefix_unstable_by_matches_full_sort_prefix() {
        for len in 0..80usize {
            for keep in [0usize, 1, 2, 3, 5, 8, 13, 40, 80] {
                let mut state = 0x5107_2026_0708u64 ^ ((len as u64) << 32) ^ keep as u64;
                let mut full: Vec<(u32, usize)> = (0..len)
                    .map(|i| ((((xs(&mut state) >> 9) as u32) ^ ((i as u32) & 7)), i))
                    .collect();
                let mut partial = full.clone();
                full.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

                sort_top_prefix_unstable_by(&mut partial, keep, |a, b| {
                    b.0.cmp(&a.0).then(a.1.cmp(&b.1))
                });

                let prefix = keep.min(len);
                assert_eq!(&partial[..prefix], &full[..prefix], "len={len} keep={keep}");
            }
        }
    }

    #[test]
    fn beam_best_gm_matches_oracle() {
        GM_PRUNE_FALLBACKS.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut state = 0xC0AC_4BEA_2026_0703u64;
        let mut keep_cases = 0u32;
        for case in 0..260 {
            let rows0 = random_stack(&mut state);
            let gm0 = random_gm(&mut state, &rows0);
            let pieces = random_queue(&mut state);
            let b2b = (xs(&mut state) % 10) as i32 - 1;
            let combo = (xs(&mut state) % 8) as i32 - 1;
            let pending = (xs(&mut state) % 7) as i32;
            let bw = beam_width_for(case, &mut state);
            let mult = MULTS[(xs(&mut state) as usize) % 4];
            let surge = xs(&mut state) % 2 == 0;
            let keep = random_keep(&mut state, &rows0, &pieces, None);
            if keep.is_some() {
                keep_cases += 1;
            }
            let got = beam_best_gm(
                &rows0,
                gm0,
                &pieces,
                b2b,
                combo,
                pending,
                keep.as_deref(),
                bw,
                mult,
                surge,
            );
            let want = oracle::beam_best_gm(
                &rows0,
                gm0,
                &pieces,
                b2b,
                combo,
                pending,
                keep.as_deref(),
                bw,
                mult,
                surge,
            );
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "case={case} got={got} want={want} bw={bw} surge={surge} pieces={pieces:?}"
            );
        }
        assert!(keep_cases >= 60, "keep coverage too thin: {keep_cases}");
        println!(
            "gm_prune_fallbacks={}",
            GM_PRUNE_FALLBACKS.load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    #[test]
    fn beam_best_gm_line_matches_oracle() {
        let mut state = 0x11FE_C0AC_2026_0703u64;
        let mut nonempty_steps = 0u32;
        for case in 0..200 {
            let rows0 = random_stack(&mut state);
            let gm0 = random_gm(&mut state, &rows0);
            let pieces = random_queue(&mut state);
            let b2b = (xs(&mut state) % 10) as i32 - 1;
            let combo = (xs(&mut state) % 8) as i32 - 1;
            let pending = (xs(&mut state) % 7) as i32;
            let bw = beam_width_for(case, &mut state);
            let mult = MULTS[(xs(&mut state) as usize) % 4];
            let hole_w = [0.0, 0.5, 2.0][(xs(&mut state) as usize) % 3];
            let height_w = [0.0, 1.0][(xs(&mut state) as usize) % 2];
            let height_grace = [0.0, 2.0][(xs(&mut state) as usize) % 2];
            let terminal_lambda = [0.0, 0.5, 2.0][(xs(&mut state) as usize) % 3];
            let dig_combo_w = [0.0, 2.0, 4.0][(xs(&mut state) as usize) % 3];
            let dig_attack_w = [0.0, 0.75][(xs(&mut state) as usize) % 2];
            let spin_w = [0.0, -2.0, 1.0][(xs(&mut state) as usize) % 3];
            let got = beam_best_gm_line(
                &rows0,
                gm0,
                &pieces,
                b2b,
                combo,
                pending,
                bw,
                mult,
                hole_w,
                height_w,
                height_grace,
                terminal_lambda,
                dig_combo_w,
                dig_attack_w,
                spin_w,
            );
            let want = oracle::beam_best_gm_line(
                &rows0,
                gm0,
                &pieces,
                b2b,
                combo,
                pending,
                bw,
                mult,
                hole_w,
                height_w,
                height_grace,
                terminal_lambda,
                dig_combo_w,
                dig_attack_w,
                spin_w,
            );
            match (&got, &want) {
                (Some(g), Some(w)) => {
                    assert_eq!(
                        g.attack.to_bits(),
                        w.attack.to_bits(),
                        "case={case} attack {g:?} vs {w:?}"
                    );
                    assert_eq!(
                        g.selection_score.to_bits(),
                        w.selection_score.to_bits(),
                        "case={case} sel"
                    );
                    assert_eq!(g.final_rows, w.final_rows, "case={case} final_rows");
                    assert_eq!(g.steps.len(), w.steps.len(), "case={case} step count");
                    for (i, (gs, ws)) in g.steps.iter().zip(w.steps.iter()).enumerate() {
                        assert_eq!(gs.rows, ws.rows, "case={case} step={i} rows");
                        assert_eq!(
                            gs.attack.to_bits(),
                            ws.attack.to_bits(),
                            "case={case} step={i} attack"
                        );
                        assert_eq!(
                            gs.surge_potential_delta.to_bits(),
                            ws.surge_potential_delta.to_bits(),
                            "case={case} step={i} surge delta"
                        );
                        assert_eq!(
                            (gs.lines, gs.b2b, gs.combo, gs.spin),
                            (ws.lines, ws.b2b, ws.combo, ws.spin),
                            "case={case} step={i} chain"
                        );
                        assert_eq!(
                            (gs.b2b_before, gs.combo_before, gs.is_surge_release),
                            (ws.b2b_before, ws.combo_before, ws.is_surge_release),
                            "case={case} step={i} before-state"
                        );
                        assert_eq!(gs.mv, ws.mv, "case={case} step={i} move");
                    }
                    if !g.steps.is_empty() {
                        nonempty_steps += 1;
                    }
                }
                (None, None) => {}
                _ => panic!("case={case}: option mismatch got={got:?} want={want:?}"),
            }
        }
        assert!(
            nonempty_steps >= 120,
            "line coverage too thin: {nonempty_steps}"
        );
    }

    #[test]
    fn beam_best_gm_gi_matches_oracle() {
        let mut state = 0x61C0_AC4B_2026_0703u64;
        let mut garbage_cases = 0u32;
        for case in 0..200 {
            let rows0 = random_stack(&mut state);
            let gm0 = random_gm(&mut state, &rows0);
            let pieces = random_queue(&mut state);
            let b2b = (xs(&mut state) % 10) as i32 - 1;
            let combo = (xs(&mut state) % 8) as i32 - 1;
            let pending = (xs(&mut state) % 7) as i32;
            let bw = beam_width_for(case, &mut state);
            let max_gi = (xs(&mut state) % 5) as u32;
            let stride = max_gi as usize;
            let multipliers: Vec<f64> = (0..pieces.len().saturating_sub(1))
                .map(|_| MULTS[(xs(&mut state) as usize) % 4])
                .collect();
            let garbage_counts: Vec<u32> = (0..pieces.len())
                .map(|_| (xs(&mut state) % (max_gi as u64 + 2)) as u32)
                .collect();
            let garbage_rows: Vec<u16> = (0..pieces.len() * stride)
                .map(|_| {
                    if xs(&mut state) % 4 == 0 {
                        0
                    } else {
                        let mut r = (xs(&mut state) & 0x3FF) as u16;
                        if r == FULL_ROW {
                            r &= !(1u16 << (xs(&mut state) % 10));
                        }
                        r
                    }
                })
                .collect();
            if garbage_counts.iter().any(|&c| c > 0) && max_gi > 0 {
                garbage_cases += 1;
            }
            let keep = random_keep(
                &mut state,
                &rows0,
                &pieces,
                Some((&garbage_rows, &garbage_counts, stride)),
            );
            let got = beam_best_gm_gi(
                &rows0,
                gm0,
                &pieces,
                b2b,
                combo,
                pending,
                keep.as_deref(),
                bw,
                &multipliers,
                &garbage_rows,
                &garbage_counts,
                max_gi,
            );
            let want = oracle::beam_best_gm_gi(
                &rows0,
                gm0,
                &pieces,
                b2b,
                combo,
                pending,
                keep.as_deref(),
                bw,
                &multipliers,
                &garbage_rows,
                &garbage_counts,
                max_gi,
            );
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "case={case} got={got} want={want} bw={bw} max_gi={max_gi} pieces={pieces:?}"
            );
        }
        assert!(
            garbage_cases >= 80,
            "garbage coverage too thin: {garbage_cases}"
        );
    }

    #[test]
    #[ignore]
    fn kernel_timing_probe_old_vs_new() {
        let mut rows0 = [0u16; 40];
        for row in rows0.iter_mut().take(6) {
            *row = 0x03FF & !(1u16 << 4);
        }
        rows0[6] = 0b0000110111;
        rows0[7] = 0b0000100101;
        let gm0 = rows_nonempty_mask(&rows0) & 0x3F;
        let pieces = [0u8, 2, 1, 3, 5];
        let iters = 30;

        let time = |f: &dyn Fn() -> f64| {
            let mut sink = 0.0;
            for _ in 0..3 {
                sink += f();
            }
            let t = std::time::Instant::now();
            for _ in 0..iters {
                sink += f();
            }
            (t.elapsed().as_micros() / iters as u128, sink)
        };

        let (gm_new, s1) =
            time(&|| beam_best_gm(&rows0, gm0, &pieces, 1, 0, 0, None, 300, 1.0, true));
        let (gm_old, s2) =
            time(&|| oracle::beam_best_gm(&rows0, gm0, &pieces, 1, 0, 0, None, 300, 1.0, true));
        assert_eq!(s1.to_bits(), s2.to_bits());
        let (ln_new, s3) = time(&|| {
            beam_best_gm_line(
                &rows0, gm0, &pieces, 1, 0, 0, 300, 1.0, 1.5, 0.35, 2.0, 1.5, 2.0, 0.5, -1.0,
            )
            .map_or(0.0, |r| r.selection_score)
        });
        let (ln_old, s4) = time(&|| {
            oracle::beam_best_gm_line(
                &rows0, gm0, &pieces, 1, 0, 0, 300, 1.0, 1.5, 0.35, 2.0, 1.5, 2.0, 0.5, -1.0,
            )
            .map_or(0.0, |r| r.selection_score)
        });
        assert_eq!(s3.to_bits(), s4.to_bits());
        println!(
            "kernel_timing gm_old={gm_old}us gm_new={gm_new}us ({:.2}x)  line_old={ln_old}us line_new={ln_new}us ({:.2}x)",
            gm_old as f64 / gm_new as f64,
            ln_old as f64 / ln_new as f64
        );
    }

    // Pins the eval sign convention the terminal_lambda re-rank depends on:
    // default-weight evaluate() is 0 on an empty board and DROPS as holes
    // appear, so "cleaner terminal" means HIGHER eval. Without this anchor the
    // re-rank invariant test below is circular (it would pass with either
    // sign, as the 2026-07-09 inverted-sign bug proved).
    #[test]
    fn eval_default_weights_score_cleaner_boards_higher() {
        let weights = EvalWeights::default();
        let empty = [0u16; 40];
        let mut clean = [0u16; 40];
        clean[..4].fill(0b0111111111);
        let mut holey = clean;
        holey[0] = 0b0111111011;
        let e_empty = evaluate(&board_from_u16(&empty), &weights);
        let e_clean = evaluate(&board_from_u16(&clean), &weights);
        let e_holey = evaluate(&board_from_u16(&holey), &weights);
        assert_eq!(e_empty, 0.0, "empty board must anchor eval at 0");
        assert!(
            e_clean > e_holey,
            "covered hole must lower eval: clean={e_clean} holey={e_holey}"
        );
    }

    #[test]
    fn terminal_lambda_never_picks_dirtier_terminal_and_moves_lines() {
        let mut state = 0x7E12_ACE5_2026_0709u64;
        let weights = EvalWeights::default();
        let mut changed = 0u32;
        let mut strictly_cleaner = 0u32;
        for case in 0..200 {
            let rows0 = random_stack(&mut state);
            let gm0 = random_gm(&mut state, &rows0);
            let pieces = random_queue(&mut state);
            let base = beam_best_gm_line(
                &rows0, gm0, &pieces, 1, 0, 0, 120, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            );
            let shaped = beam_best_gm_line(
                &rows0, gm0, &pieces, 1, 0, 0, 120, 1.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0,
            );
            let (Some(base), Some(shaped)) = (base, shaped) else {
                continue;
            };
            let eval_base = evaluate(&board_from_u16(&base.final_rows), &weights) as f64;
            let eval_shaped = evaluate(&board_from_u16(&shaped.final_rows), &weights) as f64;
            assert!(
                eval_shaped >= eval_base - 1e-6,
                "case={case} lambda picked dirtier terminal: {eval_shaped} < {eval_base}"
            );
            if shaped.final_rows != base.final_rows {
                changed += 1;
            }
            if eval_shaped > eval_base + 1e-6 {
                strictly_cleaner += 1;
            }
        }
        assert!(changed > 0, "terminal lambda never changed a line at 3.0");
        assert!(
            strictly_cleaner > 0,
            "terminal lambda never improved a terminal board at 3.0"
        );
    }

    // Dig-shaping sign pin (non-circular, per the 2026-07-09 lambda lesson):
    // asserts population direction, not per-case monotonicity — an in-beam
    // term interacts with pruning, so single cases may trade combo away, but
    // a sign error would push the aggregate DOWN. Also pins the gm gate:
    // garbage-free starts must be bit-identical with weights engaged.
    #[test]
    fn dig_weights_raise_line_combo_and_gate_on_garbage() {
        let mut state = 0xD16C_0DE5_2026_0711u64;
        let mut changed = 0u32;
        let mut sum_base = 0i64;
        let mut sum_shaped = 0i64;
        let mut zero_gm_cases = 0u32;
        for case in 0..200 {
            let hole_rows = 4 + (xs(&mut state) % 5) as usize;
            let mut rows0 = [0u16; 40];
            for row in rows0.iter_mut().take(hole_rows) {
                *row = 0x03FF & !(1u16 << (xs(&mut state) % 10));
            }
            let gm0 = if case % 5 == 0 {
                0
            } else {
                (1u64 << hole_rows) - 1
            };
            let pieces = random_queue(&mut state);
            let base = beam_best_gm_line(
                &rows0, gm0, &pieces, 1, 0, 0, 120, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            );
            let shaped = beam_best_gm_line(
                &rows0, gm0, &pieces, 1, 0, 0, 120, 1.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.75, 0.0,
            );
            let (Some(base), Some(shaped)) = (base, shaped) else {
                continue;
            };
            if gm0 == 0 {
                zero_gm_cases += 1;
                assert_eq!(
                    base.final_rows, shaped.final_rows,
                    "case={case} dig weights fired without garbage"
                );
                assert_eq!(
                    base.attack.to_bits(),
                    shaped.attack.to_bits(),
                    "case={case} attack drifted without garbage"
                );
                assert_eq!(
                    base.selection_score.to_bits(),
                    shaped.selection_score.to_bits(),
                    "case={case} sel drifted without garbage"
                );
                continue;
            }
            let max_combo =
                |r: &CoachLineResult| r.steps.iter().map(|s| s.combo.max(0)).max().unwrap_or(0);
            sum_base += i64::from(max_combo(&base));
            sum_shaped += i64::from(max_combo(&shaped));
            if shaped.final_rows != base.final_rows {
                changed += 1;
            }
        }
        println!("dig sign-pin: base={sum_base} shaped={sum_shaped} changed={changed}");
        assert!(
            zero_gm_cases >= 20,
            "gate coverage too thin: {zero_gm_cases}"
        );
        assert!(
            changed > 0,
            "dig weights never changed a line on garbage boards"
        );
        assert!(
            sum_shaped > sum_base,
            "dig weights failed to raise aggregate line combo: {sum_shaped} <= {sum_base}"
        );
    }

    #[test]
    fn spin_w_negative_reduces_line_spin_clears() {
        let mut state = 0x5217_0DE5_2026_0712u64;
        let mut changed = 0u32;
        let mut spins_base = 0i64;
        let mut spins_shaped = 0i64;
        let mut attack_base = 0.0f64;
        let mut attack_shaped = 0.0f64;
        for _case in 0..200 {
            let hole_rows = 4 + (xs(&mut state) % 5) as usize;
            let mut rows0 = [0u16; 40];
            for row in rows0.iter_mut().take(hole_rows) {
                *row = 0x03FF & !(1u16 << (xs(&mut state) % 10));
            }
            let pieces = random_queue(&mut state);
            let base = beam_best_gm_line(
                &rows0, 0, &pieces, 1, 0, 0, 120, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            );
            let shaped = beam_best_gm_line(
                &rows0, 0, &pieces, 1, 0, 0, 120, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -3.0,
            );
            let (Some(base), Some(shaped)) = (base, shaped) else {
                continue;
            };
            let spins = |r: &CoachLineResult| -> i64 {
                r.steps
                    .iter()
                    .filter(|s| s.lines > 0 && s.spin != 0)
                    .count() as i64
            };
            spins_base += spins(&base);
            spins_shaped += spins(&shaped);
            attack_base += base.attack;
            attack_shaped += shaped.attack;
            if shaped.final_rows != base.final_rows {
                changed += 1;
            }
        }
        println!(
            "spin sign-pin: base={spins_base} shaped={spins_shaped} changed={changed} atk {attack_base:.0}->{attack_shaped:.0}"
        );
        assert!(
            spins_base >= 10,
            "fixtures produced too few base spin clears: {spins_base}"
        );
        assert!(changed > 0, "spin_w never changed a line");
        assert!(
            spins_shaped < spins_base,
            "spin_w=-3 failed to reduce spin clears: {spins_shaped} >= {spins_base}"
        );
    }

    #[test]
    fn place_on_rows_matches_board_place_on_playable_moves() {
        let mut state = 0x9A0B_0A4D_2026_0703u64;
        let mut moves = MoveBuffer::new();
        let mut checked = 0u32;
        for _ in 0..400 {
            let rows = random_stack(&mut state);
            let b = board_from_u16(&rows);
            let p = piece_from_external((xs(&mut state) % 7) as u8).unwrap();
            moves.clear();
            generate_playable(&b, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut want = b.clone();
                want.place(m);
                let mut got = rows;
                place_on_rows(&mut got, m);
                assert_eq!(got, want.rows);
                assert_eq!(row_clear_mask(&got), want.line_clears());
                let cleared = want.line_clears();
                if cleared != 0 {
                    want.clear_lines(cleared);
                    compact_rows(&mut got, cleared);
                    assert_eq!(got, want.rows);
                }
                checked += 1;
            }
        }
        assert!(checked > 2000, "placement coverage too thin: {checked}");
    }

    #[test]
    fn external_piece_roundtrip_stays_stable() {
        let expected = [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::S,
            Piece::Z,
            Piece::J,
            Piece::L,
        ];
        for (external, piece) in expected.into_iter().enumerate() {
            assert_eq!(piece_from_external(external as u8), Some(piece));
            assert_eq!(piece_to_external(piece), external as u8);
        }
        assert_eq!(piece_from_external(7), None);
    }
}
