use sha2::{Digest, Sha256};

use super::{
    install_opener_runtime, installed_opener_catalog, isolated_catalog_test, set_opener_catalog,
    CatalogError, OpenerInstallError,
};
use crate::openers::WitnessCatalogError;

fn catalog_json(id: &str, label: &str) -> String {
    format!(
        r#"{{
            "formatVersion": 2,
            "futureAssetField": true,
            "openers": [{{
                "id": "{id}",
                "aliases": {{ "en": "{label}", "futureAliasField": "kept tolerant" }},
                "shapeKey": "shape-{id}",
                "searchShapes": [["__________", "IIII______"]],
                "tree": [{{
                    "id": 0,
                    "parent": null,
                    "pieces": 1,
                    "rows": ["IIII______"],
                    "src": [],
                    "futureNodeField": 42
                }}],
                "flatPages": [],
                "futureRecordField": "kept tolerant"
            }}]
        }}"#
    )
}

#[test]
fn installs_v2_catalog_and_replaces_prior_catalog_deterministically() {
    let _scope = isolated_catalog_test();
    let first = catalog_json("first", "First");
    let first_stats = match set_opener_catalog(first.as_bytes()) {
        Ok(stats) => stats,
        Err(error) => panic!("first catalog should install: {error}"),
    };

    assert_eq!(first_stats.opener_count, 1);
    assert_eq!(first_stats.tree_node_count, 1);
    assert_eq!(first_stats.search_shape_count, 1);

    let second = catalog_json("second", "Second");
    let second_stats = match set_opener_catalog(second.as_bytes()) {
        Ok(stats) => stats,
        Err(error) => panic!("replacement catalog should install: {error}"),
    };
    assert_eq!(second_stats, first_stats);

    let Some(installed) = installed_opener_catalog() else {
        panic!("catalog should be installed");
    };
    let Some(record) = installed.openers.first() else {
        panic!("installed catalog should retain its record");
    };
    assert_eq!(record.id, "second");
    assert_eq!(record.aliases.en, "Second");
    let Some(node) = record.tree.first() else {
        panic!("installed record should retain its tree node");
    };
    assert!(node.placements.is_none());
}

#[test]
fn rejects_malformed_catalog_json_with_typed_error() {
    let _scope = isolated_catalog_test();
    let result = set_opener_catalog(br#"{"formatVersion": 2, "openers": [}"#);
    assert!(matches!(result, Err(CatalogError::MalformedJson { .. })));
}

#[test]
fn rejects_unsupported_catalog_version_with_typed_error() {
    let _scope = isolated_catalog_test();
    let result = set_opener_catalog(br#"{"formatVersion": 1, "openers": []}"#);
    assert!(matches!(
        result,
        Err(CatalogError::UnsupportedFormatVersion { found: 1 })
    ));
}

#[test]
fn rejects_invalid_catalogs_without_replacing_the_installed_snapshot() {
    let _scope = isolated_catalog_test();
    let valid = catalog_json("valid", "Valid");
    if let Err(error) = set_opener_catalog(valid.as_bytes()) {
        panic!("valid catalog should install: {error}");
    }
    let invalid = valid.replace("IIII______", "IIII_____Q");

    assert!(matches!(
        set_opener_catalog(invalid.as_bytes()),
        Err(CatalogError::InvalidCatalog { .. })
    ));
    let Some(installed) = installed_opener_catalog() else {
        panic!("failed catalog installation must retain the prior snapshot");
    };
    assert_eq!(installed.openers[0].id, "valid");
}

#[test]
fn rejects_catalog_boards_taller_than_forty_rows() {
    let _scope = isolated_catalog_test();
    let rows = vec![r#""__________""#; 41].join(",");
    let invalid = catalog_json("too-tall", "Too Tall")
        .replace(r#"["__________", "IIII______"]"#, &format!("[{rows}]"));

    assert!(matches!(
        set_opener_catalog(invalid.as_bytes()),
        Err(CatalogError::InvalidCatalog { reason })
            if reason == "catalog boards must fit within 40 rows"
    ));
}

fn witness_json(catalog: &str, rows: &str) -> String {
    let catalog_sha256 = format!("{:x}", Sha256::digest(catalog.as_bytes()));
    format!(
        r#"{{
            "schemaVersion":1,
            "openerAssetSha256":"{catalog_sha256}",
            "enrichmentSha256":"{hash}",
            "provenanceSha256":"{hash}",
            "summary":{{"witnesses":1,"runtimeTargets":1,"viewerOnly":0}},
            "witnesses":[{{
                "recordId":"valid",
                "recordName":"Valid",
                "searchShapeIndex":0,
                "candidateId":"search-shape:valid:1",
                "candidateDigest":"{hash}",
                "resultsSha256":"{hash}",
                "lane":"sourceTree",
                "proofMethod":"sourceTreeStrictReplay",
                "targetFrame":"postClear",
                "queueScope":"freshRound",
                "targetRows":["{rows}"],
                "startRows":[],
                "steps":[],
                "runtimeTarget":{{"lockedPieceOrdinal":1,"rows":["{rows}"]}}
            }}]
        }}"#,
        hash = "a".repeat(64)
    )
}

#[test]
fn installs_catalog_and_companion_as_one_unit() {
    let _scope = isolated_catalog_test();
    let catalog = catalog_json("valid", "Valid");
    let witnesses = witness_json(&catalog, "IIII______");

    let stats = match install_opener_runtime(catalog.as_bytes(), Some(witnesses.as_bytes())) {
        Ok(stats) => stats,
        Err(error) => panic!("catalog and companion should install together: {error}"),
    };
    assert_eq!(stats.catalog.opener_count, 1);
    let Some(witness_stats) = stats.witnesses else {
        panic!("a supplied companion must report witness statistics");
    };
    assert_eq!(witness_stats.witness_count, 1);
    assert_eq!(witness_stats.runtime_target_count, 1);
    let Some(installed) = installed_opener_catalog() else {
        panic!("runtime should be installed");
    };
    assert_eq!(installed.runtime_search_shape_targets.len(), 1);

    let without_companion = match install_opener_runtime(catalog.as_bytes(), None) {
        Ok(stats) => stats,
        Err(error) => panic!("catalog alone should install: {error}"),
    };
    assert_eq!(without_companion.catalog, stats.catalog);
    assert!(without_companion.witnesses.is_none());
    let Some(installed) = installed_opener_catalog() else {
        panic!("runtime should be installed");
    };
    assert!(installed.runtime_search_shape_targets.is_empty());
}

#[test]
fn rejected_companion_leaves_the_previous_runtime_installed() {
    let _scope = isolated_catalog_test();
    let first = catalog_json("first", "First");
    if let Err(error) = install_opener_runtime(first.as_bytes(), None) {
        panic!("first catalog should install: {error}");
    }
    let second = catalog_json("valid", "Valid");
    let mismatched = witness_json(&first, "IIII______");

    assert!(matches!(
        install_opener_runtime(second.as_bytes(), Some(mismatched.as_bytes())),
        Err(OpenerInstallError::Witnesses(
            WitnessCatalogError::CatalogIdentityMismatch
        ))
    ));
    let Some(installed) = installed_opener_catalog() else {
        panic!("failed install must retain the prior runtime");
    };
    assert_eq!(installed.openers[0].id, "first");

    assert!(matches!(
        install_opener_runtime(b"!", Some(mismatched.as_bytes())),
        Err(OpenerInstallError::Catalog(
            CatalogError::MalformedJson { .. }
        ))
    ));
}
