// perft.rs -- strict placement-tree perft: each distinct reachable
// placement expands exactly once, bulk counting at depth 1.
use crate::board::{Board, BOARD_HEIGHT, FULL_ROW};
use crate::header::*;
use crate::move_buffer::MoveBuffer;
use crate::smear::{band_words, h_gen, SBoard, TLINES};
use crate::smear_core::{
    count_smear_band, count_smear_band_pair, count_smear_rows, generate_placements, pack_band,
};
#[cfg(feature = "rayon")]
use rayon::prelude::*;

const QUEUE: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
    Piece::T,
];

fn queue_piece(depth: usize) -> Piece {
    QUEUE[depth % 7]
}

fn placements(board: &Board, piece: Piece) -> MoveBuffer {
    let mut moves = MoveBuffer::new();
    generate_placements(board, &mut moves, piece, false);
    moves
}

/// serial perft: clone + do_move, bulk counting at depth 1
pub fn perft(board: &Board, queue_offset: usize, depth: usize) -> u64 {
    if depth == 0 {
        return 1;
    }

    let piece = queue_piece(queue_offset);

    if depth == 1 {
        return u64::from(crate::movegen::count_moves_dispatch(board, piece, false));
    }

    let ml = placements(board, piece);

    if ml.is_empty() {
        return 0;
    }

    if depth == 2 {
        return last_level(board, &ml, queue_piece(queue_offset + 1));
    }

    let mut nodes: u64 = 0;
    for m in ml.iter() {
        let mut child = board.clone();
        child.do_move(m);
        nodes += perft(&child, queue_offset + 1, depth - 1);
    }
    nodes
}

// Fused final level: children are leaves consumed only by bulk counting,
// so full Board clone, cols maintenance, and do_move line_clears are
// skipped. Clear detection is bounded to the piece's 4-row cell span.
fn last_level(board: &Board, ml: &MoveBuffer, p2: Piece) -> u64 {
    let mut occ = 0u64;
    for c in 0..COL_NB {
        occ |= board.cols[c];
    }
    let parent_h = (64 - occ.leading_zeros()) as usize;
    let parent_sb: SBoard<8> = pack_band(&board.rows);
    let hg = h_gen(p2 as usize);
    let tl = TLINES as usize;
    let mut nodes = 0u64;
    let mut pending_delta = [0u64; 8];
    let mut pending_h = 0usize;
    let mut pending_words = 0usize;
    let mut has_pending = false;
    for m in ml.iter() {
        // generate_placements emits strictly reachable lock placements
        // only (pinned by 42k-case parity and reference-BFS cross-check),
        // so legal_lock_placement re-validation is skipped.
        let pc = m.cells();
        let x = m.x();
        let y = m.y();
        let mut delta = [0u64; 8];
        let mut ymin = BOARD_HEIGHT;
        let mut ymax = 0usize;
        let mut add_cell = |cx: i32, cy: i32| {
            let (cxu, cyu) = (cx as usize, cy as usize);
            debug_assert!(cxu < COL_NB && cyu < BOARD_HEIGHT);
            delta[cyu / tl] |= 1u64 << ((cyu % tl) * COL_NB + cxu);
            ymin = ymin.min(cyu);
            ymax = ymax.max(cyu);
        };
        add_cell(x, y);
        for i in 0..3 {
            add_cell(pc[i].x as i32 + x, pc[i].y as i32 + y);
        }
        let full: u64 = FULL_ROW as u64;
        let mut cleared_rows = 0u64;
        for yy in ymin..=ymax {
            let lane = full << ((yy % tl) * COL_NB);
            if (parent_sb.d[yy / tl] | delta[yy / tl]) & lane == lane {
                cleared_rows |= 1u64 << yy;
            }
        }
        // Post-place height: piece raises stack to ymax+1 or leaves
        // parent_h; each cleared row sits below the top, so compaction
        // lowers by exactly popcount(cleared).
        let h = parent_h.max(ymax + 1);
        if cleared_rows == 0 {
            let words = band_words(h as i32 + hg);
            if has_pending && pending_words == words {
                let (left, right) = count_child_pair_by_words(
                    words,
                    &parent_sb,
                    &pending_delta,
                    pending_h,
                    &delta,
                    h,
                    p2,
                );
                nodes += u64::from(left) + u64::from(right);
                has_pending = false;
            } else {
                if has_pending {
                    nodes += u64::from(count_child_by_words(
                        pending_words,
                        &parent_sb,
                        &pending_delta,
                        pending_h,
                        p2,
                    ));
                }
                pending_delta = delta;
                pending_h = h;
                pending_words = words;
                has_pending = true;
            }
        } else {
            if has_pending {
                nodes += u64::from(count_child_by_words(
                    pending_words,
                    &parent_sb,
                    &pending_delta,
                    pending_h,
                    p2,
                ));
                has_pending = false;
            }
            nodes += u64::from(count_cleared_child(
                board,
                m,
                h - cleared_rows.count_ones() as usize,
                cleared_rows,
                ymin,
                p2,
            ));
        }
    }
    if has_pending {
        nodes += u64::from(count_child_by_words(
            pending_words,
            &parent_sb,
            &pending_delta,
            pending_h,
            p2,
        ));
    }
    nodes
}

fn count_child_by_words(
    words: usize,
    parent: &SBoard<8>,
    delta: &[u64; 8],
    h: usize,
    p: Piece,
) -> u32 {
    match words {
        1 => count_child::<1>(parent, delta, h, p),
        2 => count_child::<2>(parent, delta, h, p),
        3 => count_child::<3>(parent, delta, h, p),
        4 => count_child::<4>(parent, delta, h, p),
        _ => count_child::<8>(parent, delta, h, p),
    }
}

fn count_child<const N: usize>(parent: &SBoard<8>, delta: &[u64; 8], h: usize, p: Piece) -> u32 {
    let mut d = [0u64; N];
    for i in 0..N {
        d[i] = parent.d[i] | delta[i];
    }
    count_smear_band(&SBoard { d }, h as i32, p, false)
}

fn count_child_pair_by_words(
    words: usize,
    parent: &SBoard<8>,
    delta_a: &[u64; 8],
    ha: usize,
    delta_b: &[u64; 8],
    hb: usize,
    p: Piece,
) -> (u32, u32) {
    match words {
        1 => count_child_pair::<1>(parent, delta_a, ha, delta_b, hb, p),
        2 => (
            count_child::<2>(parent, delta_a, ha, p),
            count_child::<2>(parent, delta_b, hb, p),
        ),
        3 => (
            count_child::<3>(parent, delta_a, ha, p),
            count_child::<3>(parent, delta_b, hb, p),
        ),
        4 => (
            count_child::<4>(parent, delta_a, ha, p),
            count_child::<4>(parent, delta_b, hb, p),
        ),
        _ => (
            count_child::<8>(parent, delta_a, ha, p),
            count_child::<8>(parent, delta_b, hb, p),
        ),
    }
}

fn count_child_pair<const N: usize>(
    parent: &SBoard<8>,
    delta_a: &[u64; 8],
    ha: usize,
    delta_b: &[u64; 8],
    hb: usize,
    p: Piece,
) -> (u32, u32) {
    let mut da = [0u64; N];
    let mut db = [0u64; N];
    for i in 0..N {
        da[i] = parent.d[i] | delta_a[i];
        db[i] = parent.d[i] | delta_b[i];
    }
    count_smear_band_pair(
        &SBoard { d: da },
        ha as i32,
        &SBoard { d: db },
        hb as i32,
        p,
        false,
    )
}

// Slow path for line-clearing children: materialize rows, compact,
fn count_cleared_child(
    board: &Board,
    m: &Move,
    h: usize,
    cleared_rows: u64,
    ymin: usize,
    p2: Piece,
) -> u32 {
    let mut rows = board.rows;
    let pc = m.cells();
    let x = m.x();
    let y = m.y();
    rows[y as usize] |= 1 << x;
    for i in 0..3 {
        rows[(pc[i].y as i32 + y) as usize] |= 1 << (pc[i].x as i32 + x);
    }
    let mut write = ymin;
    for read in ymin..BOARD_HEIGHT {
        if cleared_rows & (1u64 << read) == 0 {
            rows[write] = rows[read];
            write += 1;
        }
    }
    for row in rows.iter_mut().take(BOARD_HEIGHT).skip(write) {
        *row = 0;
    }
    count_smear_rows(&rows, h as i32, p2, false)
}

/// perft with move buffers built at every level, including leaves.
/// Same tree and counts as `perft`; this is the methodology the cobra
/// perft CLI uses (leaf move lists are materialized, not bulk counted),
/// so NPS from this mode is the cross-engine comparable number.
pub fn perft_movelist(board: &Board, queue_offset: usize, depth: usize) -> u64 {
    if depth == 0 {
        return 1;
    }
    let ml = placements(board, queue_piece(queue_offset));
    if depth == 1 {
        return std::hint::black_box(&ml).len() as u64;
    }
    let mut nodes = 0u64;
    for m in ml.iter() {
        let mut child = board.clone();
        child.do_move(m);
        nodes += perft_movelist(&child, queue_offset + 1, depth - 1);
    }
    nodes
}

/// divide: print per-move breakdown at root
pub fn divide(board: &Board, depth: usize) -> u64 {
    let piece = queue_piece(0);
    let ml = placements(board, piece);
    let mut total: u64 = 0;

    for m in ml.iter() {
        let mut child = board.clone();
        child.do_move(m);
        let count = perft(&child, 1, depth - 1);
        println!(
            "{:?} ({},{}) r={:?}: {}",
            m.piece(),
            m.x(),
            m.y(),
            m.rotation(),
            count
        );
        total += count;
    }
    println!("Total: {total}");
    total
}

/// parallel perft: two-level work split for high core saturation
pub fn perft_parallel(board: &Board, depth: usize) -> u64 {
    if depth <= 2 {
        return perft(board, 0, depth);
    }

    // expand first 2 plies into work units
    let piece0 = queue_piece(0);
    let ml0 = placements(board, piece0);

    let work_units: Vec<Board> = ml0
        .iter()
        .flat_map(|m0| {
            let mut b1 = board.clone();
            b1.do_move(m0);
            let piece1 = queue_piece(1);
            let ml1 = placements(&b1, piece1);
            ml1.iter()
                .map(|m1| {
                    let mut b2 = b1.clone();
                    b2.do_move(m1);
                    b2
                })
                .collect::<Vec<_>>()
        })
        .collect();

    #[cfg(feature = "rayon")]
    {
        work_units.par_iter().map(|b| perft(b, 2, depth - 2)).sum()
    }
    #[cfg(not(feature = "rayon"))]
    {
        work_units.iter().map(|b| perft(b, 2, depth - 2)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Strict placement-tree pins. D1-D4 unchanged; D5 drops
    // 3,573,524 -> 3,500,883 (-2.03%) because the old emission tree
    // over-produced on L/J overhangs. Cross-validated by reference BFS.
    const D1: u64 = 17;
    const D2: u64 = 153;
    const D3: u64 = 5266;
    const D4: u64 = 188561;
    const D5: u64 = 3500883;

    #[test]
    fn test_perft_d1() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 1), D1);
    }

    #[test]
    fn test_perft_d2() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 2), D2);
    }

    #[test]
    fn test_perft_d3() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 3), D3);
    }

    #[test]
    fn test_perft_d4() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 4), D4);
    }

    #[test]
    fn test_perft_d5() {
        let b = Board::new();
        assert_eq!(perft(&b, 0, 5), D5);
    }

    #[test]
    fn test_perft_movelist_matches_count_kernel_d1_d4() {
        let b = Board::new();
        for (depth, expected) in [(1, D1), (2, D2), (3, D3), (4, D4)] {
            assert_eq!(perft_movelist(&b, 0, depth), expected, "movelist D{depth}");
        }
    }

    #[test]
    #[ignore] // slow in debug builds
    fn test_perft_movelist_d5() {
        let b = Board::new();
        assert_eq!(perft_movelist(&b, 0, 5), D5);
    }
}
