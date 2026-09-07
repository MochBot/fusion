use super::align::{align_round_exact, AlignBudget, Observation};
use super::census::{CompileCensus, EdgeRef};
use super::compile::{CompileBudget, PlacementSpec};
use super::cost::EditCosts;
use super::evidence::write_census;
use super::frames::DeclaredFrame;
use super::graph::{CanonicalKey, StateId, TransitionLabel};
use super::legality::{
    engine_board_from_masks, placement_cells, placement_is_srs_legal, LegalityVerdict,
};
use super::{compile_catalog_census, compile_recognition_graph};
use crate::header::{Move, Piece, Rotation};
use crate::move_buffer::MoveBuffer;
use crate::openers::board::rows_to_masks_floor_up;
use crate::openers::catalog::OpenerCatalog;
use crate::pathfinder::{get_input, Input};
use crate::smear_core::generate_placements;

const REQUIRED_CENSUS_METRICS: &[&str] = &[
    "stateTotal",
    "letteredStateTotal",
    "greyStateTotal",
    "lockTransitions",
    "epsilonTransitions",
    "branchingHistogram",
    "legalOrderCount",
    "legalityAttempts",
    "supportValidAttempts",
    "srsLegalAttempts",
    "srsRejectedAttempts",
    "unreachableFromSpawnAttempts",
    "unsupportedAttempts",
    "notAPlacementAttempts",
    "endpointRejections",
    "supportObservedEdges",
    "srsValidEdges",
    "supportObservedWithoutExactSrsEdges",
    "directImpossibleEdges",
    "bridgeExposedImpossibleEdges",
    "blockedDescendants",
    "budgetExceededEdges",
    "bridgeExposedBudgetEdges",
    "shiftedCompiledEdges",
    "dfsCompiledLargeEdges",
    "frameInconsistentEdges",
    "bridgeExposedFrameInconsistentEdges",
    "bridgedEdges",
    "bridgeRescuedDescendants",
    "exactKeyBuckets",
    "epsilonClosureMax",
    "activeProductStateMax",
    "compileMicros",
    "prefixAlignmentMicros",
    "residentGraphBytes",
    "maxStatesPerEdge",
    "maxPlacementsPerEdge",
    "maxDfsVisitsPerEdge",
    "maxTotalStates",
    "dfsVisitHistogram",
    "baselineImpossibleNowCompiled",
    "baselineBudgetNowCompiled",
];

#[test]
fn g1_accepts_only_the_two_accepted_budget_edge_identities() {
    let mut census = g1_ready_census();
    census.budget_exceeded_edges = vec![
        budget_edge("sasasa123-634", 4, false),
        budget_edge("sasasa123-933", 2, true),
    ];

    assert!(census.g1_failures().is_empty());
}

#[test]
fn g1_reports_an_unaccepted_budget_edge_alongside_accepted_exclusions() {
    let mut census = g1_ready_census();
    census.budget_exceeded_edges = vec![
        budget_edge("sasasa123-634", 4, false),
        budget_edge("sasasa123-933", 2, true),
        budget_edge("other", 9, false),
    ];

    assert_eq!(
        census.g1_failures(),
        ["budget-exceeded edges: other:9 mirrored=false"]
    );
}

#[test]
fn g1_accepts_both_chiralities_of_an_accepted_budget_edge() {
    let mut census = g1_ready_census();
    census.budget_exceeded_edges = vec![
        budget_edge("sasasa123-634", 4, false),
        budget_edge("sasasa123-634", 4, true),
    ];

    assert!(census.g1_failures().is_empty());
}

#[test]
fn budget_exceeded_summary_names_all_accepted_exclusions_that_occur() {
    let mut census = g1_ready_census();
    census.budget_exceeded_edges = vec![
        budget_edge("sasasa123-634", 4, false),
        budget_edge("sasasa123-634", 4, true),
        budget_edge("sasasa123-933", 2, false),
        budget_edge("sasasa123-933", 2, true),
    ];

    assert_eq!(
        census.budget_exceeded_summary(),
        "4 (all user-accepted exclusions: sasasa123-634:4, sasasa123-933:2)"
    );
}

fn g1_ready_census() -> CompileCensus {
    let mut census = CompileCensus::new(16_384, 32, 20_000, 4_000_000);
    census.lettered_edges = 1;
    census.resident_graph_bytes = 1;
    census
}

fn budget_edge(record: &str, node_id: u32, mirrored: bool) -> EdgeRef {
    EdgeRef {
        record: record.to_owned(),
        node_id,
        mirrored,
        reason: "interned states exceed budget".to_owned(),
    }
}

struct CrowbarProbe {
    geometry_emitted: bool,
    label_free_reachable: bool,
    spin_labelled_reachable: bool,
    exact_endpoint_reachable: bool,
}

fn crowbar_node_one_probe(catalog: &OpenerCatalog) -> CrowbarProbe {
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == "crowbar-v2")
        .unwrap();
    let parent = record.tree.iter().find(|node| node.id == 0).unwrap();
    let node = record.tree.iter().find(|node| node.id == 1).unwrap();
    let board = engine_board_from_masks(&rows_to_masks_floor_up(&parent.rows));
    let mut geometry_emitted = false;
    let mut label_free_reachable = false;
    let mut spin_labelled_reachable = false;

    for placement in node.placements.as_ref().unwrap() {
        let letter = placement.letter.as_bytes()[0];
        let cells: [[u8; 2]; 4] = placement.cells.clone().try_into().unwrap();
        let legality = placement_is_srs_legal(&board, letter, &cells);
        geometry_emitted |= legality.geometry_emitted;
        label_free_reachable |= legality.label_free_reachable;
        spin_labelled_reachable |= legality.spin_labelled_reachable;
    }

    let expected = CanonicalKey::from_board(&engine_board_from_masks(&rows_to_masks_floor_up(
        &node.rows,
    )));
    let graph = compile_catalog_census(catalog).unwrap();
    let exact_endpoint_reachable = graph.states.iter().any(|state| {
        state.physical_key.masks == expected.masks
            && state.origins.iter().any(|origin| {
                origin.record.as_ref() == "crowbar-v2" && origin.node_id == 1 && origin.bag_complete
            })
    });
    CrowbarProbe {
        geometry_emitted,
        label_free_reachable,
        spin_labelled_reachable,
        exact_endpoint_reachable,
    }
}

#[test]
fn crowbar_node_one_reaches_its_declared_endpoint() {
    let catalog = serde_json::from_slice::<OpenerCatalog>(include_bytes!(
        "../../../fixtures/openers/catalog-mini.json"
    ))
    .unwrap();

    let probe = crowbar_node_one_probe(&catalog);

    assert!(probe.geometry_emitted);
    let _reachability_instrumentation = (probe.label_free_reachable, probe.spin_labelled_reachable);
    assert!(probe.exact_endpoint_reachable);
}

#[test]
fn grey_node_becomes_an_opaque_terminal_without_a_direct_failure() {
    let catalog = catalog_from_json(
        r#"{
            "formatVersion":2,
            "openers":[{
                "id":"grey-terminal","aliases":{"en":"grey terminal"},"shapeKey":"fixture",
                "tree":[{"id":0,"parent":null,"pieces":0,"rows":["XXXXXXXXXX"],"grey":true}]
            }]
        }"#,
    );

    let graph = compile_catalog_census(&catalog).unwrap();

    assert_eq!(graph.census.grey_state_total, 2);
    assert!(graph.census.direct_impossible_edges.is_empty());
    assert!(graph
        .states
        .iter()
        .filter(|state| state.identity_opaque)
        .all(|state| {
            state.physical_key.masks.is_empty() && graph.out[state_index(&graph, state)].is_empty()
        }));
}

#[test]
fn child_of_a_frame_inconsistency_is_bridged_without_a_direct_impossible() {
    let catalog = catalog_from_json(
        r#"{
            "formatVersion":2,
            "openers":[{
                "id":"blocked-descendant","aliases":{"en":"blocked descendant"},"shapeKey":"fixture",
                "tree":[
                    {"id":0,"parent":null,"pieces":1,"rows":[],"placements":[{"letter":"O","cells":[[0,0],[2,0],[0,1],[2,1]]}]},
                    {"id":1,"parent":0,"pieces":2,"rows":["OO________","OO________"],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]}
                ]
            }]
        }"#,
    );

    let graph = compile_catalog_census(&catalog).unwrap();

    assert!(graph.census.direct_impossible_edges.is_empty());
    assert_eq!(graph.census.frame_inconsistent_edges.len(), 2);
    assert!(graph.census.blocked_descendants.is_empty());
    assert_eq!(graph.census.bridged_edges.frame_inconsistent, 2);
}

#[test]
fn unbuildable_edge_bridges_to_a_child_and_compiles_its_grandchild() {
    let catalog = catalog_from_json(
        r#"{
            "formatVersion":2,
            "openers":[{
                "id":"bridge-fixture","aliases":{"en":"Bridge fixture"},"shapeKey":"fixture",
                "tree":[
                    {"id":0,"parent":null,"pieces":1,"rows":["OO________","OO________"],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]},
                    {"id":1,"parent":0,"pieces":2,"rows":["____OO____","____OO____","__________","OO________","OO________"],"placements":[{"letter":"O","cells":[[4,3],[5,3],[4,4],[5,4]]}]},
                    {"id":2,"parent":1,"pieces":3,"rows":["____OO____","____OO____","__________","OOOO______","OOOO______"],"placements":[{"letter":"O","cells":[[2,0],[3,0],[2,1],[3,1]]}]},
                    {"id":3,"parent":2,"pieces":4,"rows":["_______OO_","_______OO_","__________","__________","____OO____","____OO____","__________","OOOO______","OOOO______"],"placements":[{"letter":"O","cells":[[7,7],[8,7],[7,8],[8,8]]}]},
                    {"id":4,"parent":3,"pieces":5,"rows":["_______OO_","_______OO_","__________","__________","____OO____","____OO____","__________","OOOOOO____","OOOOOO____"],"placements":[{"letter":"O","cells":[[4,0],[5,0],[4,1],[5,1]]}]},
                    {"id":5,"parent":4,"pieces":6,"rows":["OO________","OO________","__________","__________","_______OO_","_______OO_","__________","__________","____OO____","____OO____","__________","OOOOOO____","OOOOOO____"],"placements":[{"letter":"O","cells":[[0,11],[1,11],[0,12],[1,12]]}]},
                    {"id":6,"parent":5,"pieces":7,"rows":["OO________","OO________","__________","__________","_______OO_","_______OO_","__________","__________","____OO____","____OO____","__________","OOOOOOOO__","OOOOOOOO__"],"placements":[{"letter":"O","cells":[[6,0],[7,0],[6,1],[7,1]]}]}
                ]
            }]
        }"#,
    );

    let graph = compile_catalog_census(&catalog).unwrap();
    let state_for = |node_id| {
        graph
            .states
            .iter()
            .position(|state| {
                state.origins.iter().any(|origin| {
                    origin.record.as_ref() == "bridge-fixture"
                        && !origin.mirrored
                        && origin.node_id == node_id
                        && origin.bag_complete
                })
            })
            .map(StateId)
            .expect("fixture node should compile")
    };

    for node_id in [1, 2, 3, 4, 5, 6] {
        assert!(
            graph.states.iter().any(|state| {
                state.origins.iter().any(|origin| {
                    origin.record.as_ref() == "bridge-fixture"
                        && !origin.mirrored
                        && origin.node_id == node_id
                })
            }),
            "bridge-fixture node {node_id} should compile"
        );
    }
    let root = state_for(0);
    let bridge = state_for(1);
    let grandchild = state_for(2);
    let exposed_parent = state_for(4);
    let exposed_impossible = state_for(5);
    assert!(!graph.states[bridge.0].identity_opaque);
    assert!(graph.out[root.0].iter().any(|transition| {
        transition.to == bridge && matches!(transition.label, TransitionLabel::Bridge { .. })
    }));
    let observations = [root, bridge, grandchild]
        .into_iter()
        .map(|state| {
            Some(Observation {
                key: graph.states[state.0].physical_key.clone(),
                had_garbage: false,
            })
        })
        .collect::<Vec<_>>();
    let alignment = align_round_exact(
        &graph,
        &EditCosts::default(),
        &observations,
        &AlignBudget::default(),
    );
    let hypothesis = alignment
        .hypotheses
        .iter()
        .find(|hypothesis| {
            hypothesis
                .origin
                .as_ref()
                .is_some_and(|origin| origin.record.as_ref() == "bridge-fixture")
        })
        .expect("bridge alignment should retain the record");
    assert_eq!(
        hypothesis.total_cost,
        EditCosts::default().identity_opaque_entry
    );
    assert!(!hypothesis.identity_frozen);
    assert!(graph.out[exposed_parent.0].iter().any(|transition| {
        transition.to == exposed_impossible
            && matches!(transition.label, TransitionLabel::Bridge { .. })
    }));
    assert_eq!(graph.census.bridged_edges.direct_impossible, 6);
    assert_eq!(graph.census.bridge_rescued_descendants, 6);
    assert_eq!(graph.census.direct_impossible_edges.len(), 2);
    assert_eq!(graph.census.bridge_exposed_impossible_edges.len(), 4);
    assert_eq!(graph.census.lettered_edges, 4);
    assert!(graph
        .census
        .bridge_exposed_impossible_edges
        .iter()
        .any(|edge| { edge.record == "bridge-fixture" && !edge.mirrored && edge.node_id == 5 }));
    assert!(graph.census.epsilon_acyclic);
}

#[test]
#[ignore]
fn number_one_subgraph_compiles_states_past_node_five() {
    let mut catalog = serde_json::from_slice::<OpenerCatalog>(include_bytes!(
        "../../../fixtures/openers/catalog-full.json"
    ))
    .unwrap();
    catalog.openers.retain(|record| record.id == "number-one");

    let graph = compile_recognition_graph(&catalog, &CompileBudget::default()).unwrap();
    assert!(graph.states.iter().any(|state| {
        state.origins.iter().any(|origin| {
            origin.record.as_ref() == "number-one" && !origin.mirrored && origin.node_id > 5
        })
    }));
}

#[test]
fn absent_declared_placement_cell_is_reported_as_frame_inconsistent() {
    let catalog = catalog_from_json(
        r#"{
            "formatVersion":2,
            "openers":[{
                "id":"endpoint","aliases":{"en":"endpoint"},"shapeKey":"fixture",
                "tree":[{"id":0,"parent":null,"pieces":1,"rows":[],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]}]
            }]
        }"#,
    );

    let graph = compile_catalog_census(&catalog).unwrap();

    assert_eq!(graph.census.endpoint_rejections, 0);
    assert!(graph.census.direct_impossible_edges.is_empty());
    assert_eq!(graph.census.frame_inconsistent_edges.len(), 2);
}

#[test]
fn declared_frame_shifts_cells_above_an_early_clear() {
    let physical = placement_is_srs_legal(
        &crate::board::Board::new(),
        b'O',
        &[[0, 0], [1, 0], [0, 1], [1, 1]],
    );
    assert!(matches!(physical.verdict, LegalityVerdict::Legal { .. }));
    let catalog = catalog_from_json(
        r#"{
            "formatVersion":2,
            "openers":[{
                "id":"declared-frame-shift","aliases":{"en":"declared frame shift"},"shapeKey":"fixture",
                "tree":[
                    {"id":0,"parent":null,"pieces":0,"rows":[],"preClearRows":["XXXXXXXXXX"],"clearRows":[0],"placements":[]},
                    {"id":1,"parent":0,"pieces":1,"rows":["OO________","OO________"],"preClearRows":["OO________","OO________","XXXXXXXXXX"],"clearRows":[0],"placements":[{"letter":"O","cells":[[0,1],[1,1],[0,2],[1,2]]}]}
                ]
            }]
        }"#,
    );

    let graph = compile_catalog_census(&catalog).unwrap();

    assert!(
        graph.census.srs_valid_edges >= 2,
        "srs-valid={} direct={:?} frame={:?}",
        graph.census.srs_valid_edges,
        graph.census.direct_impossible_edges,
        graph.census.frame_inconsistent_edges
    );
    assert!(graph.census.direct_impossible_edges.is_empty());
    assert!(graph.out.iter().flatten().any(|transition| {
        matches!(
            &transition.label,
            super::graph::TransitionLabel::Lock { cells, .. }
                if *cells == [[0, 0], [1, 0], [0, 1], [1, 1]]
        )
    }));
}

#[test]
fn declared_frame_compiles_real_deferred_clear_regressions() {
    for catalog in real_deferred_clear_catalogs() {
        let record = &catalog.openers[0];
        let parent = &record.tree[0];
        let child = &record.tree[1];
        let placements = placement_specs(child);
        let start = DeclaredFrame::start(child, Some(parent), &placements, false).unwrap();
        let mut builder = super::graph::GraphBuilder::new();
        let parent_state = builder.intern(
            start.physical_board(),
            super::record::control(0, false, parent.id, 0),
            super::record::origin(record, false, parent, 0, true),
            false,
        );
        let mut census = CompileCensus::new(512, 32, 20_000, 4_000_000);

        let completed = super::edge::compile_edge(
            &mut builder,
            &mut census,
            super::edge::EdgeInput {
                record,
                record_index: 0,
                node: child,
                parent: Some(parent),
                mirrored: false,
                parent_states: &[parent_state],
                placements: &placements,
                budget: &CompileBudget::default(),
                bridge_exposed: false,
            },
        )
        .unwrap();

        let expected = engine_board_from_masks(&rows_to_masks_floor_up(&child.rows));
        assert!(
            !completed.is_empty(),
            "{}:{} did not compile",
            record.id,
            child.id
        );
        assert!(completed
            .iter()
            .all(|state| builder.board(*state).rows == expected.rows));
    }
}

#[test]
fn inconsistent_declared_clear_rows_are_reported_without_a_direct_failure() {
    let catalog = catalog_from_json(
        r#"{
            "formatVersion":2,
            "openers":[{
                "id":"inconsistent-clear","aliases":{"en":"inconsistent clear"},"shapeKey":"fixture",
                "tree":[
                    {"id":0,"parent":null,"pieces":0,"rows":[],"preClearRows":["XXXXXXXXXX"],"clearRows":[0],"placements":[]},
                    {"id":1,"parent":0,"pieces":1,"rows":["OO________","OO________"],"preClearRows":["OO________","OO________","XXXXXXXXXX"],"clearRows":[],"placements":[{"letter":"O","cells":[[0,1],[1,1],[0,2],[1,2]]}]}
                ]
            }]
        }"#,
    );

    let graph = compile_catalog_census(&catalog).unwrap();

    assert!(graph.census.direct_impossible_edges.is_empty());
    assert!(graph.census.frame_inconsistent_edges.iter().any(|edge| {
        edge.record == "inconsistent-clear"
            && edge.node_id == 1
            && edge.reason == "declared full rows differ from child clearRows"
    }));
}

#[test]
fn budget_edges_are_evaluated_not_capped() {
    let (census, completed) = compile_juice_budget_edge(CompileBudget::default());
    let crevasse = crevasse_budget_edge_catalog();
    let (crevasse_census, crevasse_completed) =
        compile_catalog_edge(&crevasse, CompileBudget::default());

    // juice-st:4 and crevasse-stacking:9 are unbuildable as drawn: floating cells have no clear.
    assert!(!completed.is_empty());
    assert!(!crevasse_completed.is_empty());
    assert!(census.budget_exceeded_edges.is_empty());
    assert!(crevasse_census.budget_exceeded_edges.is_empty());
    assert!(census
        .direct_impossible_edges
        .iter()
        .any(|edge| { edge.record == "juice-st" && edge.reason == "no SRS-valid order" }));
    assert!(crevasse_census.direct_impossible_edges.iter().any(|edge| {
        edge.record == "crevasse-stacking"
            && edge.node_id == 9
            && edge.reason == "no SRS-valid order"
    }));
}

#[test]
fn dfs_visit_budget_is_recorded_for_a_legal_catalog_edge() {
    let budget = CompileBudget {
        max_dfs_visits_per_edge: 0,
        ..CompileBudget::default()
    };
    let catalog = real_deferred_clear_catalogs().remove(0);
    let (census, _) = compile_catalog_edge(&catalog, budget);

    assert!(census.budget_exceeded_edges.iter().any(|edge| {
        edge.record == "aitch-stacking" && edge.reason == "DFS visits exceed budget"
    }));
}

fn catalog_from_json(raw: &str) -> OpenerCatalog {
    serde_json::from_str(raw).unwrap()
}

fn state_index(graph: &super::graph::RecognitionGraph, target: &super::graph::ModelState) -> usize {
    graph
        .states
        .iter()
        .position(|state| std::ptr::eq(state, target))
        .unwrap()
}

#[test]
fn census_compilation_does_not_require_a_historical_report() {
    let catalog = catalog_from_json(r#"{"formatVersion":2,"openers":[]}"#);
    let graph = compile_catalog_census(&catalog).unwrap();
    assert!(graph.states.is_empty());
    assert!(graph.census.epsilon_acyclic);
}

#[test]
fn census_reports_every_required_metric() {
    let catalog = serde_json::from_slice::<OpenerCatalog>(include_bytes!(
        "../../../fixtures/openers/catalog-mini.json"
    ))
    .unwrap();

    let graph = compile_catalog_census(&catalog).unwrap();
    let census = serde_json::to_value(&graph.census).unwrap();

    for metric in REQUIRED_CENSUS_METRICS {
        assert!(
            census.get(metric).is_some(),
            "missing census metric {metric}"
        );
    }
    assert!(census["shiftedCompiledEdges"].is_array());
    assert!(census["dfsCompiledLargeEdges"].is_array());
    assert!(census["frameInconsistentEdges"].is_array());
    assert!(census["bridgedEdges"].is_object());
    assert!(census["bridgeRescuedDescendants"].is_u64());
    assert!(census["dfsVisitHistogram"].is_object());
    assert!(census["baselineImpossibleNowCompiled"].is_u64());
    assert!(census["baselineBudgetNowCompiled"].is_u64());
    assert!(census["baselineImpossibleNowCompiledDetails"].is_array());
    assert!(census["baselineBudgetNowCompiledDetails"].is_array());
    let (states, transitions, buckets, epsilon_order) = graph.graph_shape();
    assert!(states > 0);
    assert!(transitions > 0);
    assert!(buckets > 0);
    assert_eq!(epsilon_order, states);
    assert!(graph.census.reports_every_required_metric());
}

#[test]
fn census_summary_reports_bridge_exposed_failure_counts() {
    let mut census = CompileCensus::new(1, 1, 1, 1);
    let edge = EdgeRef {
        record: "summary-fixture".to_owned(),
        node_id: 1,
        mirrored: false,
        reason: "fixture".to_owned(),
    };
    census.bridge_exposed_impossible_edges = vec![edge.clone(); 2];
    census.bridge_exposed_budget_edges = vec![edge.clone(); 3];
    census.bridge_exposed_frame_inconsistent_edges = vec![edge; 4];
    let output = std::env::temp_dir().join(format!(
        "fusion-census-bridge-exposed-summary-{}",
        std::process::id()
    ));

    write_census(&census, &output).unwrap();
    let summary = std::fs::read_to_string(output.join("summary.md")).unwrap();
    std::fs::remove_dir_all(&output).unwrap();

    assert!(summary.contains("- Bridge-exposed impossible edges: 2"));
    assert!(summary.contains("- Bridge-exposed budget-exceeded edges: 3"));
    assert!(summary.contains("- Bridge-exposed frame-inconsistent edges: 4"));
}

fn placement_specs(node: &crate::openers::catalog::OpenerTreeNode) -> Vec<PlacementSpec> {
    node.placements
        .as_ref()
        .unwrap()
        .iter()
        .map(|placement| PlacementSpec {
            letter: placement.letter.as_bytes()[0],
            cells: placement.cells.clone().try_into().unwrap(),
        })
        .collect()
}

fn real_deferred_clear_catalogs() -> Vec<OpenerCatalog> {
    vec![
        catalog_from_json(
            r#"{"formatVersion":2,"openers":[{"id":"aitch-stacking","aliases":{"en":"Aitch Stacking"},"shapeKey":"fixture","tree":[{"id":45,"parent":null,"pieces":33,"rows":["JJ___JJJOO","J____SSJOO","LL_ZZTIIII"],"preClearRows":["JJ___JJJOO","J____SSJOO","JTZZSSSSLL","LTTZZSSOOL","LTZZTTTOOL","LL_ZZTIIII"],"clearRows":[1,2,3],"placements":[{"letter":"T","cells":[[1,1],[1,2],[2,2],[1,3]]},{"letter":"L","cells":[[9,1],[9,2],[8,3],[9,3]]},{"letter":"Z","cells":[[3,2],[4,2],[2,3],[3,3]]},{"letter":"S","cells":[[4,3],[5,3],[5,4],[6,4]]},{"letter":"J","cells":[[7,4],[5,5],[6,5],[7,5]]},{"letter":"O","cells":[[8,4],[9,4],[8,5],[9,5]]}]},{"id":51,"parent":45,"pieces":34,"rows":["______IIII","JJ___JJJOO","J____SSJOO","LL_ZZTIIII"],"preClearRows":["______IIII","JJ___JJJOO","J____SSJOO","JTZZSSSSLL","LTTZZSSOOL","LTZZTTTOOL","LL_ZZTIIII"],"clearRows":[1,2,3],"placements":[{"letter":"I","cells":[[6,6],[7,6],[8,6],[9,6]]}]}]}]}"#,
        ),
        catalog_from_json(
            r#"{"formatVersion":2,"openers":[{"id":"ajanba-tsd","aliases":{"en":"Ajanba TSD"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":7,"rows":["S_________","SS__ZZ_LLL"],"preClearRows":["S_________","SS__ZZ_LLL","JSTTTZZLOO","JJJTIIIIOO"],"clearRows":[0,1],"placements":[{"letter":"J","cells":[[0,0],[1,0],[2,0],[0,1]]},{"letter":"T","cells":[[3,0],[2,1],[3,1],[4,1]]},{"letter":"I","cells":[[4,0],[5,0],[6,0],[7,0]]},{"letter":"O","cells":[[8,0],[9,0],[8,1],[9,1]]},{"letter":"S","cells":[[1,1],[0,2],[1,2],[0,3]]},{"letter":"Z","cells":[[5,1],[6,1],[4,2],[5,2]]},{"letter":"L","cells":[[7,1],[7,2],[8,2],[9,2]]}]},{"id":58,"parent":0,"pieces":8,"rows":["S____TTT__","SS__ZZTLLL"],"preClearRows":["S____TTT__","SS__ZZTLLL","JSTTTZZLOO","JJJTIIIIOO"],"clearRows":[0,1],"placements":[{"letter":"T","cells":[[6,2],[5,3],[6,3],[7,3]]}]}]}]}"#,
        ),
        catalog_from_json(
            r#"{"formatVersion":2,"openers":[{"id":"appl-cannon","aliases":{"en":"Appl Cannon"},"shapeKey":"fixture","tree":[{"id":1,"parent":null,"pieces":13,"rows":["_Z__S_____","ZZ__SS____","ZJ___SOOOO","IJL_TSSLLL"],"preClearRows":["_Z__S_____","ZZ__SS____","ZJ___SOOOO","IJJJTTOOOO","IJJTTTTZZL","IJL_TSSLLL"],"clearRows":[1,2],"placements":[{"letter":"T","cells":[[4,0],[3,1],[4,1],[4,2]]},{"letter":"L","cells":[[7,0],[8,0],[9,0],[9,1]]},{"letter":"J","cells":[[1,2],[2,2],[3,2],[1,3]]},{"letter":"O","cells":[[8,2],[9,2],[8,3],[9,3]]},{"letter":"Z","cells":[[0,3],[0,4],[1,4],[1,5]]},{"letter":"S","cells":[[5,3],[4,4],[5,4],[4,5]]}]},{"id":3,"parent":1,"pieces":14,"rows":["I_________","I_________","I_________","IZ__S_____","ZZ__SS____","ZJ___SOOOO","IJL_TSSLLL"],"preClearRows":["I_________","I_________","I_________","IZ__S_____","ZZ__SS____","ZJ___SOOOO","IJJJTTOOOO","IJJTTTTZZL","IJL_TSSLLL"],"clearRows":[1,2],"placements":[{"letter":"I","cells":[[0,5],[0,6],[0,7],[0,8]]}]}]}]}"#,
        ),
    ]
}

fn compile_juice_budget_edge(budget: CompileBudget) -> (CompileCensus, Vec<super::graph::StateId>) {
    let catalog = catalog_from_json(
        r#"{"formatVersion":2,"openers":[{"id":"juice-st","aliases":{"en":"Juice ST"},"shapeKey":"fixture","tree":[{"id":3,"parent":null,"pieces":7,"rows":["_________Z","______OOZZ","______OOZJ","LLL_TTTSSJ"],"preClearRows":["_________Z","______OOZZ","______OOZJ","LLL_TTTSSJ","LIIIITSSJJ"],"clearRows":[0],"placements":[{"letter":"L","cells":[[0,0],[0,1],[1,1],[2,1]]},{"letter":"I","cells":[[1,0],[2,0],[3,0],[4,0]]},{"letter":"T","cells":[[5,0],[4,1],[5,1],[6,1]]},{"letter":"S","cells":[[6,0],[7,0],[7,1],[8,1]]},{"letter":"J","cells":[[8,0],[9,0],[9,1],[9,2]]},{"letter":"O","cells":[[6,2],[7,2],[6,3],[7,3]]},{"letter":"Z","cells":[[8,2],[8,3],[9,3],[9,4]]}]},{"id":4,"parent":3,"pieces":18,"rows":["_________Z","___LL_OOZZ","____LJOOZ_","____LJ____","____JJ____","I________L","I__ZZ_JLLL","I___ZZJJJZ"],"preClearRows":["_________Z","___LL_OOZZ","____LJOOZ_","____LJ____","____JJ____","I________L","I__ZZ_JLLL","I___ZZJJJZ","ISSTOOOOZZ","SSTTOOOOZJ","LLLTTTTSSJ"],"clearRows":[0,1,2],"placements":[{"letter":"T","cells":[[3,0],[2,1],[3,1],[3,2]]},{"letter":"S","cells":[[0,1],[1,1],[1,2],[2,2]]},{"letter":"O","cells":[[4,1],[5,1],[4,2],[5,2]]},{"letter":"I","cells":[[0,2],[0,3],[0,4],[0,5]]},{"letter":"Z","cells":[[4,3],[5,3],[3,4],[4,4]]},{"letter":"J","cells":[[6,3],[7,3],[8,3],[6,4]]},{"letter":"L","cells":[[7,4],[8,4],[9,4],[9,5]]},{"letter":"J","cells":[[4,6],[5,6],[5,7],[5,8]]},{"letter":"L","cells":[[4,7],[4,8],[3,9],[4,9]]},{"letter":"O","cells":[[6,8],[7,8],[6,9],[7,9]]},{"letter":"Z","cells":[[8,8],[8,9],[9,9],[9,10]]}]}]}]}"#,
    );
    compile_catalog_edge(&catalog, budget)
}

fn crevasse_budget_edge_catalog() -> OpenerCatalog {
    catalog_from_json(
        r#"{"formatVersion":2,"openers":[{"id":"crevasse-stacking","aliases":{"en":"Crevasse Stacking"},"shapeKey":"fixture","tree":[{"id":8,"parent":null,"pieces":14,"rows":["____I__JJJ","LL__I__ZZJ","LLOOI__SZZ","LLOOI__SSI"],"preClearRows":["____I__JJJ","LL__I__ZZJ","LLOOI__SZZ","LLOOI__SSI","LLSSZTTTSI","JSSZZTTOOI"],"clearRows":[0,1],"placements":[{"letter":"T","cells":[[6,0],[5,1],[6,1],[7,1]]}]},{"id":9,"parent":8,"pieces":23,"rows":["LJ________","LJJJ______","LLOO______","__OO______","________S_","OO____ZZSS","OOLL___ZZS","JJJLI_IJJJ","LLJLI_IZZJ","LLOOI_ISZZ","LLOOI_ISSI"],"placements":[{"letter":"I","cells":[[6,0],[6,1],[6,2],[6,3]]},{"letter":"J","cells":[[2,2],[0,3],[1,3],[2,3]]},{"letter":"L","cells":[[3,2],[3,3],[2,4],[3,4]]},{"letter":"O","cells":[[0,4],[1,4],[0,5],[1,5]]},{"letter":"Z","cells":[[7,4],[8,4],[6,5],[7,5]]},{"letter":"S","cells":[[9,4],[8,5],[9,5],[8,6]]},{"letter":"O","cells":[[2,7],[3,7],[2,8],[3,8]]},{"letter":"L","cells":[[0,8],[1,8],[0,9],[0,10]]},{"letter":"J","cells":[[1,9],[2,9],[3,9],[1,10]]}]}]}]}"#,
    )
}

fn compile_catalog_edge(
    catalog: &OpenerCatalog,
    budget: CompileBudget,
) -> (CompileCensus, Vec<super::graph::StateId>) {
    let record = &catalog.openers[0];
    let parent = &record.tree[0];
    let child = &record.tree[1];
    let placements = placement_specs(child);
    let start = DeclaredFrame::start(child, Some(parent), &placements, false).unwrap();
    let mut builder = super::graph::GraphBuilder::new();
    let parent_state = builder.intern(
        start.physical_board(),
        super::record::control(0, false, parent.id, 0),
        super::record::origin(record, false, parent, 0, true),
        false,
    );
    let mut census = CompileCensus::new(512, 32, 20_000, 4_000_000);
    let completed = super::edge::compile_edge(
        &mut builder,
        &mut census,
        super::edge::EdgeInput {
            record,
            record_index: 0,
            node: child,
            parent: Some(parent),
            mirrored: false,
            parent_states: &[parent_state],
            placements: &placements,
            budget: &budget,
            bridge_exposed: false,
        },
    )
    .unwrap();
    (census, completed)
}

#[test]
fn geometry_absent_from_movegen_is_not_accepted_as_a_legality_candidate() {
    let board = engine_board_from_masks(&[0x3bf, 0x3bf, 0x3bf, 0x3bf, 0x3cf, 0x3c7]);
    let cells = placement_cells(Move::new(Piece::I, Rotation::East, 6, 2, false)).unwrap();

    let legality = placement_is_srs_legal(&board, b'I', &cells);

    assert!(!legality.geometry_emitted);
    assert!(!legality.support_valid);
    assert!(matches!(legality.verdict, LegalityVerdict::NotAPlacement));
}

#[test]
fn census_names_support_without_exact_srs_as_an_incomplete_edge_signal() {
    let catalog = serde_json::from_slice::<OpenerCatalog>(include_bytes!(
        "../../../fixtures/openers/catalog-mini.json"
    ))
    .unwrap();

    let graph = compile_catalog_census(&catalog).unwrap();
    let census = serde_json::to_value(&graph.census).unwrap();

    assert!(census.get("supportObservedWithoutExactSrsEdges").is_some());
    assert!(census.get("supportMinusSrsEdgeDelta").is_none());
}

#[test]
fn kick_only_t_placement_is_accepted_with_lock_mechanics() {
    let board = engine_board_from_masks(&[0x155, 0x2aa, 0x155, 0x2aa, 0x175, 0x2ea, 0x1d5, 0x2ba]);
    let mut moves = MoveBuffer::new();
    generate_placements(&board, &mut moves, Piece::T, false);
    let target = moves
        .iter()
        .copied()
        .find(|candidate| {
            get_input(&board, candidate, false, false)
                .data
                .iter()
                .any(|input| {
                    matches!(
                        input,
                        Input::RotateCw | Input::RotateCcw | Input::RotateFlip
                    )
                })
        })
        .unwrap();
    let cells = placement_cells(target).unwrap();

    let legality = placement_is_srs_legal(&board, b'T', &cells);

    let LegalityVerdict::Legal { mechanics, .. } = legality.verdict else {
        panic!("expected a legal T placement");
    };
    assert!(legality.support_valid);
    assert_eq!(mechanics.lines_cleared, 0);
}

#[test]
fn legal_order_counting_is_opt_in_for_census_only() {
    use super::compile::CompileObserver;

    struct DefaultObserver;
    impl CompileObserver for DefaultObserver {}

    assert!(
        !DefaultObserver.wants_legal_orders(),
        "default observer must skip legal-order DP"
    );
    assert!(
        CompileCensus::new(512, 32, 20_000, 4_000_000).wants_legal_orders(),
        "census must still count legal orders"
    );
}

#[test]
fn production_compile_matches_census_graph_while_census_keeps_legal_orders() {
    let catalog = two_o_orders_catalog();

    let production =
        super::compile::compile_recognition_subgraph(&catalog, &CompileBudget::default())
            .expect("production compile succeeds");
    let census_graph = compile_catalog_census(&catalog).expect("census compile succeeds");

    let production_transitions: usize = production.graph.out.iter().map(Vec::len).sum();
    let census_transitions: usize = census_graph.out.iter().map(Vec::len).sum();
    assert_eq!(production.graph.states.len(), census_graph.states.len());
    assert_eq!(production_transitions, census_transitions);
    assert_eq!(
        production.graph.exact_index.len(),
        census_graph.exact_index.len()
    );
    assert!(!production.budget_exceeded);
    assert!(census_graph.census.budget_exceeded_edges.is_empty());
    assert!(
        census_graph.census.legal_order_count > 0,
        "census must still count legal orders"
    );
}

#[test]
fn skipping_observer_reports_no_legal_orders_without_changing_completed_states() {
    use super::compile::CompileObserver;

    struct SkippingSpy {
        reports: u32,
    }
    impl CompileObserver for SkippingSpy {
        fn legal_orders(&mut self, _count: u64) {
            self.reports = self.reports.saturating_add(1);
        }
    }

    let catalog = two_o_orders_catalog();
    let budget = CompileBudget::default();
    let mut spy_builder = super::graph::GraphBuilder::new();
    let mut spy = SkippingSpy { reports: 0 };
    let spy_completed = compile_child_edge(&mut spy_builder, &mut spy, &catalog, &budget);
    let mut census_builder = super::graph::GraphBuilder::new();
    let mut census = CompileCensus::new(512, 32, 20_000, 4_000_000);
    let census_completed = compile_child_edge(&mut census_builder, &mut census, &catalog, &budget);

    assert!(!spy.wants_legal_orders());
    assert!(!spy_completed.is_empty());
    assert_eq!(spy_completed.len(), census_completed.len());
    assert_eq!(
        spy.reports, 0,
        "production-style observer must skip legal-order DP"
    );
    assert!(
        census.legal_order_count > 0,
        "census must still count legal orders"
    );
}

fn two_o_orders_catalog() -> OpenerCatalog {
    catalog_from_json(
        r#"{"formatVersion":2,"openers":[{
            "id":"two-o-orders","aliases":{"en":"two o orders"},"shapeKey":"fixture",
            "tree":[
                {"id":0,"parent":null,"pieces":1,"rows":["OO________","OO________"],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]},
                {"id":1,"parent":0,"pieces":2,"rows":["OOOO______","OOOO______"],"placements":[{"letter":"O","cells":[[2,0],[3,0],[2,1],[3,1]]}]}
            ]
        }]}"#,
    )
}

fn compile_child_edge<O: super::compile::CompileObserver>(
    builder: &mut super::graph::GraphBuilder,
    observer: &mut O,
    catalog: &OpenerCatalog,
    budget: &CompileBudget,
) -> Vec<super::graph::StateId> {
    let record = &catalog.openers[0];
    let parent = &record.tree[0];
    let child = &record.tree[1];
    let placements = placement_specs(child);
    let start = DeclaredFrame::start(child, Some(parent), &placements, false).unwrap();
    let parent_state = builder.intern(
        start.physical_board(),
        super::record::control(0, false, parent.id, 0),
        super::record::origin(record, false, parent, 0, true),
        false,
    );
    super::edge::compile_edge(
        builder,
        observer,
        super::edge::EdgeInput {
            record,
            record_index: 0,
            node: child,
            parent: Some(parent),
            mirrored: false,
            parent_states: &[parent_state],
            placements: &placements,
            budget,
            bridge_exposed: false,
        },
    )
    .unwrap()
}

#[test]
#[ignore]
fn full_catalog_census() {
    let catalog = serde_json::from_slice::<OpenerCatalog>(include_bytes!(
        "../../../fixtures/openers/catalog-full.json"
    ))
    .unwrap();
    let mut graph = compile_catalog_census(&catalog).unwrap();
    graph
        .census
        .join_path_a_baseline(std::path::Path::new(
            "evidence-out/opener-recognition-census/census.json",
        ))
        .unwrap();

    if let Some(output) = std::env::var_os("OPENER_CENSUS_OUT") {
        write_census(&graph.census, std::path::Path::new(&output)).unwrap();
    }
    let failures = graph.census.g1_failures();
    let impossible = graph
        .census
        .direct_impossible_edges
        .iter()
        .map(|edge| {
            format!(
                "{}:{} mirrored={} {}",
                edge.record, edge.node_id, edge.mirrored, edge.reason
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        failures.is_empty(),
        "G1 FAIL: {}\n{impossible}",
        failures.join("; ")
    );
}
