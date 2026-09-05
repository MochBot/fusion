use crate::openers::{
    analyze_opener_round, set_opener_catalog, OpenerObservation, OpenerRoundInput,
};

#[test]
fn confirms_a_direct_later_endpoint_and_retains_it_after_misses() {
    let _scope = crate::openers::isolated_catalog_test();
    install(CATALOG_WITH_LATER_ENDPOINT);

    let analysis = analyze(&[
        None,
        Some(observation(1)),
        Some(observation(0b1111111100)),
        Some(observation(2)),
        Some(observation(4)),
    ]);
    let matched = analysis
        .catalogued_board_match
        .expect("the authored third ordinal endpoint should confirm directly");

    assert_eq!(matched.first_match_index, 2);
    assert_eq!(matched.anchor_index, 2);
    assert_eq!(matched.matching_openers[0].id, "alpha");
}

#[test]
fn rejects_wrong_ordinals_search_shapes_and_preclear_rows() {
    let _scope = crate::openers::isolated_catalog_test();
    install(CATALOG_WITH_NON_ENDPOINT_BOARDS);

    let analysis = analyze(&[Some(observation(0b0000001111))]);

    assert!(analysis.catalogued_board_match.is_none());
}

#[test]
fn keeps_letter_mismatched_endpoints_out_of_weak_phase_assessment() {
    let _scope = crate::openers::isolated_catalog_test();
    install(CATALOG_WITH_LETTER_ENDPOINT);

    let analysis = analyze(&[Some(observation(0b0000001111))]);

    assert!(analysis.catalogued_board_match.is_some());
    assert!(analysis.assessments[0]
        .as_ref()
        .is_some_and(|assessment| !assessment.on_script));
}

#[test]
fn retains_every_record_in_a_twenty_five_way_exact_tie() {
    let _scope = crate::openers::isolated_catalog_test();
    let openers = (0..25)
        .map(|index| {
            format!(
                r#"{{"id":"record-{index:02}","aliases":{{"en":"Record {index}"}},"shapeKey":"record-{index}","tree":[{{"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]}}]}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let catalog = format!(r#"{{"formatVersion":2,"openers":[{openers}]}}"#);
    install(catalog.as_bytes());

    let analysis = analyze(&[Some(observation(0b0000001111))]);
    let matched = analysis
        .catalogued_board_match
        .expect("the common endpoint should retain every tied record");

    assert_eq!(matched.matching_openers.len(), 25);
    assert_eq!(
        matched
            .matching_openers
            .iter()
            .map(|opener| opener.id.as_str())
            .collect::<Vec<_>>(),
        (0..25)
            .map(|index| format!("record-{index:02}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn reanchors_a_survivor_after_a_non_null_alien_board() {
    let _scope = crate::openers::isolated_catalog_test();
    install(CATALOG_WITH_REJOIN);

    let analysis = analyze(&[
        Some(observation(0b0000001111)),
        Some(observation(1)),
        Some(observation(0b1111111100)),
    ]);
    let matched = analysis
        .catalogued_board_match
        .expect("the deeper alpha endpoint should rejoin after the alien board");

    assert_eq!(matched.anchor_index, 2);
    assert_eq!(
        matched
            .matching_openers
            .iter()
            .map(|opener| opener.id.as_str())
            .collect::<Vec<_>>(),
        ["alpha"]
    );
}

fn analyze(observations: &[Option<OpenerObservation>]) -> crate::openers::OpenerRoundAnalysis {
    match analyze_opener_round(&OpenerRoundInput {
        observations: observations.to_vec(),
        ..OpenerRoundInput::default()
    }) {
        Ok(analysis) => analysis,
        Err(error) => panic!("installed test catalog should analyze: {error}"),
    }
}

fn observation(mask: u16) -> OpenerObservation {
    OpenerObservation {
        post_board: Some(vec![mask]),
        post_gmask: Some(vec![0]),
        post_letters: None,
    }
}

fn install(catalog: &[u8]) {
    if let Err(error) = set_opener_catalog(catalog) {
        panic!("test catalog should install: {error}");
    }
}

const CATALOG_WITH_LATER_ENDPOINT: &[u8] = br#"
{"formatVersion":2,"openers":[{"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
  {"id":3,"parent":null,"pieces":3,"rows":["__XXXXXXXX"]}
]}]}
"#;

const CATALOG_WITH_NON_ENDPOINT_BOARDS: &[u8] = br#"
{"formatVersion":2,"openers":[
  {"id":"wrong-ordinal","aliases":{"en":"Wrong"},"shapeKey":"wrong","tree":[
    {"id":1,"parent":null,"pieces":3,"rows":["XXXX______"]}
  ]},
  {"id":"unenriched","aliases":{"en":"Unenriched"},"shapeKey":"unenriched","searchShapes":[["XXXX______"]],"tree":[
    {"id":2,"parent":null,"pieces":1,"rows":["__________"],"preClearRows":["XXXX______"]}
  ]}
]}
"#;

const CATALOG_WITH_LETTER_ENDPOINT: &[u8] = br#"
{"formatVersion":2,"openers":[{"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
  {"id":1,"parent":null,"pieces":1,"rows":["IIII______"]}
]}]}
"#;

const CATALOG_WITH_REJOIN: &[u8] = br#"
{"formatVersion":2,"openers":[
  {"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
    {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]},
    {"id":2,"parent":1,"pieces":3,"rows":["__XXXXXXXX"]}
  ]},
  {"id":"charlie","aliases":{"en":"Charlie"},"shapeKey":"charlie","tree":[
    {"id":3,"parent":null,"pieces":3,"rows":["__XXXXXXXX"]}
  ]}
]}
"#;
