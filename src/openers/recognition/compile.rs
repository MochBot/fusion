use std::fmt;

#[cfg(test)]
use super::graph::StateId;
use super::graph::{BridgeReason, GraphBuilder};
use super::legality::LegalityVerdict;
use super::record::compile_record;
use super::RecognitionGraph;
use crate::openers::catalog::{OpenerCatalog, OpenerRecord, OpenerTreeNode};

pub(super) trait CompileObserver {
    fn budget_exceeded(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
        _reason: &str,
        _bridge_exposed: bool,
    ) {
    }

    fn blocked_descendant(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
    ) {
    }

    fn direct_impossible(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
        _reason: &str,
        _bridge_exposed: bool,
    ) {
    }

    fn legality_attempt(&mut self, _support_valid: bool, _verdict: &LegalityVerdict) {}

    fn lettered_edge(&mut self, _bridge_exposed: bool) {}

    fn edge_state_count(&mut self, _count: u32) {}

    fn dfs_visits(&mut self, _visits: u32) {}

    fn legal_orders(&mut self, _count: u64) {}

    fn wants_legal_orders(&self) -> bool {
        false
    }

    fn did_exceed_budget(&self) -> bool {
        false
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn wants_stage_profile(&self) -> bool {
        false
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn record_legality(&mut self, _elapsed: std::time::Duration) {}

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn record_intern(&mut self, _elapsed: std::time::Duration) {}

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn record_transition(&mut self, _elapsed: std::time::Duration) {}

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn record_finish(&mut self, _elapsed: std::time::Duration) {}

    fn support_observed(&mut self) {}

    fn support_without_exact_srs(&mut self) {}

    fn srs_valid(&mut self, _record: &OpenerRecord, _node: &OpenerTreeNode, _mirrored: bool) {}

    fn shifted_compiled(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
    ) {
    }

    fn dfs_compiled_large(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
    ) {
    }

    fn epsilon_transition(&mut self) {}

    fn frame_inconsistent(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
        _reason: &str,
        _bridge_exposed: bool,
    ) {
    }

    fn bridged_edge(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
        _reason: BridgeReason,
    ) {
    }

    fn bridge_rescued_descendants(&mut self, _state_count: u32) {}
}

struct ProductionObserver {
    budget_exceeded: bool,
}

impl CompileObserver for ProductionObserver {
    fn budget_exceeded(
        &mut self,
        _record: &OpenerRecord,
        _node: &OpenerTreeNode,
        _mirrored: bool,
        _reason: &str,
        _bridge_exposed: bool,
    ) {
        self.budget_exceeded = true;
    }

    fn did_exceed_budget(&self) -> bool {
        self.budget_exceeded
    }
}

/// Native-test-only stage clocks stay behind `wants_stage_profile`: the
/// production observer never opts in, so these helpers compile to a direct
/// call outside `cfg(all(test, not(target_arch = "wasm32")))` profiling.
pub(super) fn timed_legality<O: CompileObserver, T>(
    _observer: &mut O,
    run: impl FnOnce() -> T,
) -> T {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    if _observer.wants_stage_profile() {
        let started = std::time::Instant::now();
        let value = run();
        _observer.record_legality(started.elapsed());
        return value;
    }
    run()
}

pub(super) fn timed_intern<O: CompileObserver, T>(_observer: &mut O, run: impl FnOnce() -> T) -> T {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    if _observer.wants_stage_profile() {
        let started = std::time::Instant::now();
        let value = run();
        _observer.record_intern(started.elapsed());
        return value;
    }
    run()
}

pub(super) fn timed_transition<O: CompileObserver, T>(
    _observer: &mut O,
    run: impl FnOnce() -> T,
) -> T {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    if _observer.wants_stage_profile() {
        let started = std::time::Instant::now();
        let value = run();
        _observer.record_transition(started.elapsed());
        return value;
    }
    run()
}

pub(crate) struct CompileOutcome {
    pub(crate) graph: RecognitionGraph,
    pub(crate) budget_exceeded: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CompileBudget {
    pub(crate) max_states_per_edge: u32,
    pub(crate) max_placements_per_edge: u8,
    pub(crate) max_dfs_visits_per_edge: u32,
    pub(crate) max_total_states: u32,
}

impl Default for CompileBudget {
    fn default() -> Self {
        Self {
            // 2026-08-30 probe: 1.38M/4M total states; two canonical k=28 edges remain excluded.
            max_states_per_edge: 16_384,
            max_placements_per_edge: 32,
            max_dfs_visits_per_edge: 20_000,
            max_total_states: 4_000_000,
        }
    }
}

#[derive(Debug)]
pub(crate) enum CompileError {
    #[cfg(test)]
    EpsilonSubgraphCyclic {
        at: StateId,
    },
    TotalStateBudgetExceeded {
        states: u32,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(test)]
            Self::EpsilonSubgraphCyclic { at } => {
                write!(formatter, "epsilon subgraph cyclic at state {}", at.0)
            }
            Self::TotalStateBudgetExceeded { states } => {
                write!(
                    formatter,
                    "recognition graph exceeded total-state budget at {states}"
                )
            }
        }
    }
}

impl std::error::Error for CompileError {}

#[derive(Clone)]
pub(super) struct PlacementSpec {
    pub(super) letter: u8,
    pub(super) cells: [[u8; 2]; 4],
}

#[cfg(test)]
pub(crate) fn compile_recognition_graph(
    catalog: &OpenerCatalog,
    budget: &CompileBudget,
) -> Result<RecognitionGraph, CompileError> {
    compile_recognition_subgraph(catalog, budget).map(|outcome| outcome.graph)
}

pub(crate) fn compile_recognition_subgraph(
    catalog: &OpenerCatalog,
    budget: &CompileBudget,
) -> Result<CompileOutcome, CompileError> {
    let mut observer = ProductionObserver {
        budget_exceeded: false,
    };
    compile_recognition_subgraph_with_observer(catalog, budget, &mut observer)
}

pub(super) fn compile_recognition_subgraph_with_observer<O: CompileObserver>(
    catalog: &OpenerCatalog,
    budget: &CompileBudget,
    observer: &mut O,
) -> Result<CompileOutcome, CompileError> {
    let mut builder = GraphBuilder::new();

    for (record_index, record) in catalog.openers.iter().enumerate() {
        if record.shape_key.starts_with("stub-") || record.tree.is_empty() {
            continue;
        }
        for mirrored in [false, true] {
            compile_record(
                &mut builder,
                observer,
                record,
                u32::try_from(record_index).unwrap_or(u32::MAX),
                mirrored,
                budget,
            )?;
        }
    }
    #[cfg(all(test, not(target_arch = "wasm32")))]
    let finish_wanted = observer.wants_stage_profile();
    #[cfg(all(test, not(target_arch = "wasm32")))]
    let finish_started = finish_wanted.then(std::time::Instant::now);
    let graph = builder.finish();
    #[cfg(all(test, not(target_arch = "wasm32")))]
    if let Some(started) = finish_started {
        observer.record_finish(started.elapsed());
    }
    let budget_exceeded = observer.did_exceed_budget();
    Ok(CompileOutcome {
        graph,
        budget_exceeded,
    })
}

#[cfg(test)]
pub(super) fn compile_catalog_census(
    catalog: &OpenerCatalog,
    budget: &CompileBudget,
) -> Result<RecognitionGraph, CompileError> {
    use std::time::Instant;

    use super::census::CompileCensus;
    use super::metrics::finalize_census;

    let started = Instant::now();
    let mut builder = GraphBuilder::new();
    let mut census = CompileCensus::new(
        budget.max_states_per_edge,
        budget.max_placements_per_edge,
        budget.max_dfs_visits_per_edge,
        budget.max_total_states,
    );
    for (record_index, record) in catalog.openers.iter().enumerate() {
        if record.shape_key.starts_with("stub-") || record.tree.is_empty() {
            continue;
        }
        for mirrored in [false, true] {
            compile_record(
                &mut builder,
                &mut census,
                record,
                u32::try_from(record_index).unwrap_or(u32::MAX),
                mirrored,
                budget,
            )?;
        }
    }
    finalize_census(&mut census, &builder, started);
    if !census.epsilon_acyclic {
        return Err(CompileError::EpsilonSubgraphCyclic { at: StateId(0) });
    }
    Ok(builder.finish_with_census(census))
}
