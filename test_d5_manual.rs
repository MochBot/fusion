use fusion_core::board::PackedBoard;
use fusion_core::piece::Piece;

fn main() {
    let board = PackedBoard::EMPTY;
    let queue = [Piece::I, Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z, Piece::T];
    println!("Testing D5...");
    println!("Expected: 3,500,883");
    // Import perft_packed from the library
    extern crate fusion_engine;
    let actual = fusion_engine::perft::perft_packed(&board, &queue, 5);
    println!("D5 result: {}", actual);
    println!("Delta: {}", (actual as i64) - 3_500_883i64);
}
