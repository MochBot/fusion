//! Byte identity and throughput of the opener entry points on the production
//! build (an integration test links the library without `cfg(test)`).
//!
//! `opener_rounds_are_byte_identical` pins the SHA-256 of each round's
//! serialized `OpenerRoundAnalysis` for every real-round corpus the tests
//! carry, so a speed change that alters any output fails. With
//! `OPENER_PERF_OUT` set it also writes each round's JSON there for diffing.
//!
//! `opener_throughput` times catalog install, first and warm per-round analysis,
//! and labeling passes over the replay rounds (whole rounds and the first 20
//! locks) on `OPENER_THROUGHPUT_THREADS` threads (default 1):
//!
//! ```bash
//! cargo test --profile fast --test openers_perf -- --ignored --test-threads=1 --nocapture
//! ```

use std::time::{Duration, Instant};

use fusion_engine::openers::{
    analyze_opener_round, install_opener_runtime, OpenerObservation, OpenerRoundAnalysis,
    OpenerRoundInput,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const FULL_CATALOG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/openers/catalog-full.json"
));
const MINI_CATALOG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/openers/catalog-mini.json"
));

/// Per-round digests of the replay fixture against the full catalog, in order.
const REPLAY_FULL: &[&str] = &[
    "95b4c47f22700c25e2bae32d21686b804790bc0bbd795aaf336b2d64f8200c6b",
    "211ae826f209b32560756f34b25e7aeb734713ef9ca417e5879dcac830f989ab",
    "0e2c548fc1af152353e24d0e9da3fc1713234d2857cb63c4b24041b36d69cbe2",
    "f935a53bcbaf404be957619f2c02b6c0af856f3ab7d68600a6b91380dab91ba5",
    "0468e33410cb0d4e933633b418717825e20ab6d827128dde253b676e8b6d62f4",
    "d02bd762a26f28b6acc18fb4c3652334ba2960c1faa35beca4f41f9605205477",
    "2d369ef5e61354c969edd0d42ce2bdc1e0f5d869c22c8c55c2dd2dc5100cd7db",
    "81bda8216a59687332a6b5efee28f8feb0fa2b0ff00e6942560543943642575f",
    "607bc4fbd4777cd500a730ae46119e2f5197647c7cb82f2bca76c4c021e88e20",
    "ba3af677b8e69cc07131f960ef1251329556975474f3311b9d18cfa1058d624c",
    "57bfee8a5f389aa2b707d181fb4c41a43fee7f67bfba29c240d84c13c88cf1c8",
    "76036fec675864323684903a2ae0b0ad196760ffe75f255001c33c9d0283e03b",
    "2e400c4cf7a03910694b3812b1e5a3ea16a796dbe1560c98f52208390b813c6d",
    "f243f26b46f5865cbdb1069ed2e5089ff8ed60f5a25b5aef5041457616009661",
    "bd925615b21c24466bfeb7184866b4972a999e598a13b707612aafd4ae0c2ca2",
    "f6140a2917b7ca6ec6845822311934fafe509bcdc04415078606588493013610",
    "2522efdc801d933800363362e87021dea6fe359f1a40550d6dad1885432ecd17",
    "06d77f2fb7d8139966267f6103ac159fec2caa0beff8052ecdcb0035f95d0acb",
    "b5e4522c2df2763ebb76569298a8b8a9bc052e492316c18a00508c16d4f9ca9b",
    "48f04f3011a7ee03a7711620b804a65fd4a3bd3403014e1bab3282ae3066b6e6",
    "463962a85e29b4060ad6fe688d9d8abf9be5eac4af171fee50eb492dddc58734",
    "72c7e5ce167ada7a5644d755d271f50954edcb1f32fd3927308de408fb2cbaf2",
    "74114b2fa75a4df6ded91f5388ec322a1dd239793ea2cbdc879332e0cd75f9af",
    "4f8a96badce514635664d51be045071e9b7d6c37335d7d559d15245d44b35dd5",
    "2da6c2aaa7e7f71daf249fc2952a4359d18598319cf61af09d8793190eac22ec",
    "4280f17975325632c38121d99c928868b149477e5857f3406204a95efb2b44a9",
    "1deb4c473d43bff5050ba7810bd67d2e6333d9835cd458f2f5de8ce957304fbb",
    "21ed526acb0a22cd80307a8d52a7ccbdf316378578509e7c3a93f90e4776f2d7",
    "288036a7434875fb8e370e1d2cd5ca1f99d2c683f67ea95ac230c67e48b538d6",
    "2675cc70f860b6a4647a309592b2625b2e96405d3ebf18481f461572a8ed48d8",
    "732918152d85b922cbeeb5ab70081214cc17626917251014a65e601168491d65",
    "c94e007b736feca3144fd2b1ad85537d43cc914283e109c67e0e1d0bff22dd94",
    "3d3b5e31df7befc0e4c64bf7b08f227d5a83ef4da750821ab448970156a03656",
    "ccb67f52785ba11823ad1a44019c9a239f0d54cab2edf9ce4f4d731092847d14",
];
/// Per-round digests of the parity rounds against the full catalog.
const PARITY_FULL: &[&str] = &[
    "d6e1d6d25db7b391652dd0a4205c9cbf527b681341c830260a2ee4f7f7d24da8",
    "01c5d772ce47b073196ecff35f5fa82efd153b8cc38cac1623c7addb72dcc893",
    "68c387c54c3f4c578d607fd85e239a10c8fd89cda6847345c7450f40699128b1",
    "d6e1d6d25db7b391652dd0a4205c9cbf527b681341c830260a2ee4f7f7d24da8",
    "af494432d8d20cddbfbe8274e0927d32a76be9c8526f3a018d1611f21d388503",
];
/// Per-round digests of the parity rounds against the mini catalog.
const PARITY_MINI: &[&str] = &[
    "a976ae3df080925904a114922e9bc49a98a8637616ee1551e6724738fc09999c",
    "6455f10606ada2b8effd5029ff28f752cd6d263ab3200c85066573e00a0ef33b",
    "e6d4f0c7c32bae0b4f01e31b1b9e98b363cf3b4bcc3c4afd6ddaf02be468aa9b",
    "a976ae3df080925904a114922e9bc49a98a8637616ee1551e6724738fc09999c",
    "367852ef961fee7ac05d90ad4a347dd6fe915f6a31d60133243744c8619c5bd0",
];

#[derive(Deserialize)]
struct CapturedRound {
    replay: String,
    round: usize,
    input: OpenerRoundInput,
}

#[derive(Deserialize)]
struct ParityFixture {
    rounds: Vec<ParityRound>,
}

#[derive(Deserialize)]
struct ParityRound {
    input: ParityInput,
}

#[derive(Deserialize)]
struct ParityInput {
    name: String,
    observations: Vec<Option<OpenerObservation>>,
}

fn replay_rounds() -> Vec<(String, OpenerRoundInput)> {
    let rounds: Vec<CapturedRound> = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/perf/replay-round-inputs.json"
    )))
    .expect("captured replay-round fixture should parse");
    rounds
        .into_iter()
        .map(|round| (format!("{}#{}", round.replay, round.round), round.input))
        .collect()
}

fn parity_rounds() -> Vec<(String, OpenerRoundInput)> {
    let fixture: ParityFixture = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/parity-rounds.json"
    )))
    .expect("parity round fixture should parse");
    fixture
        .rounds
        .into_iter()
        .map(|round| {
            (
                round.input.name,
                OpenerRoundInput {
                    observations: round.input.observations,
                    ..OpenerRoundInput::default()
                },
            )
        })
        .collect()
}

fn install(catalog: &[u8]) {
    install_opener_runtime(catalog, None).expect("catalog should install");
}

fn analyze(input: &OpenerRoundInput) -> OpenerRoundAnalysis {
    analyze_opener_round(input).expect("catalog is installed")
}

fn serialized(analysis: &OpenerRoundAnalysis) -> Vec<u8> {
    serde_json::to_vec(analysis).expect("analysis should serialize")
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Analyzes every round in order on one install and returns each round's digest.
fn lane_digests(lane: &str, catalog: &[u8], rounds: &[(String, OpenerRoundInput)]) -> Vec<String> {
    install(catalog);
    let out = std::env::var_os("OPENER_PERF_OUT").map(std::path::PathBuf::from);
    rounds
        .iter()
        .enumerate()
        .map(|(index, (name, input))| {
            let bytes = serialized(&analyze(input));
            if let Some(dir) = &out {
                let dir = dir.join(lane);
                std::fs::create_dir_all(&dir).expect("identity output directory");
                let file = format!("{index:03}-{}.json", name.replace(['/', '#'], "_"));
                std::fs::write(dir.join(file), &bytes).expect("identity output write");
            }
            format!("{:x}", Sha256::digest(&bytes))
        })
        .collect()
}

// Both tests install the process-wide runtime, so run them with --test-threads=1.
#[test]
#[ignore = "pins every corpus round's output; under a minute on the fast profile"]
fn opener_rounds_are_byte_identical() {
    let replay = replay_rounds();
    let parity = parity_rounds();
    let lanes = [
        (
            "replay-full",
            lane_digests("replay-full", FULL_CATALOG, &replay),
            REPLAY_FULL,
        ),
        (
            "parity-full",
            lane_digests("parity-full", FULL_CATALOG, &parity),
            PARITY_FULL,
        ),
        (
            "parity-mini",
            lane_digests("parity-mini", MINI_CATALOG, &parity),
            PARITY_MINI,
        ),
    ];
    for (lane, actual, expected) in &lanes {
        let mismatched = actual
            .iter()
            .zip(expected.iter())
            .enumerate()
            .filter(|(_, (actual, expected))| actual != *expected)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        assert!(
            actual.len() == expected.len() && mismatched.is_empty(),
            "{lane}: {} rounds, {} pinned, mismatched rounds {mismatched:?}; actual:\n{actual:#?}",
            actual.len(),
            expected.len(),
        );
    }
}

/// Labels `passes` copies of `rounds` from a fresh install, dealt round-robin
/// over `threads` threads, and returns the wall time.
fn label(rounds: &[(String, OpenerRoundInput)], passes: usize, threads: usize) -> Duration {
    install(FULL_CATALOG);
    let started = Instant::now();
    std::thread::scope(|scope| {
        for thread in 0..threads {
            scope.spawn(move || {
                for job in (thread..passes * rounds.len()).step_by(threads) {
                    std::hint::black_box(analyze(&rounds[job % rounds.len()].1));
                }
            });
        }
    });
    started.elapsed()
}

#[test]
#[ignore = "timing only"]
fn opener_throughput() {
    let rounds = replay_rounds();
    let setting = |name: &str, default: usize| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let passes = setting("OPENER_THROUGHPUT_PASSES", 5);
    let threads = setting("OPENER_THROUGHPUT_THREADS", 1);

    let install_ms = (0..3)
        .map(|_| {
            let started = Instant::now();
            install(FULL_CATALOG);
            millis(started.elapsed())
        })
        .collect::<Vec<_>>();
    println!("install catalog-full: {install_ms:.1?} ms");

    install(FULL_CATALOG);
    let (mut first, mut warm, mut first_max) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    for (_, input) in &rounds {
        let started = Instant::now();
        let cold = serialized(&analyze(input));
        let elapsed = started.elapsed();
        first += elapsed;
        first_max = first_max.max(elapsed);
        let started = Instant::now();
        let again = serialized(&analyze(input));
        warm += started.elapsed();
        assert_eq!(cold, again);
    }
    let count = rounds.len() as f64;
    println!(
        "per round over {} rounds: first mean {:.2} ms (max {:.1}), warm mean {:.2} ms",
        rounds.len(),
        millis(first) / count,
        millis(first_max),
        millis(warm) / count
    );

    // The fastest of several warm passes resists load from other processes.
    let warm_pass = (0..passes)
        .map(|_| {
            let started = Instant::now();
            for (_, input) in &rounds {
                std::hint::black_box(analyze(input));
            }
            millis(started.elapsed()) / count
        })
        .fold(f64::INFINITY, f64::min);
    println!("warm per round, fastest of {passes} passes: {warm_pass:.2} ms");

    // The offline labeler sends the boards after each of the first 20 locks.
    let first_20 = rounds
        .iter()
        .map(|(name, input)| {
            let mut input = input.clone();
            input.observations.truncate(20);
            (name.clone(), input)
        })
        .collect::<Vec<_>>();
    for (lane, rounds) in [("whole rounds", &rounds), ("first 20 locks", &first_20)] {
        let elapsed = label(rounds, passes, threads);
        let labeled = passes * rounds.len();
        println!(
            "throughput ({lane}): {labeled} rounds in {:.0} ms on {threads} thread(s) from a fresh install: {:.1} rounds/s",
            millis(elapsed),
            labeled as f64 / elapsed.as_secs_f64()
        );
    }
}
