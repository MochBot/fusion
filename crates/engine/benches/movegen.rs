use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fusion_core::{Board, Piece};
use fusion_engine::generate_moves;
use fusion_engine::perft::perft;

fn bench_generate_moves(c: &mut Criterion) {
    let board = Board::default();

    let pieces = [
        (Piece::I, "I"),
        (Piece::O, "O"),
        (Piece::T, "T"),
        (Piece::S, "S"),
        (Piece::Z, "Z"),
        (Piece::J, "J"),
        (Piece::L, "L"),
    ];

    for (piece, name) in pieces {
        c.bench_function(&format!("generate_moves_{}", name), |b| {
            b.iter(|| generate_moves(black_box(&board), black_box(piece)))
        });
    }
}

fn bench_perft(c: &mut Criterion) {
    let board = Board::default();
    let queue = [
        Piece::I,
        Piece::O,
        Piece::L,
        Piece::J,
        Piece::S,
        Piece::Z,
        Piece::T,
    ];

    c.bench_function("perft_depth_1", |b| {
        b.iter(|| perft(black_box(&board), black_box(&queue[..1]), 1))
    });

    c.bench_function("perft_depth_2", |b| {
        b.iter(|| perft(black_box(&board), black_box(&queue[..2]), 2))
    });

    c.bench_function("perft_depth_3", |b| {
        b.iter(|| perft(black_box(&board), black_box(&queue[..3]), 3))
    });
}

criterion_group!(benches, bench_generate_moves, bench_perft);
criterion_main!(benches);
