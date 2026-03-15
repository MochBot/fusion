// perft_cli.rs -- perft driver, matches cobra-movegen CLI output format
use direct_cobra_copy::board::{Board, State};
use direct_cobra_copy::header::Piece;
use direct_cobra_copy::movegen::MoveList;
use std::time::Instant;

fn perft(state: &State, queue: &[Piece], depth: usize) -> u64 {
    if depth == 0 {
        return 1;
    }
    let p = queue[0];
    let remaining = &queue[1..];
    let ml = MoveList::new(&state.board, p);
    if depth == 1 {
        return ml.size() as u64;
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
    let ml = MoveList::new(&state.board, p);
    let mut total = 0u64;
    for m in ml.iter() {
        let mut next = state.clone();
        next.do_move(m);
        let count = if depth <= 2 {
            MoveList::new(&next.board, remaining[0]).size() as u64
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
            let count = perft(&state, &queue[..depth], depth);
            let elapsed = start.elapsed();
            println!("Depth {}: {} ({:.3}s)", depth, count, elapsed.as_secs_f64());
        }
    }
}
