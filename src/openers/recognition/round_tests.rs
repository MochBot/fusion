use crate::openers::analyze::{analyze_opener_round_with_catalog, OpenerRoundInput};
use crate::openers::catalog::{OpenerCatalog, OpenerRecord};
use crate::openers::catalogued_match::{
    match_catalogued_boards, MatchingOpener, RoundCataloguedBoardMatch,
};
use crate::openers::guide::{select_subject, GuideBasis};
use crate::openers::matcher::BoardMatch;
use crate::openers::phase::{assess_opener_phase, OpenerAssessment, OpenerObservation};
use crate::openers::target::build_targets;

use super::super::align::{align_round_exact, AlignBudget, Observation};
use super::super::battery::synthesize_walk_observations;
use super::super::compile::{compile_recognition_graph, CompileBudget};
use super::super::cost::{EditCosts, EditOps};
use super::super::graph::{RecognitionGraph, StateId, TransitionLabel};
use super::{
    recognize_round, shortlist, singleton_confirmed_id, RoundHypothesis, RoundRecognition,
};
use crate::openers::recognition::RecordGraphCache;

const CONFIRMED_TIE_CATALOG: &str = r#"{
  "formatVersion": 2,
  "openers": [
    {
      "id": "mountainous-stacking-2",
      "aliases": {"en": "Mountainous Stacking 2"},
      "shapeKey": "confirmed-shape",
      "tree": [{"id": 1, "parent": null, "pieces": 14, "rows": ["LLLL______"]}]
    },
    {
      "id": "alternate",
      "aliases": {"en": "Alternate"},
      "shapeKey": "alternate-shape",
      "tree": [{"id": 19, "parent": null, "pieces": 48, "rows": ["JJJJ______"]}]
    }
  ]
}"#;

fn hypothesis(record: &str, node_id: u32, total_cost: u32, margin: u32) -> RoundHypothesis {
    RoundHypothesis {
        record: record.to_owned(),
        node_id,
        route_name: None,
        mirrored: false,
        total_cost,
        margin,
        ops: EditOps {
            synchronous: 13,
            substitutions: 0,
            cell_mismatches: 0,
            observed_only: 164,
            model_only: 0,
        },
        identity_frozen: false,
    }
}

fn singleton_mountainous_match() -> RoundCataloguedBoardMatch {
    RoundCataloguedBoardMatch {
        first_match_index: 5,
        anchor_index: 6,
        matching_openers: vec![MatchingOpener {
            id: "mountainous-stacking-2".to_owned(),
            name: "Mountainous Stacking 2".to_owned(),
            deepest_pieces: 7,
            candidate_node_ids: Vec::new(),
            route_name: None,
            mirrored: None,
        }],
    }
}

fn recognition(hypotheses: Vec<RoundHypothesis>) -> RoundRecognition {
    RoundRecognition {
        retrieval_bounded: true,
        shortlist_size: 15,
        truncated: true,
        shortlist_compile_skipped: 0,
        per_lock: Vec::new(),
        hypotheses,
        unknown_cost: 1062,
        edit_costs: EditCosts::default(),
    }
}

#[test]
fn shortlist_deduplicates_v1_matches_and_reports_the_cap() {
    let assessments = vec![Some(assessment("primary", &["runner", "primary"]))];

    let shortlist = shortlist(&assessments, None);

    assert_eq!(shortlist.ids, ["primary", "runner"]);
    assert!(!shortlist.truncated);
}

#[test]
fn shortlist_caps_v1_evidence_at_the_authored_limit() {
    let assessments = (0..25)
        .map(|index| Some(assessment(&format!("record-{index}"), &[])))
        .collect::<Vec<_>>();

    let shortlist = shortlist(&assessments, None);

    assert_eq!(shortlist.ids.len(), 24);
    assert_eq!(shortlist.ids.first().map(String::as_str), Some("record-0"));
    assert_eq!(shortlist.ids.last().map(String::as_str), Some("record-23"));
    assert!(shortlist.truncated);
}

#[test]
fn tied_catalogued_matches_have_no_preferred_confirmed_identity() {
    let matched = RoundCataloguedBoardMatch {
        first_match_index: 0,
        anchor_index: 1,
        matching_openers: vec![
            MatchingOpener {
                id: "alpha".to_owned(),
                name: "Alpha".to_owned(),
                deepest_pieces: 2,
                candidate_node_ids: vec![2],
                route_name: Some("Alpha Route".to_owned()),
                mirrored: Some(false),
            },
            MatchingOpener {
                id: "beta".to_owned(),
                name: "Beta".to_owned(),
                deepest_pieces: 2,
                candidate_node_ids: vec![3],
                route_name: Some("Beta Route".to_owned()),
                mirrored: Some(true),
            },
        ],
    };

    assert_eq!(singleton_confirmed_id(Some(&matched)), None);
}

#[test]
fn singleton_confirmation_breaks_a_cross_shape_recognition_tie() {
    let catalog: OpenerCatalog =
        serde_json::from_str(CONFIRMED_TIE_CATALOG).expect("tie catalog should parse");
    let matched = singleton_mountainous_match();
    let recognition = recognition(vec![
        hypothesis("alternate", 19, 820, 0),
        hypothesis("mountainous-stacking-2", 1, 820, 40),
    ]);

    let subject = select_subject(&catalog, Some(&matched), Some(&recognition))
        .expect("the singleton confirmed record should own the guide");

    assert_eq!(subject.basis, GuideBasis::Confirmed);
    assert_eq!(subject.record_id, "mountainous-stacking-2");
    assert_eq!(subject.node_id, Some(1));
    assert_eq!(subject.anchor_lock, Some(6));
}

#[test]
fn singleton_confirmation_does_not_override_lower_cost_recognition() {
    let catalog: OpenerCatalog =
        serde_json::from_str(CONFIRMED_TIE_CATALOG).expect("tie catalog should parse");
    let matched = singleton_mountainous_match();
    let recognition = recognition(vec![
        hypothesis("alternate", 19, 820, 40),
        hypothesis("mountainous-stacking-2", 1, 860, 202),
    ]);

    let subject = select_subject(&catalog, Some(&matched), Some(&recognition))
        .expect("the lower-cost recognition should still carry a nearest guide");

    assert_eq!(subject.basis, GuideBasis::Nearest);
    assert_eq!(subject.record_id, "alternate");
    assert_eq!(subject.node_id, Some(19));
    assert_eq!(subject.anchor_lock, None);
}

#[test]
fn recognize_round_returns_none_without_catalog_as_an_internal_guard() {
    let assessments = vec![Some(assessment("fixture", &[]))];
    let observations = vec![Some(observation())];

    assert!(recognize_round(
        None,
        &RecordGraphCache::default(),
        &assessments,
        None,
        &observations
    )
    .is_none());
    assert!(recognize_round(
        Some(&catalog()),
        &RecordGraphCache::default(),
        &assessments,
        None,
        &[]
    )
    .is_none());
}

#[test]
fn recognition_returns_a_deterministic_exact_hypothesis_for_v1_evidence() {
    let catalog = catalog();
    let assessments = vec![Some(assessment("fixture", &[]))];
    let observations = vec![Some(observation())];

    let first = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        match_catalogued_boards(&catalog, &observations).as_ref(),
        &observations,
    )
    .expect("v1 evidence should produce recognition");
    let second = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        match_catalogued_boards(&catalog, &observations).as_ref(),
        &observations,
    )
    .expect("v1 evidence should produce recognition");

    assert_eq!(
        first
            .hypotheses
            .first()
            .map(|hypothesis| hypothesis.record.as_str()),
        Some("fixture")
    );
    assert_eq!(
        first
            .hypotheses
            .first()
            .map(|hypothesis| hypothesis.total_cost),
        Some(0)
    );
    assert_eq!(
        serde_json::to_vec(&first).ok(),
        serde_json::to_vec(&second).ok()
    );
}

#[test]
fn analyzer_attaches_route_recognition_for_a_generated_legal_round() {
    let catalog: OpenerCatalog = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse");
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == "crowbar-v2")
        .expect("catalog fixture should contain crowbar")
        .clone();
    let input = OpenerRoundInput {
        observations: legal_observations(&record),
        ..OpenerRoundInput::default()
    };

    let analysis = analyze_opener_round_with_catalog(&catalog, &input);
    let recognition = analysis
        .recognition
        .expect("generated legal observations should produce recognition");

    assert_eq!(
        recognition
            .hypotheses
            .first()
            .map(|hypothesis| hypothesis.record.as_str()),
        Some("crowbar-v2")
    );
    assert_eq!(
        recognition
            .hypotheses
            .first()
            .map(|hypothesis| hypothesis.total_cost),
        Some(0)
    );
}

#[test]
fn recognition_scans_through_the_final_observation_slot() {
    let catalog: OpenerCatalog = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse");
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == "crowbar-v2")
        .expect("catalog fixture should contain crowbar");
    let mut observations = legal_observations(record);
    let mut assessments = assess_opener_phase(&build_targets(&catalog), &observations);
    observations.extend_from_within(..4);
    assessments.extend((0..4).map(|_| Some(assessment("crowbar-v2", &[]))));

    let recognition = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        match_catalogued_boards(&catalog, &observations).as_ref(),
        &observations,
    )
    .expect("extended on-script walk should produce recognition");

    assert_eq!(recognition.per_lock.len(), observations.len());
}

#[test]
fn recognition_includes_unassessed_later_observations() {
    let catalog: OpenerCatalog = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse");
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == "crowbar-v2")
        .expect("catalog fixture should contain crowbar");
    let observations = legal_observations(record);
    let assessments = assess_opener_phase(&build_targets(&catalog), &observations);
    let window_only = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        match_catalogued_boards(&catalog, &observations).as_ref(),
        &observations,
    )
    .expect("on-script walk should produce recognition");

    let mut extended_observations = observations;
    extended_observations.extend((0..6).map(|index| {
        Some(OpenerObservation {
            post_board: Some(vec![1u16 << index]),
            post_gmask: Some(vec![0]),
            post_letters: None,
        })
    }));
    let mut extended_assessments = assessments;
    extended_assessments.extend((0..6).map(|_| None));
    let extended = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &extended_assessments,
        match_catalogued_boards(&catalog, &extended_observations).as_ref(),
        &extended_observations,
    )
    .expect("v1-assessed opener window should produce recognition");

    assert_ne!(
        serde_json::to_vec(&window_only).expect("recognition should serialize"),
        serde_json::to_vec(&extended).expect("recognition should serialize")
    );
    assert_eq!(extended.per_lock.len(), extended_observations.len());
}

#[test]
fn recognition_returns_none_when_v1_assessed_no_locks() {
    let assessments = vec![None];
    let observations = vec![Some(observation())];

    assert!(recognize_round(
        Some(&catalog()),
        &RecordGraphCache::default(),
        &assessments,
        match_catalogued_boards(&catalog(), &observations).as_ref(),
        &observations
    )
    .is_none());
}

#[test]
fn recognition_keeps_a_singleton_confirmed_identity_when_available() {
    #[derive(serde::Deserialize)]
    struct ParityFixture {
        rounds: Vec<ParityRound>,
    }

    #[derive(serde::Deserialize)]
    struct ParityRound {
        input: ParityInput,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ParityInput {
        observations: Vec<Option<OpenerObservation>>,
    }

    let catalog: OpenerCatalog = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse");
    let fixture: ParityFixture = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/parity-rounds.json"
    )))
    .expect("parity-round fixture should parse");
    let mut checked = 0;

    for round in fixture.rounds {
        let analysis = analyze_opener_round_with_catalog(
            &catalog,
            &OpenerRoundInput {
                observations: round.input.observations,
                ..OpenerRoundInput::default()
            },
        );
        let Some(report) = analysis.report else {
            continue;
        };
        let recognition = analysis
            .recognition
            .expect("singleton confirmed round should produce recognition");
        let best = recognition
            .hypotheses
            .first()
            .expect("recognition should retain a best hypothesis");
        assert_eq!(best.record, report.opener_id);
        assert!(best.total_cost < recognition.unknown_cost);
        checked += 1;
    }

    assert!(
        checked > 0,
        "fixture should contain a clean completed round"
    );
}

#[test]
fn recognition_dto_is_stable_across_independent_compiles() {
    // This catches within-process allocation-order drift; sorted compilation makes the same
    // StateId ordering hold across processes for identical catalog bytes.
    let catalog: OpenerCatalog = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse");
    let first_graph = compile_recognition_graph(&catalog, &CompileBudget::default())
        .expect("catalog fixture should compile");
    let second_graph = compile_recognition_graph(&catalog, &CompileBudget::default())
        .expect("catalog fixture should compile");
    let input_for = |graph: &RecognitionGraph| OpenerRoundInput {
        observations: legal_observations_from_graph(graph, "crowbar-v2"),
        ..OpenerRoundInput::default()
    };

    let first = analyze_opener_round_with_catalog(&catalog, &input_for(&first_graph))
        .recognition
        .expect("legal round should produce recognition");
    let second = analyze_opener_round_with_catalog(&catalog, &input_for(&second_graph))
        .recognition
        .expect("legal round should produce recognition");

    assert_eq!(
        serde_json::to_vec(&first).expect("recognition should serialize"),
        serde_json::to_vec(&second).expect("recognition should serialize")
    );
}

#[test]
#[ignore]
fn shortlist_recognition_matches_full_alignment_for_v1_matched_legal_walks() {
    let catalog_path = std::env::var("OPENER_RECOGNITION_CATALOG")
        .expect("OPENER_RECOGNITION_CATALOG must name the real catalog JSON");
    let catalog_bytes = std::fs::read(catalog_path).expect("real catalog JSON should be readable");
    let catalog: OpenerCatalog =
        serde_json::from_slice(&catalog_bytes).expect("real catalog JSON should parse");
    let graph = compile_recognition_graph(&catalog, &CompileBudget::default())
        .expect("real catalog should compile");
    let targets = build_targets(&catalog);
    let mut record_ids = catalog
        .openers
        .iter()
        .filter(|record| !record.shape_key.starts_with("stub-") && !record.tree.is_empty())
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    record_ids.sort();

    let mut matched = 0;
    let mut v1_missed = 0;
    let mut synthesis_skipped = 0;
    let mut legal_walks = 0;
    for record_id in record_ids {
        if legal_walks == 25 {
            break;
        }
        let Ok(walk) = synthesize_walk_observations(&graph, &record_id, false) else {
            synthesis_skipped += 1;
            continue;
        };
        legal_walks += 1;
        let observations = walk
            .iter()
            .map(|observation| observation.as_ref().map(observation_from_alignment))
            .collect::<Vec<_>>();
        let assessments = assess_opener_phase(&targets, &observations);
        if !assessments
            .iter()
            .flatten()
            .any(|assessment| assessment.r#match.is_some())
        {
            v1_missed += 1;
            continue;
        }
        let recognized = recognize_round(
            Some(&catalog),
            &RecordGraphCache::default(),
            &assessments,
            match_catalogued_boards(&catalog, &observations).as_ref(),
            &observations,
        )
        .expect("v1-matched legal walk should produce recognition");
        let full = align_round_exact(
            &graph,
            &EditCosts::default(),
            &walk,
            &AlignBudget::default(),
        );
        let full_identity = full
            .hypotheses
            .first()
            .and_then(|hypothesis| hypothesis.origin.as_ref())
            .map(|origin| origin.record.as_ref())
            .expect("full alignment should retain a legal identity");
        let shortlist_identity = recognized
            .hypotheses
            .first()
            .map(|hypothesis| hypothesis.record.as_str())
            .expect("shortlist alignment should retain a legal identity");
        assert_eq!(shortlist_identity, full_identity, "{record_id}");
        matched += 1;
    }
    assert_eq!(
        legal_walks, 25,
        "real catalog must provide 25 synthesizable legal walks"
    );
    println!(
        "shortlist parity: legal_walks={legal_walks}, matched={matched}, v1_missed={v1_missed}, synthesis_skipped={synthesis_skipped}"
    );
}

fn catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{"formatVersion":2,"openers":[{"id":"fixture","aliases":{"en":"Fixture"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":0,"rows":[],"placements":[]}]}]}"#,
    )
    .expect("fixture catalog should parse")
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

fn observation() -> OpenerObservation {
    OpenerObservation {
        post_board: Some(Vec::new()),
        post_gmask: Some(Vec::new()),
        post_letters: None,
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

fn observation_from_alignment(observation: &Observation) -> OpenerObservation {
    OpenerObservation {
        post_board: Some(observation.key.masks.to_vec()),
        post_gmask: Some(vec![0; observation.key.masks.len()]),
        post_letters: None,
    }
}
