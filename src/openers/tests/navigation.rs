use crate::openers::catalog::navigation::{
    ancestors, children_of, deepest_by_pieces, node_by_id, path_to, record_by_id, siblings_of,
};
use crate::openers::catalog::{OpenerCatalog, OpenerRecord, OpenerTreeNode};
use crate::openers::{
    analyze_opener_round, set_opener_catalog, OpenerObservation, OpenerRoundInput,
    GUIDE_VARIATION_LIMIT,
};

const NAV_CATALOG: &str = r#"{
  "formatVersion": 2,
  "openers": [
    {
      "id": "dup",
      "aliases": {"en": "First Dup"},
      "shapeKey": "first-dup",
      "tree": [
        {"id": 0, "parent": null, "pieces": 1, "rows": ["IIII______"]}
      ]
    },
    {
      "id": "dup",
      "aliases": {"en": "Second Dup"},
      "shapeKey": "second-dup",
      "tree": [
        {"id": 0, "parent": null, "pieces": 2, "rows": ["IIIIIIIIII"]}
      ]
    },
    {
      "id": "ordered",
      "aliases": {"en": "Ordered"},
      "shapeKey": "ordered",
      "tree": [
        {"id": 1, "parent": null, "pieces": 1, "rows": ["IIII______"]},
        {"id": 7, "parent": 1, "pieces": 5, "rows": ["IIIIIIIIII"]},
        {"id": 3, "parent": 1, "pieces": 5, "rows": ["IIIIIIIIII"]},
        {"id": 5, "parent": 1, "pieces": 5, "rows": ["IIIIIIIIII"]},
        {"id": 2, "parent": 1, "pieces": 2, "rows": ["IIIIIIIIII"]},
        {"id": 4, "parent": 2, "pieces": 3, "rows": ["IIIIIIIIII"]},
        {"id": 6, "parent": 99, "pieces": 4, "rows": ["IIIIIIIIII"]}
      ]
    }
  ]
}"#;

const ORDERED_VARIATIONS_CATALOG: &str = r#"{
  "formatVersion": 2,
  "openers": [{
    "id": "ordered-guide",
    "aliases": {"en": "Ordered Guide"},
    "shapeKey": "ordered-guide",
    "tree": [
      {"id": 1, "parent": null, "pieces": 1, "rows": ["IIII______"]},
      {"id": 10, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]},
      {"id": 3, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]},
      {"id": 7, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]},
      {"id": 2, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]},
      {"id": 9, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]},
      {"id": 4, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]},
      {"id": 8, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]},
      {"id": 5, "parent": 1, "pieces": 2, "rows": ["IIII______", "IIII______"]}
    ]
  }]
}"#;

fn nav_catalog() -> OpenerCatalog {
    match serde_json::from_str(NAV_CATALOG) {
        Ok(catalog) => catalog,
        Err(error) => panic!("navigation catalog should parse: {error}"),
    }
}

fn ordered_record(catalog: &OpenerCatalog) -> &OpenerRecord {
    match record_by_id(catalog, "ordered") {
        Some(record) => record,
        None => panic!("navigation catalog should contain the ordered record"),
    }
}

fn node_ids(nodes: Vec<&OpenerTreeNode>) -> Vec<u32> {
    nodes.iter().map(|node| node.id).collect()
}

#[test]
fn record_lookup_returns_the_first_match_in_catalog_order() {
    let catalog = nav_catalog();

    let record = match record_by_id(&catalog, "dup") {
        Some(record) => record,
        None => panic!("duplicate record id should resolve"),
    };
    assert_eq!(record.shape_key, "first-dup");
}

#[test]
fn node_lookup_is_scoped_to_each_record() {
    let catalog = nav_catalog();

    let first = match node_by_id(&catalog.openers[0], 0) {
        Some(node) => node,
        None => panic!("first record should own node 0"),
    };
    let second = match node_by_id(&catalog.openers[1], 0) {
        Some(node) => node,
        None => panic!("second record should own node 0"),
    };
    assert_eq!(first.pieces, 1);
    assert_eq!(second.pieces, 2);
}

#[test]
fn children_follow_authored_vector_order_not_id_order() {
    let catalog = nav_catalog();
    let record = ordered_record(&catalog);

    assert_eq!(node_ids(children_of(record, 1).collect()), [7, 3, 5, 2]);
}

#[test]
fn siblings_follow_authored_order_and_exclude_the_anchor() {
    let catalog = nav_catalog();
    let record = ordered_record(&catalog);
    let anchor = match node_by_id(record, 2) {
        Some(node) => node,
        None => panic!("ordered record should contain node 2"),
    };

    assert_eq!(node_ids(siblings_of(record, anchor).collect()), [7, 3, 5]);
}

#[test]
fn strict_path_is_root_first_and_rejects_missing_parents() {
    let catalog = nav_catalog();
    let record = ordered_record(&catalog);
    let leaf = match node_by_id(record, 4) {
        Some(node) => node,
        None => panic!("ordered record should contain node 4"),
    };
    let orphan = match node_by_id(record, 6) {
        Some(node) => node,
        None => panic!("ordered record should contain node 6"),
    };

    let path = match path_to(record, leaf) {
        Some(path) => path,
        None => panic!("complete chain should resolve"),
    };
    assert_eq!(node_ids(path), [1, 2, 4]);
    assert!(path_to(record, orphan).is_none());
}

#[test]
fn lenient_walk_visits_the_existing_prefix_and_stops() {
    let catalog = nav_catalog();
    let record = ordered_record(&catalog);

    assert_eq!(
        node_ids(ancestors(record, node_by_id(record, 4)).collect()),
        [4, 2, 1]
    );
    assert_eq!(
        node_ids(ancestors(record, node_by_id(record, 6)).collect()),
        [6]
    );
    assert!(ancestors(record, None).next().is_none());
    assert!(ancestors(record, node_by_id(record, 99)).next().is_none());
}

#[test]
fn deepest_tie_prefers_the_smallest_node_id() {
    let catalog = nav_catalog();
    let record = ordered_record(&catalog);

    let deepest = match deepest_by_pieces(record.tree.iter()) {
        Some(node) => node,
        None => panic!("ordered record should have a deepest node"),
    };
    assert_eq!(deepest.id, 3);
    assert_eq!(deepest.pieces, 5);
}

fn observation(mask: u16, letters: &str) -> OpenerObservation {
    OpenerObservation {
        post_board: Some(vec![mask]),
        post_gmask: Some(vec![0]),
        post_letters: Some(vec![letters.to_owned()]),
    }
}

#[test]
fn guide_variations_keep_authored_order_within_the_six_item_limit() {
    let _scope = crate::openers::isolated_catalog_test();
    if let Err(error) = set_opener_catalog(ORDERED_VARIATIONS_CATALOG.as_bytes()) {
        panic!("ordered catalog should install: {error}");
    }
    let analysis = match analyze_opener_round(&OpenerRoundInput {
        observations: vec![Some(observation(0b0000001111, "IIII______"))],
        ..OpenerRoundInput::default()
    }) {
        Ok(analysis) => analysis,
        Err(error) => panic!("catalog is installed: {error}"),
    };

    let guide = analysis.guide.expect("the root board should confirm");
    assert_eq!(guide.record_id, "ordered-guide");
    assert_eq!(guide.variations_total, 8);
    assert_eq!(guide.variations.len(), GUIDE_VARIATION_LIMIT);
    assert_eq!(
        guide
            .variations
            .iter()
            .map(|variation| variation.node_id)
            .collect::<Vec<_>>(),
        [10, 3, 7, 2, 9, 4]
    );
}
