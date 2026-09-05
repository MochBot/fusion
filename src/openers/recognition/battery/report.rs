use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::openers::recognition::align::LockAlignment;
use crate::openers::recognition::cost::EditCosts;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatteryReport {
    pub(crate) edit_costs: EditCosts,
    pub(crate) required_records: Vec<String>,
    pub(crate) required_records_present: bool,
    pub(crate) selected_records: Vec<String>,
    pub(crate) skipped: Vec<String>,
    pub(crate) cap_skipped: u32,
    pub(crate) synthesis_subgraph_too_large: Vec<String>,
    pub(crate) synthesis_fuel_exhausted: Vec<String>,
    pub(crate) synthesis_visits_total: u64,
    pub(crate) synthesis_visits_max: u32,
    pub(crate) synthesis_fuel_limit: u32,
    pub(crate) synthesis_walk_lengths: Vec<WalkLength>,
    pub(crate) required_record_exclusions: Vec<String>,
    pub(crate) legal: LegalCase,
    pub(crate) one_error: OneErrorCase,
    pub(crate) hybrid: HybridCase,
    pub(crate) transposition: TranspositionCase,
    pub(crate) negatives: NegativeCase,
    pub(crate) recall: RecallCase,
    pub(crate) rejoin: RejoinCase,
    pub(crate) metrics: Metrics,
    pub(crate) hard: HardOutcomes,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MiniBatteryReport {
    pub(crate) legal: LegalCase,
    pub(crate) hard: HardOutcomes,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegalCase {
    pub(crate) checked: u32,
    pub(crate) zero_cost: u32,
    pub(crate) unknown_strictly_worse: u32,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OneErrorCase {
    pub(crate) substitutions: Rate,
    pub(crate) extras: Rate,
    pub(crate) missing: Rate,
    pub(crate) margins: Vec<u32>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HybridCase {
    pub(crate) checked: u32,
    pub(crate) zero_cost: u32,
    pub(crate) best_costs: Vec<u32>,
    pub(crate) x_wins: u32,
    pub(crate) y_wins: u32,
    pub(crate) other_wins: u32,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranspositionCase {
    pub(crate) checked: u32,
    pub(crate) equal_zero_cost: u32,
    pub(crate) records: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NegativeCase {
    pub(crate) checked: u32,
    pub(crate) unknown_wins: u32,
    pub(crate) zero_cost_collisions: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecallCase {
    pub(crate) lock_one_substitution: Rate,
    pub(crate) lock_two_substitution: Rate,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RejoinCase {
    pub(crate) checked: u32,
    pub(crate) passed: u32,
    pub(crate) best_costs: Vec<u32>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Rate {
    pub(crate) passed: u32,
    pub(crate) total: u32,
}

impl Rate {
    pub(crate) fn record(&mut self, passed: bool) {
        self.total = self.total.saturating_add(1);
        self.passed = self.passed.saturating_add(u32::from(passed));
    }

    fn percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            100.0 * f64::from(self.passed) / f64::from(self.total)
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Metrics {
    pub(crate) alignment_micros_total: u64,
    pub(crate) alignment_micros_histogram: BTreeMap<String, u32>,
    pub(crate) active_product_states_max: u32,
    pub(crate) seeds_total: u64,
    pub(crate) truncations: u32,
    pub(crate) legal_truncations: u32,
    pub(crate) reseed_truncations: u32,
    pub(crate) evicted_zero_cost: u32,
    pub(crate) legal_evicted_zero_cost: u32,
    pub(crate) zero_cost_excess: u32,
    pub(crate) per_lock_evidence: Vec<LockEvidence>,
    pub(crate) slow_alignments: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LockEvidence {
    pub(crate) record: String,
    pub(crate) locks: Vec<LockAlignment>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WalkLength {
    pub(crate) record: String,
    pub(crate) mirrored: bool,
    pub(crate) original_locks: u32,
    pub(crate) capped_locks: u32,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HardOutcomes {
    pub(crate) legal_walks: bool,
    pub(crate) substitution_retention: bool,
    pub(crate) missing_retention: bool,
    pub(crate) hybrid_nonzero: bool,
    pub(crate) transposition_zero_cost: bool,
    pub(crate) negatives_unknown: bool,
    pub(crate) rejoin_nonzero: bool,
    pub(crate) legal_exact_without_zero_cost_eviction: bool,
}

pub(crate) fn write_report(report: &BatteryReport, output: &Path) -> io::Result<()> {
    std::fs::create_dir_all(output)?;
    let json = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    std::fs::write(output.join("battery.json"), json)?;
    std::fs::write(output.join("summary.md"), markdown(report))
}

fn markdown(report: &BatteryReport) -> String {
    format!(
        "# Opener Recognition Behavioral Battery\n\n\
## Hard Assertions\n\n\
- legal walks: {}\n\
- substitution retention: {}\n\
- missing retention: {}\n\
- hybrid nonzero: {}\n\
- transposition zero-cost: {}\n\
- negatives unknown: {}\n\
- legal exact without zero-cost eviction: {}\n\n\
## Cases\n\n\
- Legal: {}/{} zero cost; unknown strictly worse {}/{}\n\
- One-error substitution: {}/{} ({:.2}%); extra top-8: {}/{} ({:.2}%); missing: {}/{} ({:.2}%)\n\
- Hybrid: {} checked, {} zero-cost\n\
- Transposition: {}/{} equal zero-cost\n\
- Negatives: {}/{} unknown wins\n\
- Rejoin: {}/{} walked records rejoined at nonzero cost\n\
- Recall lock 1: {}/{} ({:.2}%); lock 2: {}/{} ({:.2}%)\n\n\
## Reported\n\n\
- Required records present: {}; exclusions: {}\n\n\
## Metrics\n\n\
 - Edit costs: synchronous {}; substitution {}; cell {}; observed-only {}; model-only {}; opaque-entry {}; unknown {};\n\
 - Alignments: {} microseconds total; max active product states {}; seeds {}; truncations {} (legal {}); reseed truncations {}; zero-cost evictions {} (legal {}); zero-cost excess {}\n\
- Required records: {}\n\
- Selected records: {}\n\
- Skipped: {}; cap-skipped: {}\n\
- Synthesis subgraph too large: {}\n\
- Synthesis fuel exhausted: {}\n\
- Synthesis visits: {} total; {} max per record; fuel limit {}\n\
- Synthesis walk lengths: {}\n\
- Required record exclusions: {}\n\
- Slow alignments: {}\n",
        report.hard.legal_walks,
        report.hard.substitution_retention,
        report.hard.missing_retention,
        report.hard.hybrid_nonzero,
        report.hard.transposition_zero_cost,
        report.hard.negatives_unknown,
        report.hard.legal_exact_without_zero_cost_eviction,
        report.legal.zero_cost,
        report.legal.checked,
        report.legal.unknown_strictly_worse,
        report.legal.checked,
        report.one_error.substitutions.passed,
        report.one_error.substitutions.total,
        report.one_error.substitutions.percent(),
        report.one_error.extras.passed,
        report.one_error.extras.total,
        report.one_error.extras.percent(),
        report.one_error.missing.passed,
        report.one_error.missing.total,
        report.one_error.missing.percent(),
        report.hybrid.checked,
        report.hybrid.zero_cost,
        report.transposition.equal_zero_cost,
        report.transposition.checked,
        report.negatives.unknown_wins,
        report.negatives.checked,
        report.rejoin.passed,
        report.rejoin.checked,
        report.recall.lock_one_substitution.passed,
        report.recall.lock_one_substitution.total,
        report.recall.lock_one_substitution.percent(),
        report.recall.lock_two_substitution.passed,
        report.recall.lock_two_substitution.total,
        report.recall.lock_two_substitution.percent(),
        report.required_records_present,
        report.required_record_exclusions.join("; "),
        report.edit_costs.synchronous,
        report.edit_costs.substitute_letter,
        report.edit_costs.cell_mismatch,
        report.edit_costs.observed_only,
        report.edit_costs.model_only,
        report.edit_costs.identity_opaque_entry,
        report.edit_costs.unknown_per_lock,
        report.metrics.alignment_micros_total,
        report.metrics.active_product_states_max,
        report.metrics.seeds_total,
        report.metrics.truncations,
        report.metrics.legal_truncations,
        report.metrics.reseed_truncations,
        report.metrics.evicted_zero_cost,
        report.metrics.legal_evicted_zero_cost,
        report.metrics.zero_cost_excess,
        report.required_records.join(", "),
        report.selected_records.join(", "),
        report.skipped.join("; "),
        report.cap_skipped,
        report.synthesis_subgraph_too_large.join("; "),
        report.synthesis_fuel_exhausted.join("; "),
        report.synthesis_visits_total,
        report.synthesis_visits_max,
        report.synthesis_fuel_limit,
        report
            .synthesis_walk_lengths
            .iter()
            .map(|length| {
                format!(
                    "{} mirrored={}: {}/{}",
                    length.record, length.mirrored, length.capped_locks, length.original_locks
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
        report.required_record_exclusions.join("; "),
        report.metrics.slow_alignments.join("; "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_serializes_and_renders_synthesis_skips_and_fuel_stats() {
        let report = BatteryReport {
            edit_costs: EditCosts::default(),
            required_records: Vec::new(),
            required_records_present: false,
            selected_records: Vec::new(),
            skipped: Vec::new(),
            cap_skipped: 0,
            synthesis_subgraph_too_large: vec!["too-large".to_owned()],
            synthesis_fuel_exhausted: vec!["fuel-exhausted".to_owned()],
            synthesis_visits_total: 64,
            synthesis_visits_max: 50,
            synthesis_fuel_limit: 50_000,
            synthesis_walk_lengths: vec![WalkLength {
                record: "capped".to_owned(),
                mirrored: false,
                original_locks: 20,
                capped_locks: 16,
            }],
            required_record_exclusions: vec!["required-excluded".to_owned()],
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

        let json = serde_json::to_string(&report).unwrap();
        let rendered = markdown(&report);

        assert!(json.contains("synthesisSubgraphTooLarge"));
        assert!(json.contains("editCosts"));
        assert!(json.contains("rejoin"));
        assert!(json.contains("synthesisFuelExhausted"));
        assert!(json.contains("synthesisVisitsTotal"));
        assert!(json.contains("synthesisWalkLengths"));
        assert!(json.contains("requiredRecordExclusions"));
        assert!(rendered.contains("Synthesis subgraph too large: too-large"));
        assert!(rendered.contains("Synthesis fuel exhausted: fuel-exhausted"));
        assert!(
            rendered.contains("Synthesis visits: 64 total; 50 max per record; fuel limit 50000")
        );
        assert!(rendered.contains("Synthesis walk lengths: capped mirrored=false: 16/20"));
        assert!(rendered.contains("Required record exclusions: required-excluded"));
    }
}
