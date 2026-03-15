// profiling harness: measures where time is spent in perft
// breakdown: generate vs do_move vs board_clone

use std::time::Instant;

use direct_cobra_copy::board::Board;
use direct_cobra_copy::header::Piece;
use direct_cobra_copy::movegen::{generate, MoveBuffer};

static QUEUE: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
    Piece::T,
];

struct Stats {
    nodes: u64,
    generate_ns: u64,
    do_move_ns: u64,
    clone_ns: u64,
}

fn perft_profile(
    board: &Board,
    queue: &[Piece],
    depth: usize,
    idx: usize,
    stats: &mut Stats,
) -> u64 {
    if depth == 0 {
        stats.nodes += 1;
        return 1;
    }

    let piece = queue[idx % queue.len()];
    let next_idx = idx + 1;

    // time generate
    let mut moves = MoveBuffer::new();
    let t0 = Instant::now();
    generate(board, &mut moves, piece, false);
    stats.generate_ns += t0.elapsed().as_nanos() as u64;

    if depth == 1 {
        let count = moves.len() as u64;
        stats.nodes += count;
        return count;
    }

    let mut total = 0u64;
    for m in moves.iter() {
        // time clone
        let t1 = Instant::now();
        let mut next_board = board.clone();
        stats.clone_ns += t1.elapsed().as_nanos() as u64;

        // time do_move
        let t2 = Instant::now();
        next_board.do_move(m);
        stats.do_move_ns += t2.elapsed().as_nanos() as u64;

        total += perft_profile(&next_board, queue, depth - 1, next_idx, stats);
    }
    total
}

fn main() {
    let depth: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let board = Board::new();
    let mut stats = Stats {
        nodes: 0,
        generate_ns: 0,
        do_move_ns: 0,
        clone_ns: 0,
    };

    let t_total = Instant::now();
    let count = perft_profile(&board, &QUEUE, depth, 0, &mut stats);
    let total_ms = t_total.elapsed().as_millis();

    println!("depth={depth} count={count} nodes={}", stats.nodes);
    println!("total: {total_ms}ms");
    println!(
        "generate: {}ms ({:.1}%)",
        stats.generate_ns / 1_000_000,
        stats.generate_ns as f64 / (total_ms as f64 * 1_000_000.0) * 100.0
    );
    println!(
        "do_move:  {}ms ({:.1}%)",
        stats.do_move_ns / 1_000_000,
        stats.do_move_ns as f64 / (total_ms as f64 * 1_000_000.0) * 100.0
    );
    println!(
        "clone:    {}ms ({:.1}%)",
        stats.clone_ns / 1_000_000,
        stats.clone_ns as f64 / (total_ms as f64 * 1_000_000.0) * 100.0
    );
    let other = total_ms as f64
        - (stats.generate_ns + stats.do_move_ns + stats.clone_ns) as f64 / 1_000_000.0;
    println!(
        "other:    {:.0}ms ({:.1}%)",
        other,
        other / total_ms as f64 * 100.0
    );
}
