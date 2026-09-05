mod openers {
    mod phase {
        use fusion_engine::openers::catalog::OpenerCatalog;
        use fusion_engine::openers::matcher::BoardMatch;
        use fusion_engine::openers::phase::{
            assess_opener_phase, OpenerAssessment, OpenerObservation,
        };
        use fusion_engine::openers::target::build_targets;
        use serde::Deserialize;

        #[test]
        fn keeps_letter_mismatched_small_endpoints_out_of_weak_phase_assessment() {
            let catalog: OpenerCatalog = match serde_json::from_str(
                r#"{"formatVersion":2,"openers":[{"id":"fixture","aliases":{"en":"Fixture"},"shapeKey":"fixture","tree":[{"id":1,"parent":null,"pieces":1,"rows":["IIII______"]}]}]}"#,
            ) {
                Ok(catalog) => catalog,
                Err(error) => panic!("fixture catalog should parse: {error}"),
            };
            let observations = [Some(OpenerObservation {
                post_board: Some(vec![0b0000001111]),
                post_gmask: Some(vec![0]),
                post_letters: Some(vec!["ZZZZ______".to_owned()]),
            })];

            let assessments = assess_opener_phase(&build_targets(&catalog), &observations);

            assert!(assessments[0]
                .as_ref()
                .is_some_and(|assessment| !assessment.on_script && assessment.r#match.is_none()));
        }

        #[test]
        fn keeps_assessing_the_first_fourteen_locks_after_an_early_miss() {
            let catalog: OpenerCatalog = match serde_json::from_str(
                r#"{"formatVersion":2,"openers":[{"id":"fixture","aliases":{"en":"Fixture"},"shapeKey":"fixture","tree":[{"id":1,"parent":null,"pieces":1,"rows":["IIII______"]},{"id":2,"parent":1,"pieces":2,"rows":["ZZZZZZZZ__"]}]}]}"#,
            ) {
                Ok(catalog) => catalog,
                Err(error) => panic!("phase catalog should parse: {error}"),
            };
            let observations = [
                Some(OpenerObservation {
                    post_board: Some(vec![0b0000000011]),
                    post_gmask: Some(vec![0]),
                    post_letters: None,
                }),
                Some(OpenerObservation {
                    post_board: Some(vec![0b0011111111]),
                    post_gmask: Some(vec![0]),
                    post_letters: Some(vec!["ZZZZZZZZ__".to_owned()]),
                }),
            ];

            let assessments = assess_opener_phase(&build_targets(&catalog), &observations);

            assert!(assessments[0]
                .as_ref()
                .is_some_and(|assessment| !assessment.on_script));
            assert!(assessments[1]
                .as_ref()
                .is_some_and(|assessment| assessment.on_script));
        }

        #[test]
        fn returns_none_when_observation_or_required_post_lock_data_is_missing() {
            let observations = [
                None,
                Some(OpenerObservation {
                    post_board: None,
                    post_gmask: Some(Vec::new()),
                    post_letters: None,
                }),
                Some(OpenerObservation {
                    post_board: Some(Vec::new()),
                    post_gmask: None,
                    post_letters: None,
                }),
            ];

            let assessments = assess_opener_phase(&[], &observations);

            assert_eq!(assessments, vec![None, None, None]);
        }

        #[test]
        fn assesses_recorded_rounds_when_the_expected_result_has_no_search_shape() {
            let catalog: OpenerCatalog = match serde_json::from_slice(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/openers/catalog-mini.json"
            ))) {
                Ok(catalog) => catalog,
                Err(error) => panic!("catalog fixture should parse: {error}"),
            };
            let fixture: RoundFixture = match serde_json::from_slice(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/openers/parity-rounds.json"
            ))) {
                Ok(fixture) => fixture,
                Err(error) => panic!("round fixture should parse: {error}"),
            };
            let targets = build_targets(&catalog);
            let mut checked = 0;

            for round in fixture.rounds {
                if uses_search_shape(&round.expected.assessments) {
                    continue;
                }
                let actual = assess_opener_phase(&targets, &round.input.observations);

                assert_assessments(&actual, &round.expected.assessments, &round.input.name);
                checked += 1;
            }

            assert!(checked > 0, "fixture should contain tree-only phase cases");
        }

        fn uses_search_shape(assessments: &[Option<OpenerAssessment>]) -> bool {
            assessments.iter().flatten().any(|assessment| {
                assessment
                    .r#match
                    .iter()
                    .chain(assessment.runners_up.iter())
                    .any(|matched| matched.node_id.is_none())
            })
        }

        fn assert_assessments(
            actual: &[Option<OpenerAssessment>],
            expected: &[Option<OpenerAssessment>],
            case_name: &str,
        ) {
            assert_eq!(actual.len(), expected.len(), "{case_name}");
            for (actual, expected) in actual.iter().zip(expected) {
                match (actual, expected) {
                    (None, None) => {}
                    (Some(actual), Some(expected)) => {
                        assert_eq!(actual.on_script, expected.on_script, "{case_name}");
                        assert_eq!(actual.board_cells, expected.board_cells, "{case_name}");
                        assert_eq!(
                            actual.r#match.as_ref().map(match_identity),
                            expected.r#match.as_ref().map(match_identity),
                            "{case_name}"
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
                                .collect::<Vec<_>>(),
                            "{case_name}"
                        );
                    }
                    _ => panic!("{case_name} assessment presence differs"),
                }
            }
        }

        fn match_identity(board_match: &BoardMatch) -> (&str, Option<u32>, bool, bool, u32, u32) {
            (
                &board_match.opener_id,
                board_match.node_id,
                board_match.mirrored,
                board_match.complete,
                board_match.overlap_cells,
                board_match.stray_cells,
            )
        }

        #[derive(Deserialize)]
        struct RoundFixture {
            rounds: Vec<RoundCase>,
        }

        #[derive(Deserialize)]
        struct RoundCase {
            input: RoundInput,
            expected: RoundExpected,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RoundInput {
            name: String,
            observations: Vec<Option<OpenerObservation>>,
        }

        #[derive(Deserialize)]
        struct RoundExpected {
            assessments: Vec<Option<OpenerAssessment>>,
        }
    }
}
