use fusion_engine::openers::{
    analyze_opener_round, set_opener_catalog, AnalyzeError, BoardMatch, OpenerAssessment,
    OpenerLockPolicy, OpenerObservation, OpenerRoundInput,
};
use serde::Deserialize;

#[test]
fn serializes_the_confirmed_catalogued_board_match_through_the_installed_catalog_seam() {
    let empty = OpenerRoundInput::default();
    assert!(matches!(
        analyze_opener_round(&empty),
        Err(AnalyzeError::NoCatalog)
    ));
    install_catalog();

    let analysis = match analyze_opener_round(&OpenerRoundInput {
        observations: vec![
            Some(observation(0b0000001111)),
            None,
            Some(observation(0b1111111100)),
        ],
        ..OpenerRoundInput::default()
    }) {
        Ok(analysis) => analysis,
        Err(error) => panic!("installed catalog should analyze a round: {error}"),
    };
    let matched = match analysis.catalogued_board_match.as_ref() {
        Some(matched) => matched,
        None => panic!("the recorded boards should confirm the catalogued opener"),
    };

    assert_eq!(matched.first_match_index, 0);
    assert_eq!(matched.anchor_index, 2);
    assert_eq!(matched.matching_openers.len(), 1);
    let opener = &matched.matching_openers[0];
    assert_eq!(opener.id, "alpha");
    assert_eq!(opener.deepest_pieces, 3);
    assert_eq!(opener.candidate_node_ids, [2]);
    assert_eq!(opener.route_name.as_deref(), Some("Alpha Route"));
    assert_eq!(opener.mirrored, Some(false));
    assert_eq!(
        analysis.report.as_ref().map(|report| report.name.as_str()),
        Some("Alpha")
    );

    let serialized = match serde_json::to_value(&analysis) {
        Ok(value) => value,
        Err(error) => panic!("analysis should serialize: {error}"),
    };
    let dto = match serialized.get("cataloguedBoardMatch") {
        Some(dto) => dto,
        None => panic!("serialized analysis should include the catalogued match DTO"),
    };
    assert_eq!(dto["matchingOpeners"][0]["id"], "alpha");
    assert_eq!(dto["matchingOpeners"][0]["deepestPieces"], 3);
    assert!(dto.get("deepestPieces").is_none());
    assert_eq!(dto["firstMatchIndex"], 0);
    assert_eq!(dto["anchorIndex"], 2);
    let report = &serialized["report"];
    assert_eq!(report["openerId"], "alpha");
    assert_eq!(report["routeName"], "Alpha Route");
    assert_eq!(report["mirrored"], false);
    assert_eq!(report["firstMatchIndex"], 0);
    assert_eq!(report["anchorIndex"], 2);
    assert!(report.get("verdict").is_none());

    assert_policy_and_assessment_parity();
}

fn assert_policy_and_assessment_parity() {
    let catalog_bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    ));
    let fixture: RoundFixture = match serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/parity-rounds.json"
    ))) {
        Ok(fixture) => fixture,
        Err(error) => panic!("round fixture should parse: {error}"),
    };
    if let Err(error) = set_opener_catalog(catalog_bytes) {
        panic!("mini catalog should install: {error}");
    }
    let first_round = match fixture.rounds.first() {
        Some(round) => round,
        None => panic!("fixture should contain an on-script round"),
    };
    let on_script = analyze(&first_round.input.observations, Some(5));

    assert_assessment_identity(&on_script.assessments, &first_round.expected.assessments);
    assert_policy(
        &on_script.policies[3],
        OpenerLockPolicy {
            b2b_break_exempt: true,
            board_mess_exempt: true,
            attack_gap_exempt: true,
            attack_gap_included_in_tier_distributions: false,
        },
        "on-script lock before relax depth",
    );
    assert_policy(
        &on_script.policies[5],
        OpenerLockPolicy {
            b2b_break_exempt: true,
            board_mess_exempt: true,
            attack_gap_exempt: false,
            attack_gap_included_in_tier_distributions: true,
        },
        "on-script lock at relax depth",
    );
    let full_chain = analyze(&first_round.input.observations, None);
    assert_policy(
        &full_chain.policies[6],
        OpenerLockPolicy {
            b2b_break_exempt: true,
            board_mess_exempt: true,
            attack_gap_exempt: true,
            attack_gap_included_in_tier_distributions: false,
        },
        "on-script lock without relaxation",
    );

    let broken_round = match fixture.rounds.get(1) {
        Some(round) => round,
        None => panic!("fixture should contain an off-script round"),
    };
    let off_script = analyze(&broken_round.input.observations, Some(5));
    assert_policy(
        &off_script.policies[4],
        OpenerLockPolicy {
            b2b_break_exempt: false,
            board_mess_exempt: false,
            attack_gap_exempt: false,
            attack_gap_included_in_tier_distributions: true,
        },
        "off-script lock",
    );
    assert_policy(
        &off_script.policies[0],
        OpenerLockPolicy {
            b2b_break_exempt: false,
            board_mess_exempt: false,
            attack_gap_exempt: false,
            attack_gap_included_in_tier_distributions: true,
        },
        "missing observation",
    );
}

fn analyze(
    observations: &[Option<OpenerObservation>],
    relax_depth: Option<usize>,
) -> fusion_engine::openers::OpenerRoundAnalysis {
    match analyze_opener_round(&OpenerRoundInput {
        observations: observations.to_vec(),
        relax_depth,
        ..OpenerRoundInput::default()
    }) {
        Ok(analysis) => analysis,
        Err(error) => panic!("installed catalog should analyze: {error}"),
    }
}

fn assert_policy(actual: &OpenerLockPolicy, expected: OpenerLockPolicy, case_name: &str) {
    assert_eq!(actual, &expected, "{case_name}");
}

fn assert_assessment_identity(
    actual: &[Option<OpenerAssessment>],
    expected: &[Option<OpenerAssessment>],
) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        match (actual, expected) {
            (None, None) => {}
            (Some(actual), Some(expected)) => {
                assert_eq!(actual.on_script, expected.on_script);
                assert_eq!(actual.board_cells, expected.board_cells);
                assert_eq!(
                    actual.r#match.as_ref().map(match_identity),
                    expected.r#match.as_ref().map(match_identity)
                );
                assert_eq!(
                    actual
                        .runners_up
                        .iter()
                        .map(match_identity)
                        .collect::<Vec<_>>(),
                    expected
                        .runners_up
                        .iter()
                        .map(match_identity)
                        .collect::<Vec<_>>()
                );
            }
            _ => panic!("assessment presence differs"),
        }
    }
}

fn match_identity(board_match: &BoardMatch) -> (&str, Option<u32>, bool, bool) {
    (
        &board_match.opener_id,
        board_match.node_id,
        board_match.mirrored,
        board_match.complete,
    )
}

#[derive(Deserialize)]
struct RoundFixture {
    rounds: Vec<RoundCase>,
}

#[derive(Deserialize)]
struct RoundCase {
    input: RoundFixtureInput,
    expected: RoundFixtureExpected,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoundFixtureInput {
    observations: Vec<Option<OpenerObservation>>,
}

#[derive(Deserialize)]
struct RoundFixtureExpected {
    assessments: Vec<Option<OpenerAssessment>>,
}

fn observation(mask: u16) -> OpenerObservation {
    OpenerObservation {
        post_board: Some(vec![mask]),
        post_gmask: Some(vec![0]),
        post_letters: Some(vec!["ZZZZ______".to_owned()]),
    }
}

fn install_catalog() {
    let catalog = br#"
    {"formatVersion":2,"openers":[
      {"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
        {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]},
        {"id":2,"parent":1,"pieces":3,"rows":["__XXXXXXXX"],"routeName":"Alpha Route"}
      ]},
      {"id":"beta","aliases":{"en":"Beta"},"shapeKey":"beta","tree":[
        {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]}
      ]}
    ]}
    "#;
    if let Err(error) = set_opener_catalog(catalog) {
        panic!("test catalog should install: {error}");
    }
}
