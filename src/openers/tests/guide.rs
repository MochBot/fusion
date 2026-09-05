use fusion_engine::openers::catalog::OpenerCatalog;
use fusion_engine::openers::guide::{GuideBasis, GUIDE_VARIATION_LIMIT};
use fusion_engine::openers::{analyze_opener_round, OpenerObservation, OpenerRoundInput};

use super::perf::{install_live_catalog, load_rounds};

const GUIDE_CATALOG: &str = r#"{
  "formatVersion": 2,
  "openers": [
    {
      "id": "alpha",
      "aliases": {"en": "Alpha", "jp": "アルファ", "alt": ["Alpha Prime"]},
      "shapeKey": "alpha",
      "dependencies": "L>S (J>Z for mirror)",
      "cover": {"pct": 42.5, "covered": 2142, "total": 5040},
      "pcChance": "Rare",
      "tags": ["1st Bag TSD"],
      "links": [{"label": "Hard Drop", "url": "https://example.test/alpha"}],
      "tree": [
        {"id": 1, "parent": null, "pieces": 1, "rows": ["LLLL______"], "annotations": ["1st bag"],
         "est": {"attack": 0, "cumAttack": 0, "b2b": -1, "combo": 0}},
        {"id": 2, "parent": 1, "pieces": 2, "rows": ["LLLL____ZZ"], "routeName": "Alpha Route",
         "preClearRows": ["LLLLSSSSZZ"], "clearRows": [0],
         "est": {"attack": 1, "cumAttack": 1, "b2b": 0, "combo": 0, "clear": "single"}},
        {"id": 3, "parent": 2, "pieces": 3, "rows": ["LLLL_JJJZZ"], "routeName": "Alpha J Top", "annotations": ["2nd bag"]},
        {"id": 4, "parent": 2, "pieces": 3, "rows": ["LLLLOOOOZZ"], "annotations": ["2nd bag", "alt"]},
        {"id": 5, "parent": 2, "pieces": 3, "rows": ["LLLLTTTTZZ"]},
        {"id": 6, "parent": 2, "pieces": 3, "rows": ["LLLLIIIIZZ"]},
        {"id": 7, "parent": 2, "pieces": 3, "rows": ["LLLLSSSSZZ"]},
        {"id": 8, "parent": 2, "pieces": 3, "rows": ["LLLLZZZZZZ"]},
        {"id": 9, "parent": 2, "pieces": 3, "rows": ["LLLLJJJJZZ"]}
      ]
    }
  ]
}"#;

const ORDINARY_CONTINUATION_CATALOG: &str = r#"{
  "formatVersion": 2,
  "openers": [{
    "id": "ordinary",
    "aliases": {"en": "Ordinary"},
    "shapeKey": "ordinary",
    "tree": [
      {"id": 1, "parent": null, "pieces": 1, "rows": ["LLLL______"]},
      {"id": 2, "parent": 1, "pieces": 2, "rows": ["LLLL____ZZ"]},
      {"id": 3, "parent": 2, "pieces": 3, "rows": ["LLLLJJJJZZ"], "routeName": "Ordinary Route"}
    ]
  }]
}"#;

const ANNOTATED_CONTINUATION_CATALOG: &str = r#"{
  "formatVersion": 2,
  "openers": [{
    "id": "payoff",
    "aliases": {"en": "Payoff"},
    "shapeKey": "payoff",
    "tree": [
      {"id": 1, "parent": null, "pieces": 1, "rows": ["LLLL______"]},
      {"id": 2, "parent": 1, "pieces": 2, "rows": ["LLLL____ZZ"]},
      {"id": 3, "parent": 2, "pieces": 3, "rows": ["LLLLJJJJZZ"], "annotations": ["Can freestyle from here"]}
    ]
  }]
}"#;

const PARTIAL_PHASE_CATALOG: &str = r#"{
  "formatVersion": 2,
  "openers": [{
    "id": "round-up",
    "aliases": {"en": "Round Up"},
    "shapeKey": "round-up",
    "tree": [
      {
        "id": 1,
        "parent": null,
        "pieces": 1,
        "rows": ["L_________", "L_________", "L_________", "L_________"],
        "placements": [{"letter": "L", "cells": [[0, 0], [0, 1], [0, 2], [0, 3]]}]
      },
      {
        "id": 2,
        "parent": 1,
        "pieces": 4,
        "rows": ["L___I_____", "L___I_____", "LSS_IOO___", "L_SSIOO___"],
        "routeName": "Arbitrary Phase",
        "placements": [
          {"letter": "S", "cells": [[2, 0], [3, 0], [1, 1], [2, 1]]},
          {"letter": "I", "cells": [[4, 0], [4, 1], [4, 2], [4, 3]]},
          {"letter": "O", "cells": [[5, 0], [6, 0], [5, 1], [6, 1]]}
        ]
      }
    ]
  }]
}"#;

fn install(catalog: &str) {
    if let Err(error) = fusion_engine::openers::set_opener_catalog(catalog.as_bytes()) {
        panic!("guide catalog should install: {error}");
    }
}

fn observation(mask: u16, letters: &str) -> OpenerObservation {
    OpenerObservation {
        post_board: Some(vec![mask]),
        post_gmask: Some(vec![0]),
        post_letters: Some(vec![letters.to_owned()]),
    }
}

fn floor_up_observation(rows: &[&str]) -> OpenerObservation {
    let letters = rows.iter().map(|row| (*row).to_owned()).collect::<Vec<_>>();
    let board = letters
        .iter()
        .map(|row| {
            row.bytes()
                .take(10)
                .enumerate()
                .filter(|(_, cell)| *cell != b'_')
                .fold(0u16, |mask, (x, _)| mask | (1 << x))
        })
        .collect::<Vec<_>>();
    OpenerObservation {
        post_gmask: Some(vec![0; board.len()]),
        post_board: Some(board),
        post_letters: Some(letters),
    }
}

fn analyze(
    observations: Vec<Option<OpenerObservation>>,
) -> fusion_engine::openers::OpenerRoundAnalysis {
    match analyze_opener_round(&OpenerRoundInput {
        observations,
        ..OpenerRoundInput::default()
    }) {
        Ok(analysis) => analysis,
        Err(error) => panic!("catalog is installed: {error}"),
    }
}

#[test]
fn confirmed_guide_walks_the_path_and_lists_children_as_variations() {
    let _scope = crate::openers::isolated_catalog_test();
    install(GUIDE_CATALOG);

    let analysis = analyze(vec![
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
    ]);

    let guide = analysis
        .guide
        .expect("a confirmed record should carry a guide");
    assert_eq!(guide.basis, GuideBasis::Confirmed);
    assert_eq!(guide.record_id, "alpha");
    assert_eq!(guide.name, "Alpha");
    assert_eq!(guide.route_name.as_deref(), Some("Alpha Route"));
    assert_eq!(guide.aliases.jp.as_deref(), Some("アルファ"));
    assert_eq!(guide.aliases.alt, ["Alpha Prime"]);
    assert!(!guide.mirrored);

    assert_eq!(
        guide
            .phases
            .iter()
            .map(|phase| phase.node_id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(
        guide.phases[1].pre_clear_rows.as_deref(),
        Some(&["LLLLSSSSZZ".to_owned()][..])
    );
    assert_eq!(guide.phases[1].clear_rows, [0]);
    assert_eq!(guide.phases[1].rows, ["LLLL____ZZ"]);
    assert_eq!(
        guide.phases[1]
            .est
            .as_ref()
            .and_then(|est| est.clear.as_deref()),
        Some("single")
    );

    assert_eq!(
        guide.requirements.dependencies.as_deref(),
        Some("L>S (J>Z for mirror)")
    );
    assert_eq!(guide.requirements.cover_pct, Some(42.5));
    assert_eq!(guide.requirements.tags, ["1st Bag TSD"]);
    assert_eq!(guide.links.len(), 1);

    assert_eq!(guide.variations_total, 7);
    assert_eq!(guide.variations.len(), GUIDE_VARIATION_LIMIT);
    assert_eq!(guide.variations[0].node_id, 3);
    assert_eq!(
        guide.variations[0].route_name.as_deref(),
        Some("Alpha J Top")
    );
    assert_eq!(guide.variations[1].annotations, ["2nd bag", "alt"]);
    assert!(
        guide.deviation.is_none(),
        "an exact build has no divergence: {:?}",
        guide.deviation
    );
}

#[test]
fn mirrored_build_renders_mirrored_boards_and_promotes_the_mirror_clause() {
    let _scope = crate::openers::isolated_catalog_test();
    install(GUIDE_CATALOG);

    let analysis = analyze(vec![
        Some(observation(0b1111000000, "______JJJJ")),
        Some(observation(0b1111000011, "SS____JJJJ")),
    ]);

    let guide = analysis.guide.expect("the mirrored build should confirm");
    assert!(guide.mirrored);
    assert_eq!(guide.phases[1].rows, ["SS____JJJJ"]);
    assert_eq!(guide.requirements.dependencies.as_deref(), Some("J>Z"));
}

#[test]
fn leaf_anchor_lists_its_siblings_as_other_variations() {
    let _scope = crate::openers::isolated_catalog_test();
    install(GUIDE_CATALOG);

    let analysis = analyze(vec![
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
        Some(observation(0b1111101111, "LLLL_JJJZZ")),
    ]);

    let guide = analysis.guide.expect("the deeper board should confirm");
    assert_eq!(guide.phases.last().map(|phase| phase.node_id), Some(3));
    assert_eq!(guide.route_name.as_deref(), Some("Alpha J Top"));
    assert!(guide
        .variations
        .iter()
        .all(|variation| variation.node_id != 3));
    assert_eq!(guide.variations_total, 6);
}

#[test]
fn confirmed_guide_compares_the_intended_continuation_at_its_ordinal() {
    let _scope = crate::openers::isolated_catalog_test();
    install(GUIDE_CATALOG);

    let analysis = analyze(vec![
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
        Some(observation(0b1110111111, "LLLLTT_TZZ")),
        Some(observation(0b1111111111, "LLLLTTITZZ")),
    ]);

    let guide = analysis
        .guide
        .expect("the confirmed build should carry a guide");
    assert_eq!(guide.basis, GuideBasis::Confirmed);
    assert_eq!(guide.phases.last().map(|phase| phase.node_id), Some(2));
    let deviation = guide
        .deviation
        .expect("an off-route continuation should be reported");
    assert_eq!(deviation.divergence_lock, 2);
    assert_eq!(
        deviation.lock_index, 2,
        "compared at the continuation's ordinal"
    );
    assert_eq!(
        deviation.target_node_id,
        Some(5),
        "the continuation that best overlaps the player"
    );
    assert_eq!(deviation.player_rows, ["LLLLTT_TZZ"]);
    assert_eq!(deviation.target_rows, ["LLLLTTTTZZ"]);
    assert_eq!(
        (deviation.missing, deviation.stray, deviation.wrong_letter),
        (1, 0, 0)
    );
}

#[test]
fn an_incomplete_non_payoff_continuation_still_reports_deviation() {
    let _scope = crate::openers::isolated_catalog_test();
    install(ORDINARY_CONTINUATION_CATALOG);

    let analysis = analyze(vec![
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
    ]);

    let deviation = analysis
        .guide
        .and_then(|guide| guide.deviation)
        .expect("an incomplete ordinary continuation must remain a deviation");
    assert_eq!(deviation.target_node_id, Some(3));
    assert_eq!(
        (deviation.missing, deviation.stray, deviation.wrong_letter),
        (4, 0, 0)
    );
}

#[test]
fn annotation_alone_does_not_hide_an_incomplete_continuation() {
    let _scope = crate::openers::isolated_catalog_test();
    install(ANNOTATED_CONTINUATION_CATALOG);

    let analysis = analyze(vec![
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
    ]);

    assert!(
        analysis.guide.and_then(|guide| guide.deviation).is_some(),
        "completion requires an exact pre-payoff setup, not an annotation and cell count"
    );
}

#[test]
fn one_matching_piece_rounds_up_to_the_full_arbitrary_phase() {
    let _scope = crate::openers::isolated_catalog_test();
    install(PARTIAL_PHASE_CATALOG);

    let guide = analyze(vec![
        Some(floor_up_observation(&[
            "L_________",
            "L_________",
            "L_________",
            "L_________",
        ])),
        Some(floor_up_observation(&[
            "L_SS______",
            "LSS_______",
            "L_________",
            "L_________",
        ])),
        Some(floor_up_observation(&[
            "L_SS____OO",
            "LSS_____OO",
            "L_________",
            "L_________",
        ])),
    ])
    .guide
    .expect("the confirmed opener should carry a guide");

    assert_eq!(guide.record_id, "round-up");
    assert_eq!(
        guide
            .phases
            .iter()
            .map(|phase| (phase.node_id, phase.pieces))
            .collect::<Vec<_>>(),
        [(1, 1), (2, 4)]
    );
    let rounded = guide.phases.last().expect("the full phase should be shown");
    assert_eq!(
        rounded.rows,
        ["L_SSIOO___", "LSS_IOO___", "L___I_____", "L___I_____"]
    );
    assert_eq!(guide.route_name.as_deref(), Some("Arbitrary Phase"));
    assert!(guide.deviation.is_none());
}

#[test]
fn one_matching_piece_rounds_up_in_a_mirrored_phase() {
    let _scope = crate::openers::isolated_catalog_test();
    install(PARTIAL_PHASE_CATALOG);

    let guide = analyze(vec![
        Some(floor_up_observation(&[
            "_________J",
            "_________J",
            "_________J",
            "_________J",
        ])),
        Some(floor_up_observation(&[
            "______ZZ_J",
            "_______ZZJ",
            "_________J",
            "_________J",
        ])),
        Some(floor_up_observation(&[
            "OO____ZZ_J",
            "OO_____ZZJ",
            "_________J",
            "_________J",
        ])),
    ])
    .guide
    .expect("the mirrored confirmed opener should carry a guide");

    assert!(guide.mirrored);
    assert_eq!(
        guide
            .phases
            .iter()
            .map(|phase| (phase.node_id, phase.pieces))
            .collect::<Vec<_>>(),
        [(1, 1), (2, 4)]
    );
    let rounded = guide.phases.last().expect("the full phase should be shown");
    assert_eq!(
        rounded.rows,
        ["___OOIZZ_J", "___OOI_ZZJ", "_____I___J", "_____I___J"]
    );
    assert!(guide.deviation.is_none());
}

#[test]
fn finished_opener_without_a_continuation_has_no_deviation() {
    let _scope = crate::openers::isolated_catalog_test();
    install(GUIDE_CATALOG);

    let analysis = analyze(vec![
        Some(observation(0b0000001111, "LLLL______")),
        Some(observation(0b1100001111, "LLLL____ZZ")),
        Some(observation(0b1111101111, "LLLL_JJJZZ")),
        Some(observation(0b1111111111, "LLLLIJJJZZ")),
    ]);

    let guide = analysis.guide.expect("the deeper board should confirm");
    assert_eq!(guide.phases.last().map(|phase| phase.node_id), Some(3));
    assert!(guide.deviation.is_none());
}

/// The piece-6 board is the family's shared pre-T state, authored only by the
/// terminal one-node record `tetrismaps-205`. Placing the T settles the branch,
/// so the mid-bag hit must not name that record or end the walk there.
#[test]
fn a_mid_bag_hit_does_not_name_the_round_over_the_deeper_stickspin_board() {
    let (_scope, _detached) = install_live_catalog();

    let mirrored_rows = |rows: &[&str]| -> Vec<String> {
        rows.iter()
            .rev()
            .map(|row| {
                row.chars()
                    .rev()
                    .map(|cell| match cell {
                        'L' => 'J',
                        'J' => 'L',
                        'S' => 'Z',
                        'Z' => 'S',
                        other => other,
                    })
                    .collect()
            })
            .collect()
    };
    let observation_for = |rows: &[&str]| {
        let letters = mirrored_rows(rows);
        let board = letters
            .iter()
            .map(|row| {
                row.chars()
                    .take(10)
                    .enumerate()
                    .filter(|(_, cell)| *cell != '_')
                    .fold(0u16, |mask, (x, _)| mask | (1 << x))
            })
            .collect::<Vec<_>>();
        Some(OpenerObservation {
            post_gmask: Some(vec![0; board.len()]),
            post_board: Some(board),
            post_letters: Some(letters),
        })
    };

    let analysis = analyze(vec![
        None,
        None,
        None,
        None,
        None,
        observation_for(&[
            "_Z________",
            "ZZ_______I",
            "Z______LLI",
            "OO_SSJJJLI",
            "OOSS___JLI",
        ]),
        observation_for(&["________S_", "I______TSS", "IJJ____TTS", "IJL___ZZOO"]),
    ]);

    let matched = analysis
        .catalogued_board_match
        .as_ref()
        .expect("the deeper stickspin board is catalogued");
    assert_eq!(matched.anchor_index, 6);

    let named = matched
        .matching_openers
        .iter()
        .map(|opener| opener.id.as_str())
        .collect::<Vec<_>>();
    assert!(
        !named.contains(&"tetrismaps-205"),
        "the terminal mid-bag record must not survive as the identity: {named:?}"
    );
    assert!(
        named.contains(&"stickspin") || named.contains(&"single-double-pc"),
        "the stickspin family should own the settled board: {named:?}"
    );
}

/// The SDPC-Spin branch is authored at bag boundaries (14, 21, 27, 28) and owns
/// no node at 15..20, so following it requires crossing an ordinal gap. Parking
/// at 14 leaves twelve indistinguishable branches and names the umbrella record.
#[test]
fn the_walk_crosses_a_bag_gap_to_the_deeper_sdpc_spin_node() {
    let (_scope, catalog) = install_live_catalog();
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == "single-double-pc")
        .expect("the sdpc record ships in the live catalog");
    let node_rows = |node_id: u32| {
        record
            .tree
            .iter()
            .find(|node| node.id == node_id)
            .map(|node| (node.pieces, node.rows.clone()))
            .expect("node ships in the live catalog")
    };

    let (spin_pieces, spin_rows) = node_rows(29);
    let (deeper_pieces, deeper_rows) = node_rows(30);
    assert_eq!((spin_pieces, deeper_pieces), (14, 21));

    let observation_for = |rows: &[String]| {
        let letters = rows.iter().rev().cloned().collect::<Vec<_>>();
        let board = letters
            .iter()
            .map(|row| {
                row.chars()
                    .take(10)
                    .enumerate()
                    .filter(|(_, cell)| *cell != '_')
                    .fold(0u16, |mask, (x, _)| mask | (1 << x))
            })
            .collect::<Vec<_>>();
        Some(OpenerObservation {
            post_gmask: Some(vec![0; board.len()]),
            post_board: Some(board),
            post_letters: Some(letters),
        })
    };

    let mut observations = vec![None; deeper_pieces as usize];
    observations[spin_pieces as usize - 1] = observation_for(&spin_rows);
    observations[deeper_pieces as usize - 1] = observation_for(&deeper_rows);

    let analysis = analyze(observations);
    let matched = analysis
        .catalogued_board_match
        .as_ref()
        .expect("both boards are catalogued");

    assert_eq!(
        matched.anchor_index,
        deeper_pieces as usize - 1,
        "the walk must reach the bag-3 node instead of parking on the bag-2 tie"
    );
    let opener = matched
        .matching_openers
        .iter()
        .find(|opener| opener.id == "single-double-pc")
        .expect("the sdpc record owns both boards");
    assert_eq!(
        opener.route_name.as_deref(),
        Some("SDPC Spin"),
        "the deeper node must name the variation, not the umbrella record"
    );
}

#[test]
fn partial_sdpc_spin_phase_rounds_up_to_full_bag_three() {
    let (_scope, _catalog) = install_live_catalog();
    let setup_rows = [
        "IJL_ZZZZOO",
        "JJ_LLJSSSI",
        "IJ__LJSOOI",
        "IJ_LLJJOOI",
        "IZ___OOSSI",
        "IZZ__OO_SS",
        "__Z_______",
    ];
    let mut observations = vec![None; 21];
    observations[13] = Some(floor_up_observation(&[
        "IJL_ZZZZOO",
        "JJ_L__SSSI",
        "_J____SOOI",
        "_J_____OOI",
        "_________I",
    ]));
    observations[19] = Some(floor_up_observation(&setup_rows));
    observations[20] = Some(floor_up_observation(&[
        "IJL_ZZZZOO",
        "JJ_LLJSSSI",
        "IJ__LJSOOI",
        "IJ_LLJJOOI",
        "IZ___OOSSI",
        "IZZ__OOZSS",
        "__Z____ZZ_",
        "________Z_",
    ]));

    let guide = analyze(observations)
        .guide
        .expect("the completed SDPC Spin setup should carry a guide");
    assert_eq!(guide.record_id, "single-double-pc");
    assert!(guide.mirrored);
    assert_eq!(
        guide
            .phases
            .iter()
            .map(|phase| phase.pieces)
            .collect::<Vec<_>>(),
        [7, 14, 21]
    );
    let setup = guide.phases.last().expect("bag three should be shown");
    assert_eq!(setup.node_id, 45);
    assert_eq!(
        setup.rows,
        ["IJL_ZZZZOO", "IZ___OOSSI", "IZZ__OO_SS", "__Z_______"]
    );
    assert_eq!(setup.clear_rows, [1, 2, 3]);
    assert_eq!(
        setup.est.as_ref().and_then(|est| est.clear.as_deref()),
        Some("T-spin triple")
    );
    assert!(
        guide.deviation.is_none(),
        "play after the completed setup is no longer part of the opener"
    );
}

/// `single-double-pc` lists "SDPC Spin" in `alt`, so an alias-wide redundancy
/// filter erases the route and the chip degrades to the umbrella record name.
#[test]
fn a_variation_route_survives_when_it_is_also_an_alt_alias() {
    let (_scope, catalog) = install_live_catalog();
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == "single-double-pc")
        .expect("the sdpc record ships in the live catalog");
    assert!(
        record
            .aliases
            .alt
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case("SDPC Spin")),
        "the fixture depends on the route also being an alt alias"
    );

    let node = record
        .tree
        .iter()
        .find(|node| node.id == 29)
        .expect("node 29 ships in the live catalog");
    let letters = node.rows.iter().rev().cloned().collect::<Vec<_>>();
    let board = letters
        .iter()
        .map(|row| {
            row.chars()
                .take(10)
                .enumerate()
                .filter(|(_, cell)| *cell != '_')
                .fold(0u16, |mask, (x, _)| mask | (1 << x))
        })
        .collect::<Vec<_>>();

    let mut observations = vec![None; node.pieces as usize];
    observations[node.pieces as usize - 1] = Some(OpenerObservation {
        post_gmask: Some(vec![0; board.len()]),
        post_board: Some(board),
        post_letters: Some(letters),
    });

    let guide = analyze(observations)
        .guide
        .expect("the catalogued board should produce a guide");
    assert_eq!(guide.record_id, "single-double-pc");
    assert_eq!(guide.route_name.as_deref(), Some("SDPC Spin"));
}

#[test]
fn every_live_corpus_guide_is_structurally_valid() {
    let (_scope, _detached) = install_live_catalog();
    let rounds = load_rounds();

    for round in &rounds {
        let analysis = analyze_opener_round(&round.input).expect("catalog is installed");
        let Some(guide) = &analysis.guide else {
            continue;
        };
        let mut urls = guide
            .links
            .iter()
            .map(|link| link.url.as_str())
            .collect::<Vec<_>>();
        let total = urls.len();
        urls.sort_unstable();
        urls.dedup();
        assert_eq!(
            urls.len(),
            total,
            "{} {}: duplicate source urls break the keyed source list",
            round.replay,
            round.round
        );
        assert!(
            !guide.phases.is_empty(),
            "{} {}: guide without phases",
            round.replay,
            round.round
        );
        assert!(
            guide.variations.len() <= GUIDE_VARIATION_LIMIT,
            "{} {}: {} variations exceeds the cap",
            round.replay,
            round.round,
            guide.variations.len()
        );
    }
}

#[test]
fn guide_catalog_parses_the_shipping_record_fields() {
    let catalog: OpenerCatalog = match serde_json::from_str(GUIDE_CATALOG) {
        Ok(catalog) => catalog,
        Err(error) => panic!("guide catalog should parse: {error}"),
    };
    let record = &catalog.openers[0];
    assert_eq!(record.links[0].label, "Hard Drop");
    assert_eq!(
        record.tree[1].est.as_ref().map(|est| est.cum_attack),
        Some(1)
    );
}

#[test]
#[ignore]
fn dump_live_corpus_guides() {
    let (_scope, _detached) = install_live_catalog();
    for round in load_rounds() {
        let analysis = analyze_opener_round(&round.input).expect("catalog is installed");
        let locks = round
            .input
            .observations
            .iter()
            .filter(|o| o.is_some())
            .count();
        match analysis.catalogued_board_match.as_ref() {
            Some(matched) => println!(
                "{} r{}: locks={locks} anchor={} first={} openers={:?}",
                round.replay,
                round.round,
                matched.anchor_index,
                matched.first_match_index,
                matched
                    .matching_openers
                    .iter()
                    .map(|opener| format!(
                        "{}#{:?}~{:?}",
                        opener.id, opener.candidate_node_ids, opener.route_name
                    ))
                    .collect::<Vec<_>>()
            ),
            None => println!(
                "{} r{}: locks={locks} no confirmation",
                round.replay, round.round
            ),
        }
        let Some(guide) = analysis.guide else {
            println!("{} r{}: -", round.replay, round.round);
            continue;
        };
        println!(
            "{} r{}: {:?} {} route={:?} phases={:?} vars={}/{} deps={:?} cover={:?} dev={}",
            round.replay,
            round.round,
            guide.basis,
            guide.record_id,
            guide.route_name,
            guide.phases.iter().map(|p| p.pieces).collect::<Vec<_>>(),
            guide.variations.len(),
            guide.variations_total,
            guide.requirements.dependencies,
            guide.requirements.cover_pct,
            guide
                .deviation
                .as_ref()
                .map_or("none".to_owned(), |d| format!(
                    "lock{} node{:?} miss{} stray{} wrong{}\n    P {:?}\n    T {:?}",
                    d.lock_index,
                    d.target_node_id,
                    d.missing,
                    d.stray,
                    d.wrong_letter,
                    d.player_rows,
                    d.target_rows
                ))
        );
    }
}
