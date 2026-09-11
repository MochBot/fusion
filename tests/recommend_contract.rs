// Shared wire fixtures and native replay boundary checks.
use fusion_engine::recommend::{
    parse_recommend_candidate, parse_recommend_outcome, reconstruct_native_hold_flags,
    NativeEnvelope, RecommendCompletion, RecommendInput,
};

use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/recommend-v1.json");

#[test]
fn native_chain_counters_use_the_replay_convention() {
    use fusion_engine::recommend::{recommend_native, RecommendOutcome, RecommendRequest};
    let mut rows = vec![0; 40];
    rows[..4].fill(1022);
    let request: RecommendRequest = serde_json::from_value(serde_json::json!({
        "version":1,"source":"native-search","config":"full-search",
        "input":{"kind":"fullPosition","holdSupported":true,"queueExtension7bag":false},
        "situation":{"boardRows":rows,"gmask":vec![0;40],"pieces":[0],"hold":null,
            "b2b":-1,"combo":-1,"pending":0,"mult":1,"beamWidth":20}
    }))
    .expect("request");
    let RecommendOutcome::Ready { candidate } = recommend_native(&request) else {
        panic!("quad available");
    };
    assert_eq!(candidate.assumptions.seed_b2b, -1);
    assert_eq!(candidate.assumptions.seed_combo, -1);
    assert_eq!(candidate.mechanics[0].lines, Some(4));
    assert_eq!(candidate.mechanics[0].b2b_before, Some(-1));
    assert_eq!(candidate.mechanics[0].b2b_after, Some(0));
    assert_eq!(candidate.mechanics[0].combo_before, Some(-1));
    assert_eq!(candidate.mechanics[0].combo_after, Some(0));
    assert_eq!(candidate.surge_total, Some(0.0));
}

#[test]
fn native_request_seeds_match_internal_chain_counters() {
    use fusion_engine::board::Board;
    use fusion_engine::eval::EvalWeights;
    use fusion_engine::header::Piece;
    use fusion_engine::recommend::{recommend_native, RecommendOutcome, RecommendRequest};
    use fusion_engine::search::{search, SearchConfig, SearchRequest};
    use fusion_engine::state::GameState;
    let mut state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
    state.b2b = 5;
    state.combo = 3;
    let config = SearchConfig {
        beam_width: 20,
        depth: 3,
        extend_queue_7bag: false,
        ..SearchConfig::default()
    };
    let weights = EvalWeights::default();
    let expected = search(
        &state,
        &SearchRequest {
            config: &config,
            weights: &weights,
            runtime: None,
            forced_root_move: None,
        },
    )
    .expect("reference search");
    let request: RecommendRequest = serde_json::from_value(serde_json::json!({
        "version":1,"source":"native-search","config":"full-search",
        "input":{"kind":"fullPosition","holdSupported":true,"queueExtension7bag":false},
        "situation":{"boardRows":vec![0;40],"gmask":vec![0;40],"pieces":[2,0,1],"hold":null,
            "b2b":4,"combo":2,"pending":0,"mult":1,"beamWidth":20}
    }))
    .expect("request");
    let RecommendOutcome::Ready { candidate } = recommend_native(&request) else {
        panic!("recommendation");
    };
    assert_eq!(candidate.scores.native_composite, Some(expected.best.score));
}

#[test]
fn shared_requests_execute_through_the_native_json_boundary() {
    use fusion_engine::recommend::{RecommendInput, RecommendOutcome, RecommendRequest};
    let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture JSON");
    for case in fixture["requests"]
        .as_array()
        .expect("shared request cases")
    {
        let json = fusion_engine::recommend::recommend_json(&case["request"].to_string());
        let outcome: Value = serde_json::from_str(&json).expect("native JSON output");
        assert_eq!(
            outcome["status"], case["expected"]["status"],
            "{}",
            case["name"]
        );
        assert_eq!(
            outcome.get("reason"),
            case["expected"].get("reason"),
            "{}",
            case["name"]
        );
        let parsed =
            parse_recommend_outcome(&outcome).expect("native output satisfies the shared contract");
        // Ready outcomes must carry observable candidate semantics, not only
        // a ready status: a real path with matching mechanics, totals that
        // add up, and assumptions that reflect the request.
        if let RecommendOutcome::Ready { candidate } = &parsed {
            let request: RecommendRequest =
                serde_json::from_value(case["request"].clone()).expect("fixture request parses");
            let RecommendInput::FullPosition {
                hold_supported,
                queue_extension_7bag,
            } = request.input
            else {
                panic!(
                    "shared executable requests are full positions: {}",
                    case["name"]
                );
            };
            assert!(!candidate.path.is_empty(), "{}", case["name"]);
            assert_eq!(
                candidate.path.len(),
                candidate.mechanics.len(),
                "{}",
                case["name"]
            );
            assert_eq!(
                candidate.requested_plies,
                request.situation.pieces.len(),
                "horizon comes from the supplied pieces: {}",
                case["name"]
            );
            assert_eq!(
                candidate.completed_plies,
                candidate.path.len(),
                "{}",
                case["name"]
            );
            assert_eq!(
                candidate.assumptions.hold_supported, hold_supported,
                "{}",
                case["name"]
            );
            assert_eq!(
                candidate.assumptions.queue_extension_7bag, queue_extension_7bag,
                "{}",
                case["name"]
            );
            assert_eq!(
                candidate.assumptions.seed_b2b, request.situation.b2b,
                "{}",
                case["name"]
            );
            assert_eq!(
                candidate.assumptions.seed_combo, request.situation.combo,
                "{}",
                case["name"]
            );
            for step in &candidate.mechanics {
                assert!(
                    step.raw_attack.is_some() && step.lines.is_some(),
                    "executed steps carry real mechanics: {}",
                    case["name"]
                );
            }
            assert_eq!(
                candidate.shaped_value,
                candidate
                    .raw_total
                    .zip(candidate.surge_total)
                    .map(|(a, b)| a + b),
                "{}",
                case["name"]
            );
            assert!(
                candidate.scores.native_composite.is_some(),
                "native search reports its composite: {}",
                case["name"]
            );
        }
    }
}

#[test]
fn shared_hold_request_plays_the_held_piece_through_the_json_boundary() {
    use fusion_engine::recommend::RecommendOutcome;
    let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture JSON");
    let case = fixture["requests"]
        .as_array()
        .expect("shared request cases")
        .iter()
        .find(|case| case["name"] == "native hold action plays the held piece with real mechanics")
        .expect("fixture must carry the shared hold request");
    let json = fusion_engine::recommend::recommend_json(&case["request"].to_string());
    let outcome: Value = serde_json::from_str(&json).expect("native JSON output");
    let parsed =
        parse_recommend_outcome(&outcome).expect("native output satisfies the shared contract");
    let RecommendOutcome::Ready { candidate } = &parsed else {
        panic!("hold request must execute");
    };
    // L (external 6) is the preset hold: it is neither the current piece nor
    // queued, so it can only enter the path through a real hold action.
    let held = candidate
        .path
        .iter()
        .zip(candidate.mechanics.iter())
        .find(|(placement, _)| placement.piece == 6 && placement.hold_used)
        .expect("held L must be played with hold_used");
    assert_eq!(held.0.piece, 6);
    assert!(held.0.hold_used);
    assert!(
        held.1.raw_attack.is_some() && held.1.lines.is_some(),
        "the hold step carries replayed mechanics"
    );
    assert_eq!(candidate.completion, RecommendCompletion::Complete);
    assert_eq!(candidate.requested_plies, 4);
    assert_eq!(candidate.completed_plies, 4);
}

#[test]
fn native_request_returns_a_real_continuation_and_mechanics() {
    use fusion_engine::recommend::{recommend_native, RecommendOutcome, RecommendRequest};
    let request: RecommendRequest = serde_json::from_value(serde_json::json!({
        "version": 1, "source": "native-search", "config": "full-search",
        "input": {"kind":"fullPosition", "holdSupported":true, "queueExtension7bag":false},
        "situation": {"boardRows":vec![0;40], "gmask":vec![0;40], "pieces":[2,0,1,3],
            "hold":null, "b2b":0, "combo":0, "pending":0, "mult":1, "beamWidth":20}
    }))
    .expect("canonical native request");
    let RecommendOutcome::Ready { candidate } = recommend_native(&request) else {
        panic!("native search should produce a continuation");
    };
    assert!(candidate.path.len() > 1);
    assert_eq!(candidate.path.len(), candidate.mechanics.len());
    assert!(candidate.final_rows.is_some());
    assert_eq!(
        candidate.shaped_value,
        candidate
            .raw_total
            .zip(candidate.surge_total)
            .map(|(a, b)| a + b)
    );
    assert!(candidate.scores.native_composite.is_some());
    assert!(!candidate.assumptions.queue_extension_7bag);
}

#[test]
fn request_input_round_trips_browser_field_names() {
    for input in [
        serde_json::json!({
            "kind": "fullPosition",
            "holdSupported": true,
            "queueExtension7bag": false
        }),
        serde_json::json!({
            "kind": "fixedSequence",
            "extendBeyondSupplied": false,
            "holdSupported": false
        }),
    ] {
        let parsed: RecommendInput =
            serde_json::from_value(input.clone()).expect("browser input must deserialize");
        assert_eq!(
            serde_json::to_value(parsed).expect("serialize input"),
            input
        );
    }
}

#[test]
fn final_placement_does_not_require_a_following_piece() {
    use fusion_engine::header::{Move, Piece, Rotation};

    let envelope = NativeEnvelope {
        current: Piece::T,
        hold: None,
        queue: &[],
    };
    let placement = Move::new(Piece::T, Rotation::North, 4, 0, false);

    assert_eq!(
        reconstruct_native_hold_flags(&envelope, &[placement]),
        Ok(vec![false])
    );
}

#[test]
fn native_normalization_preserves_the_known_root_hold_action() {
    use fusion_engine::board::Board;
    use fusion_engine::eval::EvalWeights;
    use fusion_engine::header::Piece;
    use fusion_engine::recommend::{normalize_search_result, NativeNormalizeOptions};
    use fusion_engine::search::{search, SearchConfig, SearchRequest};
    use fusion_engine::state::GameState;
    let mut state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
    state.hold = Some(Piece::T);
    let config = SearchConfig {
        beam_width: 20,
        depth: 1,
        extend_queue_7bag: false,
        ..SearchConfig::default()
    };
    let weights = EvalWeights::default();
    let mut result = search(
        &state,
        &SearchRequest {
            config: &config,
            weights: &weights,
            runtime: None,
            forced_root_move: None,
        },
    )
    .expect("search result");
    result.best.hold_used = true;
    let candidate = normalize_search_result(
        &state,
        &result,
        &NativeNormalizeOptions {
            requested_plies: 1,
            queue_extension_7bag: false,
            garbage_multiplier: 1.0,
        },
    )
    .expect("normalize legal same-piece hold");
    assert!(candidate.path[0].hold_used);
}

#[test]
fn recommend_fixtures_parse_and_round_trip() {
    let value: serde_json::Value =
        serde_json::from_str(FIXTURE).expect("recommend-v1.json must be valid JSON");
    let cases = value
        .get("cases")
        .and_then(|c| c.as_array())
        .expect("fixture must carry a cases array");
    assert!(cases.len() >= 3, "need ready + unavailable coverage");
    for case in cases {
        let outcome = case
            .get("outcome")
            .expect("each case must carry an outcome");
        let parsed = parse_recommend_outcome(outcome).expect("outcome must parse");
        let reserialized = serde_json::to_value(&parsed).expect("outcome must serialize");
        let reparsed = parse_recommend_outcome(&reserialized).expect("outcome must re-parse");
        assert_eq!(
            serde_json::to_value(&reparsed).expect("re-serialize"),
            reserialized,
            "outcome round-trip must be stable"
        );
    }
}

#[test]
fn recommend_failed_outcome_stays_distinct_from_unavailable() {
    use fusion_engine::recommend::RecommendOutcome;
    let value: serde_json::Value =
        serde_json::from_str(FIXTURE).expect("recommend-v1.json must be valid JSON");
    let cases = value
        .get("cases")
        .and_then(|c| c.as_array())
        .expect("fixture must carry a cases array");
    let failed = cases
        .iter()
        .find(|c| {
            c.get("name")
                == Some(&serde_json::json!(
                    "execution failure stays distinct from unavailable"
                ))
        })
        .expect("fixture must include a failed outcome");
    assert!(
        matches!(
            parse_recommend_outcome(&failed["outcome"]).expect("failed outcome must parse"),
            RecommendOutcome::Failed { .. }
        ),
        "execution failure is neither ready nor unavailable"
    );
}

#[test]
fn recommend_ready_candidate_preserves_partial_completion_and_unavailable_facts() {
    let value: serde_json::Value =
        serde_json::from_str(FIXTURE).expect("recommend-v1.json must be valid JSON");
    let cases = value.get("cases").and_then(|c| c.as_array()).unwrap();
    let partial = cases
        .iter()
        .find(|c| {
            c.get("outcome")
                .and_then(|o| o.get("candidate"))
                .and_then(|cand| cand.get("completion"))
                .and_then(|c| c.as_str())
                == Some("partial")
        })
        .expect("fixture must include a partial-completion candidate");
    let candidate = parse_recommend_candidate(&partial["outcome"]["candidate"]).unwrap();
    assert_eq!(candidate.completion, RecommendCompletion::Partial);
    assert!(
        candidate.final_gmask.is_none(),
        "unavailable gmask must stay absent, never a zero array"
    );
}
