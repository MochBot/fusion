use crate::openers::{
    analyze_opener_round, set_opener_catalog, set_search_shape_witnesses, OpenerObservation,
    OpenerRoundInput,
};
use sha2::{Digest, Sha256};

#[test]
fn retains_initial_ties_then_narrows_on_a_later_absolute_ordinal() {
    let _scope = crate::openers::isolated_catalog_test();
    install_catalog(CATALOG_WITH_CONTINUATION);

    let analysis = analyze(&[
        Some(observation(0b0000001111, "JJJJ______")),
        None,
        Some(observation(0b1111111100, "________JJ")),
    ]);

    let matched = analysis
        .catalogued_board_match
        .expect("the third observation should narrow the initial matching opener set");

    assert_eq!(matched.first_match_index, 0);
    assert_eq!(matched.anchor_index, 2);
    assert_eq!(matching_ids(&matched), ["alpha"]);
    let alpha = matching_opener(&matched, "alpha");
    assert_eq!(alpha.deepest_pieces, 3);
    assert_eq!(alpha.candidate_node_ids, [2]);
    assert_eq!(alpha.route_name.as_deref(), Some("Alpha Route"));
    assert_eq!(alpha.mirrored, Some(false));
    assert_eq!(
        analysis
            .report
            .as_ref()
            .map(|report| report.opener_id.as_str()),
        Some("alpha")
    );
}

#[test]
fn recovers_a_later_exact_record_after_a_terminal_early_match() {
    let _scope = crate::openers::isolated_catalog_test();
    install_catalog(CATALOG_WITH_TERMINAL_EARLY_MATCH);

    let analysis = analyze(&[
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1100000000, "________JJ")),
    ]);

    let matched = analysis
        .catalogued_board_match
        .expect("the later exact board should recover the continuing opener");

    assert_eq!(matched.first_match_index, 0);
    assert_eq!(matched.anchor_index, 1);
    assert_eq!(matching_ids(&matched), ["late"]);
    assert_eq!(matching_opener(&matched, "late").deepest_pieces, 2);
}

#[test]
fn leaves_tied_catalogued_records_without_a_report() {
    let _scope = crate::openers::isolated_catalog_test();
    install_catalog(CATALOG_WITH_TIE);

    let analysis = analyze(&[
        Some(observation(0b0000001111, "ZZZZ______")),
        Some(observation(0b1111111100, "________ZZ")),
    ]);

    let matched = analysis
        .catalogued_board_match
        .expect("the shared first board should confirm both records");

    assert_eq!(matching_ids(&matched), ["alpha", "beta"]);
    let alpha = matching_opener(&matched, "alpha");
    let beta = matching_opener(&matched, "beta");
    assert_eq!(alpha.candidate_node_ids, [2]);
    assert_eq!(alpha.route_name.as_deref(), Some("Alpha Route"));
    assert_eq!(alpha.mirrored, Some(false));
    assert_eq!(beta.candidate_node_ids, [3]);
    assert_eq!(beta.route_name.as_deref(), Some("Beta Route"));
    assert_eq!(beta.mirrored, Some(true));
    assert!(analysis.report.is_none());
}

#[test]
fn advances_through_an_authored_empty_board() {
    let _scope = crate::openers::isolated_catalog_test();
    install_catalog(CATALOG_WITH_EMPTY_CONTINUATION);

    let analysis = analyze(&[
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0, "__________")),
    ]);

    let matched = analysis
        .catalogued_board_match
        .expect("the empty authored node should reanchor the matching set");

    assert_eq!(matched.anchor_index, 1);
    assert_eq!(matching_ids(&matched), ["alpha"]);
    let alpha = matching_opener(&matched, "alpha");
    assert_eq!(alpha.deepest_pieces, 2);
    assert_eq!(alpha.candidate_node_ids, [2]);
    assert_eq!(alpha.mirrored, None);
}

#[test]
fn withholds_route_when_one_identity_has_multiple_paths() {
    let _scope = crate::openers::isolated_catalog_test();
    install_catalog(CATALOG_WITH_AMBIGUOUS_PATHS);

    let analysis = analyze(&[
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1111111111, "LLLLLLLLLL")),
    ]);

    let matched = analysis
        .catalogued_board_match
        .expect("the deeper board should retain the one confirmed identity");

    assert_eq!(matching_ids(&matched), ["alpha"]);
    let alpha = matching_opener(&matched, "alpha");
    assert_eq!(alpha.candidate_node_ids, [2, 3]);
    assert_eq!(alpha.route_name, None);
    let report = analysis
        .report
        .expect("a singleton identity should project a report");
    assert_eq!(report.route_name, None);
    assert_eq!(report.mirrored, None);
}

#[test]
fn confirms_a_companion_post_clear_target_without_inventing_tree_semantics() {
    let _scope = crate::openers::isolated_catalog_test();
    install_catalog(CATALOG_WITH_DETACHED_TARGET);
    install_witnesses(CATALOG_WITH_DETACHED_TARGET, "XXXX______", 2);

    let analysis = analyze(&[None, Some(observation(0b0000001111, "XXXX______"))]);

    let matched = analysis
        .catalogued_board_match
        .expect("the validated companion target should confirm its opener identity");
    assert_eq!(matching_ids(&matched), ["alpha"]);
    let alpha = matching_opener(&matched, "alpha");
    assert_eq!(alpha.deepest_pieces, 2);
    assert!(alpha.candidate_node_ids.is_empty());
    assert_eq!(alpha.route_name, None);
    assert_eq!(alpha.mirrored, None);
    assert_eq!(
        analysis.report.map(|report| report.opener_id),
        Some("alpha".to_owned())
    );
}

#[test]
fn replacing_the_catalog_clears_its_bound_companion_targets() {
    let _scope = crate::openers::isolated_catalog_test();
    install_catalog(CATALOG_WITH_DETACHED_TARGET);
    install_witnesses(CATALOG_WITH_DETACHED_TARGET, "XXXX______", 2);
    install_catalog(CATALOG_WITH_CONTINUATION);

    let analysis = analyze(&[None, Some(observation(0b0000001111, "XXXX______"))]);

    assert!(analysis.catalogued_board_match.is_none());
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

fn matching_ids(matched: &crate::openers::RoundCataloguedBoardMatch) -> Vec<&str> {
    matched
        .matching_openers
        .iter()
        .map(|opener| opener.id.as_str())
        .collect()
}

fn matching_opener<'a>(
    matched: &'a crate::openers::RoundCataloguedBoardMatch,
    id: &str,
) -> &'a crate::openers::MatchingOpener {
    match matched
        .matching_openers
        .iter()
        .find(|opener| opener.id == id)
    {
        Some(opener) => opener,
        None => panic!("catalogued match should include {id}"),
    }
}

fn observation(mask: u16, letters: &str) -> OpenerObservation {
    OpenerObservation {
        post_board: Some(vec![mask]),
        post_gmask: Some(vec![0]),
        post_letters: Some(vec![letters.to_owned()]),
    }
}

fn install_catalog(catalog: &[u8]) {
    if let Err(error) = set_opener_catalog(catalog) {
        panic!("test catalog should install: {error}");
    }
}

fn install_witnesses(catalog: &[u8], rows: &str, ordinal: u32) {
    let catalog_sha256 = format!("{:x}", Sha256::digest(catalog));
    let witnesses = format!(
        r#"{{
            "schemaVersion":1,
            "openerAssetSha256":"{catalog_sha256}",
            "enrichmentSha256":"{hash}",
            "provenanceSha256":"{hash}",
            "summary":{{"witnesses":1,"runtimeTargets":1,"viewerOnly":0}},
            "witnesses":[{{
                "recordId":"alpha",
                "recordName":"Alpha",
                "searchShapeIndex":0,
                "candidateId":"search-shape:alpha:1",
                "candidateDigest":"{hash}",
                "resultsSha256":"{hash}",
                "lane":"sourceTree",
                "proofMethod":"sourceTreeStrictReplay",
                "targetFrame":"postClear",
                "queueScope":"freshRound",
                "targetRows":["{rows}"],
                "startRows":[],
                "steps":[],
                "runtimeTarget":{{"lockedPieceOrdinal":{ordinal},"rows":["{rows}"]}}
            }}]
        }}"#,
        hash = "a".repeat(64)
    );
    if let Err(error) = set_search_shape_witnesses(witnesses.as_bytes()) {
        panic!("test witness companion should install: {error}");
    }
}

const CATALOG_WITH_CONTINUATION: &[u8] = br#"
{
  "formatVersion": 2,
  "openers": [
    {"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]},
      {"id":2,"parent":1,"pieces":3,"rows":["__XXXXXXXX"],"routeName":"Alpha Route"}
    ]},
    {"id":"beta","aliases":{"en":"Beta"},"shapeKey":"beta","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]}
    ]},
    {"id":"charlie","aliases":{"en":"Charlie"},"shapeKey":"charlie","tree":[
      {"id":3,"parent":null,"pieces":3,"rows":["__XXXXXXXX"],"routeName":"Charlie Route"}
    ]}
  ]
}
"#;

const CATALOG_WITH_TERMINAL_EARLY_MATCH: &[u8] = br#"
{
  "formatVersion": 2,
  "openers": [
    {"id":"early","aliases":{"en":"Early"},"shapeKey":"early","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["LLLL______"]}
    ]},
    {"id":"late","aliases":{"en":"Late"},"shapeKey":"late","tree":[
      {"id":2,"parent":null,"pieces":2,"rows":["________JJ"]}
    ]}
  ]
}
"#;

const CATALOG_WITH_TIE: &[u8] = br#"
{
  "formatVersion": 2,
  "openers": [
    {"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]},
      {"id":2,"parent":1,"pieces":2,"rows":["__XXXXXXXX"],"routeName":"Alpha Route"}
    ]},
    {"id":"beta","aliases":{"en":"Beta"},"shapeKey":"beta","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]},
      {"id":3,"parent":1,"pieces":2,"rows":["XXXXXXXX__"],"routeName":"Beta Route"}
    ]}
  ]
}
"#;

const CATALOG_WITH_EMPTY_CONTINUATION: &[u8] = br#"
{
  "formatVersion": 2,
  "openers": [
    {"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]},
      {"id":2,"parent":1,"pieces":2,"rows":[],"routeName":"Alpha Empty"}
    ]},
    {"id":"beta","aliases":{"en":"Beta"},"shapeKey":"beta","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]}
    ]}
  ]
}
"#;

const CATALOG_WITH_AMBIGUOUS_PATHS: &[u8] = br#"
{
  "formatVersion": 2,
  "openers": [
    {"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["XXXX______"]},
      {"id":2,"parent":1,"pieces":2,"rows":["XXXXXXXXXX"],"routeName":"Route One"},
      {"id":3,"parent":1,"pieces":2,"rows":["XXXXXXXXXX"],"routeName":"Route Two"}
    ]}
  ]
}
"#;

const CATALOG_WITH_DETACHED_TARGET: &[u8] = br#"
{
  "formatVersion": 2,
  "openers": [
    {"id":"alpha","aliases":{"en":"Alpha"},"shapeKey":"alpha","searchShapes":[["XXXX______"]],"tree":[
      {"id":1,"parent":null,"pieces":1,"rows":["OO________"]}
    ]}
  ]
}
"#;
