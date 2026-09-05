use super::{align_round_exact, AlignBudget, AlignmentResult, Observation};
use crate::openers::catalog::OpenerCatalog;
use crate::openers::recognition::compile_catalog_census;
use crate::openers::recognition::cost::EditCosts;
use crate::openers::recognition::graph::{RecognitionGraph, StateId};
use crate::openers::recognition::retrieval::SeedBudget;

mod opaque;

#[test]
fn legal_trace_has_zero_cost_identity_and_a_strictly_worse_unknown() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &legal_trace(&graph),
        &AlignBudget::default(),
    );

    let best = result.hypotheses.first().unwrap();
    let unknown = result
        .hypotheses
        .iter()
        .find(|hypothesis| hypothesis.origin.is_none())
        .unwrap();

    assert_eq!(best.origin.as_ref().unwrap().record.as_ref(), "two-os");
    assert_eq!(best.total_cost, 0);
    assert!(unknown.total_cost > best.total_cost);
    assert_eq!(result.best_margin, Some(12));
}

#[test]
fn one_removed_cell_costs_one_and_retains_the_record_with_one_cell_mismatch() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let mut observations = trace_for_nodes(&graph, &[1, 2]);
    observations[1].as_mut().unwrap().key.masks[0] &= !1;

    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &observations,
        &AlignBudget::default(),
    );
    let best = known(&result);

    assert_eq!(best.total_cost, 1);
    assert_eq!(best.ops.cell_mismatches, 1);
}

#[test]
fn extra_uncatalogued_board_is_an_observed_only_edit_of_cost_five() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let observations = [
        observation_for_node(&graph, 1),
        Some(alien_observation()),
        observation_for_node(&graph, 2),
    ];

    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &observations,
        &AlignBudget::default(),
    );
    let best = known(&result);

    assert_eq!(best.total_cost, 5);
    assert_eq!(best.ops.observed_only, 1);
}

#[test]
fn dropped_lock_uses_one_model_only_edit_of_cost_four() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let observations = [
        observation_for_node(&graph, 1),
        observation_for_node(&graph, 3),
    ];

    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &observations,
        &AlignBudget::default(),
    );
    let best = known(&result);

    assert_eq!(best.total_cost, 4);
    assert_eq!(best.ops.model_only, 1);
}

#[test]
fn non_catalog_stack_trace_leaves_unknown_as_the_only_hypothesis() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let observations = [
        Some(alien_observation()),
        Some(alien_observation()),
        Some(alien_observation()),
    ];

    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &observations,
        &AlignBudget::default(),
    );

    assert_eq!(result.hypotheses.len(), 1);
    assert!(result.hypotheses[0].origin.is_none());
    assert_eq!(result.hypotheses[0].total_cost, 18);
}

#[test]
fn later_exact_state_rejoins_from_unknown_after_an_alien_board() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let observations = [
        observation_for_node(&graph, 1),
        Some(alien_observation()),
        observation_for_node(&graph, 6),
    ];

    let costs = EditCosts {
        observed_only: 20,
        ..EditCosts::default()
    };
    let result = align_round_exact(&graph, &costs, &observations, &AlignBudget::default());
    let best = known(&result);

    assert_eq!(
        best.state.map(|state| &graph.states[state.0].physical_key),
        Some(&graph.states[state_for_node(&graph, 6).0].physical_key)
    );
    assert_eq!(best.total_cost, 18);
}

#[test]
fn missing_evidence_allows_a_free_single_model_advance() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let observations = [
        observation_for_node(&graph, 1),
        None,
        observation_for_node(&graph, 3),
    ];

    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &observations,
        &AlignBudget::default(),
    );
    let best = known(&result);

    assert_eq!(best.total_cost, 0);
    assert_eq!(best.ops.model_only, 0);
    assert_eq!(result.per_lock[1].unknown_cost, 6);
}

#[test]
fn zero_cost_excess_is_reported_without_eviction() {
    let graph = compile_catalog_census(&two_o_catalog()).unwrap();
    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &[observation_for_node(&graph, 1)],
        &AlignBudget {
            max_active_product_states: 1,
            ..AlignBudget::default()
        },
    );

    assert!(!result.truncated_any);
    assert!(!result.per_lock[0].truncated);
    assert_eq!(result.per_lock[0].active_product_states, 4);
    assert_eq!(result.per_lock[0].evicted_zero_cost, 0);
    assert_eq!(result.per_lock[0].zero_cost_excess, 3);
}

#[test]
fn active_limit_retains_every_zero_cost_collision_state() {
    let graph = compile_catalog_census(&collision_catalog(2)).unwrap();
    let observation = Some(Observation {
        key: graph.states[0].physical_key.clone(),
        had_garbage: false,
    });
    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &[observation],
        &AlignBudget {
            max_active_product_states: 1,
            ..AlignBudget::default()
        },
    );

    assert_eq!(result.per_lock[0].active_product_states, 4);
    assert_eq!(result.per_lock[0].best_cost, Some(0));
    assert_eq!(result.per_lock[0].evicted_zero_cost, 0);
    assert_eq!(result.per_lock[0].zero_cost_excess, 3);
}

#[test]
fn reseed_budget_is_reported_separately_for_collision_buckets() {
    let graph = compile_catalog_census(&collision_catalog(33)).unwrap();
    let observation = Some(Observation {
        key: graph.states[0].physical_key.clone(),
        had_garbage: false,
    });
    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &[observation.clone(), observation],
        &AlignBudget {
            seed: SeedBudget { max_seeds: 64 },
            max_active_product_states: 128,
        },
    );

    assert_eq!(result.per_lock[1].seeded, 66);
    assert!(result.per_lock[1].reseed_truncated);
}

#[test]
fn exact_seed_ties_keep_a_nonfirst_collision_identity_at_zero_cost() {
    let graph = compile_catalog_census(&collision_catalog(2)).unwrap();
    let observation = Some(Observation {
        key: graph.states[0].physical_key.clone(),
        had_garbage: false,
    });
    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &[observation],
        &AlignBudget {
            seed: SeedBudget { max_seeds: 1 },
            max_active_product_states: 16,
        },
    );

    assert!(result.hypotheses.iter().any(|hypothesis| {
        hypothesis.total_cost == 0
            && hypothesis
                .origin
                .as_ref()
                .is_some_and(|origin| origin.record.as_ref() == "collision-1")
    }));
}

fn legal_trace(graph: &RecognitionGraph) -> Vec<Option<Observation>> {
    trace_for_nodes(graph, &[1, 2])
}

fn trace_for_nodes(graph: &RecognitionGraph, node_ids: &[u32]) -> Vec<Option<Observation>> {
    node_ids
        .iter()
        .copied()
        .map(|node_id| observation_for_node(graph, node_id))
        .collect()
}

fn observation_for_node(graph: &RecognitionGraph, node_id: u32) -> Option<Observation> {
    let state = state_for_node(graph, node_id);
    Some(Observation {
        key: graph.states[state.0].physical_key.clone(),
        had_garbage: false,
    })
}

fn alien_observation() -> Observation {
    Observation {
        key: crate::openers::recognition::graph::CanonicalKey {
            masks: vec![0b1010101010, 0b0101010101, 0b1010101010].into(),
            letters: None,
        },
        had_garbage: false,
    }
}

fn known(result: &AlignmentResult) -> &super::FinalHypothesis {
    result
        .hypotheses
        .iter()
        .find(|hypothesis| hypothesis.origin.is_some())
        .unwrap()
}

fn state_for_node(graph: &RecognitionGraph, node_id: u32) -> StateId {
    graph
        .states
        .iter()
        .position(|state| {
            state.origins.iter().any(|origin| {
                origin.record.as_ref() == "two-os" && !origin.mirrored && origin.node_id == node_id
            })
        })
        .map(StateId)
        .unwrap()
}

fn two_o_catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{
            "formatVersion": 2,
            "openers": [{
                "id": "two-os",
                "aliases": {"en": "two os"},
                "shapeKey": "fixture",
                "tree": [
                    {"id": 0, "parent": null, "pieces": 0, "rows": [], "placements": []},
                    {"id": 1, "parent": 0, "pieces": 1, "rows": ["OO________", "OO________"], "placements": [{"letter": "O", "cells": [[0, 0], [1, 0], [0, 1], [1, 1]]}]},
                    {"id": 2, "parent": 1, "pieces": 2, "rows": ["OOOO______", "OOOO______"], "placements": [{"letter": "O", "cells": [[2, 0], [3, 0], [2, 1], [3, 1]]}]},
                    {"id": 3, "parent": 2, "pieces": 3, "rows": ["OOOOOO____", "OOOOOO____"], "placements": [{"letter": "O", "cells": [[4, 0], [5, 0], [4, 1], [5, 1]]}]},
                    {"id": 4, "parent": 3, "pieces": 4, "rows": ["OOOOOOOO__", "OOOOOOOO__"], "placements": [{"letter": "O", "cells": [[6, 0], [7, 0], [6, 1], [7, 1]]}]},
                    {"id": 5, "parent": 4, "pieces": 5, "rows": [], "preClearRows": ["XXXXXXXXXX", "XXXXXXXXXX"], "clearRows": [0, 1], "placements": [{"letter": "O", "cells": [[8, 0], [9, 0], [8, 1], [9, 1]]}]},
                    {"id": 6, "parent": 5, "pieces": 6, "rows": ["OO________", "OO________"], "placements": [{"letter": "O", "cells": [[0, 0], [1, 0], [0, 1], [1, 1]]}]}
                ]
            }]
        }"#,
    )
    .unwrap()
}

fn collision_catalog(records: usize) -> OpenerCatalog {
    let openers = (0..records)
        .map(|index| {
            format!(
                r#"{{"id":"collision-{index}","aliases":{{"en":"collision"}},"shapeKey":"fixture","tree":[{{"id":0,"parent":null,"pieces":0,"rows":["I_________"],"placements":[]}}]}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    serde_json::from_str(&format!(r#"{{"formatVersion":2,"openers":[{openers}]}}"#)).unwrap()
}
