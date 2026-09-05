use std::time::Instant;

use crate::openers::catalog::OpenerCatalog;
use crate::openers::recognition::align::{align_round_exact, AlignmentResult, Observation};
use crate::openers::recognition::cost::EditCosts;
use crate::openers::recognition::{compile_recognition_graph, CompileBudget, RecognitionGraph};

use super::report::{
    BatteryReport, HardOutcomes, HybridCase, LegalCase, LockEvidence, Metrics, NegativeCase,
    OneErrorCase, RecallCase, RejoinCase, TranspositionCase, WalkLength,
};
use super::walk::{
    add_lowest_column_zero_cell, substitute_occupied_letter, synthesize_record, SynthesisFuel,
    MAX_SYNTHESIS_VISITS_PER_RECORD,
};

mod cases;
use cases::{run_hybrids, run_negatives, run_transpositions};

const REQUIRED_PREFIXES: [(&str, &str); 3] = [
    ("pco", "pco"),
    ("dt-cannon", "dt"),
    ("single-double-pc", "single-double-pc"),
];
const MAX_TRANSPOSTION_PAIRS_PER_RECORD: usize = 50;
const MAX_SYNTHESIS_STATES: u32 = 4_096;
const MAX_REQUIRED_SYNTHESIS_STATES: u32 = 16_384;

struct Walk {
    id: String,
    mirrored: bool,
    observations: Vec<Option<Observation>>,
    transpositions: Vec<(Vec<Option<Observation>>, Vec<Option<Observation>>)>,
}

pub(crate) fn run_behavioral_battery(
    graph: &RecognitionGraph,
    catalog: &OpenerCatalog,
) -> BatteryReport {
    let mut report = BatteryReport {
        edit_costs: EditCosts::default(),
        required_records: required_ids(catalog),
        required_records_present: false,
        selected_records: Vec::new(),
        skipped: Vec::new(),
        cap_skipped: 0,
        synthesis_subgraph_too_large: Vec::new(),
        synthesis_fuel_exhausted: Vec::new(),
        synthesis_visits_total: 0,
        synthesis_visits_max: 0,
        synthesis_fuel_limit: MAX_SYNTHESIS_VISITS_PER_RECORD,
        synthesis_walk_lengths: Vec::new(),
        required_record_exclusions: Vec::new(),
        legal: LegalCase::default(),
        one_error: OneErrorCase::default(),
        hybrid: HybridCase::default(),
        transposition: TranspositionCase::default(),
        negatives: NegativeCase::default(),
        recall: RecallCase::default(),
        rejoin: RejoinCase::default(),
        metrics: Metrics::default(),
        hard: HardOutcomes::default(),
    };
    eprintln!("[battery] synthesis start");
    let synthesis_started = Instant::now();
    let required_ids = report.required_records.clone();
    let required = walks_for_ids(
        catalog,
        &required_ids,
        MAX_REQUIRED_SYNTHESIS_STATES,
        true,
        &mut report,
    );
    let extra_ids = extra_ids(catalog, &required_ids);
    let extras = walks_for_ids(
        catalog,
        &extra_ids,
        MAX_SYNTHESIS_STATES,
        false,
        &mut report,
    );
    let mut legal_walks = required;
    legal_walks.extend(extras);
    eprintln!(
        "[battery] synthesis finish {}ms",
        synthesis_started.elapsed().as_millis()
    );
    report.selected_records = legal_walks.iter().map(walk_name).collect();
    report.selected_records.sort();
    report.required_records_present = required_ids.iter().all(|id| {
        legal_walks.iter().any(|walk| walk.id == *id)
            && !report.required_record_exclusions.contains(id)
    });
    phase("legal", || run_legal(graph, &legal_walks, &mut report));
    phase("one-error", || {
        run_errors(
            graph,
            legal_walks.iter().filter(|walk| !walk.mirrored).take(50),
            &mut report,
        )
    });
    phase("rejoin", || run_rejoin(graph, &legal_walks, &mut report));
    phase("hybrid", || run_hybrids(graph, &legal_walks, &mut report));
    phase("transposition", || {
        run_transpositions(graph, &legal_walks, &mut report)
    });
    phase("negatives", || run_negatives(graph, &mut report));
    report.hard.legal_walks = report.legal.checked > 0
        && report.legal.checked == report.legal.zero_cost
        && report.legal.checked == report.legal.unknown_strictly_worse;
    report.hard.substitution_retention = report.one_error.substitutions.total > 0
        && report.one_error.substitutions.passed == report.one_error.substitutions.total;
    report.hard.missing_retention = report.one_error.missing.total > 0
        && report.one_error.missing.passed == report.one_error.missing.total;
    report.hard.hybrid_nonzero = report.hybrid.checked == 20 && report.hybrid.zero_cost == 0;
    report.hard.transposition_zero_cost = report.transposition.checked >= 10
        && report.transposition.checked == report.transposition.equal_zero_cost;
    report.hard.negatives_unknown =
        report.negatives.checked == 25 && report.negatives.checked == report.negatives.unknown_wins;
    report.hard.rejoin_nonzero =
        report.rejoin.checked == 20 && report.rejoin.checked == report.rejoin.passed;
    report.hard.legal_exact_without_zero_cost_eviction = report.legal.checked > 0
        && report.legal.checked == report.legal.zero_cost
        && report.metrics.legal_evicted_zero_cost == 0;
    report
}

fn required_ids(catalog: &OpenerCatalog) -> Vec<String> {
    REQUIRED_PREFIXES
        .into_iter()
        .filter_map(|(exact, prefix)| {
            catalog
                .openers
                .iter()
                .find(|record| record.id == exact)
                .or_else(|| {
                    catalog
                        .openers
                        .iter()
                        .filter(|record| record.id.starts_with(prefix))
                        .min_by_key(|record| &record.id)
                })
                .map(|record| record.id.clone())
        })
        .collect()
}

fn extra_ids(catalog: &OpenerCatalog, required: &[String]) -> Vec<String> {
    let mut ids = catalog
        .openers
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids.into_iter()
        .filter(|id| !required.contains(id))
        .take(100)
        .collect()
}

fn walks_for_ids(
    catalog: &OpenerCatalog,
    ids: &[String],
    maximum_states: u32,
    required: bool,
    report: &mut BatteryReport,
) -> Vec<Walk> {
    let mut walks = Vec::new();
    for id in ids {
        let Some(record) = catalog.openers.iter().find(|record| record.id == *id) else {
            report.skipped.push(format!("missing record {id}"));
            record_required_exclusion(report, id, required);
            continue;
        };
        let single = OpenerCatalog {
            format_version: catalog.format_version,
            openers: vec![record.clone()],
        };
        let graph = match compile_recognition_graph(&single, &CompileBudget::default()) {
            Ok(graph) => graph,
            Err(error) => {
                report.skipped.push(format!("{id}: {error}"));
                record_required_exclusion(report, id, required);
                continue;
            }
        };
        let state_count = u32::try_from(graph.states.len()).unwrap_or(u32::MAX);
        if state_count > maximum_states {
            report.synthesis_subgraph_too_large.push(id.clone());
            record_required_exclusion(report, id, required);
            continue;
        }
        let started = Instant::now();
        let mut fuel = SynthesisFuel::new(MAX_SYNTHESIS_VISITS_PER_RECORD);
        let mut record_walks = Vec::new();
        let mut complete = true;
        for mirrored in [false, true] {
            let Some(synthesis) = synthesize_record(
                &graph,
                id,
                mirrored,
                MAX_TRANSPOSTION_PAIRS_PER_RECORD,
                &mut fuel,
            ) else {
                complete = false;
                break;
            };
            record_walks.push(Walk {
                id: id.clone(),
                mirrored,
                observations: synthesis.observations,
                transpositions: synthesis.transpositions,
            });
            report.synthesis_walk_lengths.push(WalkLength {
                record: id.clone(),
                mirrored,
                original_locks: synthesis.original_walk_locks,
                capped_locks: synthesis.capped_walk_locks,
            });
        }
        let stats = fuel.stats();
        report.synthesis_visits_total = report
            .synthesis_visits_total
            .saturating_add(u64::from(stats.visits));
        report.synthesis_visits_max = report.synthesis_visits_max.max(stats.visits);
        eprintln!(
            "[battery] {id} states={state_count} elapsed_ms={}",
            started.elapsed().as_millis()
        );
        if stats.fuel_exhausted {
            report.synthesis_fuel_exhausted.push(id.clone());
            record_required_exclusion(report, id, required);
        } else if complete {
            walks.extend(record_walks);
        } else {
            report
                .skipped
                .push(format!("no compiled lock walk for {id}"));
            record_required_exclusion(report, id, required);
        }
    }
    report.skipped.sort();
    report.synthesis_subgraph_too_large.sort();
    report.synthesis_fuel_exhausted.sort();
    report.synthesis_walk_lengths.sort_by(|left, right| {
        left.record
            .cmp(&right.record)
            .then(left.mirrored.cmp(&right.mirrored))
    });
    report.required_record_exclusions.sort();
    walks
}

fn record_required_exclusion(report: &mut BatteryReport, id: &str, required: bool) {
    if required {
        report.required_record_exclusions.push(id.to_owned());
    }
}

fn run_legal(graph: &RecognitionGraph, walks: &[Walk], report: &mut BatteryReport) {
    for (index, walk) in walks.iter().enumerate() {
        let result = align(graph, &walk.id, &walk.observations, &mut report.metrics);
        if (index + 1) % 100 == 0 {
            eprintln!("[battery] legal alignments {}", index + 1);
        }
        let unknown_worse = unknown_cost(&result).is_some_and(|cost| cost > best_cost(&result));
        let exact = is_best_record(&result, &walk.id) && best_cost(&result) == 0;
        report.legal.checked = report.legal.checked.saturating_add(1);
        report.legal.zero_cost = report.legal.zero_cost.saturating_add(u32::from(exact));
        report.legal.unknown_strictly_worse = report
            .legal
            .unknown_strictly_worse
            .saturating_add(u32::from(unknown_worse));
        report.metrics.legal_truncations = report
            .metrics
            .legal_truncations
            .saturating_add(u32::from(result.truncated_any));
        report.metrics.legal_evicted_zero_cost =
            report.metrics.legal_evicted_zero_cost.saturating_add(
                result
                    .per_lock
                    .iter()
                    .map(|lock| lock.evicted_zero_cost)
                    .sum::<u32>(),
            );
    }
}

fn run_errors<'a>(
    graph: &RecognitionGraph,
    walks: impl Iterator<Item = &'a Walk>,
    report: &mut BatteryReport,
) {
    for walk in walks {
        for index in 0..walk.observations.len().min(4) {
            let Some(observation) = walk.observations[index].as_ref() else {
                continue;
            };
            if let Some(substitution) = substitute_occupied_letter(observation) {
                let mut trace = walk.observations.clone();
                trace[index] = Some(substitution);
                let result = align(graph, &walk.id, &trace, &mut report.metrics);
                report
                    .one_error
                    .substitutions
                    .record(is_best_record(&result, &walk.id));
                record_margin(&result, report);
                if index == 0 {
                    report
                        .recall
                        .lock_one_substitution
                        .record(any_record(&result, &walk.id));
                }
                if index == 1 {
                    report
                        .recall
                        .lock_two_substitution
                        .record(any_record(&result, &walk.id));
                }
            }
            let base = walk.observations[index.saturating_sub(1)]
                .as_ref()
                .unwrap_or(observation);
            let mut trace = walk.observations.clone();
            trace.insert(index, Some(add_lowest_column_zero_cell(base)));
            let result = align(graph, &walk.id, &trace, &mut report.metrics);
            report
                .one_error
                .extras
                .record(any_record(&result, &walk.id));
            let mut trace = walk.observations.clone();
            trace.remove(index);
            let result = align(graph, &walk.id, &trace, &mut report.metrics);
            report
                .one_error
                .missing
                .record(is_best_record(&result, &walk.id));
        }
    }
}

fn run_rejoin(graph: &RecognitionGraph, walks: &[Walk], report: &mut BatteryReport) {
    for walk in walks.iter().filter(|walk| !walk.mirrored) {
        if report.rejoin.checked == 20 {
            break;
        }
        if walk.observations.len() < 5 {
            continue;
        }
        let Some(base) = walk.observations[2].as_ref() else {
            continue;
        };
        let mut observations = walk.observations[..3].to_vec();
        observations.push(Some(add_lowest_column_zero_cell(base)));
        observations.extend_from_slice(&walk.observations[4..]);
        let result = align(graph, &walk.id, &observations, &mut report.metrics);
        let cost = best_cost(&result);
        let passed = cost > 0 && is_best_record(&result, &walk.id);
        report.rejoin.checked = report.rejoin.checked.saturating_add(1);
        report.rejoin.passed = report.rejoin.passed.saturating_add(u32::from(passed));
        report.rejoin.best_costs.push(cost);
    }
}

fn align(
    graph: &RecognitionGraph,
    record: &str,
    observations: &[Option<Observation>],
    metrics: &mut Metrics,
) -> AlignmentResult {
    let started = Instant::now();
    let result = align_round_exact(
        graph,
        &EditCosts::default(),
        observations,
        &Default::default(),
    );
    let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    if micros > 5_000_000 {
        metrics
            .slow_alignments
            .push(format!("{record}:{} locks", observations.len()));
    }
    metrics.alignment_micros_total = metrics.alignment_micros_total.saturating_add(micros);
    *metrics
        .alignment_micros_histogram
        .entry(histogram_bucket(micros))
        .or_default() += 1;
    metrics.active_product_states_max = metrics.active_product_states_max.max(
        result
            .per_lock
            .iter()
            .map(|lock| lock.active_product_states)
            .max()
            .unwrap_or_default(),
    );
    metrics.seeds_total = metrics.seeds_total.saturating_add(
        result
            .per_lock
            .iter()
            .map(|lock| u64::from(lock.seeded))
            .sum::<u64>(),
    );
    metrics.truncations = metrics
        .truncations
        .saturating_add(u32::from(result.truncated_any));
    metrics.reseed_truncations = metrics.reseed_truncations.saturating_add(
        result
            .per_lock
            .iter()
            .map(|lock| u32::from(lock.reseed_truncated))
            .sum::<u32>(),
    );
    metrics.evicted_zero_cost = metrics.evicted_zero_cost.saturating_add(
        result
            .per_lock
            .iter()
            .map(|lock| lock.evicted_zero_cost)
            .sum::<u32>(),
    );
    metrics.zero_cost_excess = metrics.zero_cost_excess.saturating_add(
        result
            .per_lock
            .iter()
            .map(|lock| lock.zero_cost_excess)
            .sum::<u32>(),
    );
    metrics.per_lock_evidence.push(LockEvidence {
        record: record.to_owned(),
        locks: result.per_lock.clone(),
    });
    result
}

fn phase(name: &str, run: impl FnOnce()) {
    let started = Instant::now();
    eprintln!("[battery] {name} start");
    run();
    eprintln!(
        "[battery] {name} finish {}ms",
        started.elapsed().as_millis()
    );
}

fn histogram_bucket(micros: u64) -> String {
    let upper = [100, 1_000, 10_000, 100_000, 1_000_000]
        .into_iter()
        .find(|upper| micros <= *upper);
    upper.map_or_else(|| ">1000000".to_owned(), |upper| format!("<={upper}"))
}

fn best_cost(result: &AlignmentResult) -> u32 {
    result
        .hypotheses
        .first()
        .map_or(u32::MAX, |hypothesis| hypothesis.total_cost)
}

fn unknown_cost(result: &AlignmentResult) -> Option<u32> {
    result
        .hypotheses
        .iter()
        .find(|hypothesis| hypothesis.origin.is_none())
        .map(|hypothesis| hypothesis.total_cost)
}

fn any_record(result: &AlignmentResult, record: &str) -> bool {
    result.hypotheses.iter().any(|hypothesis| {
        hypothesis
            .origin
            .as_ref()
            .is_some_and(|origin| origin.record.as_ref() == record)
    })
}

fn is_best_record(result: &AlignmentResult, record: &str) -> bool {
    result.hypotheses.iter().any(|hypothesis| {
        hypothesis.total_cost == best_cost(result)
            && hypothesis
                .origin
                .as_ref()
                .is_some_and(|origin| origin.record.as_ref() == record)
    })
}

fn record_margin(result: &AlignmentResult, report: &mut BatteryReport) {
    if let Some(margin) = result.best_margin {
        report.one_error.margins.push(margin);
    }
}

fn walk_name(walk: &Walk) -> String {
    format!("{} mirrored={}", walk.id, walk.mirrored)
}
