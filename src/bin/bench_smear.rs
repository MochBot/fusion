//! Perft benchmark for the row-major smeared-bitboard generator.
//!
//! Mirrors the upstream cobra bench interface: a single queue-string argument
//! (e.g. "IOLJSZT") and the same one-line output format, so the two binaries
//! can be interleaved for A/B runs.

use direct_cobra_copy::smear;
use std::time::Instant;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "IOLJSZT".into());
    let queue = match smear::parse_queue(&arg) {
        Some(q) if !q.is_empty() => q,
        _ => {
            eprintln!("Invalid queue: {arg}");
            std::process::exit(1);
        }
    };

    let start = Instant::now();
    let nodes = smear::perft(&queue);
    let ms = start.elapsed().as_millis() as u64;
    println!(
        "Depth: {} Nodes: {} Time: {}ms NPS: {}",
        queue.len(),
        nodes,
        ms,
        nodes * 1000 / ms.max(1)
    );
}
