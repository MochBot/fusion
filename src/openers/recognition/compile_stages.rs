//! Native-test-only single-record compile stage diagnostics.
//!
//! `compile_single_record_stages` recompiles one catalog record through the
//! shared `compile_recognition_subgraph_with_observer` control flow with a
//! profiling observer, so the measured work is the actual compiler, not a copy.
//!
//! Overlap model: `legality`, `intern`, `transition`, and `finish` are
//! non-overlapping exclusive leaves. Each wraps exactly one GraphBuilder
//! method call (`placement_legality`, `intern` / `intern_with_physical_key`,
//! `add_transition`, `finish`); call-site argument preparation (control and
//! origin keys, board and physical-key clones, transition labels) runs
//! outside the spans. `total_wall` is the inclusive wall time of the whole
//! single-record compile. `other_residual` is the residual of that same
//! compile, `total_wall` minus the leaf sum, covering frame transforms,
//! board locking, DFS traversal and bookkeeping, plus the profiling clocks'
//! own overhead. It is a residual, never a directly timed span, and must not
//! be read as exclusive time for any single operation.

use std::time::Duration;

use super::compile::{compile_recognition_subgraph_with_observer, CompileBudget, CompileObserver};
use crate::openers::catalog::OpenerCatalog;

#[derive(Clone, Debug, Default)]
pub(crate) struct StageTotals {
    pub legality: Duration,
    pub intern: Duration,
    pub transition: Duration,
    pub finish: Duration,
    pub legality_calls: u64,
    pub intern_calls: u64,
    pub transition_calls: u64,
}

impl StageTotals {
    pub(crate) fn leaves_sum(&self) -> Duration {
        self.legality + self.intern + self.transition + self.finish
    }
}

pub(crate) struct StageProfilingObserver {
    pub budget_exceeded: bool,
    pub stages: StageTotals,
}

impl StageProfilingObserver {
    pub(crate) fn new() -> Self {
        Self {
            budget_exceeded: false,
            stages: StageTotals::default(),
        }
    }
}

impl CompileObserver for StageProfilingObserver {
    fn budget_exceeded(
        &mut self,
        _record: &crate::openers::catalog::OpenerRecord,
        _node: &crate::openers::catalog::OpenerTreeNode,
        _mirrored: bool,
        _reason: &str,
        _bridge_exposed: bool,
    ) {
        self.budget_exceeded = true;
    }

    fn did_exceed_budget(&self) -> bool {
        self.budget_exceeded
    }

    fn wants_stage_profile(&self) -> bool {
        true
    }

    fn record_legality(&mut self, elapsed: Duration) {
        self.stages.legality += elapsed;
        self.stages.legality_calls = self.stages.legality_calls.saturating_add(1);
    }

    fn record_intern(&mut self, elapsed: Duration) {
        self.stages.intern += elapsed;
        self.stages.intern_calls = self.stages.intern_calls.saturating_add(1);
    }

    fn record_transition(&mut self, elapsed: Duration) {
        self.stages.transition += elapsed;
        self.stages.transition_calls = self.stages.transition_calls.saturating_add(1);
    }

    fn record_finish(&mut self, elapsed: Duration) {
        self.stages.finish += elapsed;
    }
}

// `wants_legal_orders` is deliberately not overridden: the default `false`
// matches production, so legal-order DP stays off and completed states,
// budgets, and legal moves are unaffected by profiling.

#[derive(Clone, Debug)]
pub(crate) struct SingleRecordStageReport {
    pub record_id: String,
    pub success: bool,
    pub budget_exceeded: bool,
    pub error: Option<String>,
    pub graph_states: usize,
    pub graph_transitions: usize,
    pub exact_buckets: usize,
    pub total_wall: Duration,
    pub stages: StageTotals,
    pub other_residual: Duration,
}

/// Recompiles one record with stage profiling. Returns `None` only when the
/// record id is absent from the catalog; compile failures still report the
/// stages observed before the failure.
pub(crate) fn compile_single_record_stages(
    catalog: &OpenerCatalog,
    record_id: &str,
) -> Option<SingleRecordStageReport> {
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == record_id)?
        .clone();
    let single = OpenerCatalog {
        format_version: catalog.format_version,
        openers: vec![record],
    };
    let budget = CompileBudget::default();
    let mut observer = StageProfilingObserver::new();
    let total_started = std::time::Instant::now();
    let outcome = compile_recognition_subgraph_with_observer(&single, &budget, &mut observer);
    let total_wall = total_started.elapsed();
    let leaves = observer.stages.leaves_sum();
    let other_residual = total_wall.checked_sub(leaves).expect(
        "stage leaves are disjoint sub-spans of the same compile and must not exceed its wall time",
    );
    match outcome {
        Ok(outcome) => Some(SingleRecordStageReport {
            record_id: record_id.to_owned(),
            success: !outcome.budget_exceeded,
            budget_exceeded: outcome.budget_exceeded,
            error: None,
            graph_states: outcome.graph.states.len(),
            graph_transitions: outcome.graph.out.iter().map(Vec::len).sum(),
            exact_buckets: outcome.graph.exact_index.len(),
            total_wall,
            stages: observer.stages,
            other_residual,
        }),
        Err(error) => Some(SingleRecordStageReport {
            record_id: record_id.to_owned(),
            success: false,
            budget_exceeded: observer.budget_exceeded,
            error: Some(error.to_string()),
            graph_states: 0,
            graph_transitions: 0,
            exact_buckets: 0,
            total_wall,
            stages: observer.stages,
            other_residual,
        }),
    }
}
