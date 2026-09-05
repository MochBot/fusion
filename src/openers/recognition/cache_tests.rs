use std::sync::Arc;

use super::cache::{uncached_shortlist_graph, union_graphs, RecordGraphCache};
use super::compile::{compile_recognition_subgraph, CompileBudget};
use super::graph::{RecognitionGraph, TransitionLabel};
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
fn cached_union_matches_a_direct_merged_compile() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::default();
    let shortlist = ids(&["crowbar-v2", "perfect-clear-opener"]);

    let cached = cache
        .shortlist_graph(&catalog, &shortlist)
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
    let cache = RecordGraphCache::default();

    cache
        .shortlist_graph(&catalog, &ids(&["crowbar-v2"]))
        .expect("compiles");
    cache
        .shortlist_graph(&catalog, &ids(&["crowbar-v2", "perfect-clear-opener"]))
        .expect("compiles");
    cache
        .shortlist_graph(&catalog, &ids(&["perfect-clear-opener", "crowbar-v2"]))
        .expect("compiles");

    assert_eq!(cache.len(), 2, "each record compiles once per snapshot");
}

#[test]
fn skipped_and_unknown_records_are_counted_like_the_uncached_path() {
    let catalog = mini_catalog();
    let cache = RecordGraphCache::default();
    let shortlist = ids(&["crowbar-v2", "not-in-catalog", "lightningspin"]);

    let cached = cache
        .shortlist_graph(&catalog, &shortlist)
        .expect("compiles");
    let uncached = uncached_shortlist_graph(&catalog, &shortlist).expect("compiles");

    assert_eq!(cached.compile_skipped, uncached.compile_skipped);
    assert_eq!(cached.graph.states.len(), uncached.graph.states.len());
    assert!(cache
        .shortlist_graph(&catalog, &ids(&["not-in-catalog"]))
        .is_none());
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
