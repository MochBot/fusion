use std::sync::Arc;

use super::cache::{uncached_shortlist_graph, union_graphs, RecordGraphCache};
use super::compile::{compile_borrowed_records, compile_recognition_subgraph, CompileBudget};
use super::graph::{RecognitionGraph, TransitionLabel};
#[cfg(not(target_arch = "wasm32"))]
use super::profile_tests::assert_graph_eq;
use crate::openers::catalog::OpenerCatalog;

fn mini_catalog() -> OpenerCatalog {
    serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse")
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

type StateFingerprint = (Vec<u8>, bool, Vec<(usize, u8)>);

/// Structural fingerprint that is independent of how the graph was assembled.
fn fingerprint(graph: &RecognitionGraph) -> Vec<StateFingerprint> {
    let mut states = graph
        .states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let mut origins = state
                .origins
                .iter()
                .map(|origin| format!("{origin:?}").into_bytes())
                .collect::<Vec<_>>();
            origins.sort();
            let mut out = graph.out[index]
                .iter()
                .map(|transition| {
                    let label = match transition.label {
                        TransitionLabel::Lock { .. } => 0,
                        TransitionLabel::Epsilon { .. } => 1,
                        TransitionLabel::Bridge { .. } => 2,
                    };
                    (transition.to.0, label)
                })
                .collect::<Vec<_>>();
            out.sort_unstable();
            (origins.concat(), state.identity_opaque, out)
        })
        .collect::<Vec<_>>();
    states.sort();
    states
}

fn single(catalog: &OpenerCatalog, id: &str) -> Arc<RecognitionGraph> {
    Arc::new(
        compile_recognition_subgraph(
            &OpenerCatalog {
                format_version: catalog.format_version,
                openers: catalog
                    .openers
                    .iter()
                    .filter(|record| record.id == id)
                    .cloned()
                    .collect(),
            },
            &CompileBudget::default(),
        )
        .expect("record compiles")
        .graph,
    )
}

#[test]
fn indexed_lookup_keeps_first_duplicate_and_missing_ids() {
    let base = mini_catalog();
    let first = base
        .openers
        .iter()
        .find(|record| record.id == "crowbar-v2")
        .expect("fixture record")
        .clone();
    let mut empty_first = first.clone();
    empty_first.shape_key = "stub-first".to_owned();
    empty_first.tree.clear();
    let catalog = OpenerCatalog {
        format_version: base.format_version,
        openers: vec![empty_first, first],
    };
    let cache = RecordGraphCache::for_catalog(catalog.clone());

    let graph = cache
        .shortlist_graph(&ids(&["crowbar-v2"]))
        .expect("the first duplicate is an eligible empty graph");
    assert!(graph.graph.states.is_empty());
    assert_eq!(graph.compile_skipped, 0);
    assert!(cache.shortlist_graph(&ids(&["not-in-catalog"])).is_none());
}

#[test]
fn cached_union_matches_a_direct_merged_compile() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::for_catalog(catalog.clone());
    let shortlist = ids(&["crowbar-v2", "perfect-clear-opener"]);

    let cached = cache
        .shortlist_graph(&shortlist)
        .expect("shortlist compiles");
    let direct = uncached_shortlist_graph(&catalog, &shortlist).expect("direct merge compiles");

    assert_eq!(cached.graph.states.len(), direct.graph.states.len());
    assert_eq!(
        cached.graph.exact_index.len(),
        direct.graph.exact_index.len()
    );
    assert_eq!(fingerprint(&cached.graph), fingerprint(&direct.graph));
    assert_eq!(cache.len(), 2);
}

#[test]
fn repeated_shortlists_reuse_compiled_records() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::for_catalog(catalog.clone());

    cache
        .shortlist_graph(&ids(&["crowbar-v2"]))
        .expect("compiles");
    cache
        .shortlist_graph(&ids(&["crowbar-v2", "perfect-clear-opener"]))
        .expect("compiles");
    cache
        .shortlist_graph(&ids(&["perfect-clear-opener", "crowbar-v2"]))
        .expect("compiles");

    assert_eq!(cache.len(), 2, "each record compiles once per snapshot");
}

#[test]
fn skipped_and_unknown_records_are_counted_like_the_uncached_path() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::for_catalog(catalog.clone());
    let shortlist = ids(&["crowbar-v2", "not-in-catalog", "lightningspin"]);

    let cached = cache.shortlist_graph(&shortlist).expect("compiles");
    let uncached = uncached_shortlist_graph(&catalog, &shortlist).expect("compiles");

    assert_eq!(cached.compile_skipped, uncached.compile_skipped);
    assert_eq!(cached.graph.states.len(), uncached.graph.states.len());
    assert!(cache.shortlist_graph(&ids(&["not-in-catalog"])).is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn borrowed_records_preserve_subset_order_repeats_and_budget_errors() {
    let catalog = mini_catalog();
    let order = [
        "perfect-clear-opener",
        "stub-crowbar-v2",
        "crowbar-v2",
        "lightningspin",
        "perfect-clear-opener",
    ];
    let records = order
        .iter()
        .map(|id| {
            catalog
                .openers
                .iter()
                .find(|record| record.id == *id)
                .expect("fixture record")
        })
        .collect::<Vec<_>>();
    let cloned = OpenerCatalog {
        format_version: catalog.format_version,
        openers: records.iter().map(|record| (*record).clone()).collect(),
    };
    for budget in [
        CompileBudget::default(),
        CompileBudget {
            max_total_states: 16,
            ..CompileBudget::default()
        },
    ] {
        let borrowed = compile_borrowed_records(records.iter().copied(), &budget);
        let reference = compile_recognition_subgraph(&cloned, &budget);
        match (borrowed, reference) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(actual.budget_exceeded, expected.budget_exceeded);
                assert_graph_eq(&actual.graph, &expected.graph);
            }
            (Err(actual), Err(expected)) => assert_eq!(actual.to_string(), expected.to_string()),
            _ => panic!("borrowed and cloned compilation must have the same outcome"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cached_repeated_and_empty_records_match_uncached_traversal() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::for_catalog(catalog.clone());
    let shortlist = ids(&[
        "perfect-clear-opener",
        "stub-crowbar-v2",
        "crowbar-v2",
        "lightningspin",
        "perfect-clear-opener",
    ]);
    let cached = cache.shortlist_graph(&shortlist).expect("cached graph");
    let direct = uncached_shortlist_graph(&catalog, &shortlist).expect("reference graph");
    assert_eq!(cached.compile_skipped, direct.compile_skipped);
    assert_graph_eq(&cached.graph, &direct.graph);
}

#[test]
fn union_over_the_summed_budget_defers_to_the_compiler() {
    let catalog = mini_catalog();
    let parts = [
        single(&catalog, "crowbar-v2"),
        single(&catalog, "perfect-clear-opener"),
    ];
    let total = parts.iter().map(|graph| graph.states.len()).sum::<usize>();

    assert!(union_graphs(&parts, u32::try_from(total).expect("fits")).is_some());
    assert!(
        union_graphs(&parts, u32::try_from(total - 1).expect("fits")).is_none(),
        "a summed total past the limit must not be decided by the union"
    );
}
