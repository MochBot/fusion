// bench_perft.rs -- full speed + accuracy benchmark
use direct_cobra_copy::board::Board;
use direct_cobra_copy::perft::{perft, perft_parallel};
use std::time::Instant;

const COBRA_REF: [u64; 7] = [
    17,         // D1
    153,        // D2
    5266,       // D3
    188561,     // D4
    3500883,    // D5
    67088390,   // D6
    2705999255, // D7
];

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let parallel = args.iter().any(|a| a == "--parallel" || a == "-p");
    let mode = if parallel {
        "Parallel (Rayon)"
    } else {
        "Serial"
    };

    println!("=== Fusion-2 Perft Benchmark ({}) ===", mode);
    println!();

    println!(
        "{:>5}  {:>15}  {:>12}  {:>10}  {:>8}",
        "Depth", "Nodes", "Time", "NPS", "Delta"
    );
    println!("{}", "-".repeat(60));

    for depth in 1..=7 {
        let board = Board::new();
        let t = Instant::now();
        let nodes = if parallel {
            perft_parallel(&board, depth)
        } else {
            perft(&board, 0, depth)
        };
        let elapsed = t.elapsed().as_secs_f64();
        let expected = COBRA_REF[depth - 1];
        let delta: i64 = nodes as i64 - expected as i64;
        let delta_str = if delta == 0 {
            "✓".to_string()
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
    println!("Cobra reference D7: {}", COBRA_REF[6]);
}
