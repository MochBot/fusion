// perft.rs -- integration pins for the strict placement tree.
// Every node expands each distinct reachable placement exactly once.
// D1-D5 match the cobra CLI baselines in fixtures/perft/baselines.txt.
use fusion_engine::board::Board;
use fusion_engine::header::Piece;
use fusion_engine::movegen::MoveList;
use fusion_engine::perft::{perft, perft_movelist};

const STRICT: [(usize, u64); 5] = [(1, 17), (2, 153), (3, 5266), (4, 188561), (5, 3500883)];

#[test]
fn count_kernel_pins_d1_d3() {
    let board = Board::new();
    for (depth, expected) in &STRICT[..3] {
        assert_eq!(perft(&board, 0, *depth), *expected, "D{depth}");
    }
}

#[test]
fn movelist_pins_d1_d3() {
    let board = Board::new();
    for (depth, expected) in &STRICT[..3] {
        assert_eq!(perft_movelist(&board, 0, *depth), *expected, "D{depth}");
    }
}

#[test]
#[ignore] // slow in debug builds
fn count_kernel_pins_d4_d5() {
    let board = Board::new();
    for (depth, expected) in &STRICT[3..] {
        assert_eq!(perft(&board, 0, *depth), *expected, "D{depth}");
    }
}

#[test]
#[ignore] // slow in debug builds
fn movelist_pins_d4_d5() {
    let board = Board::new();
    for (depth, expected) in &STRICT[3..] {
        assert_eq!(perft_movelist(&board, 0, *depth), *expected, "D{depth}");
    }
}

#[test]
fn modes_agree_on_seeded_boards() {
    let board = Board::new();
    for depth in 1..=3 {
        assert_eq!(
            perft(&board, 0, depth),
            perft_movelist(&board, 0, depth),
            "mode divergence at D{depth}"
        );
    }
}

// per-piece D1 counts on the empty board, via the production generator
#[test]
fn per_piece_d1_counts() {
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
