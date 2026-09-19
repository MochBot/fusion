use std::fmt;

#[cfg(test)]
use super::graph::StateId;
use super::graph::{BridgeReason, GraphBuilder};
use super::legality::LegalityVerdict;
use super::record::compile_record;
use super::RecognitionGraph;
#[cfg(test)]
use crate::openers::catalog::OpenerCatalog;
use crate::openers::catalog::{OpenerRecord, OpenerTreeNode};

/// One measurement point in the recognition compile, delivered through
/// [`CompileMetrics::record`]. Only test observers read the payload; the engine
/// build constructs and discards it, hence the dead-code allowance there.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) enum CompileMetricEvent<'a> {
    #[cfg(test)]
    BudgetExceeded {
        record: &'a OpenerRecord,
        node: &'a OpenerTreeNode,
        mirrored: bool,
        reason: &'a str,
        bridge_exposed: bool,
    },
    BlockedDescendant {
        record: &'a OpenerRecord,
        node: &'a OpenerTreeNode,
        mirrored: bool,
    },
    DirectImpossible {
        record: &'a OpenerRecord,
        node: &'a OpenerTreeNode,
        mirrored: bool,
        reason: &'a str,
        bridge_exposed: bool,
    },
    LegalityAttempt {
        support_valid: bool,
        verdict: &'a LegalityVerdict,
    },
    LetteredEdge {
        bridge_exposed: bool,
    },
    EdgeStateCount(u32),
    DfsVisits(u32),
    LegalOrders(u64),
    SupportObserved,
    SupportWithoutExactSrs,
    SrsValid {
        record: &'a OpenerRecord,
        node: &'a OpenerTreeNode,
        mirrored: bool,
    },
    ShiftedCompiled {
        record: &'a OpenerRecord,
        node: &'a OpenerTreeNode,
        mirrored: bool,
    },
    DfsCompiledLarge {
        record: &'a OpenerRecord,
        node: &'a OpenerTreeNode,
        mirrored: bool,
    },
    EpsilonTransition,
    FrameInconsistent {
        record: &'a OpenerRecord,
        node: &'a OpenerTreeNode,
        mirrored: bool,
        reason: &'a str,
        bridge_exposed: bool,
    },
    BridgedEdge(BridgeReason),
    BridgeRescuedDescendants(u32),
}

/// The exclusive leaves of one record compile; only stage profiling clocks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CompileStage {
    Legality,
    Intern,
    Transition,
    Finish,
}

/// Observation seam over the shared compile control flow. Production runs
/// `ProductionMetrics`, which records nothing; the budget verdict is not here but
/// in `CompileBudgetState`.
pub(super) trait CompileMetrics {
    fn record(&mut self, _event: CompileMetricEvent<'_>) {}

    fn wants_legal_orders(&self) -> bool {
        false
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn wants_stage_profile(&self) -> bool {
        false
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn record_stage(&mut self, _stage: CompileStage, _elapsed: std::time::Duration) {}
}

#[derive(Default)]
pub(super) struct CompileBudgetState {
    exceeded: bool,
}

impl CompileBudgetState {
    pub(super) fn mark_exceeded(&mut self) {
        self.exceeded = true;
    }

    pub(super) fn was_exceeded(&self) -> bool {
        self.exceeded
    }
}

struct ProductionMetrics;

impl CompileMetrics for ProductionMetrics {}

pub(super) fn mark_budget_exceeded<O: CompileMetrics>(
    budget_state: &mut CompileBudgetState,
    _metrics: &mut O,
    _record: &OpenerRecord,
    _node: &OpenerTreeNode,
    _mirrored: bool,
    _reason: &str,
    _bridge_exposed: bool,
) {
    budget_state.mark_exceeded();
    #[cfg(test)]
    _metrics.record(CompileMetricEvent::BudgetExceeded {
        record: _record,
        node: _node,
        mirrored: _mirrored,
        reason: _reason,
        bridge_exposed: _bridge_exposed,
    });
}

/// Stage clocks stay behind `wants_stage_profile`: production never opts in, so
/// outside native-test profiling this is a plain call of `run`.
pub(super) fn timed_stage<O: CompileMetrics, T>(
    _observer: &mut O,
    _stage: CompileStage,
    run: impl FnOnce() -> T,
) -> T {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    if _observer.wants_stage_profile() {
        let started = std::time::Instant::now();
        let value = run();
        _observer.record_stage(_stage, started.elapsed());
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
    EpsilonSubgraphCyclic { at: StateId },
    TotalStateBudgetExceeded {
        states: u32,
        /// Whether an edge budget was already exceeded and bridged before this failure.
        #[cfg(test)]
        edge_budget_exceeded: bool,
    },
}

impl CompileError {
    /// Whether an edge budget was exceeded before this failure.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn edge_budget_exceeded(&self) -> bool {
        match self {
            Self::TotalStateBudgetExceeded {
                edge_budget_exceeded,
                ..
            } => *edge_budget_exceeded,
            #[cfg(test)]
            _ => false,
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(test)]
            Self::EpsilonSubgraphCyclic { at } => {
                write!(formatter, "epsilon subgraph cyclic at state {}", at.0)
            }
            Self::TotalStateBudgetExceeded { states, .. } => {
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

/// Production entry over records the caller already resolved, in compile
/// order; avoids building a temporary single-record catalog.
pub(crate) fn compile_borrowed_records<'a>(
    records: impl IntoIterator<Item = &'a OpenerRecord>,
    budget: &CompileBudget,
) -> Result<CompileOutcome, CompileError> {
    compile_borrowed_records_with_metrics(records, budget, &mut ProductionMetrics)
}

/// The shared record walk: one compile index per candidate position, enumerated
/// before the stub/empty-tree skip, so a skipped entry still consumes its
/// ordinal and the same records compile as a subset at their local positions.
fn compile_records<'a, O: CompileMetrics>(
    records: impl IntoIterator<Item = &'a OpenerRecord>,
    budget: &CompileBudget,
    metrics: &mut O,
    budget_state: &mut CompileBudgetState,
    builder: &mut GraphBuilder,
) -> Result<(), CompileError> {
    for (record_index, record) in records.into_iter().enumerate() {
        if record.shape_key.starts_with("stub-") || record.tree.is_empty() {
            continue;
        }
        for mirrored in [false, true] {
            compile_record(
                builder,
                metrics,
                budget_state,
                record,
                u32::try_from(record_index).unwrap_or(u32::MAX),
                mirrored,
                budget,
            )?;
        }
    }
    Ok(())
}

/// Catalog-shaped seam kept for the test profilers; it enumerates the catalog's
/// own records in catalog order.
#[cfg(test)]
pub(super) fn compile_recognition_subgraph_with_metrics<O: CompileMetrics>(
    catalog: &OpenerCatalog,
    budget: &CompileBudget,
    metrics: &mut O,
) -> Result<CompileOutcome, CompileError> {
    compile_borrowed_records_with_metrics(catalog.openers.iter(), budget, metrics)
}

fn compile_borrowed_records_with_metrics<'a, O: CompileMetrics>(
    records: impl IntoIterator<Item = &'a OpenerRecord>,
    budget: &CompileBudget,
    metrics: &mut O,
) -> Result<CompileOutcome, CompileError> {
    let mut builder = GraphBuilder::new();
    let mut budget_state = CompileBudgetState::default();
    compile_records(records, budget, metrics, &mut budget_state, &mut builder)?;
    let graph = timed_stage(metrics, CompileStage::Finish, || builder.finish());
    Ok(CompileOutcome {
        graph,
        budget_exceeded: budget_state.was_exceeded(),
    })
}

#[cfg(test)]
pub(crate) fn compile_recognition_subgraph(
    catalog: &OpenerCatalog,
    budget: &CompileBudget,
) -> Result<CompileOutcome, CompileError> {
    let mut metrics = ProductionMetrics;
    compile_recognition_subgraph_with_metrics(catalog, budget, &mut metrics)
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
    let mut budget_state = CompileBudgetState::default();
    compile_records(
        catalog.openers.iter(),
        budget,
        &mut census,
        &mut budget_state,
        &mut builder,
    )?;
    finalize_census(&mut census, &builder, started);
    if !census.epsilon_acyclic {
        return Err(CompileError::EpsilonSubgraphCyclic { at: StateId(0) });
    }
    Ok(builder.finish_with_census(census))
}
