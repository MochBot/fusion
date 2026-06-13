//! Perft benchmark for the row-major smeared-bitboard generator.
//!
//! Mirrors the upstream cobra bench interface: a single queue-string argument
//! (e.g. "IOLJSZT") and the same one-line output format, so the two binaries
//! can be compared directly.

use direct_cobra_copy::smear;
use std::time::Instant;

/// Dispatch to the multithreaded driver when requested and available.
fn run(queue: &[usize], mt: bool) -> u64 {
    #[cfg(feature = "rayon")]
    if mt {
        return smear::perft_mt(queue);
    }
    let _ = mt;
    smear::perft(queue)
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "IOLJSZT".into());
    let queue = match smear::parse_queue(&arg) {
        Some(q) if !q.is_empty() => q,
        _ => {
            eprintln!("Invalid queue: {arg}");
            std::process::exit(1);
        }
    };

    let mt = std::env::args().nth(2).is_some_and(|s| s == "mt");
    // Surface the worker count: RAYON_NUM_THREADS silently caps the pool, which
    // makes multithreaded NPS numbers incomparable unless the size is recorded.
    #[cfg(feature = "rayon")]
    if mt {
        println!("Threads: {}", rayon::current_num_threads());
    }
    let start = Instant::now();
    let nodes = run(&queue, mt);
    let ms = start.elapsed().as_millis() as u64;
    println!(
        "Depth: {} Nodes: {} Time: {}ms NPS: {}",
        queue.len(),
        nodes,
        ms,
        nodes * 1000 / ms.max(1)
    );
}
