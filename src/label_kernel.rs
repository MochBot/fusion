//! Shared labeling kernel extracted from `src/bin/label_opp.rs`.
//!
//! Pure, reusable primitives for K-horizon attack labeling over `.ctx` records:
//! context parsing, 1-ply expansion with exact S2 attack (`expand_raw`), the
//! K-horizon beam (`beam_best`) with keep-line/garbage-injection, player-line
//! reconstruction (`reconstruct`), and board helpers. The position-level
//! `label_opp` bin and the per-candidate `label_opp_cand` bin both consume this
//! module so their oracle semantics stay byte-identical.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::attack::calculate_attack_s2_tl_with_multiplier;
use crate::board::{Board, FULL_ROW};
use crate::header::Piece;
use crate::move_buffer::MoveBuffer;
use crate::movegen::generate_playable;

pub const FRAME_BYTES: usize = 195;
pub const OPP_BYTES: usize = 186;

/// Byte length of one `.ctx` record for horizon `k`: (k+1) frames + opp block + outcome.
pub fn record_bytes(k: usize) -> usize {
    FRAME_BYTES * (k + 1) + OPP_BYTES + 4
}

#[derive(Default)]
pub struct ProfileCounters {
    pub beam_calls: AtomicU64,
    pub beam_nodes: AtomicU64,
    pub moves: AtomicU64,
    pub generated_unique: AtomicU64,
    pub generate_ns: AtomicU64,
    pub score_ns: AtomicU64,
    pub prune_ns: AtomicU64,
}

pub static PROFILE: ProfileCounters = ProfileCounters {
    beam_calls: AtomicU64::new(0),
    beam_nodes: AtomicU64::new(0),
    moves: AtomicU64::new(0),
    generated_unique: AtomicU64::new(0),
    generate_ns: AtomicU64::new(0),
    score_ns: AtomicU64::new(0),
    prune_ns: AtomicU64::new(0),
};

pub fn add_counter(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

pub fn elapsed_ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

#[derive(Clone)]
pub struct FrameRec {
    pub rows: [u16; 40],
    pub gmask: [u16; 40],
    pub piece: i8,
    pub hold: i8,
    pub queue: [i8; 5],
    pub b2b: i32,
    pub combo: i32,
    pub pending: i32,
    pub atk: f64,
    pub mult: f64,
}

impl Default for FrameRec {
    fn default() -> Self {
        Self {
            rows: [0; 40],
            gmask: [0; 40],
            piece: -1,
            hold: -1,
            queue: [-1; 5],
            b2b: 0,
            combo: 0,
            pending: 0,
            atk: 0.0,
            mult: 1.0,
        }
    }
}

#[derive(Clone)]
pub struct ContextRec {
    pub frames: Vec<FrameRec>,
    pub opp_rows: [u16; 40],
    pub opp_gmask: [u16; 40],
    pub opp_piece: i8,
    pub opp_queue: [i8; 5],
    pub opp_b2b: i32,
    pub opp_combo: i32,
    pub opp_pending: i32,
    pub opp_mult: f64,
    pub outcome: i32,
}

impl Default for ContextRec {
    fn default() -> Self {
        Self::with_horizon(5)
    }
}

impl ContextRec {
    pub fn with_horizon(k: usize) -> Self {
        Self {
            frames: vec![FrameRec::default(); k + 1],
            opp_rows: [0; 40],
            opp_gmask: [0; 40],
            opp_piece: -1,
            opp_queue: [-1; 5],
            opp_b2b: 0,
            opp_combo: 0,
            opp_pending: 0,
            opp_mult: 1.0,
            outcome: 0,
        }
    }
}

#[derive(Clone)]
pub struct ExpandRec {
    pub attack: f64,
    pub rows: [u16; 40],
}

#[derive(Clone)]
pub struct Node {
    pub rows: [u16; 40],
    pub gm: u64,
    pub acc: i64,
    pub b2b: i32,
    pub combo: i32,
    pub pending: i32,
}

#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

impl Hasher for FxHasher {
    fn write(&mut self, mut bytes: &[u8]) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        while bytes.len() >= 8 {
            let v = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            self.hash = (self.hash.rotate_left(5) ^ v).wrapping_mul(K);
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut b = [0u8; 8];
            b[..bytes.len()].copy_from_slice(bytes);
            self.hash = (self.hash.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(K);
        }
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

pub type FxMap<V> = std::collections::HashMap<[u16; 40], V, BuildHasherDefault<FxHasher>>;

fn read_u16(buf: &[u8], off: &mut usize) -> u16 {
    let v = u16::from_le_bytes([buf[*off], buf[*off + 1]]);
    *off += 2;
    v
}

fn read_i8(buf: &[u8], off: &mut usize) -> i8 {
    let v = buf[*off] as i8;
    *off += 1;
    v
}

fn read_i32(buf: &[u8], off: &mut usize) -> i32 {
    let v = i32::from_le_bytes(buf[*off..*off + 4].try_into().unwrap());
    *off += 4;
    v
}

fn read_f64(buf: &[u8], off: &mut usize) -> f64 {
    let v = f64::from_le_bytes(buf[*off..*off + 8].try_into().unwrap());
    *off += 8;
    v
}

pub fn parse_frame(buf: &[u8], off: &mut usize) -> FrameRec {
    let mut rec = FrameRec::default();
    for row in &mut rec.rows {
        *row = read_u16(buf, off) & 0x03ff;
    }
    for row in &mut rec.gmask {
        *row = read_u16(buf, off) & 0x03ff;
    }
    rec.piece = read_i8(buf, off);
    rec.hold = read_i8(buf, off);
    for q in 0..5 {
        rec.queue[q] = read_i8(buf, off);
    }
    rec.b2b = read_i32(buf, off);
    rec.combo = read_i32(buf, off);
    rec.pending = read_i32(buf, off);
    rec.atk = read_f64(buf, off);
    rec.mult = read_f64(buf, off);
    rec
}

pub fn parse_context(buf: &[u8], k: usize) -> ContextRec {
    let mut off = 0;
    let mut rec = ContextRec::with_horizon(k);
    for frame in &mut rec.frames {
        *frame = parse_frame(buf, &mut off);
    }
    for row in &mut rec.opp_rows {
        *row = read_u16(buf, &mut off) & 0x03ff;
    }
    for row in &mut rec.opp_gmask {
        *row = read_u16(buf, &mut off) & 0x03ff;
    }
    rec.opp_piece = read_i8(buf, &mut off);
    for q in 0..5 {
        rec.opp_queue[q] = read_i8(buf, &mut off);
    }
    rec.opp_b2b = read_i32(buf, &mut off);
    rec.opp_combo = read_i32(buf, &mut off);
    rec.opp_pending = read_i32(buf, &mut off);
    rec.opp_mult = read_f64(buf, &mut off);
    rec.outcome = read_i32(buf, &mut off);
    debug_assert_eq!(off, record_bytes(k));
    rec
}

pub fn piece_from_external(v: i8) -> Option<Piece> {
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

pub fn board_from_rows(rows: &[u16; 40]) -> Board {
    let mut board = Board::new();
    board.rows = *rows;
    board.cols = [0; 10];
    for (y, row) in board.rows.iter().enumerate() {
        let mut bits = *row as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            board.cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }
    board
}

pub fn place_rows(rows: &mut [u16; 40], m: &crate::header::Move) {
    let x = m.x();
    let y = m.y();
    let xu = x as usize;
    let yu = y as usize;
    if xu < 10 && yu < 40 {
        rows[yu] |= 1u16 << x;
    }
    let pc = m.cells();
    for i in 0..3 {
        let cx = (pc[i].x as i32 + x) as usize;
        let cy = (pc[i].y as i32 + y) as usize;
        if cx < 10 && cy < 40 {
            rows[cy] |= 1u16 << cx;
        }
    }
}

pub fn clear_rows(rows: &mut [u16; 40], cleared: u64) {
    if cleared == 0 {
        return;
    }
    let mut write = 0usize;
    for read in 0..40 {
        if cleared & (1u64 << read) == 0 {
            rows[write] = rows[read];
            write += 1;
        }
    }
    for row in rows.iter_mut().take(40).skip(write) {
        *row = 0;
    }
}

pub fn line_clears(rows: &[u16; 40]) -> u64 {
    let mut cleared = 0u64;
    for (y, row) in rows.iter().enumerate() {
        if *row == FULL_ROW {
            cleared |= 1u64 << y;
        }
    }
    cleared
}

pub fn is_empty(rows: &[u16; 40]) -> bool {
    rows.iter().all(|&row| row == 0)
}

pub fn gm_bits(gmask: &[u16; 40]) -> u64 {
    let mut bits = 0u64;
    for (y, row) in gmask.iter().enumerate() {
        if *row != 0 {
            bits |= 1u64 << y;
        }
    }
    bits
}

pub fn compact_bits(gm: u64, cleared: u64) -> u64 {
    if cleared == 0 {
        return gm;
    }
    let mut out = 0u64;
    let mut write = 0u32;
    let mut keep = !cleared;
    while keep != 0 {
        let y = keep.trailing_zeros();
        if gm & (1u64 << y) != 0 {
            out |= 1u64 << write;
        }
        write += 1;
        keep &= keep - 1;
    }
    out
}

pub fn nonempty_bits(rows: &[u16; 40]) -> u64 {
    let mut bits = 0u64;
    for (y, row) in rows.iter().enumerate() {
        if *row != 0 {
            bits |= 1u64 << y;
        }
    }
    bits
}

pub fn expand_raw(s: &FrameRec, piece: i8) -> Vec<ExpandRec> {
    let Some(p) = piece_from_external(piece) else { return Vec::new(); };
    let board = board_from_rows(&s.rows);
    let mut moves = MoveBuffer::new();
    generate_playable(&board, &mut moves, p, false);
    let mut out = Vec::with_capacity(moves.as_slice().len());
    for m in moves.as_slice() {
        let mut rows = s.rows;
        place_rows(&mut rows, m);
        let cleared = line_clears(&rows);
        let lines = cleared.count_ones() as u8;
        clear_rows(&mut rows, cleared);
        let garbage_cleared = (cleared & gm_bits(&s.gmask)).count_ones() as u8;
        let attack = calculate_attack_s2_tl_with_multiplier(
            lines,
            m.spin(),
            s.b2b,
            s.combo,
            is_empty(&rows),
            garbage_cleared,
            s.mult,
        );
        out.push(ExpandRec { attack: attack.attack as f64, rows });
    }
    out
}

pub fn bottom_garbage_run(gm: &[u16; 40]) -> usize {
    let mut n = 0usize;
    for row in gm {
        if *row != 0 {
            n += 1;
        } else {
            break;
        }
    }
    n
}

pub fn shift_down_key(rows: &[u16; 40], g: usize) -> [u16; 40] {
    let mut key = [0u16; 40];
    for (y, value) in key.iter_mut().enumerate() {
        *value = rows.get(y + g).copied().unwrap_or(0);
    }
    key
}

pub fn reconstruct(frames: &[FrameRec]) -> Option<(f64, Vec<usize>, Vec<[u16; 40]>)> {
    let k = frames.len().checked_sub(1)?;
    let mut acc = 0.0;
    let mut garbage_counts = vec![0usize; k];
    let mut garbage_rows = vec![[0u16; 40]; k];
    for t in 0..k {
        let s = &frames[t];
        let nx = &frames[t + 1];
        let max_g = bottom_garbage_run(&nx.gmask);
        let tries: [i8; 2] = if s.hold >= 0 && s.hold != s.piece { [s.piece, s.hold] } else { [s.piece, -1] };
        let mut chosen: Option<(usize, usize, Vec<ExpandRec>)> = None;
        for pc in tries {
            if pc < 0 || chosen.is_some() {
                continue;
            }
            let flat = expand_raw(s, pc);
            let mut cand: HashMap<[u16; 40], Vec<usize>> = HashMap::new();
            for (idx, rec) in flat.iter().enumerate() {
                cand.entry(rec.rows).or_default().push(idx);
            }
            for g in 0..=max_g {
                let key = if g == 0 { nx.rows } else { shift_down_key(&nx.rows, g) };
                let Some(offs) = cand.get(&key) else { continue; };
                let mut pick = offs[0];
                for &idx in offs {
                    if (flat[idx].attack - s.atk).abs() < 1e-6 {
                        pick = idx;
                        break;
                    }
                }
                chosen = Some((pick, g, flat));
                break;
            }
        }
        let (pick, g, flat) = chosen?;
        acc += flat[pick].attack;
        garbage_counts[t] = g;
        for (i, row) in garbage_rows[t].iter_mut().enumerate().take(g.min(40)) {
            *row = frames[t + 1].rows[i];
        }
    }
    Some((acc, garbage_counts, garbage_rows))
}

#[allow(clippy::too_many_arguments)]
pub fn beam_best(
    rows0: &[u16; 40],
    gmask0: &[u16; 40],
    pieces: &[i8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep_line: Option<&[[u16; 40]]>,
    multipliers: &[f64],
    garbage_counts: Option<&[usize]>,
    garbage_rows: Option<&[[u16; 40]]>,
    beam_width: usize,
    profile: bool,
) -> f64 {
    if profile {
        add_counter(&PROFILE.beam_calls, 1);
    }
    let mut beam = vec![Node { rows: *rows0, gm: gm_bits(gmask0), acc: 0, b2b, combo, pending }];
    for t in 0..pieces.len() {
        let Some(piece) = piece_from_external(pieces[t]) else { break; };
        let mult = multipliers.get(t).copied().unwrap_or(1.0);
        let gc_insert = garbage_counts.and_then(|counts| counts.get(t)).copied().unwrap_or(0).min(40);
        let mut inserted_bits = 0u64;
        let grow = garbage_rows.and_then(|rows| rows.get(t)).copied().unwrap_or([0u16; 40]);
        for (i, row) in grow.iter().enumerate().take(gc_insert) {
            if *row & 0x03ff != 0 {
                inserted_bits |= 1u64 << i;
            }
        }

        let mut unique: Vec<Node> = Vec::with_capacity(beam_width.saturating_mul(2));
        let mut unique_index = FxMap::<usize>::with_capacity_and_hasher(beam.len().saturating_mul(40), Default::default());
        for node in &beam {
            if profile {
                add_counter(&PROFILE.beam_nodes, 1);
            }
            let board = board_from_rows(&node.rows);
            let mut moves = MoveBuffer::new();
            let gen_start = Instant::now();
            generate_playable(&board, &mut moves, piece, false);
            if profile {
                add_counter(&PROFILE.generate_ns, elapsed_ns(gen_start));
                add_counter(&PROFILE.moves, moves.as_slice().len() as u64);
            }
            let score_start = Instant::now();
            for m in moves.as_slice() {
                let mut rows = node.rows;
                place_rows(&mut rows, m);
                let cleared = line_clears(&rows);
                let lines = cleared.count_ones() as u8;
                clear_rows(&mut rows, cleared);
                let attack = calculate_attack_s2_tl_with_multiplier(
                    lines,
                    m.spin(),
                    node.b2b,
                    node.combo,
                    is_empty(&rows),
                    (cleared & node.gm).count_ones() as u8,
                    mult,
                );
                let mut child_gm = compact_bits(node.gm, cleared);
                if child_gm != 0 {
                    child_gm &= nonempty_bits(&rows);
                }
                if gc_insert > 0 {
                    let mut shifted = [0u16; 40];
                    shifted[gc_insert..40].copy_from_slice(&rows[..(40 - gc_insert)]);
                    for (i, row) in grow.iter().enumerate().take(gc_insert) {
                        shifted[i] = *row & 0x03ff;
                    }
                    rows = shifted;
                    child_gm = ((child_gm << gc_insert) | inserted_bits) & ((1u64 << 40) - 1);
                }
                let child = Node {
                    rows,
                    gm: child_gm,
                    acc: node.acc + attack.attack as i64,
                    b2b: attack.b2b_after,
                    combo: attack.combo_after,
                    pending: (node.pending - lines as i32).max(0),
                };
                if let Some(&idx) = unique_index.get(&child.rows) {
                    if child.acc > unique[idx].acc {
                        unique[idx] = child;
                    }
                } else {
                    unique_index.insert(child.rows, unique.len());
                    unique.push(child);
                }
            }
            if profile {
                add_counter(&PROFILE.score_ns, elapsed_ns(score_start));
            }
        }
        if unique.is_empty() {
            break;
        }
        if profile {
            add_counter(&PROFILE.generated_unique, unique.len() as u64);
        }
        let prune_start = Instant::now();
        unique.sort_by(|a, b| b.acc.cmp(&a.acc));
        let keepb = keep_line.map(|keep| keep[t]);
        let mut pruned = Vec::with_capacity(beam_width);
        let mut kept = false;
        for child in &unique {
            let rows = child.rows;
            if Some(rows) == keepb {
                kept = true;
            }
            pruned.push(child.clone());
            if pruned.len() >= beam_width {
                break;
            }
        }
        if let Some(kb) = keepb {
            if !kept {
                for child in &unique {
                    if child.rows == kb {
                        pruned.push(child.clone());
                        break;
                    }
                }
            }
        }
        beam = pruned;
        if profile {
            add_counter(&PROFILE.prune_ns, elapsed_ns(prune_start));
        }
    }
    beam.iter().map(|node| node.acc).max().unwrap_or(0) as f64
}

/// First-ply candidate afterstate; byte-parity target is rankDumpRecallCandidates (rank-dump-recall.ts).
#[derive(Clone)]
pub struct Candidate {
    pub rows: [u16; 40],
    pub gmask: [u16; 40],
    pub b2b: i32,
    pub combo: i32,
    pub immediate: f64,
}

/// Parity target: compactGmask (rank-dump-recall.ts) — drop cleared rows from gmask VALUES, not a bitmask.
pub fn compact_gmask_rows(gmask: &[u16; 40], cleared: u64) -> [u16; 40] {
    let mut out = [0u16; 40];
    let mut write = 0usize;
    for y in 0..40 {
        if cleared & (1u64 << y) == 0 {
            out[write] = gmask[y];
            write += 1;
        }
    }
    out
}

/// Parity target: expand_all_gm enumeration in rankDumpRecallCandidates — one candidate per playable move, no dedup.
pub fn expand_candidates(s: &FrameRec, piece: i8) -> Vec<Candidate> {
    let Some(p) = piece_from_external(piece) else { return Vec::new(); };
    let board = board_from_rows(&s.rows);
    let mut moves = MoveBuffer::new();
    generate_playable(&board, &mut moves, p, false);
    let mut out = Vec::with_capacity(moves.as_slice().len());
    for m in moves.as_slice() {
        let mut rows = s.rows;
        place_rows(&mut rows, m);
        let cleared = line_clears(&rows);
        let lines = cleared.count_ones() as u8;
        let gmask = compact_gmask_rows(&s.gmask, cleared);
        clear_rows(&mut rows, cleared);
        let garbage_cleared = (cleared & gm_bits(&s.gmask)).count_ones() as u8;
        let attack = calculate_attack_s2_tl_with_multiplier(
            lines,
            m.spin(),
            s.b2b,
            s.combo,
            is_empty(&rows),
            garbage_cleared,
            s.mult,
        );
        out.push(Candidate {
            rows,
            gmask,
            b2b: attack.b2b_after,
            combo: attack.combo_after,
            immediate: attack.attack as f64,
        });
    }
    out
}

/// Parity target: injectGarbage (rank-dump-k7-attack.ts) — bottom garbage insert, shift up, mark gmask.
pub fn inject_garbage(rows: &[u16; 40], gmask: &[u16; 40], count: usize, garbage: &[u16; 40]) -> ([u16; 40], [u16; 40]) {
    if count == 0 {
        return (*rows, *gmask);
    }
    let mut nr = [0u16; 40];
    let mut ng = [0u16; 40];
    for i in 0..count.min(40) {
        nr[i] = garbage[i];
        ng[i] = garbage[i];
    }
    let mut y = 0usize;
    while y + count < 40 {
        nr[y + count] = rows[y];
        ng[y + count] = gmask[y];
        y += 1;
    }
    (nr, ng)
}

pub fn height_holes(rows: &[u16; 40]) -> (u32, u32) {
    let mut max_h = 0u32;
    let mut holes = 0u32;
    for x in 0..10 {
        let mut top: i32 = -1;
        for y in (0..40).rev() {
            if rows[y] & (1u16 << x) != 0 {
                top = y as i32;
                break;
            }
        }
        if top >= 0 {
            max_h = max_h.max(top as u32 + 1);
            for row in rows.iter().take(top as usize) {
                if *row & (1u16 << x) == 0 {
                    holes += 1;
                }
            }
        }
    }
    (max_h, holes)
}
