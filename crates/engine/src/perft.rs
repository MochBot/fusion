//! perft - performance test for movegen verification
//! BFS archived - SSA is now the sole movegen implementation

use crate::apply::{apply_move, apply_move_mut, unapply_move};
use crate::move_list::MoveList;
use crate::movegen_bitboard::{
    count_moves_bitboard, generate_moves_bitboard, generate_moves_bitboard_no_spin,
};
use fusion_core::{Board, Piece};
use rayon::prelude::*;

/// Open-addressed transposition table with power-of-2 masking.
/// Each entry stores a full 64-bit key for collision detection.
pub struct TransTable {
    entries: Vec<TTEntry>,
    mask: usize,
}

#[derive(Clone, Copy)]
struct TTEntry {
    key: u64,
    value: u64,
}

impl TransTable {
    fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two();
        Self {
            entries: vec![TTEntry { key: !0, value: 0 }; cap],
            mask: cap - 1,
        }
    }

    #[inline(always)]
    fn probe(&self, key: u64) -> Option<u64> {
        let idx = key as usize & self.mask;
        let entry = unsafe { self.entries.get_unchecked(idx) };
        if entry.key == key {
            Some(entry.value)
        } else {
            None
        }
    }

    #[inline(always)]
    fn store(&mut self, key: u64, value: u64) {
        let idx = key as usize & self.mask;
        let entry = unsafe { self.entries.get_unchecked_mut(idx) };
        entry.key = key;
        entry.value = value;
    }
}

/// Pack (zobrist_hash, depth, piece) into a single u64 TT key
#[inline(always)]
fn tt_key(hash: u64, depth: u32, piece: u8) -> u64 {
    hash ^ ((depth as u64) << 3) ^ (piece as u64).wrapping_mul(0x9e3779b97f4a7c15)
}

#[inline(always)]
fn generate_moves_with_tspin_toggle(board: &Board, piece: Piece, enable_tspin: bool) -> MoveList {
    if enable_tspin {
        generate_moves_bitboard(board, piece)
    } else {
        generate_moves_bitboard_no_spin(board, piece)
    }
}

#[inline(always)]
fn perft_cobra_with_tspin(
    board: &mut Board,
    queue: &[Piece],
    depth: u32,
    enable_tspin: bool,
) -> u64 {
    if depth == 0 {
        return 1;
    }

    if queue.is_empty() {
        return 1;
    }

    if depth == 1 {
        return count_moves_bitboard(board, queue[0]) as u64;
    }

    let mut nodes = 0u64;
    for mv in generate_moves_with_tspin_toggle(board, queue[0], enable_tspin) {
        let undo = apply_move_mut(board, &mv);
        nodes += perft_cobra_with_tspin(board, &queue[1..], depth - 1, enable_tspin);
        unapply_move(board, &undo);
    }
    nodes
}

/// perft - counts leaf nodes at depth, classic Cobra-style recursion
pub fn perft(board: &Board, queue: &[Piece], depth: u32) -> u64 {
    let mut local_board = board.clone();
    perft_cobra_with_tspin(&mut local_board, queue, depth, true)
}

/// perft with T-Spin detection disabled (moves are relabeled as non-spin)
pub fn perft_no_tspin(board: &Board, queue: &[Piece], depth: u32) -> u64 {
    let mut local_board = board.clone();
    perft_cobra_with_tspin(&mut local_board, queue, depth, false)
}

/// Fast perft with move/unmove pattern - avoids board cloning
pub fn perft_fast(board: &mut Board, queue: &[Piece], depth: u32) -> u64 {
    perft_cobra_with_tspin(board, queue, depth, true)
}

/// Fast perft with T-Spin detection disabled
pub fn perft_fast_no_tspin(board: &mut Board, queue: &[Piece], depth: u32) -> u64 {
    perft_cobra_with_tspin(board, queue, depth, false)
}

/// Perft with transposition table
pub fn perft_cached(board: &mut Board, queue: &[Piece], depth: u32, cache: &mut TransTable) -> u64 {
    perft_cached_ssa_with_tspin(board, queue, depth, true, cache)
}

/// Parallel perft - splits top-level moves across threads
pub fn perft_parallel(board: &Board, queue: &[Piece], depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }

    if depth == 1 {
        if queue.is_empty() {
            return 1;
        }
        return count_moves_bitboard(board, queue[0]) as u64;
    }

    if queue.is_empty() {
        return 1;
    }

    let moves = generate_moves_bitboard(board, queue[0]);

    moves
        .as_slice()
        .par_iter()
        .map(|mv| {
            let mut local_board = board.clone();
            let undo = apply_move_mut(&mut local_board, mv);
            let result = perft_fast(&mut local_board, &queue[1..], depth - 1);
            unapply_move(&mut local_board, &undo);
            result
        })
        .sum()
}

/// Full optimized perft - parallel + cached per thread + SSA movegen
pub fn perft_optimized(board: &Board, queue: &[Piece], depth: u32) -> u64 {
    // Now uses SSA - same as perft_optimized_ssa
    perft_optimized_ssa(board, queue, depth)
}

/// Full optimized perft using SSA movegen - two-level parallel + cached per thread
pub fn perft_optimized_ssa(board: &Board, queue: &[Piece], depth: u32) -> u64 {
    perft_optimized_ssa_with_tspin(board, queue, depth, true)
}

/// Full optimized perft using SSA movegen with T-Spin toggle
pub fn perft_optimized_ssa_with_tspin(
    board: &Board,
    queue: &[Piece],
    depth: u32,
    enable_tspin: bool,
) -> u64 {
    if depth == 0 {
        return 1;
    }

    if depth == 1 {
        if queue.is_empty() {
            return 1;
        }
        return count_moves_bitboard(board, queue[0]) as u64;
    }

    if queue.is_empty() {
        return 1;
    }

    if depth == 2 {
        let moves = generate_moves_with_tspin_toggle(board, queue[0], enable_tspin);
        return moves
            .as_slice()
            .par_iter()
            .map(|mv| {
                let (next_board, _) = apply_move(board, mv);
                count_moves_bitboard(&next_board, queue[1]) as u64
            })
            .sum();
    }

    let moves_d1 = generate_moves_with_tspin_toggle(board, queue[0], enable_tspin);

    let work_units: Vec<_> = moves_d1
        .iter()
        .flat_map(|mv1| {
            let (b1, _) = apply_move(board, mv1);
            let moves_d2 = generate_moves_with_tspin_toggle(&b1, queue[1], enable_tspin);
            moves_d2
                .into_iter()
                .map(move |mv2| {
                    let (b2, _) = apply_move(&b1, &mv2);
                    b2
                })
                .collect::<Vec<_>>()
        })
        .collect();

    work_units
        .par_iter()
        .map(|b2| {
            let mut local_board = b2.clone();
            let mut cache = TransTable::new(1 << 17);
            perft_cached_ssa_with_tspin(
                &mut local_board,
                &queue[2..],
                depth - 2,
                enable_tspin,
                &mut cache,
            )
        })
        .sum()
}

/// Full optimized perft with T-Spin detection disabled
pub fn perft_optimized_ssa_no_tspin(board: &Board, queue: &[Piece], depth: u32) -> u64 {
    perft_optimized_ssa_with_tspin(board, queue, depth, false)
}

/// Cached perft using SSA movegen with open-addressed TT
#[inline(always)]
fn perft_cached_ssa_with_tspin(
    board: &mut Board,
    queue: &[Piece],
    depth: u32,
    enable_tspin: bool,
    cache: &mut TransTable,
) -> u64 {
    if depth == 0 {
        return 1;
    }

    if queue.is_empty() {
        return 1;
    }

    if depth == 1 {
        return count_moves_bitboard(board, queue[0]) as u64;
    }

    let next_piece = queue[0] as u8;
    let key = tt_key(board.zobrist_hash(), depth, next_piece);
    if let Some(cached) = cache.probe(key) {
        return cached;
    }

    let moves = generate_moves_with_tspin_toggle(board, queue[0], enable_tspin);
    let mut nodes = 0u64;

    for mv in moves {
        let undo = apply_move_mut(board, &mv);
        nodes += perft_cached_ssa_with_tspin(board, &queue[1..], depth - 1, enable_tspin, cache);
        unapply_move(board, &undo);
    }

    cache.store(key, nodes);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::{Board, Piece};
    use std::time::Instant;

    /// Cobra reference values (IOLJSZT queue, empty board)
    /// source: Kixenon/cobra-movegen (verified clean against upstream 2026-02-09)
    const COBRA_REF: [u64; 7] = [
        17,            // D1
        153,           // D2
        5_266,         // D3
        188_561,       // D4
        3_500_883,     // D5
        67_088_390,    // D6
        2_705_999_255, // D7
    ];

    const STANDARD_QUEUE: [Piece; 7] = [
        Piece::I,
        Piece::O,
        Piece::L,
        Piece::J,
        Piece::S,
        Piece::Z,
        Piece::T,
    ];

    /// D1 move counts per piece on empty board (from Cobra)
    const D1_PER_PIECE: [(Piece, u64); 7] = [
        (Piece::I, 17),
        (Piece::O, 9),
        (Piece::L, 34),
        (Piece::J, 34),
        (Piece::S, 17),
        (Piece::Z, 17),
        (Piece::T, 34),
    ];

    #[test]
    fn test_depth_0_returns_1() {
        assert_eq!(perft(&Board::new(), &[], 0), 1);
    }

    #[test]
    fn test_d1_per_piece_counts() {
        let board = Board::new();
        for (piece, expected) in D1_PER_PIECE {
            let count = count_moves_bitboard(&board, piece) as u64;
            assert_eq!(
                count, expected,
                "{:?}: expected {}, got {}",
                piece, expected, count
            );
        }
    }

    #[test]
    fn test_variant_fast_matches_baseline() {
        let mut board = Board::new();
        let queue = [Piece::T, Piece::I, Piece::O];
        let baseline = perft(&Board::new(), &queue, 3);
        assert_eq!(perft_fast(&mut board, &queue, 3), baseline);
    }

    #[test]
    fn test_variant_parallel_matches_baseline() {
        let board = Board::new();
        let queue = [Piece::T, Piece::I, Piece::O];
        let baseline = perft(&board, &queue, 3);
        assert_eq!(perft_parallel(&board, &queue, 3), baseline);
    }

    #[test]
    fn test_variant_optimized_matches_baseline() {
        let board = Board::new();
        let queue = [Piece::T, Piece::I, Piece::O];
        let baseline = perft(&board, &queue, 3);
        assert_eq!(perft_optimized(&board, &queue, 3), baseline);
    }

    #[test]
    fn test_cobra_parity_d1_to_d4() {
        let board = Board::new();
        for depth in 1..=4usize {
            let queue: Vec<Piece> = STANDARD_QUEUE.iter().copied().take(depth).collect();
            let nodes = perft_optimized_ssa(&board, &queue, depth as u32);
            assert_eq!(
                nodes,
                COBRA_REF[depth - 1],
                "D{}: got {}, cobra={}, delta={}",
                depth,
                nodes,
                COBRA_REF[depth - 1],
                nodes as i64 - COBRA_REF[depth - 1] as i64
            );
        }
    }

    fn run_benchmark(depth: usize) -> u64 {
        let board = Board::new();
        let queue: Vec<Piece> = STANDARD_QUEUE.iter().copied().take(depth).collect();
        let start = Instant::now();
        let nodes = perft_optimized_ssa(&board, &queue, depth as u32);
        let elapsed = start.elapsed();
        let diff = nodes as i64 - COBRA_REF[depth - 1] as i64;
        let nps = nodes as f64 / elapsed.as_secs_f64();
        println!(
            "D{}: {} nodes in {:?} ({:.0} nps, cobra={}, delta={})",
            depth,
            nodes,
            elapsed,
            nps,
            COBRA_REF[depth - 1],
            diff
        );
        nodes
    }

    #[test]
    #[ignore]
    fn test_benchmark_d5() {
        let nodes = run_benchmark(5);
        assert_eq!(
            nodes,
            COBRA_REF[4],
            "D5 delta: {}",
            nodes as i64 - COBRA_REF[4] as i64
        );
    }

    #[test]
    #[ignore]
    fn test_benchmark_d6() {
        run_benchmark(6);
    }

    #[test]
    #[ignore]
    fn test_benchmark_d7() {
        run_benchmark(7);
    }

    #[test]
    #[ignore]
    fn test_d7_spin_invariance() {
        let board = Board::new();
        let with_spin = perft_optimized_ssa(&board, &STANDARD_QUEUE, 7);
        let no_spin = perft_optimized_ssa_no_tspin(&board, &STANDARD_QUEUE, 7);
        assert_eq!(
            with_spin, no_spin,
            "spin labels must not affect node count (with={}, without={})",
            with_spin, no_spin
        );
    }

    #[test]
    #[ignore]
    fn test_d7_parallel_consistency() {
        let mut board = Board::new();
        let serial = perft_fast(&mut board, &STANDARD_QUEUE, 7);
        let parallel = perft_optimized_ssa(&Board::new(), &STANDARD_QUEUE, 7);
        assert_eq!(serial, parallel, "serial={}, parallel={}", serial, parallel);
    }

    #[test]
    #[ignore]
    fn test_d4_divide() {
        let board = Board::new();
        let queue = &STANDARD_QUEUE;
        let moves = generate_moves_with_tspin_toggle(&board, queue[0], true);
        let mut total = 0u64;
        for i in 0..moves.len() {
            let mv = &moves.as_slice()[i];
            let (child, _lines) = apply_move(&board, mv);
            let sub = perft(&child, &queue[1..], 3);
            eprintln!(
                "move {:2}: x={:2} y={:2} rot={} spin={:?}  sub={}",
                i, mv.x, mv.y, mv.rotation as u8, mv.spin_type, sub
            );
            total += sub;
        }
        eprintln!(
            "D4 divide total = {} (delta = {})",
            total,
            total as i64 - COBRA_REF[3] as i64
        );
        assert_eq!(total, COBRA_REF[3]);
    }

    #[test]
    #[ignore]
    fn test_d4_per_piece_first() {
        let board = Board::new();
        let rest = &STANDARD_QUEUE[1..];

        eprintln!("=== D4 per-piece-first isolation (new-baseline) ===");
        for &piece in &STANDARD_QUEUE {
            let mut queue = vec![piece];
            queue.extend_from_slice(rest);
            let result = perft(&Board::new(), &queue, 4);
            let d1 = count_moves_bitboard(&board, piece);
            eprintln!("{:?}: D1={}, D4={}", piece, d1, result);
        }
    }

    #[test]
    #[ignore]
    fn test_d4_piece_isolation_matrix() {
        let board = Board::new();
        let tail: [Piece; 3] = [Piece::O, Piece::L, Piece::J];

        eprintln!("=== D4 piece isolation matrix (new-baseline, fixed tail O,L,J) ===");
        for &piece in &[
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::S,
            Piece::Z,
            Piece::J,
            Piece::L,
        ] {
            let mut queue = vec![piece];
            queue.extend_from_slice(&tail);
            let result = perft(&Board::new(), &queue, 4);
            let d1 = count_moves_bitboard(&board, piece);
            eprintln!("{:?}: D1={:3}, D4={:8}", piece, d1, result);
        }
    }

    #[test]
    #[ignore]
    fn test_d4_divide_move10_level2() {
        let board = Board::new();
        let queue = &STANDARD_QUEUE;
        let moves = generate_moves_with_tspin_toggle(&board, queue[0], true);

        let root = &moves.as_slice()[10];
        let (child, _lines) = apply_move(&board, root);
        eprintln!(
            "root move 10: x={} y={} rot={} spin={:?}",
            root.x, root.y, root.rotation as u8, root.spin_type
        );

        let mut total = 0u64;
        for (idx, mv) in generate_moves_with_tspin_toggle(&child, queue[1], true)
            .as_slice()
            .iter()
            .enumerate()
        {
            let (child2, _lines2) = apply_move(&child, mv);
            let sub = perft(&child2, &queue[2..], 2);
            eprintln!(
                "o-move {:2}: x={:2} y={:2} rot={} spin={:?} sub={}",
                idx, mv.x, mv.y, mv.rotation as u8, mv.spin_type, sub
            );
            total += sub;
        }

        eprintln!("move 10 subtotal = {}", total);
    }

    #[test]
    #[ignore]
    fn test_debug_branch_j_moves_root10_o4_l4() {
        let board = Board::new();
        let queue = &STANDARD_QUEUE;

        let root_moves = generate_moves_with_tspin_toggle(&board, queue[0], true);
        let root = &root_moves.as_slice()[10];
        let (root_child, _lines) = apply_move(&board, root);

        let o_moves = generate_moves_with_tspin_toggle(&root_child, queue[1], true);
        let o = &o_moves.as_slice()[4];
        let (o_child, _lines2) = apply_move(&root_child, o);

        let l_moves = generate_moves_with_tspin_toggle(&o_child, queue[2], true);
        let l = &l_moves.as_slice()[4];
        eprintln!(
            "baseline l-move4: x={} y={} rot={} spin={:?}",
            l.x, l.y, l.rotation as u8, l.spin_type
        );
        let (l_child, _lines3) = apply_move(&o_child, l);

        let mut j_moves: Vec<_> = generate_moves_with_tspin_toggle(&l_child, Piece::J, true)
            .as_slice()
            .iter()
            .copied()
            .collect();
        j_moves.sort_by_key(|m| (m.x, m.y, m.rotation as u8, m.spin_type as u8));

        eprintln!("baseline J placements count = {}", j_moves.len());
        for (i, m) in j_moves.iter().enumerate() {
            eprintln!(
                "baseline j {:2}: x={:2} y={:2} rot={} spin={:?}",
                i, m.x, m.y, m.rotation as u8, m.spin_type
            );
        }
    }

    fn run_d5_queue(label: &str, queue: &[Piece]) -> u64 {
        let board = Board::new();
        let start = Instant::now();
        let nodes = perft_optimized_ssa(&board, &queue[..5.min(queue.len())], 5);
        let elapsed = start.elapsed();
        eprintln!(
            "{}: {} nodes in {:?} ({:.0} nps)",
            label,
            nodes,
            elapsed,
            nodes as f64 / elapsed.as_secs_f64()
        );
        nodes
    }

    /// Reversed standard queue
    #[test]
    #[ignore]
    fn test_d5_reversed_queue() {
        let queue = [Piece::T, Piece::Z, Piece::S, Piece::J, Piece::L];
        let nodes = run_d5_queue("D5-reversed", &queue);
        let nodes2 = run_d5_queue("D5-reversed-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// All T-pieces — maximizes spin variant stress
    #[test]
    #[ignore]
    fn test_d5_all_t() {
        let queue = [Piece::T; 5];
        let nodes = run_d5_queue("D5-all-T", &queue);
        let nodes2 = run_d5_queue("D5-all-T-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// All I-pieces — stress 4-state canonical dedup
    #[test]
    #[ignore]
    fn test_d5_all_i() {
        let queue = [Piece::I; 5];
        let nodes = run_d5_queue("D5-all-I", &queue);
        let nodes2 = run_d5_queue("D5-all-I-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// All O-pieces — simplest piece, baseline sanity
    #[test]
    #[ignore]
    fn test_d5_all_o() {
        let queue = [Piece::O; 5];
        let nodes = run_d5_queue("D5-all-O", &queue);
        let nodes2 = run_d5_queue("D5-all-O-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// S and Z alternating — stress group2 canonical dedup
    #[test]
    #[ignore]
    fn test_d5_sz_alternating() {
        let queue = [Piece::S, Piece::Z, Piece::S, Piece::Z, Piece::S];
        let nodes = run_d5_queue("D5-SZSZS", &queue);
        let nodes2 = run_d5_queue("D5-SZSZS-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// T then I — T-spin boards followed by I-piece placement stress
    #[test]
    #[ignore]
    fn test_d5_ttiii() {
        let queue = [Piece::T, Piece::T, Piece::I, Piece::I, Piece::I];
        let nodes = run_d5_queue("D5-TTIII", &queue);
        let nodes2 = run_d5_queue("D5-TTIII-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// I then T — I-piece boards then T-spin variant explosion
    #[test]
    #[ignore]
    fn test_d5_iiitt() {
        let queue = [Piece::I, Piece::I, Piece::I, Piece::T, Piece::T];
        let nodes = run_d5_queue("D5-IIITT", &queue);
        let nodes2 = run_d5_queue("D5-IIITT-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// Mixed with duplicates — realistic bag randomization
    #[test]
    #[ignore]
    fn test_d5_mixed_dupes_1() {
        let queue = [Piece::L, Piece::T, Piece::S, Piece::T, Piece::I];
        let nodes = run_d5_queue("D5-LTSTI", &queue);
        let nodes2 = run_d5_queue("D5-LTSTI-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// Another mixed ordering
    #[test]
    #[ignore]
    fn test_d5_mixed_dupes_2() {
        let queue = [Piece::Z, Piece::J, Piece::Z, Piece::L, Piece::O];
        let nodes = run_d5_queue("D5-ZJZLO", &queue);
        let nodes2 = run_d5_queue("D5-ZJZLO-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// All L-pieces — 4-state piece with no special canonical handling
    #[test]
    #[ignore]
    fn test_d5_all_l() {
        let queue = [Piece::L; 5];
        let nodes = run_d5_queue("D5-all-L", &queue);
        let nodes2 = run_d5_queue("D5-all-L-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// J and L alternating — mirror pair stress
    #[test]
    #[ignore]
    fn test_d5_jl_alternating() {
        let queue = [Piece::J, Piece::L, Piece::J, Piece::L, Piece::J];
        let nodes = run_d5_queue("D5-JLJLJ", &queue);
        let nodes2 = run_d5_queue("D5-JLJLJ-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// T-heavy with one S — spin variant + group2 interaction
    #[test]
    #[ignore]
    fn test_d5_ttts_j() {
        let queue = [Piece::T, Piece::T, Piece::T, Piece::S, Piece::J];
        let nodes = run_d5_queue("D5-TTTSJ", &queue);
        let nodes2 = run_d5_queue("D5-TTTSJ-2", &queue);
        assert_eq!(nodes, nodes2, "non-deterministic: {} vs {}", nodes, nodes2);
    }

    /// Run ALL randomized queues and report node counts in a summary table.
    /// This is the "catch-all" — runs every queue above plus a few more.
    #[test]
    #[ignore]
    fn test_d5_randomized_queue_matrix() {
        let queues: Vec<(&str, [Piece; 5])> = vec![
            (
                "IOLJSZT-std",
                [Piece::I, Piece::O, Piece::L, Piece::J, Piece::S],
            ),
            (
                "reversed",
                [Piece::T, Piece::Z, Piece::S, Piece::J, Piece::L],
            ),
            ("all-T", [Piece::T, Piece::T, Piece::T, Piece::T, Piece::T]),
            ("all-I", [Piece::I, Piece::I, Piece::I, Piece::I, Piece::I]),
            ("all-O", [Piece::O, Piece::O, Piece::O, Piece::O, Piece::O]),
            ("all-S", [Piece::S, Piece::S, Piece::S, Piece::S, Piece::S]),
            ("all-Z", [Piece::Z, Piece::Z, Piece::Z, Piece::Z, Piece::Z]),
            ("all-J", [Piece::J, Piece::J, Piece::J, Piece::J, Piece::J]),
            ("all-L", [Piece::L, Piece::L, Piece::L, Piece::L, Piece::L]),
            ("SZSZS", [Piece::S, Piece::Z, Piece::S, Piece::Z, Piece::S]),
            ("TTIII", [Piece::T, Piece::T, Piece::I, Piece::I, Piece::I]),
            ("IIITT", [Piece::I, Piece::I, Piece::I, Piece::T, Piece::T]),
            ("LTSTI", [Piece::L, Piece::T, Piece::S, Piece::T, Piece::I]),
            ("ZJZLO", [Piece::Z, Piece::J, Piece::Z, Piece::L, Piece::O]),
            ("JLJLJ", [Piece::J, Piece::L, Piece::J, Piece::L, Piece::J]),
            ("TTTSJ", [Piece::T, Piece::T, Piece::T, Piece::S, Piece::J]),
            ("OIIII", [Piece::O, Piece::I, Piece::I, Piece::I, Piece::I]),
            (
                "TTTTT-nosp",
                [Piece::T, Piece::T, Piece::T, Piece::T, Piece::T],
            ),
            ("SSSSS", [Piece::S, Piece::S, Piece::S, Piece::S, Piece::S]),
            ("ZZZZZ", [Piece::Z, Piece::Z, Piece::Z, Piece::Z, Piece::Z]),
        ];

        eprintln!("\n=== D5 Randomized Queue Matrix ===");
        eprintln!("{:<15} {:>12} {:>8}", "Queue", "Nodes", "Time(ms)");
        eprintln!("{}", "-".repeat(38));

        for (label, queue) in &queues {
            let board = Board::new();
            let start = Instant::now();
            let nodes = perft_optimized_ssa(&board, queue, 5);
            let ms = start.elapsed().as_millis();
            eprintln!("{:<15} {:>12} {:>8}", label, nodes, ms);
        }

        let standard_nodes = perft_optimized_ssa(&Board::new(), &STANDARD_QUEUE[..5], 5);
        assert_eq!(
            standard_nodes, COBRA_REF[4],
            "standard queue D5 broken: {} vs cobra {}",
            standard_nodes, COBRA_REF[4]
        );

        let t5_a = perft_optimized_ssa(&Board::new(), &[Piece::T; 5], 5);
        let t5_b = perft_optimized_ssa(&Board::new(), &[Piece::T; 5], 5);
        assert_eq!(t5_a, t5_b, "all-T non-deterministic");
    }

    #[test]
    #[ignore]
    fn test_d4_divide_move10_o4_level3() {
        let board = Board::new();
        let queue = &STANDARD_QUEUE;

        let root_moves = generate_moves_with_tspin_toggle(&board, queue[0], true);
        let root = &root_moves.as_slice()[10];
        let (root_child, _lines) = apply_move(&board, root);

        let o_moves = generate_moves_with_tspin_toggle(&root_child, queue[1], true);
        let o = &o_moves.as_slice()[4];
        eprintln!(
            "o-move 4: x={} y={} rot={} spin={:?}",
            o.x, o.y, o.rotation as u8, o.spin_type
        );
        let (o_child, _lines2) = apply_move(&root_child, o);

        let mut total = 0u64;
        for (idx, mv) in generate_moves_with_tspin_toggle(&o_child, queue[2], true)
            .as_slice()
            .iter()
            .enumerate()
        {
            let (child3, _lines3) = apply_move(&o_child, mv);
            let sub = perft(&child3, &queue[3..], 1);
            eprintln!(
                "l-move {:2}: x={:2} y={:2} rot={} spin={:?} sub={}",
                idx, mv.x, mv.y, mv.rotation as u8, mv.spin_type, sub
            );
            total += sub;
        }

        eprintln!("move10/o4 subtotal = {}", total);
    }
}
