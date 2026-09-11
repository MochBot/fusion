#[path = "profile_tests_cache_fallback.rs"]
mod fallback;

use super::super::cache::RecordGraphCache;
use super::super::profile::CacheRecordOutcome;
use super::{ids, mini_catalog};
use crate::openers::catalog::OpenerCatalog;

fn over_budget_catalog() -> OpenerCatalog {
    let mut placements = String::from("[");
    for index in 0..33 {
        if index > 0 {
            placements.push(',');
        }
        placements.push_str(r#"{"letter":"T","cells":[[0,0],[1,0],[2,0],[1,1]]}"#);
    }
    placements.push(']');
    let catalog = format!(
        r#"{{"formatVersion":2,"openers":[{{"id":"fixture","aliases":{{"en":"Fixture"}},"shapeKey":"fixture","tree":[{{"id":0,"parent":null,"pieces":0,"rows":[]}}]}},{{"id":"over-budget","aliases":{{"en":"Over Budget"}},"shapeKey":"over-budget-shape","tree":[{{"id":0,"parent":null,"pieces":0,"rows":[]}},{{"id":1,"parent":0,"pieces":1,"rows":["IIII______"],"placements":{placements}}}]}}]}}"#,
    );
    serde_json::from_str(&catalog).expect("over-budget catalog should parse")
}

#[test]
fn cache_first_use_cold_attempts_in_shortlist_order() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::default();

    let (graph, profile) =
        cache.shortlist_graph_profiled(&catalog, &ids(&["crowbar-v2", "perfect-clear-opener"]));

    assert!(graph.is_some());
    assert_eq!(profile.records.len(), 2);
    assert_eq!(profile.records[0].id, "crowbar-v2");
    assert_eq!(profile.records[1].id, "perfect-clear-opener");
    assert!(profile
        .records
        .iter()
        .all(|record| record.outcome == CacheRecordOutcome::ColdSuccess));
    assert!(profile
        .records
        .iter()
        .all(|record| record.compile.is_some()));
    assert!(profile.graph_total.is_some());
    assert!(profile.union_graphs.is_some());
}

#[test]
fn cache_repeated_shortlist_has_no_cold_attempts() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::default();
    let shortlist = ids(&["crowbar-v2", "perfect-clear-opener"]);

    cache
        .shortlist_graph(&catalog, &shortlist)
        .expect("shortlist compiles");
    let (graph, profile) = cache.shortlist_graph_profiled(&catalog, &shortlist);

    assert!(graph.is_some());
    assert!(profile
        .records
        .iter()
        .all(|record| record.outcome == CacheRecordOutcome::CachedSuccess));
    assert!(profile
        .records
        .iter()
        .all(|record| record.compile.is_none()));
    assert_eq!(cache.len(), 2);
}

#[test]
fn cache_overlapping_reordered_shortlist_compiles_only_new_records() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::default();

    let (_, first) = cache.shortlist_graph_profiled(&catalog, &ids(&["crowbar-v2"]));
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].outcome, CacheRecordOutcome::ColdSuccess);

    let (graph, second) =
        cache.shortlist_graph_profiled(&catalog, &ids(&["perfect-clear-opener", "crowbar-v2"]));

    assert!(graph.is_some());
    assert_eq!(second.records.len(), 2);
    assert_eq!(second.records[0].id, "perfect-clear-opener");
    assert_eq!(second.records[0].outcome, CacheRecordOutcome::ColdSuccess);
    assert_eq!(second.records[1].id, "crowbar-v2");
    assert_eq!(second.records[1].outcome, CacheRecordOutcome::CachedSuccess);
    assert_eq!(cache.len(), 2);
}

#[test]
fn cache_failed_record_is_memoized_not_retried() {
    let catalog = over_budget_catalog();
    let cache = RecordGraphCache::default();

    let (first_graph, first) = cache.shortlist_graph_profiled(&catalog, &ids(&["over-budget"]));
    assert!(first_graph.is_none());
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].outcome, CacheRecordOutcome::ColdFailure);
    assert!(first.records[0].compile.is_some());
    assert_eq!(cache.len(), 1);

    let (second_graph, second) = cache.shortlist_graph_profiled(&catalog, &ids(&["over-budget"]));
    assert!(second_graph.is_none());
    assert_eq!(second.records.len(), 1);
    assert_eq!(second.records[0].outcome, CacheRecordOutcome::CachedFailure);
    assert_eq!(second.records[0].compile, None);
    assert_eq!(cache.len(), 1);

    let mixed = ids(&["fixture", "over-budget"]);
    let normal = cache.shortlist_graph(&catalog, &mixed);
    let (profiled, mixed_profile) = cache.shortlist_graph_profiled(&catalog, &mixed);
    assert_eq!(normal.is_some(), profiled.is_some());
    assert_eq!(
        normal.map(|graph| graph.compile_skipped),
        profiled.map(|graph| graph.compile_skipped)
    );
    assert_eq!(
        mixed_profile
            .records
            .iter()
            .map(|record| record.outcome)
            .collect::<Vec<_>>(),
        [
            CacheRecordOutcome::CachedSuccess,
            CacheRecordOutcome::CachedFailure
        ]
    );
}

#[test]
fn cache_missing_ids_are_not_compile_skipped() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::default();
    let shortlist = ids(&["crowbar-v2", "not-in-catalog"]);

    let normal = cache.shortlist_graph(&catalog, &shortlist);
    let fresh = RecordGraphCache::default();
    let (profiled, profile) = fresh.shortlist_graph_profiled(&catalog, &shortlist);

    assert_eq!(
        normal.map(|graph| (graph.graph.states.len(), graph.compile_skipped)),
        profiled.map(|graph| (graph.graph.states.len(), graph.compile_skipped))
    );
    assert_eq!(profile.records.len(), 2);
    assert_eq!(
        profile.records[1].outcome,
        CacheRecordOutcome::MissingCatalogRecord
    );

    let (missing_graph, missing) =
        fresh.shortlist_graph_profiled(&catalog, &ids(&["not-in-catalog"]));
    assert!(missing_graph.is_none());
    assert_eq!(missing.records.len(), 1);
    assert_eq!(
        missing.records[0].outcome,
        CacheRecordOutcome::MissingCatalogRecord
    );
}

#[test]
fn cache_union_success_leaves_merged_unexecuted() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::default();

    let (graph, profile) =
        cache.shortlist_graph_profiled(&catalog, &ids(&["crowbar-v2", "perfect-clear-opener"]));

    assert!(graph.is_some());
    assert!(profile.union_graphs.is_some());
    assert_eq!(profile.merged_compile, None);
    assert!(profile.graph_states.is_some());
    assert!(profile.graph_transitions.is_some());
}
