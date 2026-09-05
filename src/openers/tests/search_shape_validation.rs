use crate::openers::validate_search_shape_batch;

fn catalog_json(pre_clear: bool) -> String {
    let (root, child) = if pre_clear {
        (
            r#"{
                "id": 0,
                "parent": null,
                "pieces": 0,
                "preClearRows": ["XXXXXXXXXX"],
                "clearRows": [0],
                "rows": []
            }"#,
            r#"{
                "id": 1,
                "parent": 0,
                "pieces": 1,
                "preClearRows": ["OO________", "OO________", "XXXXXXXXXX"],
                "clearRows": [0],
                "rows": ["OO________", "OO________"],
                "placements": [{"letter": "O", "cells": [[0,1],[1,1],[0,2],[1,2]]}]
            }"#,
        )
    } else {
        (
            r#"{"id": 0, "parent": null, "pieces": 0, "rows": []}"#,
            r#"{
            "id": 1,
            "parent": 0,
            "pieces": 1,
            "rows": ["IIII______"],
            "placements": [{"letter": "I", "cells": [[0,0],[1,0],[2,0],[3,0]]}]
        }"#,
        )
    };
    format!(
        r#"{{
            "formatVersion": 2,
            "openers": [{{
                "id": "fixture-record",
                "aliases": {{"en": "Fixture"}},
                "shapeKey": "fixture",
                "tree": [
                    {root},
                    {child}
                ]
            }}]
        }}"#
    )
}

fn catalog_with_invalid_descendant() -> String {
    r#"{
        "formatVersion": 2,
        "openers": [{
            "id": "fixture-record",
            "aliases": {"en": "Fixture"},
            "shapeKey": "fixture",
            "tree": [
                {"id": 0, "parent": null, "pieces": 0, "rows": []},
                {
                    "id": 1,
                    "parent": 0,
                    "pieces": 1,
                    "rows": ["IIII______"],
                    "placements": [{"letter": "I", "cells": [[0,0],[1,0],[2,0],[3,0]]}]
                },
                {
                    "id": 2,
                    "parent": 1,
                    "pieces": 2,
                    "rows": ["IIII______"],
                    "placements": [{"letter": "I", "cells": [[0,0],[1,0],[2,0],[3,0]]}]
                }
            ]
        }]
    }"#
    .to_owned()
}

fn catalog_with_source_pre_clear() -> String {
    r#"{
        "formatVersion": 2,
        "openers": [{
            "id": "fixture-record",
            "aliases": {"en": "Fixture"},
            "shapeKey": "fixture",
            "tree": [
                {
                    "id": 1,
                    "parent": null,
                    "pieces": 1,
                    "rows": ["IIII______"],
                    "placements": [{"letter": "I", "cells": [[0,0],[1,0],[2,0],[3,0]]}]
                },
                {
                    "id": 2,
                    "parent": 1,
                    "pieces": 2,
                    "rows": ["____J_____", "IIIIJJJ___"],
                    "placements": [{"letter": "J", "cells": [[4,0],[5,0],[6,0],[4,1]]}]
                },
                {
                    "id": 3,
                    "parent": 2,
                    "pieces": 3,
                    "rows": ["____J___T_"],
                    "placements": [{"letter": "T", "cells": [[7,0],[8,0],[9,0],[8,1]]}]
                }
            ]
        }]
    }"#
    .to_owned()
}

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn batch_json(source_rows: &str, frame_evidence: &str, fuel: u32, digest: &str) -> String {
    format!(
        r#"{{
            "schemaVersion": 1,
            "runId": "search-shape-review-1",
            "inputAssetSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "searchFuel": {fuel},
            "candidates": [{{
                "candidateId": "candidate-1",
                "candidateDigest": "{digest}",
                "recordId": "fixture-record",
                "lockedPieceOrdinal": 1,
                "sourceRows": {source_rows},
                "frameEvidence": {frame_evidence}
            }}]
        }}"#
    )
}

fn pre_clear_evidence(rows: &str) -> String {
    format!(
        r#"[{{"nodeId":1,"frame":"preClear","mirrorRelation":"same","confirmationRows":{rows}}}]"#
    )
}

fn source_pre_clear_evidence(rows: &str, construction_rows: &str) -> String {
    format!(
        r#"[{{"nodeId":3,"frame":"sourcePreClear","mirrorRelation":"same","confirmationRows":{rows},"constructionRows":{construction_rows}}}]"#
    )
}

fn post_clear_evidence(rows: &str) -> String {
    format!(
        r#"[{{"nodeId":1,"frame":"postClear","mirrorRelation":"same","confirmationRows":{rows}}}]"#
    )
}

fn result_json(catalog: String, batch: String) -> serde_json::Value {
    let result = validate_search_shape_batch(catalog.as_bytes(), batch.as_bytes());
    match result {
        Ok(results) => match serde_json::to_value(results) {
            Ok(value) => value,
            Err(error) => panic!("validation result must serialize: {error}"),
        },
        Err(error) => panic!("fixture boundary must parse: {error}"),
    }
}

#[test]
fn reaches_the_exact_ordinal_with_a_replayable_witness() {
    // Given: a catalogued I-piece node and a proposed pre-clear candidate at lock 1.
    let result = result_json(
        catalog_json(false),
        batch_json(
            r#"["IIII______"]"#,
            &pre_clear_evidence(r#"["IIII______"]"#),
            32,
            DIGEST,
        ),
    );

    // When: the native batch seam validates the candidate.
    let candidate = &result["results"][0];

    // Then: the exact ordinal has an independently replayable positive witness.
    assert_eq!(candidate["status"], "witnessFound", "{candidate}");
    assert_eq!(result["runId"], "search-shape-review-1");
    assert_eq!(
        result["inputAssetSha256"],
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    assert_eq!(candidate["proofMethod"], "sourceTreeStrictReplay");
    assert_eq!(candidate["witness"]["locks"][0]["ordinal"], 1);
    assert_eq!(candidate["witness"]["locks"][0]["piece"], "I");
    assert_eq!(candidate["witness"]["locks"][0]["currentBefore"], "I");
    assert_eq!(
        candidate["witness"]["locks"][0]["holdBefore"],
        serde_json::Value::Null
    );
    assert_eq!(candidate["witness"]["locks"][0]["holdUsed"], false);
    assert_eq!(
        candidate["witness"]["locks"][0]["holdAfter"],
        serde_json::Value::Null
    );
    assert_eq!(
        candidate["witness"]["locks"][0]["drawsBeforeLock"],
        serde_json::json!([{
            "drawOrdinal": 1,
            "bagOrdinal": 1,
            "slotInBag": 1,
            "piece": "I"
        }])
    );
    assert_eq!(
        candidate["witness"]["locks"][0]["placement"]["cells"],
        serde_json::json!([[1, 0], [0, 0], [2, 0], [3, 0]])
    );
    assert_eq!(
        candidate["witness"]["confirmationRows"],
        serde_json::json!(["XXXX______"])
    );
}

#[test]
fn does_not_treat_the_same_board_at_a_different_ordinal_as_reachable() {
    // Given: the identical board proposed at ordinal 2 instead of its catalogued ordinal 1.
    let result = result_json(
        catalog_json(false),
        batch_json(
            r#"["IIII______"]"#,
            r#"[{"nodeId":0,"frame":"preClear","mirrorRelation":"same","confirmationRows":[]}]"#,
            32,
            DIGEST,
        ),
    );

    // When: the batch seam validates it.
    let candidate = &result["results"][0];

    // Then: occupied-board equality cannot override the evidence ordinal contract.
    assert_eq!(candidate["status"], "invalidInput");
}

#[test]
fn confirms_the_physical_post_clear_rows_from_a_pre_clear_candidate() {
    // Given: a source pre-clear row that becomes a physical residue after the final T lock.
    let result = result_json(
        catalog_with_source_pre_clear(),
        batch_json(
            r#"["____X___X_", "XXXXXXXXXX"]"#,
            &source_pre_clear_evidence(r#"["____J___T_"]"#, r#"["____J___T_", "IIIIJJJTTT"]"#),
            20_000,
            DIGEST,
        )
        .replace(r#""lockedPieceOrdinal": 1"#, r#""lockedPieceOrdinal": 3"#),
    );

    // When: the batch seam replays the legal transition.
    let candidate = &result["results"][0];

    // Then: the witness reports the distinct physical post-clear confirmation frame.
    assert_eq!(candidate["status"], "witnessFound", "{candidate}");
    assert_eq!(
        candidate["witness"]["confirmationRows"],
        serde_json::json!(["____X___X_"])
    );
    assert_eq!(
        candidate["witness"]["locks"][2]["rowsAfter"],
        serde_json::json!(["____X___X_"])
    );
}

#[test]
fn reaches_a_mirrored_candidate_with_ten_column_mirror_semantics() {
    // Given: the mirror of the catalogued board under Fusion's 10-column transform.
    let result = result_json(
        catalog_json(false),
        batch_json(
            r#"["______IIII"]"#,
            r#"[{"nodeId":1,"frame":"preClear","mirrorRelation":"mirrored","confirmationRows":["______IIII"]}]"#,
            32,
            DIGEST,
        ),
    );

    // When: the batch seam validates both catalog orientations.
    let candidate = &result["results"][0];

    // Then: it returns a mirror witness instead of inventing an unmirrored match.
    assert_eq!(candidate["status"], "witnessFound", "{candidate}");
    assert_eq!(candidate["witness"]["mirrored"], true);
}

#[test]
fn reaches_post_clear_evidence_without_rewriting_the_source_shape() {
    // Given: an original source shape equal to the physical post-clear node frame.
    let result = result_json(
        catalog_json(false),
        batch_json(
            r#"["IIII______"]"#,
            &post_clear_evidence(r#"["IIII______"]"#),
            32,
            DIGEST,
        ),
    );

    // When: the validator checks the post-clear frame evidence.
    let candidate = &result["results"][0];

    // Then: it preserves the candidate's source rows and records the selected post-clear frame.
    assert_eq!(candidate["status"], "witnessFound");
    assert_eq!(candidate["selectedEvidence"]["frame"], "postClear");
    assert_eq!(
        candidate["selectedEvidence"]["confirmationRows"],
        serde_json::json!(["IIII______"])
    );
}

#[test]
fn verifies_a_source_supplied_alternate_pre_clear_frame() {
    // Given: a source-authorized pre-clear frame that shares a node's post-clear state.
    let result = result_json(
        catalog_with_source_pre_clear(),
        batch_json(
            r#"["____X___X_", "XXXXXXXXXX"]"#,
            &source_pre_clear_evidence(r#"["____J___T_"]"#, r#"["____J___T_", "IIIIJJJTTT"]"#),
            20_000,
            DIGEST,
        )
        .replace(r#""lockedPieceOrdinal": 1"#, r#""lockedPieceOrdinal": 3"#),
    );

    // When: Fusion searches and independently replays the source construction.
    let candidate = &result["results"][0];

    // Then: the witness proves both the supplied pre-clear board and authorized residue.
    assert_eq!(candidate["status"], "witnessFound", "{candidate}");
    assert_eq!(
        candidate["witness"]["locks"][2]["rowsBeforeClear"],
        serde_json::json!(["____X___X_", "XXXXXXXXXX"])
    );
    assert_eq!(
        candidate["witness"]["locks"][2]["rowsAfter"],
        serde_json::json!(["____X___X_"])
    );
}

#[test]
fn tries_same_ordinal_evidence_options_until_one_is_reachable() {
    // Given: two evidence options at the same ordinal, with the first in the wrong orientation.
    let result = result_json(
        catalog_json(false),
        batch_json(
            r#"["IIII______"]"#,
            r#"[
                {"nodeId":1,"frame":"preClear","mirrorRelation":"mirrored","confirmationRows":["______IIII"]},
                {"nodeId":1,"frame":"preClear","mirrorRelation":"same","confirmationRows":["IIII______"]}
            ]"#,
            32,
            DIGEST,
        ),
    );

    // When: the first evidence option cannot match the original source shape.
    let candidate = &result["results"][0];

    // Then: the later valid option becomes the selected evidence.
    assert_eq!(candidate["status"], "witnessFound");
    assert_eq!(candidate["selectedEvidence"]["mirrorRelation"], "same");
}

#[test]
fn reports_invalid_input_without_aborting_the_batch() {
    // Given: a candidate whose required digest is absent.
    let malformed = batch_json(
        r#"["IIII______"]"#,
        &pre_clear_evidence(r#"["IIII______"]"#),
        32,
        "stale-digest",
    );
    let result = result_json(catalog_json(false), malformed);

    // When: the boundary parser validates the batch.
    let candidate = &result["results"][0];

    // Then: this candidate is rejected as input rather than searched.
    assert_eq!(candidate["status"], "invalidInput");
}

#[test]
fn reports_budget_exhausted_when_witness_search_has_no_fuel() {
    // Given: a valid candidate but no fuel for exact-ordinal witness traversal.
    let result = result_json(
        catalog_json(false),
        batch_json(
            r#"["IIII______"]"#,
            &pre_clear_evidence(r#"["IIII______"]"#),
            0,
            DIGEST,
        ),
    );

    // When: the validator starts the bounded exact-ordinal proof.
    let candidate = &result["results"][0];

    // Then: exhausted work never becomes a claim of unreachability.
    assert_eq!(candidate["status"], "inconclusive");
    assert_eq!(candidate["reason"], "budgetExhausted");
}

#[test]
fn stops_at_the_target_and_never_traverses_an_invalid_descendant() {
    // Given: a reachable target at ordinal one and a child whose duplicated I placement is invalid.
    let result = result_json(
        catalog_with_invalid_descendant(),
        batch_json(
            r#"["IIII______"]"#,
            &pre_clear_evidence(r#"["IIII______"]"#),
            32,
            DIGEST,
        ),
    );

    // When: validation reconstructs the target's ancestor path.
    let candidate = &result["results"][0];

    // Then: the witness ends at the target ordinal and does not enter the child.
    assert_eq!(candidate["status"], "witnessFound");
    assert_eq!(
        candidate["witness"]["locks"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(candidate["witness"]["locks"][0]["ordinal"], 1);
}
