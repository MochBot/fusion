use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::hash::{BuildHasherDefault, Hasher};
use std::io::{self, Read, Write};
use std::time::Instant;

use direct_cobra_copy::attack::calculate_attack_s2_tl_with_multiplier;
use direct_cobra_copy::board::{Board, FULL_ROW};
use direct_cobra_copy::header::Piece;
use direct_cobra_copy::move_buffer::MoveBuffer;
use direct_cobra_copy::movegen::generate;
use rayon::prelude::*;

const K: usize = 5;
const N_FRAMES: usize = K + 1;
const FRAME_BYTES: usize = 195;
const OPP_BYTES: usize = 186;
const RECORD_BYTES: usize = FRAME_BYTES * N_FRAMES + OPP_BYTES + 4;
const STRIDE_F: usize = 464;
const STRIDE_BYTES: usize = STRIDE_F * 4;
const BEAM: usize = 300;
const BEAM_FEAT: usize = 150;

#[derive(Clone)]
struct FrameRec {
    rows: [u16; 40],
    gmask: [u16; 40],
    piece: i8,
    hold: i8,
    queue: [i8; 5],
    b2b: i32,
    combo: i32,
    pending: i32,
    atk: f64,
    mult: f64,
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
struct ContextRec {
    frames: [FrameRec; N_FRAMES],
    opp_rows: [u16; 40],
    opp_gmask: [u16; 40],
    opp_piece: i8,
    opp_queue: [i8; 5],
    opp_b2b: i32,
    opp_combo: i32,
    opp_pending: i32,
    opp_mult: f64,
    outcome: i32,
}

impl Default for ContextRec {
    fn default() -> Self {
        Self {
            frames: std::array::from_fn(|_| FrameRec::default()),
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
struct ExpandRec {
    attack: f64,
    rows: [u16; 40],
}

#[derive(Clone)]
struct Node {
    rows: [u16; 40],
    gm: u64,
    acc: i64,
    b2b: i32,
    combo: i32,
    pending: i32,
}

#[derive(Default)]
struct FxHasher {
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

type FxSet = std::collections::HashSet<[u16; 40], BuildHasherDefault<FxHasher>>;

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

fn parse_frame(buf: &[u8], off: &mut usize) -> FrameRec {
    let mut rec = FrameRec::default();
    for y in 0..40 {
        rec.rows[y] = read_u16(buf, off) & 0x03ff;
    }
    for y in 0..40 {
        rec.gmask[y] = read_u16(buf, off) & 0x03ff;
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

fn parse_context(buf: &[u8]) -> ContextRec {
    let mut off = 0;
    let mut rec = ContextRec::default();
    for t in 0..N_FRAMES {
        rec.frames[t] = parse_frame(buf, &mut off);
    }
    for y in 0..40 {
        rec.opp_rows[y] = read_u16(buf, &mut off) & 0x03ff;
    }
    for y in 0..40 {
        rec.opp_gmask[y] = read_u16(buf, &mut off) & 0x03ff;
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
    debug_assert_eq!(off, RECORD_BYTES);
    rec
}

fn piece_from_external(v: i8) -> Option<Piece> {
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

fn board_from_rows(rows: &[u16; 40]) -> Board {
    let mut board = Board::new();
    board.rows = *rows;
    board.cols = [0; 10];
    for y in 0..40 {
        let mut bits = board.rows[y] as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            board.cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }
    board
}

fn place_rows(rows: &mut [u16; 40], m: &direct_cobra_copy::header::Move) {
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

fn clear_rows(rows: &mut [u16; 40], cleared: u64) {
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

fn line_clears(rows: &[u16; 40]) -> u64 {
    let mut cleared = 0u64;
    for (y, row) in rows.iter().enumerate() {
        if *row == FULL_ROW {
            cleared |= 1u64 << y;
        }
    }
    cleared
}

fn is_empty(rows: &[u16; 40]) -> bool {
    rows.iter().all(|&row| row == 0)
}

fn gm_bits(gmask: &[u16; 40]) -> u64 {
    let mut bits = 0u64;
    for (y, row) in gmask.iter().enumerate() {
        if *row != 0 {
            bits |= 1u64 << y;
        }
    }
    bits
}

fn compact_bits(gm: u64, cleared: u64) -> u64 {
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

fn nonempty_bits(rows: &[u16; 40]) -> u64 {
    let mut bits = 0u64;
    for (y, row) in rows.iter().enumerate() {
        if *row != 0 {
            bits |= 1u64 << y;
        }
    }
    bits
}

fn expand_raw(s: &FrameRec, piece: i8) -> Vec<ExpandRec> {
    let Some(p) = piece_from_external(piece) else { return Vec::new(); };
    let board = board_from_rows(&s.rows);
    let mut moves = MoveBuffer::new();
    generate(&board, &mut moves, p, false);
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

fn bottom_garbage_run(gm: &[u16; 40]) -> usize {
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

fn shift_down_key(rows: &[u16; 40], g: usize) -> [u16; 40] {
    let mut key = [0u16; 40];
    for y in 0..40 {
        key[y] = rows.get(y + g).copied().unwrap_or(0);
    }
    key
}

fn reconstruct(frames: &[FrameRec; N_FRAMES]) -> Option<(f64, [usize; K], [[u16; 40]; K])> {
    let mut acc = 0.0;
    let mut garbage_counts = [0usize; K];
    let mut garbage_rows = [[0u16; 40]; K];
    for t in 0..K {
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
        for i in 0..g.min(40) {
            garbage_rows[t][i] = frames[t + 1].rows[i];
        }
    }
    Some((acc, garbage_counts, garbage_rows))
}

fn beam_best(
    rows0: &[u16; 40],
    gmask0: &[u16; 40],
    pieces: &[i8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep_line: Option<&[[u16; 40]; K]>,
    multipliers: &[f64],
    garbage_counts: Option<&[usize; K]>,
    garbage_rows: Option<&[[u16; 40]; K]>,
    beam_width: usize,
) -> f64 {
    let mut beam = vec![Node { rows: *rows0, gm: gm_bits(gmask0), acc: 0, b2b, combo, pending }];
    for t in 0..pieces.len() {
        let Some(piece) = piece_from_external(pieces[t]) else { break; };
        let mult = multipliers.get(t).copied().unwrap_or(1.0);
        let gc_insert = garbage_counts.map(|counts| counts[t]).unwrap_or(0).min(40);
        let mut inserted_bits = 0u64;
        let grow = garbage_rows.map(|rows| rows[t]).unwrap_or([0u16; 40]);
        for (i, row) in grow.iter().enumerate().take(gc_insert) {
            if *row & 0x03ff != 0 {
                inserted_bits |= 1u64 << i;
            }
        }

        let mut children: Vec<Node> = Vec::with_capacity(beam.len().saturating_mul(40));
        for node in &beam {
            let board = board_from_rows(&node.rows);
            let mut moves = MoveBuffer::new();
            generate(&board, &mut moves, piece, false);
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
                    for y in gc_insert..40 {
                        shifted[y] = rows[y - gc_insert];
                    }
                    for i in 0..gc_insert {
                        shifted[i] = grow[i] & 0x03ff;
                    }
                    rows = shifted;
                    child_gm = ((child_gm << gc_insert) | inserted_bits) & ((1u64 << 40) - 1);
                }
                children.push(Node {
                    rows,
                    gm: child_gm,
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
        let mut idx: Vec<usize> = (0..children.len()).collect();
        idx.sort_by(|&a, &b| children[b].acc.cmp(&children[a].acc));
        let keepb = keep_line.map(|keep| keep[t]);
        let mut seen = FxSet::with_capacity_and_hasher(children.len(), Default::default());
        let mut pruned = Vec::with_capacity(beam_width);
        let mut kept = false;
        for &ci in &idx {
            let rows = children[ci].rows;
            if !seen.insert(rows) {
                continue;
            }
            if Some(rows) == keepb {
                kept = true;
            }
            pruned.push(children[ci].clone());
            if pruned.len() >= beam_width {
                break;
            }
        }
        if let Some(kb) = keepb {
            if !kept {
                for &ci in &idx {
                    if children[ci].rows == kb {
                        pruned.push(children[ci].clone());
                        break;
                    }
                }
            }
        }
        beam = pruned;
    }
    beam.iter().map(|node| node.acc).max().unwrap_or(0) as f64
}

fn feat_attack(rows: &[u16; 40], gmask: &[u16; 40], pieces: [i8; K], b2b: i32, combo: i32, pending: i32, mult: f64) -> f64 {
    if pieces.iter().any(|&p| p < 0) {
        return 0.0;
    }
    beam_best(rows, gmask, &pieces, b2b.max(0), combo.max(0), pending.max(0), None, &[mult; K], None, None, BEAM_FEAT)
}

fn height_holes(rows: &[u16; 40]) -> (u32, u32) {
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
            for y in 0..top as usize {
                if rows[y] & (1u16 << x) == 0 {
                    holes += 1;
                }
            }
        }
    }
    (max_h, holes)
}

fn put_f32(out: &mut [u8; STRIDE_BYTES], off: &mut usize, value: f32) {
    out[*off..*off + 4].copy_from_slice(&value.to_le_bytes());
    *off += 4;
}

fn label_record(ctx: &ContextRec) -> [u8; STRIDE_BYTES] {
    let rec = &ctx.frames[0];
    let pieces = [ctx.frames[0].piece, ctx.frames[1].piece, ctx.frames[2].piece, ctx.frames[3].piece, ctx.frames[4].piece];
    let multipliers = [ctx.frames[0].mult, ctx.frames[1].mult, ctx.frames[2].mult, ctx.frames[3].mult, ctx.frames[4].mult];
    let recon = reconstruct(&ctx.frames);
    let matched = if recon.is_some() { 1.0 } else { 0.0 };
    let player_atk = recon.as_ref().map(|r| r.0).unwrap_or_else(|| ctx.frames[..K].iter().map(|f| f.atk).sum());
    let best = if let Some((_, garbage_counts, garbage_rows)) = &recon {
        let keep = [ctx.frames[1].rows, ctx.frames[2].rows, ctx.frames[3].rows, ctx.frames[4].rows, ctx.frames[5].rows];
        beam_best(&rec.rows, &rec.gmask, &pieces, rec.b2b, rec.combo, rec.pending, Some(&keep), &multipliers, Some(garbage_counts), Some(garbage_rows), BEAM)
    } else {
        beam_best(&rec.rows, &rec.gmask, &pieces, rec.b2b, rec.combo, rec.pending, None, &[rec.mult; K], None, None, BEAM)
    };
    let my_pieces = [rec.piece, rec.queue[0], rec.queue[1], rec.queue[2], rec.queue[3]];
    let opp_pieces = [ctx.opp_piece, ctx.opp_queue[0], ctx.opp_queue[1], ctx.opp_queue[2], ctx.opp_queue[3]];
    let my_best = feat_attack(&rec.rows, &rec.gmask, my_pieces, rec.b2b, rec.combo, rec.pending, rec.mult).max(0.0);
    let opp_best = feat_attack(&ctx.opp_rows, &ctx.opp_gmask, opp_pieces, ctx.opp_b2b, ctx.opp_combo, ctx.opp_pending, ctx.opp_mult).max(0.0);
    let (oh, ohl) = height_holes(&ctx.opp_rows);

    let mut out = [0u8; STRIDE_BYTES];
    let mut off = 0;
    for y in 0..40 {
        for x in 0..10 {
            put_f32(&mut out, &mut off, if rec.rows[y] & (1u16 << x) != 0 { 1.0 } else { 0.0 });
        }
    }
    for p in 0..7 {
        put_f32(&mut out, &mut off, if rec.piece == p { 1.0 } else { 0.0 });
    }
    for p in 0..7 {
        put_f32(&mut out, &mut off, if rec.hold == p { 1.0 } else { 0.0 });
    }
    for q in 0..5 {
        for p in 0..7 {
            put_f32(&mut out, &mut off, if rec.queue[q] == p { 1.0 } else { 0.0 });
        }
    }
    put_f32(&mut out, &mut off, rec.b2b.max(0) as f32);
    put_f32(&mut out, &mut off, rec.combo.max(0) as f32);
    put_f32(&mut out, &mut off, ctx.opp_b2b.max(0) as f32);
    put_f32(&mut out, &mut off, ctx.opp_combo.max(0) as f32);
    put_f32(&mut out, &mut off, ctx.opp_pending as f32);
    put_f32(&mut out, &mut off, oh as f32);
    put_f32(&mut out, &mut off, ohl as f32);
    put_f32(&mut out, &mut off, rec.pending as f32);
    put_f32(&mut out, &mut off, my_best as f32);
    put_f32(&mut out, &mut off, opp_best as f32);
    put_f32(&mut out, &mut off, best as f32);
    put_f32(&mut out, &mut off, player_atk as f32);
    put_f32(&mut out, &mut off, (best - player_atk) as f32);
    put_f32(&mut out, &mut off, ctx.outcome as f32);
    put_f32(&mut out, &mut off, matched as f32);
    debug_assert_eq!(off, STRIDE_BYTES);
    out
}

fn read_input() -> io::Result<Vec<u8>> {
    let mut input = Vec::new();
    if let Some(path) = env::args().nth(1) {
        File::open(path)?.read_to_end(&mut input)?;
    } else {
        io::stdin().read_to_end(&mut input)?;
    }
    Ok(input)
}

fn main() -> io::Result<()> {
    let input = read_input()?;
    if input.len() % RECORD_BYTES != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("input size {} is not a multiple of {RECORD_BYTES}", input.len())));
    }
    let bench = env::var_os("LABEL_OPP_BENCH").is_some();
    let t0 = Instant::now();
    let records: Vec<[u8; STRIDE_BYTES]> = input.par_chunks_exact(RECORD_BYTES).map(|chunk| label_record(&parse_context(chunk))).collect();
    let mut stdout = io::stdout().lock();
    for record in &records {
        stdout.write_all(record)?;
    }
    if bench {
        let dt = t0.elapsed().as_secs_f64();
        eprintln!("label_opp records={} samples/s={:.2}", records.len(), records.len() as f64 / dt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_record_size_matches_emitter_schema() {
        assert_eq!(RECORD_BYTES, 1360);
        assert_eq!(STRIDE_BYTES, 1856);
    }

    #[test]
    fn empty_context_labels_have_expected_static_features() {
        let mut ctx = ContextRec::default();
        for frame in &mut ctx.frames {
            frame.piece = 0;
            frame.hold = 1;
            frame.queue = [2, 3, 4, 5, 6];
            frame.mult = 1.0;
        }
        ctx.opp_piece = 0;
        ctx.opp_queue = [1, 2, 3, 4, 5];
        ctx.opp_mult = 1.0;
        let out = label_record(&ctx);
        assert_eq!(out.len(), STRIDE_BYTES);
        let piece0 = f32::from_le_bytes(out[400 * 4..401 * 4].try_into().unwrap());
        assert_eq!(piece0, 1.0);
    }
}
