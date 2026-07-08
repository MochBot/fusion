// perft_cli.rs -- perft driver, matches cobra-movegen CLI output format
use fusion_engine::board::{Board, State};
use fusion_engine::header::Piece;
use fusion_engine::move_buffer::MoveBuffer;
use fusion_engine::smear_core::generate_placements;
use std::time::Instant;

fn placements(board: &Board, p: Piece) -> MoveBuffer {
    let mut moves = MoveBuffer::new();
    generate_placements(board, &mut moves, p, false);
    moves
}

fn perft(state: &State, queue: &[Piece], depth: usize) -> u64 {
    if depth == 0 {
        return 1;
    }
    let p = queue[0];
    let remaining = &queue[1..];
    let ml = placements(&state.board, p);
    if depth == 1 {
        return std::hint::black_box(&ml).len() as u64;
    }
    let mut count = 0u64;
    for m in ml.iter() {
        let mut next = state.clone();
        next.do_move(m);
        count += perft(&next, remaining, depth - 1);
    }
    count
}

fn perft_divide(state: &State, queue: &[Piece], depth: usize) {
    let p = queue[0];
    let remaining = &queue[1..];
    let ml = placements(&state.board, p);
    let mut total = 0u64;
    for m in ml.iter() {
        let mut next = state.clone();
        next.do_move(m);
        let count = if depth <= 2 {
            placements(&next.board, remaining[0]).len() as u64
        } else {
            perft(&next, remaining, depth - 1)
        };
        println!("{:?}: {}", m, count);
        total += count;
    }
    println!("\nTotal: {}", total);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let max_depth = if args.len() > 1 {
        args[1].parse::<usize>().unwrap_or(5)
    } else {
        5
    };
    let divide = args.iter().any(|a| a == "--divide" || a == "-d");
    // --count switches to the bulk-counting kernel (same counts)
    let count_kernel = args.iter().any(|a| a == "--count" || a == "-c");

    // default queue: IOLJSZT repeating
    let queue_pieces = [
        Piece::I,
        Piece::O,
        Piece::L,
        Piece::J,
        Piece::S,
        Piece::Z,
        Piece::T,
    ];
    let mut queue = Vec::new();
    for i in 0..max_depth {
        queue.push(queue_pieces[i % queue_pieces.len()]);
    }

    let mut state = State {
        board: Board::new(),
        hold: None,
        b2b: 0,
        combo: 0,
    };
    state.init();

    println!(
        "Perft (queue: {})",
        queue
            .iter()
            .map(|p| format!("{:?}", p))
            .collect::<Vec<_>>()
            .join("")
    );

    for depth in 1..=max_depth {
        let start = Instant::now();
        if divide && depth == max_depth {
            println!("\nDepth {} (divide):", depth);
            perft_divide(&state, &queue[..depth], depth);
        } else {
            let count = if count_kernel {
                fusion_engine::perft::perft(&state.board, 0, depth)
            } else {
                perft(&state, &queue[..depth], depth)
            };
            let elapsed = start.elapsed();
            println!("Depth {}: {} ({:.3}s)", depth, count, elapsed.as_secs_f64());
        }
    }
}
