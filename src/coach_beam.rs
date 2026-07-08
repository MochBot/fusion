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
use crate::header::{Move, Piece};
use crate::move_buffer::MoveBuffer;
use crate::movegen::generate_playable;

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

fn piece_from_external(v: u8) -> Option<Piece> {
    match v {
        0 => Some(Piece::I),
        1 => Some(Piece::O),
        2 => Some(Piece::T),
        3 => Some(Piece::S),
        4 => Some(Piece::Z),
        5 => Some(Piece::J),
        6 => Some(Piece::L),
        _ => None,
    }
}

fn piece_to_external(p: Piece) -> u8 {
    match p {
        Piece::I => 0,
        Piece::O => 1,
        Piece::T => 2,
        Piece::S => 3,
        Piece::Z => 4,
        Piece::J => 5,
        Piece::L => 6,
    }
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

/// Step+wellness-emitting beam. Selection score = acc - penalty (penalty vs
/// start board). With hole_w=height_w=0 the max accumulated attack is
/// identical to the surge-shaped beam_best_gm. Reconstructs the chosen
/// line's per-step breakdown matching the TS s2BestLine contract.
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
                let sel = (acc_new - penalty_of(h_height, h_holes)).max(f64::NEG_INFINITY);
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
        idx.sort_unstable_by(|&ia, &ib| {
            let a = &order[ia as usize];
            let b = &order[ib as usize];
            b.sel
                .partial_cmp(&a.sel)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    b.acc
                        .partial_cmp(&a.acc)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(a.holes.cmp(&b.holes))
                .then(ia.cmp(&ib))
        });
        idx.truncate(bw);
        hist.push(idx.iter().map(|&i| order[i as usize]).collect());
    }

    let best = *hist.last().expect("hist starts non-empty").first()?;
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
                    let sel = acc_new - penalty_of(h_height, h_holes);
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
    fn beam_best_gm_matches_oracle() {
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
            beam_best_gm_line(&rows0, gm0, &pieces, 1, 0, 0, 300, 1.0, 1.5, 0.35, 2.0)
                .map_or(0.0, |r| r.selection_score)
        });
        let (ln_old, s4) = time(&|| {
            oracle::beam_best_gm_line(&rows0, gm0, &pieces, 1, 0, 0, 300, 1.0, 1.5, 0.35, 2.0)
                .map_or(0.0, |r| r.selection_score)
        });
        assert_eq!(s3.to_bits(), s4.to_bits());
        println!(
            "kernel_timing gm_old={gm_old}us gm_new={gm_new}us ({:.2}x)  line_old={ln_old}us line_new={ln_new}us ({:.2}x)",
            gm_old as f64 / gm_new as f64,
            ln_old as f64 / ln_new as f64
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

    /// External piece convention must match the wasm surface (Triangle order:
    /// I O T S Z J L). A drifted private copy is invisible to the oracle
    /// differential, so this is pinned against wasm_types directly.
    #[cfg(feature = "wasm")]
    #[test]
    fn external_piece_maps_match_wasm_types() {
        for v in 0u8..=7 {
            assert_eq!(
                piece_from_external(v),
                crate::wasm_types::piece_from_external(v),
                "piece_from_external({v})"
            );
        }
        for p in [
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::S,
            Piece::Z,
            Piece::J,
            Piece::L,
        ] {
            assert_eq!(
                piece_to_external(p),
                crate::wasm_types::piece_to_external(p),
                "piece_to_external({p:?})"
            );
            assert_eq!(piece_from_external(piece_to_external(p)), Some(p));
        }
    }
}
