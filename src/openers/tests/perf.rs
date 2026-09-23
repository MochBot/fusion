//! Timing harness over captured replay rounds against the live catalog asset.
//!
//! Run explicitly (ignored by default):
//!
//! ```bash
//! OPENER_PERF_OUT=evidence-out/opener-perf \
//!   cargo test --release --lib openers::tests::perf::replay_round_timing -- \
//!     --ignored --exact --test-threads=1 --nocapture
//! ```
//!
//! The inputs are Mosaic's captured observations for every player-round of the
//! two versus fixture replays. The corpus digest covers every serialized
//! analysis DTO in order, so any optimization must reproduce it exactly. The
//! installed lane measures the production path (catalog installed once, rounds
//! analyzed in replay order, caches warm across rounds); the uncached lane
//! analyzes each round against a transient identity so no compiled graph is
//! reused.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use sha2::{Digest as ShaDigest, Sha256};

use crate::openers::analyze::{
    analyze_opener_round, analyze_opener_round_with_catalog, OpenerRoundAnalysis, OpenerRoundInput,
};
use crate::openers::catalog::{
    installed_opener_catalog, isolated_catalog_test, CatalogTestScope, OpenerCatalog,
};
use crate::openers::install_opener_runtime;
#[cfg(not(target_arch = "wasm32"))]
use crate::openers::recognition::profile::{CacheRecordOutcome, CacheRecordProfile};

const LIVE_CATALOG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/openers/catalog-full.json"
);
const EXPECTED_RECOGNITION_DIGEST: &str =
    "eed18da10ca9ea166bc5562e151cf48e955cfcf4533adf6e855c7ee13be638d9";

#[derive(serde::Deserialize)]
pub(crate) struct CapturedRound {
    pub(crate) replay: String,
    pub(crate) round: usize,
    pub(crate) input: OpenerRoundInput,
}

pub(crate) fn load_rounds() -> Vec<CapturedRound> {
    serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/perf/replay-round-inputs.json"
    )))
    .expect("captured replay-round fixture should parse")
}

/// Installs the live catalog for the scope's lifetime and returns a detached
/// copy for the uncached lane.
pub(crate) fn install_live_catalog() -> (CatalogTestScope, OpenerCatalog) {
    let path =
        std::env::var("OPENER_RECOGNITION_CATALOG").unwrap_or_else(|_| LIVE_CATALOG.to_owned());
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("read {path}: {error}"));
    let scope = isolated_catalog_test();
    install_opener_runtime(&bytes, None).expect("live catalog should parse and validate");
    let installed = installed_opener_catalog().expect("catalog should be installed");
    let detached = OpenerCatalog {
        format_version: installed.format_version,
        openers: installed.openers.clone(),
    };
    (scope, detached)
}

struct Digest(Sha256);

impl Digest {
    fn new() -> Self {
        Self(Sha256::new())
    }

    fn round(&mut self, round: &CapturedRound, analysis: &OpenerRoundAnalysis) {
        self.0.update(round.replay.as_bytes());
        self.0.update([0]);
        self.0.update(round.round.to_le_bytes());
        self.0.update([0]);
        let bytes = serde_json::to_vec(analysis).expect("analysis should serialize");
        self.0.update(bytes);
    }

    fn finish(self) -> String {
        format!("{:x}", self.0.finalize())
    }
}

#[test]
#[ignore]
fn replay_round_timing() {
    let (_scope, detached) = install_live_catalog();
    let rounds = load_rounds();

    let mut installed_digest = Digest::new();
    let mut uncached_digest = Digest::new();
    let mut installed_total = Duration::ZERO;
    let mut uncached_total = Duration::ZERO;
    let mut per_round = String::new();

    for round in &rounds {
        let started = Instant::now();
        let installed = analyze_opener_round(&round.input).expect("catalog is installed");
        let installed_elapsed = started.elapsed();
        installed_total += installed_elapsed;
        installed_digest.round(round, &installed);

        let started = Instant::now();
        let uncached = analyze_opener_round_with_catalog(&detached, &round.input);
        let uncached_elapsed = started.elapsed();
        uncached_total += uncached_elapsed;
        uncached_digest.round(round, &uncached);

        let survivors = installed
            .catalogued_board_match
            .as_ref()
            .map_or(0, |matched| matched.matching_openers.len());
        let recognition = installed.recognition.as_ref();
        let _ = writeln!(
            per_round,
            "| {} | {} | {} | {} | {} | {} | {:.1} | {:.1} |",
            round.replay.trim_end_matches(".ttrm"),
            round.round,
            round.input.observations.len(),
            survivors,
            recognition.map_or(0, |value| value.shortlist_size),
            recognition.map_or(0, |value| value.shortlist_compile_skipped),
            millis(installed_elapsed),
            millis(uncached_elapsed),
        );
    }

    let installed_digest = installed_digest.finish();
    let uncached_digest = uncached_digest.finish();
    let count = rounds.len() as f64;
    let mut summary = String::new();
    let _ = writeln!(summary, "# Opener round timing\n");
    let _ = writeln!(summary, "- Rounds: {}", rounds.len());
    let _ = writeln!(
        summary,
        "- Installed-path corpus digest: `{installed_digest}`"
    );
    let _ = writeln!(summary, "- Uncached corpus digest: `{uncached_digest}`");
    let _ = writeln!(
        summary,
        "- Installed path (`analyze_opener_round`): total {:.0} ms, mean {:.1} ms",
        millis(installed_total),
        millis(installed_total) / count
    );
    let _ = writeln!(
        summary,
        "- Uncached path (`analyze_opener_round_with_catalog`): total {:.0} ms, mean {:.1} ms",
        millis(uncached_total),
        millis(uncached_total) / count
    );
    let _ = writeln!(summary, "\n| replay | round | locks | survivors | shortlist | skipped | installed ms | uncached ms |");
    let _ = writeln!(
        summary,
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
    );
    summary.push_str(&per_round);
    println!("{summary}");

    assert_eq!(
        installed_digest, uncached_digest,
        "cached installed path must reproduce the uncached analysis exactly"
    );
    assert_eq!(
        installed_digest, EXPECTED_RECOGNITION_DIGEST,
        "opener recognition output changed; update the oracle only with reviewed evidence"
    );

    if let Some(out) = std::env::var_os("OPENER_PERF_OUT") {
        let dir = std::path::PathBuf::from(out);
        std::fs::create_dir_all(&dir).expect("perf output directory should be creatable");
        std::fs::write(dir.join("summary.md"), summary).expect("perf summary should be writable");
    }
}

pub(crate) fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[test]
#[ignore]
fn replay_round_stage_timing() {
    use crate::openers::catalogued_match::match_catalogued_boards_with_targets;
    use crate::openers::phase::{assess_opener_phase, prepare_observations};
    use crate::openers::recognition::round::recognize_round;

    let (_scope, _detached) = install_live_catalog();
    let installed = installed_opener_catalog().expect("catalog should be installed");
    let rounds = load_rounds();
    let mut prepare = Duration::ZERO;
    let mut assess = Duration::ZERO;
    let mut confirm = Duration::ZERO;
    let mut recognize_cold = Duration::ZERO;
    let mut recognize_warm = Duration::ZERO;

    for round in &rounds {
        // Normalization is shared by the three stage calls below, so it is timed
        // once here and excluded from each stage mean: a printed stage number
        // never contains preparation, and prepare+assess+confirm+recognize
        // accounts for the whole measured path.
        let started = Instant::now();
        let prepared = prepare_observations(&round.input.observations);
        prepare += started.elapsed();

        let started = Instant::now();
        let assessments = assess_opener_phase(&installed.targets, &prepared);
        assess += started.elapsed();

        let started = Instant::now();
        let matched = match_catalogued_boards_with_targets(
            installed.catalog(),
            &installed.node_boards,
            &installed.runtime_search_shape_targets,
            &prepared,
        );
        confirm += started.elapsed();

        for lane in [&mut recognize_cold, &mut recognize_warm] {
            let started = Instant::now();
            let _ = recognize_round(
                &installed.compiled,
                &assessments,
                matched.as_ref(),
                &prepared,
            );
            *lane += started.elapsed();
        }
    }

    let count = rounds.len() as f64;
    println!(
        "stage means over {} rounds: prepare {:.1} ms, assess {:.1} ms, confirm {:.1} ms, recognize first-seen {:.1} ms, recognize warm {:.1} ms",
        rounds.len(),
        millis(prepare) / count,
        millis(assess) / count,
        millis(confirm) / count,
        millis(recognize_cold) / count,
        millis(recognize_warm) / count
    );
}

/// Bounded first-seen/warm recognition discriminator.
///
/// Runs a handful of fixture rounds twice through the real recognition path on
/// the shared installed cache: the first-seen call cold-compiles records this
/// snapshot has not compiled before, the warm call reuses them. Both go through
/// `recognize_round_profiled`, so every stage timing and per-record outcome is
/// observed from the actual execution rather than a copied shortlist or a
/// separate graph preflight. Measurement only; the sole assertion is
/// first-seen/warm result parity. The `prepare` column is shared observation
/// normalization, timed on its own and excluded from the stage columns;
/// `warm full` is the whole-call number that includes it.
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore]
fn round_stage_split() {
    use crate::openers::catalogued_match::match_catalogued_boards_with_targets;
    use crate::openers::guide::{build_guide, select_subject};
    use crate::openers::phase::{assess_opener_phase, prepare_observations};
    use crate::openers::recognition::round::recognize_round_profiled;
    use crate::openers::report::build_opener_report;

    let (_scope, _detached) = install_live_catalog();
    let installed = installed_opener_catalog().expect("catalog should be installed");
    let rounds = load_rounds();
    // Bounded: two large early rounds plus two mid-size ones. The installed
    // cache stays shared across picked rows, so only genuinely first-seen
    // records cold-compile; later rows reuse them.
    let picked = [0usize, 1, 7, 17]
        .into_iter()
        .filter(|index| *index < rounds.len())
        .collect::<Vec<_>>();

    println!("| round | locks | prepare | assess | confirm | report | recog first-seen | select | map | cache | cold n | cold ms | union | merged | align | result | states | transitions | recog warm | guide | warm full |");
    for index in picked {
        let round = &rounds[index];
        // Normalization is shared by every stage call in this row, so it is
        // timed on its own (the `prepare` column) and excluded from every other
        // stage column; `warm full` remains the comparable whole-call number.
        let started = Instant::now();
        let prepared = prepare_observations(&round.input.observations);
        let prepare = started.elapsed();
        let started = Instant::now();
        let assessments = assess_opener_phase(&installed.targets, &prepared);
        let assess = started.elapsed();
        let started = Instant::now();
        let matched = match_catalogued_boards_with_targets(
            installed.catalog(),
            &installed.node_boards,
            &installed.runtime_search_shape_targets,
            &prepared,
        );
        let confirm = started.elapsed();
        let started = Instant::now();
        let report = build_opener_report(matched.as_ref());
        let report_time = started.elapsed();

        let (first, first_profile) = recognize_round_profiled(
            &installed.compiled,
            &assessments,
            matched.as_ref(),
            &prepared,
        );
        let (second, warm_profile) = recognize_round_profiled(
            &installed.compiled,
            &assessments,
            matched.as_ref(),
            &prepared,
        );
        assert_eq!(
            serde_json::to_vec(&first).expect("recognition should serialize"),
            serde_json::to_vec(&second).expect("recognition should serialize"),
            "first-seen and warm recognition must agree on round {index}"
        );
        let cold_ms: f64 = first_profile
            .cache
            .records
            .iter()
            .filter_map(|record| record.compile)
            .map(millis)
            .sum();
        let cold_count = first_profile
            .cache
            .records
            .iter()
            .filter(|record| record.compile.is_some())
            .count();
        let warm_cold_count = warm_profile
            .cache
            .records
            .iter()
            .filter(|record| record.compile.is_some())
            .count();
        let started = Instant::now();
        let guide = select_subject(installed.catalog(), matched.as_ref(), first.as_ref()).and_then(
            |subject| build_guide(installed.catalog(), &subject, &prepared, first.as_ref()),
        );
        let guide_time = started.elapsed();
        let _ = (report, guide);
        let started = Instant::now();
        let _ = analyze_opener_round(&round.input).expect("catalog is installed");
        let warm_full = started.elapsed();
        println!(
            "| {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {} | {} | {} | {} | {} | {:.1} | {} | {} | {} | {} | {} | {} | {} | {:.1} | {:.1} |",
            index,
            round.input.observations.len(),
            millis(prepare),
            millis(assess),
            millis(confirm),
            millis(report_time),
            profile_millis(first_profile.total_recognition),
            profile_millis(first_profile.shortlist_selection),
            profile_millis(first_profile.observation_mapping),
            profile_millis(first_profile.cache.graph_total),
            cold_count,
            cold_ms,
            profile_millis(first_profile.cache.union_graphs),
            profile_millis(first_profile.cache.merged_compile),
            profile_millis(first_profile.align),
            profile_millis(first_profile.result_mapping),
            first_profile.cache.graph_states.map_or("-".to_owned(), |states| states.to_string()),
            first_profile
                .cache
                .graph_transitions
                .map_or("-".to_owned(), |transitions| transitions.to_string()),
            profile_millis(warm_profile.total_recognition),
            millis(guide_time),
            millis(warm_full),
        );
        println!(
            "| | warm cache: {} cold attempts, shortlist {} |",
            warm_cold_count,
            warm_profile
                .shortlist
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(","),
        );
        for record in &first_profile.cache.records {
            println!("| | rec {} {} |", record.id, record_evidence(record));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn profile_millis(span: Option<Duration>) -> String {
    span.map_or_else(
        || "-".to_owned(),
        |elapsed| format!("{:.1}", millis(elapsed)),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn record_evidence(record: &CacheRecordProfile) -> String {
    let outcome = match record.outcome {
        CacheRecordOutcome::MissingCatalogRecord => "missing",
        CacheRecordOutcome::CachedSuccess => "cached-ok",
        CacheRecordOutcome::CachedFailure => "cached-fail",
        CacheRecordOutcome::ColdSuccess => "cold-ok",
        CacheRecordOutcome::ColdFailure => "cold-fail",
    };
    record.compile.map_or_else(
        || outcome.to_owned(),
        |elapsed| format!("{outcome} {:.1}", millis(elapsed)),
    )
}
