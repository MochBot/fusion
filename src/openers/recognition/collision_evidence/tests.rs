use super::model::{RecordEvidence, RecordStatus, TransitionKind};
use super::{export_catalog, export_from_env, CollisionPair, CollisionRequest};
use crate::openers::catalog::OpenerCatalog;

#[test]
fn emits_deterministic_distinct_record_evidence_for_both_chiralities() {
    let catalog = fixture_catalog();
    let request = request(&[("fixture-b", "fixture-a"), ("fixture-a", "strict")]);

    let first = export_catalog(&catalog, &request).unwrap();
    let second = export_catalog(&catalog, &request).unwrap();

    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec_pretty(&first).unwrap(),
        serde_json::to_vec_pretty(&second).unwrap(),
    );
    assert_eq!(first.format_version, 1);
    assert_eq!(first.catalog.format_version, 2);
    assert_eq!(first.catalog.record_count, 8);
    assert_eq!(
        record_ids(&first.records),
        ["fixture-a", "fixture-b", "strict"]
    );
    assert!(first.records.iter().all(|record| {
        record
            .chiralities
            .iter()
            .map(|chirality| chirality.mirrored)
            .eq([false, true])
    }));
    let json = serde_json::to_value(&first).unwrap();
    assert_eq!(json["compileBudget"]["maxStatesPerEdge"], 16_384);
}

#[test]
fn copied_records_have_equal_chirality_signatures() {
    let evidence =
        export_catalog(&fixture_catalog(), &request(&[("fixture-a", "fixture-b")])).unwrap();

    assert_eq!(
        record(&evidence.records, "fixture-a").chiralities,
        record(&evidence.records, "fixture-b").chiralities
    );
}

#[test]
fn author_metadata_is_audit_only_for_identical_compiled_topology() {
    let evidence = export_catalog(
        &fixture_catalog(),
        &request(&[("author-metadata-a", "author-metadata-b")]),
    )
    .unwrap();
    let first = record(&evidence.records, "author-metadata-a");
    let second = record(&evidence.records, "author-metadata-b");

    for chirality in 0..2 {
        assert_eq!(
            first.chiralities[chirality].physical_state_signatures,
            second.chiralities[chirality].physical_state_signatures
        );
        assert_eq!(
            first.chiralities[chirality].state_role_signatures,
            second.chiralities[chirality].state_role_signatures
        );
        assert_eq!(
            first.chiralities[chirality].transition_signatures,
            second.chiralities[chirality].transition_signatures
        );
        assert_ne!(
            first.chiralities[chirality].origin_signatures,
            second.chiralities[chirality].origin_signatures
        );
    }
}

#[test]
fn strict_extension_contains_base_physical_and_role_signatures() {
    let evidence =
        export_catalog(&fixture_catalog(), &request(&[("fixture-a", "strict")])).unwrap();
    let base = record(&evidence.records, "fixture-a");
    let strict = record(&evidence.records, "strict");

    for chirality in 0..2 {
        let base_physical = base.chiralities[chirality]
            .physical_state_signatures
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let strict_physical = strict.chiralities[chirality]
            .physical_state_signatures
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert!(strict_physical.is_superset(&base_physical));
        assert!(strict_physical.len() > base_physical.len());
        assert!(base.chiralities[chirality]
            .state_role_signatures
            .iter()
            .all(|signature| strict.chiralities[chirality]
                .state_role_signatures
                .contains(signature)));
        assert!(
            strict.chiralities[chirality].state_role_signatures.len()
                > base.chiralities[chirality].state_role_signatures.len()
        );
        assert!(base.chiralities[chirality]
            .origin_signatures
            .iter()
            .all(|signature| strict.chiralities[chirality]
                .origin_signatures
                .contains(signature)));
        assert!(
            strict.chiralities[chirality].origin_signatures.len()
                > base.chiralities[chirality].origin_signatures.len()
        );
        assert!(base.chiralities[chirality]
            .transition_signatures
            .iter()
            .all(|signature| strict.chiralities[chirality]
                .transition_signatures
                .contains(signature)));
        assert!(
            strict.chiralities[chirality].transition_signatures.len()
                > base.chiralities[chirality].transition_signatures.len()
        );
    }
}

#[test]
fn divergent_record_has_different_physical_signatures() {
    let evidence =
        export_catalog(&fixture_catalog(), &request(&[("fixture-a", "divergent")])).unwrap();

    assert_ne!(
        record(&evidence.records, "fixture-a").chiralities[0].physical_state_signatures,
        record(&evidence.records, "divergent").chiralities[0].physical_state_signatures,
    );
}

#[test]
fn empty_tree_record_is_reported_without_compiled_states() {
    let evidence = export_catalog(&fixture_catalog(), &request(&[("empty", "fixture-a")])).unwrap();
    let empty = record(&evidence.records, "empty");

    assert_eq!(empty.status, RecordStatus::NoCompiledStates);
    assert!(empty.chiralities.iter().all(|chirality| {
        chirality.physical_state_signatures.is_empty()
            && chirality.state_role_signatures.is_empty()
            && chirality.origin_signatures.is_empty()
            && chirality.transition_signatures.is_empty()
    }));
}

#[test]
fn bridge_transitions_are_included_in_the_signature_multiset() {
    let evidence = export_catalog(&fixture_catalog(), &request(&[("bridge", "empty")])).unwrap();
    let bridge = record(&evidence.records, "bridge");

    assert!(bridge.chiralities.iter().all(|chirality| {
        chirality
            .transition_signatures
            .iter()
            .any(|entry| entry.signature.kind == TransitionKind::Bridge && entry.count > 0)
    }));
}

#[test]
fn request_validation_rejects_unsupported_duplicate_self_and_missing_records() {
    let catalog = fixture_catalog();
    for request in [
        CollisionRequest {
            format_version: 2,
            pairs: vec![],
        },
        request(&[("fixture-a", "fixture-a")]),
        request(&[("fixture-a", "fixture-b"), ("fixture-b", "fixture-a")]),
        request(&[("fixture-a", "missing")]),
    ] {
        assert!(export_catalog(&catalog, &request).is_err());
    }
}

#[test]
#[ignore = "requires OPENER_COLLISION_REQUEST, OPENER_COLLISION_ASSET, and OPENER_COLLISION_OUT"]
fn exports_requested_asset_from_explicit_environment_paths() {
    export_from_env().unwrap();
}

fn request(pairs: &[(&str, &str)]) -> CollisionRequest {
    CollisionRequest {
        format_version: 1,
        pairs: pairs
            .iter()
            .map(|(a, b)| CollisionPair {
                a: (*a).to_owned(),
                b: (*b).to_owned(),
            })
            .collect(),
    }
}

fn record<'a>(records: &'a [RecordEvidence], id: &str) -> &'a RecordEvidence {
    records.iter().find(|record| record.id == id).unwrap()
}

fn record_ids(records: &[RecordEvidence]) -> Vec<&str> {
    records.iter().map(|record| record.id.as_str()).collect()
}

fn fixture_catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{
            "formatVersion": 2,
            "openers": [
                {"id":"fixture-a","aliases":{"en":"Fixture A"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":1,"rows":["OO________","OO________"],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]}]},
                {"id":"fixture-b","aliases":{"en":"Fixture B"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":1,"rows":["OO________","OO________"],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]}]},
                {"id":"strict","aliases":{"en":"Strict"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":1,"rows":["OO________","OO________"],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]},{"id":1,"parent":0,"pieces":2,"rows":["OOOO______","OOOO______"],"placements":[{"letter":"O","cells":[[2,0],[3,0],[2,1],[3,1]]}]}]},
                {"id":"divergent","aliases":{"en":"Divergent"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":1,"rows":["__OO______","__OO______"],"placements":[{"letter":"O","cells":[[2,0],[3,0],[2,1],[3,1]]}]}]},
                {"id":"empty","aliases":{"en":"Empty"},"shapeKey":"fixture","tree":[]},
                {"id":"bridge","aliases":{"en":"Bridge"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":1,"rows":[],"placements":[{"letter":"O","cells":[[0,0],[2,0],[0,1],[2,1]]}]},{"id":1,"parent":0,"pieces":2,"rows":["OO________","OO________"],"placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]}]}
                ,{"id":"author-metadata-a","aliases":{"en":"Author A"},"shapeKey":"fixture","tree":[{"id":5,"parent":null,"pieces":1,"rows":["OO________","OO________"],"routeName":"alpha","placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]},{"id":6,"parent":5,"pieces":2,"rows":["OOOO______","OOOO______"],"routeName":"alpha-next","placements":[{"letter":"O","cells":[[2,0],[3,0],[2,1],[3,1]]}]}]}
                ,{"id":"author-metadata-b","aliases":{"en":"Author B"},"shapeKey":"fixture","tree":[{"id":40,"parent":null,"pieces":1,"rows":["OO________","OO________"],"routeName":"beta","placements":[{"letter":"O","cells":[[0,0],[1,0],[0,1],[1,1]]}]},{"id":90,"parent":40,"pieces":2,"rows":["OOOO______","OOOO______"],"routeName":"beta-next","placements":[{"letter":"O","cells":[[2,0],[3,0],[2,1],[3,1]]}]}]}
            ]
        }"#,
    )
    .unwrap()
}
