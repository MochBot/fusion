// bench_perft.rs -- perft speed benchmark, two modes
use fusion_engine::board::Board;
use fusion_engine::perft::{perft, perft_movelist, perft_parallel};
use std::time::Instant;

// Strict placement-tree counts, D1-D7. D1-D5 = fixtures/perft/baselines.txt.
const STRICT_REF: [u64; 7] = [17, 153, 5266, 188561, 3500883, 67088390, 2652750957];

fn fmt_nps(nodes: u64, secs: f64) -> String {
    let nps = nodes as f64 / secs;
    if nps >= 1e9 {
        format!("{:.2}B", nps / 1e9)
    } else if nps >= 1e6 {
        format!("{:.2}M", nps / 1e6)
    } else if nps >= 1e3 {
        format!("{:.2}K", nps / 1e3)
    } else {
        format!("{:.0}", nps)
    }
}

fn fmt_time(secs: f64) -> String {
    if secs >= 1.0 {
        format!("{:.3}s", secs)
    } else if secs >= 0.001 {
        format!("{:.3}ms", secs * 1e3)
    } else {
        format!("{:.3}µs", secs * 1e6)
    }
}

fn run_table(label: &str, note: &str, max_depth: usize, f: impl Fn(&Board, usize) -> u64) {
    println!("[{label}] {note}");
    println!(
        "{:>5}  {:>15}  {:>12}  {:>10}  {:>8}",
        "Depth", "Nodes", "Time", "NPS", "Delta"
    );
    println!("{}", "-".repeat(60));
    for depth in 1..=max_depth {
        let board = Board::new();
        let t = Instant::now();
        let nodes = f(&board, depth);
        let elapsed = t.elapsed().as_secs_f64();
        let expected = STRICT_REF[depth - 1];
        let delta: i64 = nodes as i64 - expected as i64;
        let delta_str = if delta == 0 {
            "ok".to_string()
        } else {
            format!("{:+}", delta)
        };
        println!(
            "{:>5}  {:>15}  {:>12}  {:>10}  {:>8}",
            depth,
            nodes,
            fmt_time(elapsed),
            fmt_nps(nodes, elapsed),
            delta_str
        );
    }
    println!();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let parallel = args.iter().any(|a| a == "--parallel" || a == "-p");
    let skip_movelist = args.iter().any(|a| a == "--count-only");

    println!("=== Fusion Perft Benchmark ===");
    println!();

    if parallel {
        run_table(
            "count-kernel, parallel",
            "bulk counting at the last two levels, rayon split",
            7,
            perft_parallel,
        );
    } else {
        run_table(
            "count-kernel",
            "bulk counting at the last two levels",
            7,
            |b, d| perft(b, 0, d),
        );
    }

    if !skip_movelist {
        run_table(
            "movelist",
            "move buffers built at every level, including leaves",
            7,
            |b, d| perft_movelist(b, 0, d),
        );
    }
}
