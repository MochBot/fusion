use crate::openers::validate_contextual_target_batch;

#[test]
fn replays_catalog_hints_without_claiming_source_intent() {
    // Given: one hinted and one unhinted contextual target with identical occupancy.
    let catalog = r#"{
        "formatVersion": 2,
        "openers": [{
            "id": "fixture-record",
            "aliases": {"en": "Fixture"},
            "shapeKey": "fixture",
            "tree": [
                {"id":0,"parent":null,"pieces":0,"rows":[]},
                {"id":1,"parent":0,"pieces":1,"rows":["IIII______"],"placements":[{"letter":"I","cells":[[0,0],[1,0],[2,0],[3,0]]}]}
            ]
        }]
    }"#;
    let batch = r#"{
        "schemaVersion": 1,
        "runId": "contextual-fixture",
        "inputAssetSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "searchFuel": 32,
        "maxLocks": 4,
        "candidates": [
            {
                "candidateId":"search-shape:fixture-record:1",
                "candidateDigest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "recordId":"fixture-record",
                "searchShapeIndex":0,
                "targetRows":["XXXX______"],
                "hints":[{"nodeId":1,"pieces":1,"frame":"postClear","mirrorRelation":"same"}]
            },
            {
                "candidateId":"search-shape:fixture-record:2",
                "candidateDigest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "recordId":"fixture-record",
                "searchShapeIndex":1,
                "targetRows":["XXXX______"],
                "hints":[]
            }
        ]
    }"#;

    // When: Fusion validates the contextual target batch.
    let result = match validate_contextual_target_batch(catalog.as_bytes(), batch.as_bytes()) {
        Ok(value) => match serde_json::to_value(value) {
            Ok(json) => json,
            Err(error) => panic!("validation result must serialize: {error}"),
        },
        Err(error) => panic!("fixture boundary must parse: {error}"),
    };

    // Then: the hint is a strict replay proof and the unhinted target is searched directly.
    assert_eq!(result["results"][0]["status"], "witnessFound");
    assert_eq!(
        result["results"][0]["proofMethod"],
        "catalogHintStrictReplay"
    );
    assert_eq!(result["results"][1]["status"], "witnessFound");
    assert_eq!(
        result["results"][1]["proofMethod"],
        "exactTargetOccupancySearch"
    );
    assert!(result["results"][1].get("selectedHint").is_none());
    assert_eq!(
        result["results"][1]["witness"]["confirmationRows"],
        serde_json::json!(["XXXX______"])
    );
    assert_eq!(
        result["results"][1]["witness"]["locks"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}
