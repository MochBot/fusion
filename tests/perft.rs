// perft.rs -- integration tests pinning movegen node counts (D1-D4 vs upstream, D5-D7 vs Fusion)
use direct_cobra_copy::board::Board;
use direct_cobra_copy::header::Piece;
use direct_cobra_copy::movegen::MoveList;

/// Queue order: I O L J S Z T (repeating)
const QUEUE: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
    Piece::T,
];

// board-only perft — matches Cobra's perft exactly (no State overhead)
fn perft(board: &Board, queue: &[Piece], depth: usize) -> u64 {
    if depth == 0 {
        return 1;
    }
    let p = queue[0];
    let ml = MoveList::new(board, p);
    if depth == 1 {
        return ml.size() as u64;
    }
    let mut count = 0u64;
    for m in ml.iter() {
        let mut next = board.clone();
        next.do_move(m);
        count += perft(&next, &queue[1..], depth - 1);
    }
    count
}

// D1-D4 baselines match cobra-movegen d7054ef, queue IOLJSZT, empty board.
// D5-D7 are Fusion baselines: all-spin emission adds spin-labeled move
// variants once boards develop overhangs, so deep counts exceed upstream's
// T-spin-only totals (upstream: D5 3500883, D6 67088390, D7 2705999255).

#[test]
fn test_perft_d1() {
    let board = Board::new();
    assert_eq!(perft(&board, &QUEUE, 1), 17, "D1");
}

#[test]
fn test_perft_d2() {
    let board = Board::new();
    assert_eq!(perft(&board, &QUEUE, 2), 153, "D2");
}

#[test]
fn test_perft_d3() {
    let board = Board::new();
    assert_eq!(perft(&board, &QUEUE, 3), 5266, "D3");
}

#[test]
#[ignore] // slow in debug mode
fn test_perft_d4() {
    let board = Board::new();
    assert_eq!(perft(&board, &QUEUE, 4), 188561, "D4");
}

#[test]
#[ignore]
fn test_perft_d5() {
    let board = Board::new();
    assert_eq!(perft(&board, &QUEUE, 5), 3573524, "D5");
}

#[test]
#[ignore]
fn test_perft_d6() {
    let board = Board::new();
    assert_eq!(perft(&board, &QUEUE, 6), 70797703, "D6");
}

#[test]
#[ignore]
fn test_perft_d7() {
    let board = Board::new();
    assert_eq!(perft(&board, &QUEUE, 7), 2845035044, "D7");
}

// per-piece D1 counts: I=17, O=9, L=34, J=34, S=17, Z=17, T=34
#[test]
fn test_perft_d1_per_piece() {
    let board = Board::new();
    let expected = [
        (Piece::I, 17),
        (Piece::O, 9),
        (Piece::L, 34),
        (Piece::J, 34),
        (Piece::S, 17),
        (Piece::Z, 17),
        (Piece::T, 34),
    ];
    for (piece, count) in expected {
        let ml = MoveList::new(&board, piece);
        assert_eq!(ml.size(), count, "D1 {:?}", piece);
    }
}
