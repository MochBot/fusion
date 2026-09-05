use crate::openers::validate_supplied_operation_batch;

#[test]
fn replays_an_exact_source_operation_on_its_authored_field() {
    // Given: a source-authored field and horizontal I operation that clears it.
    let batch = r#"{
        "schemaVersion": 1,
        "runId": "explicit-construction-fixture",
        "inputAssetSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "candidates": [{
            "candidateId": "search-shape:fixture:1",
            "candidateDigest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "recordId": "fixture",
            "searchShapeIndex": 0,
            "startRows": ["XXXXXX____"],
            "steps": [{
                "piece": "I",
                "cells": [[6,0],[7,0],[8,0],[9,0]],
                "expectedRowsAfter": []
            }]
        }]
    }"#;

    // When: Fusion validates the supplied operation without searching alternatives.
    let result = match validate_supplied_operation_batch(batch.as_bytes()) {
        Ok(value) => match serde_json::to_value(value) {
            Ok(json) => json,
            Err(error) => panic!("validation result must serialize: {error}"),
        },
        Err(error) => panic!("fixture boundary must parse: {error}"),
    };

    // Then: the exact move is generated, reachable, locked, and receipt-linked.
    let candidate = &result["results"][0];
    assert_eq!(candidate["status"], "witnessFound");
    assert_eq!(candidate["witness"]["steps"][0]["piece"], "I");
    assert_eq!(
        candidate["witness"]["steps"][0]["cells"],
        serde_json::json!([[6, 0], [7, 0], [8, 0], [9, 0]])
    );
    assert_eq!(
        candidate["witness"]["steps"][0]["rowsAfter"],
        serde_json::json!([])
    );
    assert_eq!(
        candidate["witness"]["queueScope"],
        "notClaimedMidConstruction"
    );
}
