//! Cold-record ranking with single-record compile stage breakdown.
//!
//! Run explicitly (ignored by default):
//!
//! ```bash
//! OPENER_COMPILE_STAGES_OUT=evidence-out/opener-compile-stages \
//!   cargo test --release --lib \
//!     openers::tests::perf_compile_stages::cold_record_ranking_with_stage_breakdown -- \
//!     --ignored --exact --test-threads=1 --nocapture
//! ```
//!
//! The ranking walks the captured replay-round fixture through the real
//! `recognize_round_profiled` path on a shared cache, so every cold record is
//! observed from actual execution: no copied shortlist, no separate graph
//! preflight. The top records are then recompiled one by one with the actual
//! compiler under stage profiling. Measurement only; the single assert per
//! top record is determinism (stage recompile agrees with the cold outcome),
//! never a timing threshold.
//!
//! Stage columns are exclusive leaves (`legality`, `intern`, `transition`,
//! `finish`); `other-residual` is the residual of that same compile and also
//! carries the profiling clocks' own overhead, so it must not be read as a
//! timed span.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

use super::perf::{install_live_catalog, load_rounds, millis};
use crate::openers::catalog::installed_opener_catalog;
use crate::openers::catalogued_match::match_catalogued_boards_with_targets;
use crate::openers::phase::assess_opener_phase;
use crate::openers::recognition::compile_stages::compile_single_record_stages;
use crate::openers::recognition::profile::CacheRecordOutcome;
use crate::openers::recognition::round::recognize_round_profiled;
use crate::openers::recognition::CompileBudget;

const TOP_N: usize = 8;

struct ColdEntry {
    compile: Duration,
    outcome: CacheRecordOutcome,
    first_round: usize,
}

#[test]
#[ignore]
fn cold_record_ranking_with_stage_breakdown() {
    let (_scope, _detached) = install_live_catalog();
    let installed = installed_opener_catalog().expect("catalog should be installed");
    let rounds = load_rounds();
    assert!(!rounds.is_empty(), "replay-round fixture must not be empty");

    let mut cold: HashMap<String, ColdEntry> = HashMap::new();
    for (round_index, round) in rounds.iter().enumerate() {
        let assessments = assess_opener_phase(&installed.targets, &round.input.observations);
        let matched = match_catalogued_boards_with_targets(
            &installed.catalog,
            &installed.node_boards,
            &installed.runtime_search_shape_targets,
            &round.input.observations,
        );
        let (_recognition, profile) = recognize_round_profiled(
            Some(&installed.catalog),
            &installed.compiled,
            &assessments,
            matched.as_ref(),
            &round.input.observations,
        );
        for record in &profile.cache.records {
            if let Some(compile) = record.compile {
                assert!(
                    cold.insert(
                        record.id.clone(),
                        ColdEntry {
                            compile,
                            outcome: record.outcome,
                            first_round: round_index,
                        }
                    )
                    .is_none(),
                    "record {} cold-compiled twice on a shared cache",
                    record.id
                );
            }
        }
    }

    let mut ranked: Vec<(&String, &ColdEntry)> = cold.iter().collect();
    assert!(!ranked.is_empty(), "fixture walk must cold-compile records");
    ranked.sort_by(|left, right| {
        right
            .1
            .compile
            .cmp(&left.1.compile)
            .then_with(|| left.0.cmp(right.0))
    });

    let mut summary = String::new();
    let _ = writeln!(summary, "# Cold-record compile stages\n");
    let _ = writeln!(summary, "- Rounds walked: {}", rounds.len());
    let _ = writeln!(summary, "- Unique cold records: {}", ranked.len());
    let budget = CompileBudget::default();
    let _ = writeln!(
        summary,
        "- CompileBudget: {} states/edge, {} placements/edge, {} DFS visits/edge, {} total states",
        budget.max_states_per_edge,
        budget.max_placements_per_edge,
        budget.max_dfs_visits_per_edge,
        budget.max_total_states,
    );
    let _ = writeln!(
        summary,
        "\n| rank | record | cold ms | cold outcome | first round |"
    );
    let _ = writeln!(summary, "| ---: | --- | ---: | --- | ---: |");
    for (rank, (id, entry)) in ranked.iter().enumerate() {
        let _ = writeln!(
            summary,
            "| {} | {} | {:.1} | {} | {} |",
            rank + 1,
            id,
            millis(entry.compile),
            outcome_label(entry.outcome),
            entry.first_round,
        );
    }

    let _ = writeln!(
        summary,
        "\n| record | stage total ms | legality ms | intern ms | transition ms | finish ms | other-residual ms | states | legality calls | intern calls | transition calls | recompile |"
    );
    let _ = writeln!(
        summary,
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |"
    );
    for (id, entry) in ranked.iter().take(TOP_N) {
        let report = compile_single_record_stages(&installed.catalog, id)
            .expect("cold record should still be in the catalog");
        assert_eq!(
            report.success,
            matches!(entry.outcome, CacheRecordOutcome::ColdSuccess),
            "stage recompile of {id} must agree with its cold outcome"
        );
        let _ = writeln!(
            summary,
            "| {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {} | {} | {} | {} | {} |",
            id,
            millis(report.total_wall),
            millis(report.stages.legality),
            millis(report.stages.intern),
            millis(report.stages.transition),
            millis(report.stages.finish),
            millis(report.other_residual),
            report.graph_states,
            report.stages.legality_calls,
            report.stages.intern_calls,
            report.stages.transition_calls,
            if report.success { "ok" } else { "fail" },
        );
        if let Some(error) = &report.error {
            let _ = writeln!(summary, "| | | | | | | | | | | | error: {error} |");
        }
    }
    println!("{summary}");

    let out = std::env::var_os("OPENER_COMPILE_STAGES_OUT").map(std::path::PathBuf::from);
    let out = out.or_else(|| {
        std::env::var_os("OPENER_PERF_OUT")
            .map(std::path::PathBuf::from)
            .map(|dir| dir.join("compile-stages"))
    });
    if let Some(dir) = out {
        std::fs::create_dir_all(&dir).expect("stage output directory should be creatable");
        std::fs::write(dir.join("summary.md"), summary).expect("stage summary should be writable");
    }
}

fn outcome_label(outcome: CacheRecordOutcome) -> &'static str {
    match outcome {
        CacheRecordOutcome::MissingCatalogRecord => "missing",
        CacheRecordOutcome::CachedSuccess => "cached-ok",
        CacheRecordOutcome::CachedFailure => "cached-fail",
        CacheRecordOutcome::ColdSuccess => "cold-ok",
        CacheRecordOutcome::ColdFailure => "cold-fail",
    }
}
