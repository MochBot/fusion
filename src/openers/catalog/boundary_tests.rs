use super::super::{
    installed_opener_catalog, isolated_catalog_test, set_opener_catalog, CatalogError,
};
use super::catalog_json;

fn tree_node_json(id: u32, parent: Option<u32>, pieces: u32) -> String {
    let parent = match parent {
        Some(parent) => parent.to_string(),
        None => "null".to_owned(),
    };
    format!(r#"{{"id": {id}, "parent": {parent}, "pieces": {pieces}, "rows": ["IIII______"]}}"#)
}

fn record_with_tree(id: &str, label: &str, nodes: &str) -> String {
    format!(
        r#"{{"id": "{id}", "aliases": {{"en": "{label}"}}, "shapeKey": "shape-{id}", "tree": [{nodes}]}}"#
    )
}

fn catalog_with_openers(openers: &str) -> String {
    format!(r#"{{"formatVersion": 2, "openers": [{openers}]}}"#)
}

#[test]
fn rejects_duplicate_tree_node_ids_within_one_record() {
    let _scope = isolated_catalog_test();
    let nodes = [tree_node_json(0, None, 1), tree_node_json(0, None, 1)].join(",");
    let catalog = catalog_with_openers(&record_with_tree("dup", "Dup", &nodes));

    assert!(matches!(
        set_opener_catalog(catalog.as_bytes()),
        Err(CatalogError::InvalidCatalog { reason })
            if reason == "tree node IDs must be unique per opener"
    ));
}

#[test]
fn allows_the_same_node_id_in_different_records() {
    let _scope = isolated_catalog_test();
    let openers = [
        record_with_tree("first", "First", &tree_node_json(4, None, 1)),
        record_with_tree("second", "Second", &tree_node_json(4, None, 1)),
    ]
    .join(",");
    let catalog = catalog_with_openers(&openers);

    let stats = match set_opener_catalog(catalog.as_bytes()) {
        Ok(stats) => stats,
        Err(error) => panic!("per-record node IDs should install: {error}"),
    };
    assert_eq!(stats.opener_count, 2);
    assert_eq!(stats.tree_node_count, 2);
    let Some(installed) = installed_opener_catalog() else {
        panic!("catalog should be installed");
    };
    assert_eq!(installed.openers[0].tree[0].id, 4);
    assert_eq!(installed.openers[1].tree[0].id, 4);
}

#[test]
fn rejects_parent_cycles() {
    let _scope = isolated_catalog_test();
    let nodes = [tree_node_json(1, Some(2), 2), tree_node_json(2, Some(1), 3)].join(",");
    let catalog = catalog_with_openers(&record_with_tree("cycle", "Cycle", &nodes));

    assert!(matches!(
        set_opener_catalog(catalog.as_bytes()),
        Err(CatalogError::InvalidCatalog { reason })
            if reason == "tree parent chains must not contain cycles"
    ));

    let nodes = tree_node_json(5, Some(5), 2);
    let catalog = catalog_with_openers(&record_with_tree("self-cycle", "Self Cycle", &nodes));

    assert!(matches!(
        set_opener_catalog(catalog.as_bytes()),
        Err(CatalogError::InvalidCatalog { reason })
            if reason == "tree parent chains must not contain cycles"
    ));
}

#[test]
fn rejects_missing_parents() {
    let _scope = isolated_catalog_test();
    let nodes = [tree_node_json(1, None, 1), tree_node_json(2, Some(99), 2)].join(",");
    let catalog = catalog_with_openers(&record_with_tree("orphan", "Orphan", &nodes));

    assert!(matches!(
        set_opener_catalog(catalog.as_bytes()),
        Err(CatalogError::InvalidCatalog { reason })
            if reason == "tree parents must refer to an existing node"
    ));
}

#[test]
fn accepts_forward_parents_sparse_ids_multiple_roots_and_non_monotonic_pieces() {
    let _scope = isolated_catalog_test();
    let nodes = [
        tree_node_json(9, Some(3), 2),
        tree_node_json(3, None, 5),
        tree_node_json(100, None, 1),
    ]
    .join(",");
    let catalog = catalog_with_openers(&record_with_tree("sparse", "Sparse", &nodes));

    let stats = match set_opener_catalog(catalog.as_bytes()) {
        Ok(stats) => stats,
        Err(error) => panic!("forward sparse trees should install: {error}"),
    };
    assert_eq!(stats.opener_count, 1);
    assert_eq!(stats.tree_node_count, 3);
}

#[test]
fn rejected_tree_shape_leaves_the_previous_snapshot_installed() {
    let _scope = isolated_catalog_test();
    let valid = catalog_json("valid", "Valid");
    if let Err(error) = set_opener_catalog(valid.as_bytes()) {
        panic!("valid catalog should install: {error}");
    }
    let nodes = [tree_node_json(1, Some(2), 2), tree_node_json(2, Some(1), 3)].join(",");
    let cyclic = catalog_with_openers(&record_with_tree("cycle", "Cycle", &nodes));

    assert!(matches!(
        set_opener_catalog(cyclic.as_bytes()),
        Err(CatalogError::InvalidCatalog { .. })
    ));
    let Some(installed) = installed_opener_catalog() else {
        panic!("failed catalog installation must retain the prior snapshot");
    };
    assert_eq!(installed.openers[0].id, "valid");
}
