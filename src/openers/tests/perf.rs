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
//! The captured inputs are the exact `OpenerRoundInput` payloads Mosaic sent
//! through `analyze_opener_round` for every player-round of the two versus
//! fixture replays. The corpus digest covers every serialized analysis DTO in
//! order, so any optimization must reproduce it exactly. The installed lane
//! measures the production path (catalog installed once, rounds analyzed in
//! replay order, caches warm across rounds); the uncached lane analyzes each
//! round against a transient identity so no compiled graph is reused.

use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use crate::openers::analyze::{
    analyze_opener_round, analyze_opener_round_with_catalog, OpenerRoundAnalysis, OpenerRoundInput,
};
use crate::openers::catalog::{
    installed_opener_catalog, isolated_catalog_test, set_opener_catalog, CatalogTestScope,
    OpenerCatalog,
};

const LIVE_CATALOG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/openers/catalog-full.json"
);

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
    set_opener_catalog(&bytes).expect("live catalog should parse and validate");
    let installed = installed_opener_catalog().expect("catalog should be installed");
    let detached = OpenerCatalog {
        format_version: installed.format_version,
        openers: installed.openers.clone(),
    };
    (scope, detached)
}

struct Digest(DefaultHasher);

impl Digest {
    fn new() -> Self {
        Self(DefaultHasher::new())
    }

    fn round(&mut self, round: &CapturedRound, analysis: &OpenerRoundAnalysis) {
        round.replay.hash(&mut self.0);
        round.round.hash(&mut self.0);
        serde_json::to_vec(analysis)
            .expect("analysis should serialize")
            .hash(&mut self.0);
    }

    fn finish(self) -> String {
        format!("{:016x}", self.0.finish())
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
    use crate::openers::phase::assess_opener_phase;
    use crate::openers::recognition::round::recognize_round;

    let (_scope, _detached) = install_live_catalog();
    let installed = installed_opener_catalog().expect("catalog should be installed");
    let rounds = load_rounds();
    let mut assess = Duration::ZERO;
    let mut confirm = Duration::ZERO;
    let mut recognize_cold = Duration::ZERO;
    let mut recognize_warm = Duration::ZERO;

    for round in &rounds {
        let started = Instant::now();
        let assessments = assess_opener_phase(&installed.targets, &round.input.observations);
        assess += started.elapsed();

        let started = Instant::now();
        let matched = match_catalogued_boards_with_targets(
            &installed.catalog,
            &installed.node_boards,
            &installed.runtime_search_shape_targets,
            &round.input.observations,
        );
        confirm += started.elapsed();

        for lane in [&mut recognize_cold, &mut recognize_warm] {
            let started = Instant::now();
            let _ = recognize_round(
                Some(&installed.catalog),
                &installed.compiled,
                &assessments,
                matched.as_ref(),
                &round.input.observations,
            );
            *lane += started.elapsed();
        }
    }

    let count = rounds.len() as f64;
    println!(
        "stage means over {} rounds: assess {:.1} ms, confirm {:.1} ms, recognize first-seen {:.1} ms, recognize warm {:.1} ms",
        rounds.len(),
        millis(assess) / count,
        millis(confirm) / count,
        millis(recognize_cold) / count,
        millis(recognize_warm) / count
    );
}

/// Bounded compile/union/alignment discriminator.
///
/// Runs a handful of fixture rounds and splits warm recognition into
/// `shortlist_graph` (union clone only, second call) versus full
/// `recognize_round` (union + alignment), with per-record cache-miss counts
/// and merged-graph sizes. Measurement only; asserts nothing about timing.
#[test]
#[ignore]
fn round_stage_split() {
    use crate::openers::catalogued_match::match_catalogued_boards_with_targets;
    use crate::openers::guide::{build_guide, select_subject};
    use crate::openers::phase::assess_opener_phase;
    use crate::openers::recognition::round::recognize_round;
    use crate::openers::report::build_opener_report;

    let (_scope, _detached) = install_live_catalog();
    let installed = installed_opener_catalog().expect("catalog should be installed");
    let rounds = load_rounds();
    // Bounded: two large early rounds plus two mid-size ones.
    let picked = [0usize, 1, 7, 17]
        .into_iter()
        .filter(|index| *index < rounds.len())
        .collect::<Vec<_>>();

    println!("| round | locks | assess | confirm | report | shortlist1 compile+union | misses | states | transitions | shortlist2 union | recognize union+align | align~=rec-s2 | guide | full |");
    for index in picked {
        let round = &rounds[index];
        let started = Instant::now();
        let assessments = assess_opener_phase(&installed.targets, &round.input.observations);
        let assess = started.elapsed();
        let started = Instant::now();
        let matched = match_catalogued_boards_with_targets(
            &installed.catalog,
            &installed.node_boards,
            &installed.runtime_search_shape_targets,
            &round.input.observations,
        );
        let confirm = started.elapsed();
        let started = Instant::now();
        let report = build_opener_report(matched.as_ref());
        let report_time = started.elapsed();

        // Measurement mirror of `round::shortlist` (ids only, same order/cap).
        let mut ids = Vec::new();
        if let Some(matched_ref) = matched.as_ref() {
            for opener in &matched_ref.matching_openers {
                if !ids.iter().any(|candidate| candidate == &opener.id) {
                    ids.push(opener.id.clone());
                }
            }
        }
        for assessment in assessments.iter().flatten() {
            if let Some(board_match) = &assessment.r#match {
                if !ids
                    .iter()
                    .any(|candidate| candidate == &board_match.opener_id)
                {
                    ids.push(board_match.opener_id.clone());
                }
            }
            for runner_up in &assessment.runners_up {
                if !ids
                    .iter()
                    .any(|candidate| candidate == &runner_up.opener_id)
                {
                    ids.push(runner_up.opener_id.clone());
                }
            }
        }
        ids.truncate(24);

        let before = installed.compiled.len();
        let started = Instant::now();
        let first = installed.compiled.shortlist_graph(&installed.catalog, &ids);
        let shortlist1 = started.elapsed();
        let misses = installed.compiled.len().saturating_sub(before);
        let (states, transitions) = first.as_ref().map_or((0, 0), |compiled| {
            (
                compiled.graph.states.len(),
                compiled.graph.out.iter().map(Vec::len).sum::<usize>(),
            )
        });
        let started = Instant::now();
        let _ = installed.compiled.shortlist_graph(&installed.catalog, &ids);
        let shortlist2 = started.elapsed();
        let started = Instant::now();
        let recognition = recognize_round(
            Some(&installed.catalog),
            &installed.compiled,
            &assessments,
            matched.as_ref(),
            &round.input.observations,
        );
        let recognize = started.elapsed();
        let started = Instant::now();
        let guide = select_subject(&installed.catalog, matched.as_ref(), recognition.as_ref())
            .and_then(|subject| {
                build_guide(
                    &installed.catalog,
                    &subject,
                    &round.input.observations,
                    recognition.as_ref(),
                )
            });
        let guide_time = started.elapsed();
        let _ = (report, guide);
        let started = Instant::now();
        let _ = analyze_opener_round(&round.input).expect("catalog is installed");
        let full = started.elapsed();
        println!(
            "| {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {} | {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |",
            index,
            round.input.observations.len(),
            millis(assess),
            millis(confirm),
            millis(report_time),
            millis(shortlist1),
            misses,
            states,
            transitions,
            millis(shortlist2),
            millis(recognize),
            (millis(recognize) - millis(shortlist2)).max(0.0),
            millis(guide_time),
            millis(full),
        );
    }
}
