// CPU inference latency benchmark for tract_onnx PolicyValueRuntime.
//
// Loads one or more ONNX files (each matching the canonical Phase-2 runtime
// contract: features (1, 854) f32, candidate_move_features (1, 64, 14) f32,
// candidate_mask (1, 64) bool) and times the engine-hot-path `model.run(...)`
// over N iterations after a warm-up.
//
// Usage:
//   cargo run --release --bin policy_value_infer_bench -- \
//     /tmp/triangle_v1_bench/a1_medium.policy_value.onnx \
//     /tmp/triangle_v1_bench/a1_large.policy_value.onnx \
//     /tmp/triangle_v1_bench/a1_xl.policy_value.onnx
//
// Flags:
//   --warmup N    iterations to discard before timing (default 50)
//   --iters N     timing iterations (default 500)
//   --threads N   tract_onnx threading hint via env (default = auto)
//
// Output: per-variant p50 / p95 / p99 / mean / stddev in microseconds, plus
// implied search-nodes-per-second.

use std::env;
use std::path::Path;
use std::time::Instant;

use tract_onnx::prelude::*;

const STATE_FEATURE_DIM: usize = 854;
const CANDIDATE_CAPACITY: usize = 64;
const MOVE_FEATURE_DIM: usize = 14;

fn percentile(sorted_us: &[u64], pct: f64) -> u64 {
    let n = sorted_us.len();
    if n == 0 {
        return 0;
    }
    let idx = ((n as f64) * pct / 100.0).clamp(0.0, (n - 1) as f64) as usize;
    sorted_us[idx]
}

fn build_inputs() -> TractResult<TVec<TValue>> {
    let mut rng_state = 0xdead_beefu64;
    let mut rng = || {
        rng_state = rng_state.wrapping_mul(0x100000001b3).wrapping_add(1);
        ((rng_state >> 33) as f32 / (u32::MAX as f32)) - 0.5
    };
    let features: Vec<f32> = (0..STATE_FEATURE_DIM).map(|_| rng()).collect();
    let cand_features: Vec<f32> = (0..(CANDIDATE_CAPACITY * MOVE_FEATURE_DIM))
        .map(|_| rng())
        .collect();
    let cand_mask: Vec<bool> = (0..CANDIDATE_CAPACITY).map(|i| i < 24).collect();

    let features_t = tract_ndarray::Array2::from_shape_vec((1, STATE_FEATURE_DIM), features)
        .map_err(|e| TractError::msg(format!("features: {e}")))?;
    let cand_features_t = tract_ndarray::Array3::from_shape_vec(
        (1, CANDIDATE_CAPACITY, MOVE_FEATURE_DIM),
        cand_features,
    )
    .map_err(|e| TractError::msg(format!("cand_features: {e}")))?;
    let cand_mask_t = tract_ndarray::Array2::from_shape_vec((1, CANDIDATE_CAPACITY), cand_mask)
        .map_err(|e| TractError::msg(format!("cand_mask: {e}")))?;

    Ok(tvec![
        features_t.into_tensor().into(),
        cand_features_t.into_tensor().into(),
        cand_mask_t.into_tensor().into(),
    ])
}

fn bench_one(onnx_path: &Path, warmup: usize, iters: usize, optimize: bool) -> TractResult<()> {
    let load_t0 = Instant::now();
    let typed = tract_onnx::onnx().model_for_path(onnx_path)?.into_typed()?;
    let model = if optimize {
        typed.into_optimized()?.into_runnable()?
    } else {
        typed.into_runnable()?
    };
    let load_ms = load_t0.elapsed().as_secs_f64() * 1000.0;

    println!("\n=== {} ===", onnx_path.display());
    println!("  load time : {:.1} ms", load_ms);

    let inputs = build_inputs()?;

    // Warm-up
    for _ in 0..warmup {
        let _ = model.run(inputs.clone())?;
    }

    // Timed loop. Each iter clones the input tvec because run() consumes it.
    let mut samples_us: Vec<u64> = Vec::with_capacity(iters);
    let bench_t0 = Instant::now();
    for _ in 0..iters {
        let t0 = Instant::now();
        let _ = model.run(inputs.clone())?;
        samples_us.push(t0.elapsed().as_micros() as u64);
    }
    let total_s = bench_t0.elapsed().as_secs_f64();
    samples_us.sort_unstable();

    let mean: f64 = samples_us.iter().sum::<u64>() as f64 / samples_us.len() as f64;
    let variance: f64 = samples_us
        .iter()
        .map(|&v| {
            let d = v as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / samples_us.len() as f64;
    let stddev = variance.sqrt();
    let min = samples_us.first().copied().unwrap_or(0);
    let max = samples_us.last().copied().unwrap_or(0);
    let p50 = percentile(&samples_us, 50.0);
    let p95 = percentile(&samples_us, 95.0);
    let p99 = percentile(&samples_us, 99.0);

    let nps = (iters as f64) / total_s;
    println!("  iters     : {} (warmup {})", iters, warmup);
    println!("  total     : {:.2} s", total_s);
    println!("  min       : {} us", min);
    println!("  p50       : {} us", p50);
    println!("  p95       : {} us", p95);
    println!("  p99       : {} us", p99);
    println!("  max       : {} us", max);
    println!("  mean      : {:.1} us", mean);
    println!("  stddev    : {:.1} us", stddev);
    println!("  throughput: {:.0} inferences/sec", nps);
    println!("  search    : {:.0} nodes/sec @ 1 inference/node", nps);
    Ok(())
}

fn main() -> TractResult<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut warmup = 50usize;
    let mut iters = 500usize;
    let mut optimize = false;
    let mut idx = 0;
    let mut positional: Vec<String> = Vec::new();
    while idx < args.len() {
        match args[idx].as_str() {
            "--warmup" => {
                warmup = args[idx + 1].parse().expect("--warmup expects integer");
                idx += 2;
            }
            "--iters" => {
                iters = args[idx + 1].parse().expect("--iters expects integer");
                idx += 2;
            }
            "--optimize" => {
                optimize = true;
                idx += 1;
            }
            "--threads" => {
                env::set_var("RAYON_NUM_THREADS", &args[idx + 1]);
                env::set_var("OMP_NUM_THREADS", &args[idx + 1]);
                idx += 2;
            }
            _ => {
                positional.push(std::mem::take(&mut args[idx]));
                idx += 1;
            }
        }
    }
    if positional.is_empty() {
        eprintln!(
            "usage: {} [--warmup N] [--iters N] [--threads N] <model.onnx> [more.onnx ...]",
            env::args().next().unwrap_or_default()
        );
        std::process::exit(2);
    }

    println!(
        "config: warmup={}, iters={}, optimize={}, RAYON_NUM_THREADS={:?}",
        warmup,
        iters,
        optimize,
        env::var("RAYON_NUM_THREADS").ok()
    );

    for path in &positional {
        let p = Path::new(path);
        if !p.exists() {
            eprintln!("skip: {} does not exist", p.display());
            continue;
        }
        bench_one(p, warmup, iters, optimize)?;
    }
    Ok(())
}
