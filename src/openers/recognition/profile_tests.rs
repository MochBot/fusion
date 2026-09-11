#[path = "profile_tests_cache_behavior.rs"]
mod cache_behavior;
#[path = "profile_tests_early_return.rs"]
mod early_return;
#[path = "profile_tests_parity.rs"]
mod parity;

use super::compile::{compile_recognition_graph, CompileBudget};
use super::graph::{RecognitionGraph, StateId, TransitionLabel};
use crate::openers::catalog::{OpenerCatalog, OpenerRecord};
use crate::openers::catalogued_match::{MatchingOpener, RoundCataloguedBoardMatch};
use crate::openers::matcher::BoardMatch;
use crate::openers::phase::{OpenerAssessment, OpenerObservation};

fn mini_catalog() -> OpenerCatalog {
    serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse")
}

fn fixture_catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{"formatVersion":2,"openers":[{"id":"fixture","aliases":{"en":"Fixture"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":0,"rows":[],"placements":[]}]}]}"#,
    )
    .expect("fixture catalog should parse")
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn assessment(opener_id: &str, runners_up: &[&str]) -> OpenerAssessment {
    OpenerAssessment {
        on_script: true,
        board_cells: 4,
        r#match: Some(board_match(opener_id)),
        runners_up: runners_up
            .iter()
            .map(|opener_id| board_match(opener_id))
            .collect(),
    }
}

fn board_match(opener_id: &str) -> BoardMatch {
    BoardMatch {
        opener_id: opener_id.to_owned(),
        label: opener_id.to_owned(),
        mirrored: false,
        colored: false,
        overlap_cells: 4,
        target_cells: 4,
        stray_cells: 0,
        progress: 1.0,
        complete: true,
        node_id: Some(1),
        route_name: None,
    }
}

fn observation() -> OpenerObservation {
    OpenerObservation {
        post_board: Some(Vec::new()),
        post_gmask: Some(Vec::new()),
        post_letters: None,
    }
}

fn singleton_match(opener_id: &str) -> RoundCataloguedBoardMatch {
    RoundCataloguedBoardMatch {
        first_match_index: 0,
        anchor_index: 1,
        matching_openers: vec![MatchingOpener {
            id: opener_id.to_owned(),
            name: opener_id.to_owned(),
            deepest_pieces: 1,
            candidate_node_ids: Vec::new(),
            route_name: None,
            mirrored: None,
        }],
    }
}

fn legal_observations(record: &OpenerRecord) -> Vec<Option<OpenerObservation>> {
    let graph = compile_recognition_graph(
        &OpenerCatalog {
            format_version: 2,
            openers: vec![record.clone()],
        },
        &CompileBudget::default(),
    )
    .expect("fixture record should compile");
    legal_observations_from_graph(&graph, &record.id)
}

fn legal_observations_from_graph(
    graph: &RecognitionGraph,
    record_id: &str,
) -> Vec<Option<OpenerObservation>> {
    let mut state = root_state(graph, record_id).expect("fixture record should have a root");
    let mut observations = Vec::new();

    while observations.len() < 14 {
        let Some(next) = graph.out[state.0].iter().find_map(|transition| {
            matches!(transition.label, TransitionLabel::Lock { .. })
                .then_some(transition.to)
                .filter(|candidate| matches_record(graph, *candidate, record_id))
        }) else {
            break;
        };
        let key = &graph.states[next.0].physical_key;
        observations.push(Some(OpenerObservation {
            post_board: Some(key.masks.to_vec()),
            post_gmask: Some(vec![0; key.masks.len()]),
            post_letters: None,
        }));
        state = next;
    }

    observations
}

fn root_state(graph: &RecognitionGraph, record_id: &str) -> Option<StateId> {
    graph
        .states
        .iter()
        .enumerate()
        .find(|(_, state)| {
            state.origins.iter().any(|origin| {
                origin.record.as_ref() == record_id
                    && origin.placed_subset == 0
                    && !origin.bag_complete
            })
        })
        .map(|(index, _)| StateId(index))
}

fn matches_record(graph: &RecognitionGraph, state: StateId, record_id: &str) -> bool {
    graph.states[state.0]
        .origins
        .iter()
        .any(|origin| origin.record.as_ref() == record_id)
}

/// Asserts two compiled graphs are structurally identical in traversal order:
/// every state's physical key, origins, and opacity, every transition with
/// its full label payload, the exact-index contents, and the epsilon order.
/// Census metrics are intentionally excluded: the same graph compiled under
/// different observers must compare equal here.
pub(super) fn assert_graph_eq(left: &RecognitionGraph, right: &RecognitionGraph) {
    assert_eq!(
        left.states.len(),
        right.states.len(),
        "state count must match"
    );
    for (index, (left_state, right_state)) in
        left.states.iter().zip(right.states.iter()).enumerate()
    {
        assert_eq!(
            left_state.physical_key, right_state.physical_key,
            "physical key at state {index}"
        );
        assert_eq!(
            left_state.origins, right_state.origins,
            "origins at state {index}"
        );
        assert_eq!(
            left_state.identity_opaque, right_state.identity_opaque,
            "opacity at state {index}"
        );
    }
    assert_eq!(
        left.out.len(),
        right.out.len(),
        "adjacency length must match"
    );
    for (index, (left_edges, right_edges)) in left.out.iter().zip(right.out.iter()).enumerate() {
        assert_eq!(
            left_edges.len(),
            right_edges.len(),
            "transition count at state {index}"
        );
        for (left_transition, right_transition) in left_edges.iter().zip(right_edges.iter()) {
            assert_eq!(
                format!("{:?}", left_transition),
                format!("{:?}", right_transition),
                "transition payload at state {index}"
            );
        }
    }
    assert_eq!(
        left.exact_index.len(),
        right.exact_index.len(),
        "exact-index bucket count must match"
    );
    for (key, left_ids) in &left.exact_index {
        let right_ids = right
            .exact_index
            .get(key)
            .expect("exact-index bucket key must match");
        assert_eq!(left_ids, right_ids, "exact-index bucket contents");
    }
    assert_eq!(
        left.epsilon_order, right.epsilon_order,
        "epsilon order must match"
    );
}
