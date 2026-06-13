// Native micro-benchmark + correctness gate for beam-kernel optimizations.
// Compares the BASELINE beam (faithful replica of beam_best_gm_wasm: Board clone
// + gm:[u64;40] per child, fresh Vecs per ply, HashSet<[u16;40]> dedup) against
// an OPTIMIZED beam (gm:u64 row-bitmask, arena/double-buffered child Vecs, reused
// seen set). Both MUST return the same best attack.
// Run: cargo run --release --bin bench_beam -- [iters] [beam]
use std::hash::{BuildHasherDefault, Hasher};
use std::time::Instant;

use direct_cobra_copy::attack::calculate_attack_s2_tl_with_multiplier;

// FxHash: fast non-cryptographic hasher (rustc-hash style). Exact dedup, no
// SipHash DoS overhead. Key stays [u16;40] so dedup remains bit-exact.
#[derive(Default)]
struct FxHasher { hash: u64 }
const FX_K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let v = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            self.hash = (self.hash.rotate_left(5) ^ v).wrapping_mul(FX_K);
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut buf = [0u8; 8];
            buf[..bytes.len()].copy_from_slice(bytes);
            let v = u64::from_le_bytes(buf);
            self.hash = (self.hash.rotate_left(5) ^ v).wrapping_mul(FX_K);
        }
    }
    #[inline]
    fn finish(&self) -> u64 { self.hash }
}
type FxSet = std::collections::HashSet<[u16; 40], BuildHasherDefault<FxHasher>>;
use direct_cobra_copy::board::Board;
use direct_cobra_copy::header::Piece;
use direct_cobra_copy::move_buffer::MoveBuffer;
use direct_cobra_copy::movegen::generate;

const BOARD_ROWS: [u16; 40] = {
    let mut r = [0u16; 40];
    r[0] = 0x37F; r[1] = 0x3BF; r[2] = 0x1FF; r[3] = 0x3FD; r[4] = 0x2FF; r[5] = 0x07F;
    r
};
const PIECES: [u8; 5] = [0, 1, 2, 3, 4];

fn piece_from_external(v: u8) -> Option<Piece> {
    match v { 0 => Some(Piece::I), 1 => Some(Piece::O), 2 => Some(Piece::T), 3 => Some(Piece::L),
        4 => Some(Piece::J), 5 => Some(Piece::S), 6 => Some(Piece::Z), _ => None }
}
fn board_from_row_bitmasks(rows: &[u64; 40]) -> Board {
    let mut b = Board::new();
    for (y, row) in rows.iter().enumerate().take(40) { b.rows[y] = (*row & 0x3FF) as u16; }
    b.cols = [0; 10];
    for (y, row) in b.rows.iter().enumerate() {
        let mut bits = *row as u64;
        while bits != 0 { let x = bits.trailing_zeros() as usize; b.cols[x] |= 1u64 << y; bits &= bits - 1; }
    }
    b
}

// ----------------------------- BASELINE -----------------------------
fn count_cleared_garbage_rows_arr(cleared: u64, gm: &[u64; 40]) -> u8 {
    let mut c = 0u8;
    for (y, row) in gm.iter().enumerate().take(40) { if cleared & (1u64 << y) != 0 && *row != 0 { c += 1; } }
    c
}
fn compact_garbage_rows_arr(gm: &[u64; 40], cleared: u64) -> [u64; 40] {
    if cleared == 0 { return *gm; }
    let mut out = [0u64; 40]; let mut w = 0usize;
    for (read, row) in gm.iter().enumerate().take(40) { if cleared & (1u64 << read) == 0 { out[w] = *row; w += 1; } }
    out
}
fn beam_baseline(rows0: &[u64; 40], gm0: &[u64; 40], pieces: &[u8], bw: usize) -> f64 {
    struct BNode { board: Board, gm: [u64; 40], acc: i64, b2b: i32, combo: i32, pending: i32 }
    struct Child { board: Board, acc: i64, b2b: i32, combo: i32, pending: i32, gm: [u64; 40] }
    let mut beam: Vec<BNode> = vec![BNode { board: board_from_row_bitmasks(rows0), gm: *gm0, acc: 0, b2b: 0, combo: 0, pending: 0 }];
    for &pe in pieces {
        let p = match piece_from_external(pe) { Some(p) => p, None => break };
        let mut children: Vec<Child> = Vec::new();
        for node in &beam {
            let mut moves = MoveBuffer::new();
            generate(&node.board, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut nb = node.board.clone();
                nb.place(m);
                let cleared = nb.line_clears();
                let lines = cleared.count_ones() as u8;
                if cleared != 0 { nb.clear_lines(cleared); }
                let spin = m.spin();
                let gc = count_cleared_garbage_rows_arr(cleared, &node.gm);
                let at = calculate_attack_s2_tl_with_multiplier(lines, spin, node.b2b, node.combo, nb.is_empty(), gc, 1.0);
                let mut cgm = compact_garbage_rows_arr(&node.gm, cleared);
                for (y, row) in cgm.iter_mut().enumerate() { *row &= nb.rows[y] as u64; }
                children.push(Child { board: nb, acc: node.acc + at.attack as i64, b2b: at.b2b_after, combo: at.combo_after, pending: (node.pending - lines as i32).max(0), gm: cgm });
            }
        }
        if children.is_empty() { break; }
        let mut idx: Vec<usize> = (0..children.len()).collect();
        idx.sort_by(|&a, &b| children[b].acc.cmp(&children[a].acc));
        let mut seen: std::collections::HashSet<[u16; 40]> = std::collections::HashSet::new();
        let mut pruned: Vec<BNode> = Vec::with_capacity(bw);
        for &ci in &idx {
            let c = &children[ci];
            if !seen.insert(c.board.rows) { continue; }
            pruned.push(BNode { board: c.board.clone(), gm: c.gm, acc: c.acc, b2b: c.b2b, combo: c.combo, pending: c.pending });
            if pruned.len() >= bw { break; }
        }
        beam = pruned;
    }
    beam.iter().map(|n| n.acc).max().unwrap_or(0) as f64
}

// ----------------------------- OPTIMIZED -----------------------------
// gm as u64 row-bitmask (bit y = row y still has >=1 garbage cell). Arena: child
// buffers reused across plies; reused seen set.
#[derive(Clone)]
struct ONode { board: Board, gm: u64, acc: i64, b2b: i32, combo: i32, pending: i32 }

#[inline]
fn compact_bits(gm: u64, cleared: u64) -> u64 {
    if cleared == 0 { return gm; }
    let mut out = 0u64; let mut w = 0u32;
    let mut k = !cleared;
    while k != 0 {
        let y = k.trailing_zeros();
        if gm & (1u64 << y) != 0 { out |= 1u64 << w; }
        w += 1;
        k &= k - 1;
    }
    out
}

struct Arena { cur: Vec<ONode>, nxt: Vec<ONode>, seen: FxSet, idx: Vec<usize> }
impl Arena {
    fn new(bw: usize) -> Self { Arena { cur: Vec::with_capacity(bw), nxt: Vec::with_capacity(bw * 40), seen: FxSet::with_capacity_and_hasher(bw * 40, Default::default()), idx: Vec::with_capacity(bw * 40) } }
}

fn beam_opt(rows0: &[u64; 40], gm0_bits: u64, pieces: &[u8], bw: usize, ar: &mut Arena) -> f64 {
    ar.cur.clear();
    ar.cur.push(ONode { board: board_from_row_bitmasks(rows0), gm: gm0_bits, acc: 0, b2b: 0, combo: 0, pending: 0 });
    for &pe in pieces {
        let p = match piece_from_external(pe) { Some(p) => p, None => break };
        ar.nxt.clear();
        for node in &ar.cur {
            let mut moves = MoveBuffer::new();
            generate(&node.board, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut nb = node.board.clone();
                nb.place(m);
                let cleared = nb.line_clears();
                let lines = cleared.count_ones() as u8;
                if cleared != 0 { nb.clear_lines(cleared); }
                let spin = m.spin();
                let gc = (cleared & node.gm).count_ones() as u8;
                let at = calculate_attack_s2_tl_with_multiplier(lines, spin, node.b2b, node.combo, nb.is_empty(), gc, 1.0);
                let mut cgm = compact_bits(node.gm, cleared);
                if cgm != 0 {
                    let mut ne = 0u64;
                    for (y, row) in nb.rows.iter().enumerate() { if *row != 0 { ne |= 1u64 << y; } }
                    cgm &= ne;
                }
                ar.nxt.push(ONode { board: nb, gm: cgm, acc: node.acc + at.attack as i64, b2b: at.b2b_after, combo: at.combo_after, pending: (node.pending - lines as i32).max(0) });
            }
        }
        if ar.nxt.is_empty() { ar.cur.clear(); break; }
        ar.idx.clear();
        ar.idx.extend(0..ar.nxt.len());
        let nxt = &ar.nxt;
        ar.idx.sort_by(|&a, &b| nxt[b].acc.cmp(&nxt[a].acc));
        ar.seen.clear();
        let mut kept: Vec<ONode> = Vec::with_capacity(bw);
        for &ci in &ar.idx {
            let rows = ar.nxt[ci].board.rows;
            if !ar.seen.insert(rows) { continue; }
            kept.push(ar.nxt[ci].clone());
            if kept.len() >= bw { break; }
        }
        ar.cur = kept;
    }
    ar.cur.iter().map(|n| n.acc).max().unwrap_or(0) as f64
}

#[derive(Clone)]
struct O2Node { rows: [u16; 40], gm: u64, acc: i64, b2b: i32, combo: i32, pending: i32 }

#[inline]
fn board_from_u16(rows: &[u16; 40]) -> Board {
    let mut b = Board::new();
    b.rows = *rows;
    b.cols = [0; 10];
    for (y, row) in b.rows.iter().enumerate() {
        let mut bits = *row as u64;
        while bits != 0 { let x = bits.trailing_zeros() as usize; b.cols[x] |= 1u64 << y; bits &= bits - 1; }
    }
    b
}

struct Arena2 { cur: Vec<O2Node>, nxt: Vec<O2Node>, seen: FxSet, idx: Vec<usize> }
impl Arena2 {
    fn new(bw: usize) -> Self { Arena2 { cur: Vec::with_capacity(bw), nxt: Vec::with_capacity(bw * 40), seen: FxSet::with_capacity_and_hasher(bw * 40, Default::default()), idx: Vec::with_capacity(bw * 40) } }
}

fn beam_opt2(rows0: &[u16; 40], gm0_bits: u64, pieces: &[u8], bw: usize, ar: &mut Arena2) -> f64 {
    ar.cur.clear();
    ar.cur.push(O2Node { rows: *rows0, gm: gm0_bits, acc: 0, b2b: 0, combo: 0, pending: 0 });
    for &pe in pieces {
        let p = match piece_from_external(pe) { Some(p) => p, None => break };
        ar.nxt.clear();
        for node in &ar.cur {
            let nb_board = board_from_u16(&node.rows);
            let mut moves = MoveBuffer::new();
            generate(&nb_board, &mut moves, p, false);
            for m in moves.as_slice() {
                // must mirror Board::place bounds semantics exactly (`as usize` wrap) or dedup keys diverge from baseline
                let mut cr = node.rows;
                let x = m.x(); let y = m.y();
                let xu = x as usize; let yu = y as usize;
                if xu < 10 && yu < 40 { cr[yu] |= 1u16 << x; }
                let pc = m.cells();
                for i in 0..3 {
                    let cx = (pc[i].x as i32 + x) as usize;
                    let cy = (pc[i].y as i32 + y) as usize;
                    if cx < 10 && cy < 40 { cr[cy] |= 1u16 << cx; }
                }
                let mut cleared = 0u64;
                for (yy, row) in cr.iter().enumerate() { if *row == 0x3FF { cleared |= 1u64 << yy; } }
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    let mut w = 0usize;
                    let old_rows = cr;
                    for (read, row) in old_rows.iter().enumerate() { if cleared & (1u64 << read) == 0 { cr[w] = *row; w += 1; } }
                    for row in cr.iter_mut().skip(w) { *row = 0; }
                }
                let is_empty = cr.iter().all(|&r| r == 0);
                let spin = m.spin();
                let gc = (cleared & node.gm).count_ones() as u8;
                let at = calculate_attack_s2_tl_with_multiplier(lines, spin, node.b2b, node.combo, is_empty, gc, 1.0);
                let mut cgm = compact_bits(node.gm, cleared);
                if cgm != 0 {
                    let mut ne = 0u64;
                    for (y, row) in cr.iter().enumerate() { if *row != 0 { ne |= 1u64 << y; } }
                    cgm &= ne;
                }
                ar.nxt.push(O2Node { rows: cr, gm: cgm, acc: node.acc + at.attack as i64, b2b: at.b2b_after, combo: at.combo_after, pending: (node.pending - lines as i32).max(0) });
            }
        }
        if ar.nxt.is_empty() { ar.cur.clear(); break; }
        ar.idx.clear();
        ar.idx.extend(0..ar.nxt.len());
        let nxt = &ar.nxt;
        ar.idx.sort_by(|&a, &b| nxt[b].acc.cmp(&nxt[a].acc));
        ar.seen.clear();
        let mut kept: Vec<O2Node> = Vec::with_capacity(bw);
        for &ci in &ar.idx {
            let rows = ar.nxt[ci].rows;
            if !ar.seen.insert(rows) { continue; }
            kept.push(ar.nxt[ci].clone());
            if kept.len() >= bw { break; }
        }
        ar.cur = kept;
    }
    ar.cur.iter().map(|n| n.acc).max().unwrap_or(0) as f64
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let iters: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let beam: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300);

    let mut rows0 = [0u64; 40];
    for (y, row) in BOARD_ROWS.iter().enumerate() { rows0[y] = *row as u64; }
    let gm0 = [0u64; 40];
    let gm0_bits = 0u64;

    let rb = beam_baseline(&rows0, &gm0, &PIECES, beam);
    let mut ar = Arena::new(beam);
    let ro = beam_opt(&rows0, gm0_bits, &PIECES, beam, &mut ar);
    let mut ar2 = Arena2::new(beam);
    let ro2 = beam_opt2(&BOARD_ROWS, gm0_bits, &PIECES, beam, &mut ar2);
    assert_eq!(rb, ro, "OPT result {ro} != BASELINE {rb}");
    assert_eq!(rb, ro2, "OPT2 result {ro2} != BASELINE {rb}");
    println!("correctness: baseline==opt==opt2 == {rb}");

    let mut sink = 0f64;
    for _ in 0..(iters / 10).max(1) { sink += beam_baseline(&rows0, &gm0, &PIECES, beam); }
    let t0 = Instant::now();
    for _ in 0..iters { sink += beam_baseline(&rows0, &gm0, &PIECES, beam); }
    let dt_base = t0.elapsed().as_secs_f64();

    for _ in 0..(iters / 10).max(1) { sink += beam_opt(&rows0, gm0_bits, &PIECES, beam, &mut ar); }
    let t1 = Instant::now();
    for _ in 0..iters { sink += beam_opt(&rows0, gm0_bits, &PIECES, beam, &mut ar); }
    let dt_opt = t1.elapsed().as_secs_f64();

    for _ in 0..(iters / 10).max(1) { sink += beam_opt2(&BOARD_ROWS, gm0_bits, &PIECES, beam, &mut ar2); }
    let t2 = Instant::now();
    for _ in 0..iters { sink += beam_opt2(&BOARD_ROWS, gm0_bits, &PIECES, beam, &mut ar2); }
    let dt_opt2 = t2.elapsed().as_secs_f64();

    eprintln!("sink={sink}");
    println!("BASELINE beam={beam} {:.3} ms/call  ({:.1} calls/s)", dt_base * 1000.0 / iters as f64, iters as f64 / dt_base);
    println!("OPT      beam={beam} {:.3} ms/call  ({:.1} calls/s)", dt_opt * 1000.0 / iters as f64, iters as f64 / dt_opt);
    println!("OPT2     beam={beam} {:.3} ms/call  ({:.1} calls/s)", dt_opt2 * 1000.0 / iters as f64, iters as f64 / dt_opt2);
    println!("=> opt {:.2}x  opt2 {:.2}x  (opt2 vs opt {:.2}x)", dt_base / dt_opt, dt_base / dt_opt2, dt_opt / dt_opt2);
}
