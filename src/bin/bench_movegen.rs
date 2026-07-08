// bench_movegen -- per-call movegen microbenchmark on a pinned seeded corpus.
// Reports ns/call and calls/sec per piece for the paths that matter:
//   dispatch-count = count_moves_dispatch (production count routing)
//   engine-count   = count_moves (scalar engine, always)
//   generate       = production generate() (materializing dispatch)
// Unlike perft NPS, these numbers carry no bulk-counting amortization or
// depth-dependent piece/board blend, so they are stable across harnesses and
// comparable across machines via ns * GHz = cycles/call.
use fusion_engine::board::Board;
use fusion_engine::header::Piece;
use fusion_engine::move_buffer::MoveBuffer;
use fusion_engine::movegen::{count_moves, count_moves_dispatch, generate};
use std::hint::black_box;
use std::time::Instant;

const CORPUS_SIZE: usize = 2000;
const REPS: usize = 5;

fn seeded_boards() -> Vec<Board> {
    let mut seed = 0xC0DE_2026_0611_BEEFu64;
    let mut xs = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..CORPUS_SIZE)
        .map(|_| {
            let h = 1 + (xs() % 24) as usize;
            let mut b = Board::new();
            for y in 0..h {
                let mut row = (xs() & 0x3FF) as u16;
                row &= !(1u16 << (xs() % 10));
                b.rows[y] = row;
            }
            for y in 0..h {
                let mut bits = b.rows[y] as u64;
                while bits != 0 {
                    let x = bits.trailing_zeros() as usize;
                    b.cols[x] |= 1u64 << y;
                    bits &= bits - 1;
                }
            }
            b
        })
        .collect()
}

fn bench<F: FnMut(&Board) -> u32>(boards: &[Board], mut f: F) -> (f64, u64) {
    let mut moves_total = 0u64;
    for b in boards {
        moves_total += u64::from(f(b));
    }
    let mut best = f64::MAX;
    for _ in 0..REPS {
        let t = Instant::now();
        let mut sink = 0u64;
        for b in boards {
            sink += u64::from(f(black_box(b)));
        }
        black_box(sink);
        let ns = t.elapsed().as_nanos() as f64 / boards.len() as f64;
        if ns < best {
            best = ns;
        }
    }
    (best, moves_total / boards.len() as u64)
}

fn main() {
    let boards = seeded_boards();
    let pieces = [
        Piece::I,
        Piece::O,
        Piece::T,
        Piece::L,
        Piece::J,
        Piece::S,
        Piece::Z,
    ];

    println!(
        "=== bench_movegen: {} seeded boards (h 1-24), best of {} reps ===",
        CORPUS_SIZE, REPS
    );
    println!();
    println!(
        "{:>6}  {:>10}  {:>18}  {:>18}  {:>18}",
        "piece", "avg moves", "dispatch-count", "engine-count", "generate"
    );
    println!("{}", "-".repeat(78));

    let fmt = |ns: f64| -> String { format!("{:7.1}ns {:6.2}M/s", ns, 1e3 / ns) };

    for p in pieces {
        let (disp_ns, avg_moves) = bench(&boards, |b| count_moves_dispatch(b, p, false));
        let (eng_ns, _) = bench(&boards, |b| count_moves(b, p, false));
        let (gen_ns, _) = bench(&boards, |b| {
            let mut mb = MoveBuffer::new();
            generate(b, &mut mb, p, false);
            mb.len() as u32
        });
        println!(
            "{:>6?}  {:>10}  {:>18}  {:>18}  {:>18}",
            p,
            avg_moves,
            fmt(disp_ns),
            fmt(eng_ns),
            fmt(gen_ns)
        );
    }
}
