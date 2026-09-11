use super::super::super::cache::{uncached_shortlist_graph_with_budget, RecordGraphCache};
use super::super::super::compile::CompileBudget;
use super::super::super::graph::RecognitionGraph;
use super::super::super::profile::CacheRecordOutcome;
use super::super::{assert_graph_eq, ids, mini_catalog};
use crate::openers::catalog::OpenerCatalog;

fn epsilon_catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{"formatVersion":2,"openers":[{"id":"eps-a","aliases":{"en":"Eps A"},"shapeKey":"eps-a","tree":[{"id":0,"parent":null,"pieces":0,"rows":[],"placements":[]}]},{"id":"eps-b","aliases":{"en":"Eps B"},"shapeKey":"eps-b","tree":[{"id":0,"parent":null,"pieces":0,"rows":[],"placements":[]}]}]}"#,
    )
    .expect("epsilon catalog should parse")
}

fn total_states_budget(max_total_states: u32) -> CompileBudget {
    CompileBudget {
        max_total_states,
        ..CompileBudget::default()
    }
}

fn transition_count(graph: &RecognitionGraph) -> usize {
    graph.out.iter().map(Vec::len).sum()
}

#[test]
fn union_failure_falls_back_to_merged_compile() {
    let catalog = epsilon_catalog();
    let budget = total_states_budget(1);
    let shortlist = ids(&["eps-a", "eps-b"]);
    let cache = RecordGraphCache::default();

    let (graph, profile) =
        cache.shortlist_graph_profiled_with_budget(&catalog, &shortlist, &budget);

    let cached = graph.expect("epsilon merged fallback should succeed past the union limit");
    assert!(cached.graph.states.len() > budget.max_total_states as usize);
    assert_eq!(profile.records.len(), shortlist.len());
    assert!(profile
        .records
        .iter()
        .all(|record| record.outcome == CacheRecordOutcome::ColdSuccess));
    assert!(profile
        .records
        .iter()
        .all(|record| record.compile.is_some()));
    assert!(profile.union_graphs.is_some());
    assert!(profile.merged_compile.is_some());
    assert!(profile.graph_total.is_some());
    assert_eq!(profile.graph_states, Some(cached.graph.states.len()));
    assert_eq!(
        profile.graph_transitions,
        Some(transition_count(&cached.graph))
    );
    assert_eq!(cached.compile_skipped, 0);

    let direct = uncached_shortlist_graph_with_budget(&catalog, &shortlist, &budget)
        .expect("uncached merged compile at the same budget should succeed");
    assert_eq!(cached.compile_skipped, direct.compile_skipped);
    assert_graph_eq(&cached.graph, &direct.graph);
}

#[test]
fn warm_union_failure_reuses_records_and_reruns_merged_compile() {
    let catalog = epsilon_catalog();
    let budget = total_states_budget(1);
    let shortlist = ids(&["eps-a", "eps-b"]);
    let cache = RecordGraphCache::default();

    let (cold, _) = cache.shortlist_graph_profiled_with_budget(&catalog, &shortlist, &budget);
    let cold = cold.expect("cold merged fallback should succeed");
    let (warm, warm_profile) =
        cache.shortlist_graph_profiled_with_budget(&catalog, &shortlist, &budget);

    let warm = warm.expect("warm merged fallback should succeed");
    assert!(warm_profile
        .records
        .iter()
        .all(|record| record.outcome == CacheRecordOutcome::CachedSuccess));
    assert!(warm_profile
        .records
        .iter()
        .all(|record| record.compile.is_none()));
    assert!(warm_profile.union_graphs.is_some());
    assert!(warm_profile.merged_compile.is_some());
    assert!(warm_profile.graph_total.is_some());
    assert_eq!(warm_profile.graph_states, Some(warm.graph.states.len()));
    assert_graph_eq(&cold.graph, &warm.graph);
}

#[test]
fn union_failure_with_rejected_merged_compile_reports_spans_without_dimensions() {
    let catalog = mini_catalog();
    let budget = total_states_budget(16);
    let shortlist = ids(&["crowbar-v2"]);
    let cache = RecordGraphCache::default();

    let (graph, profile) =
        cache.shortlist_graph_profiled_with_budget(&catalog, &shortlist, &budget);

    assert!(graph.is_none());
    assert_eq!(profile.records.len(), 1);
    assert_eq!(profile.records[0].outcome, CacheRecordOutcome::ColdSuccess);
    assert!(profile.records[0].compile.is_some());
    assert!(profile.union_graphs.is_some());
    assert!(profile.merged_compile.is_some());
    assert!(profile.graph_total.is_some());
    assert_eq!(profile.graph_states, None);
    assert_eq!(profile.graph_transitions, None);
    assert!(uncached_shortlist_graph_with_budget(&catalog, &shortlist, &budget).is_none());

    let (warm_graph, warm_profile) =
        cache.shortlist_graph_profiled_with_budget(&catalog, &shortlist, &budget);
    assert!(warm_graph.is_none());
    assert_eq!(
        warm_profile.records[0].outcome,
        CacheRecordOutcome::CachedSuccess
    );
    assert_eq!(warm_profile.records[0].compile, None);
    assert!(warm_profile.merged_compile.is_some());
    assert!(warm_profile.graph_total.is_some());
    assert_eq!(warm_profile.graph_states, None);
    assert_eq!(warm_profile.graph_transitions, None);
}
