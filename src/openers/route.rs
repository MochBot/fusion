use std::collections::HashSet;

use crate::openers::catalog::navigation::{ancestors, node_by_id};
use crate::openers::catalog::OpenerRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReportRoute<'a> {
    pub route_name: Option<&'a str>,
    pub final_node_id: Option<u32>,
}

pub(crate) fn resolve_report_route<'a>(
    record: &'a OpenerRecord,
    matched_node_ids_newest_first: &[u32],
) -> ReportRoute<'a> {
    let mut route_name = None;
    let mut route_pieces = None;
    let mut seen = HashSet::new();

    for matched_node_id in matched_node_ids_newest_first {
        for current in ancestors(record, node_by_id(record, *matched_node_id)) {
            if !seen.insert(current.id) {
                break;
            }
            if let Some(name) = current
                .route_name
                .as_deref()
                .filter(|name| !name.is_empty())
            {
                match route_pieces {
                    Some(pieces) if current.pieces <= pieces => {}
                    _ => {
                        route_name = Some(name);
                        route_pieces = Some(current.pieces);
                    }
                }
            }
        }
    }

    ReportRoute {
        route_name,
        final_node_id: matched_node_ids_newest_first.first().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_report_route;
    use crate::openers::catalog::{OpenerCatalog, OpenerRecord};

    fn catalog() -> OpenerCatalog {
        match serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/openers/catalog-mini.json"
        ))) {
            Ok(catalog) => catalog,
            Err(error) => panic!("mini catalog should parse: {error}"),
        }
    }

    fn record<'a>(catalog: &'a OpenerCatalog, id: &str) -> &'a OpenerRecord {
        match catalog.openers.iter().find(|record| record.id == id) {
            Some(record) => record,
            None => panic!("mini catalog should contain {id}"),
        }
    }

    #[test]
    fn resolves_sdpc_route_from_every_matched_ancestor_chain() {
        let catalog = catalog();
        let sdpc = record(&catalog, "single-double-pc");

        let route = resolve_report_route(sdpc, &[31, 6, 0]);

        assert_eq!(route.route_name, Some("SDPC Spin"));
        assert_eq!(route.final_node_id, Some(31));
    }

    #[test]
    fn returns_no_route_when_the_matched_path_has_no_route_name() {
        let catalog = catalog();
        let crowbar = record(&catalog, "crowbar-v2");

        let route = resolve_report_route(crowbar, &[0]);

        assert_eq!(route.route_name, None);
        assert_eq!(route.final_node_id, Some(0));
    }

    /// Distinct sibling route names so a traversal reversal is detectable.
    const TIE_CATALOG: &str = r#"{
      "formatVersion": 2,
      "openers": [{
        "id": "tie",
        "aliases": {"en": "Tie"},
        "shapeKey": "tie",
        "tree": [
          {"id": 0, "parent": null, "pieces": 1, "rows": ["IIII______"]},
          {"id": 1, "parent": 0, "pieces": 5, "rows": ["IIII____ZZ"], "routeName": "First Route"},
          {"id": 2, "parent": 0, "pieces": 5, "rows": ["IIII__ZZZZ"], "routeName": "Second Route"},
          {"id": 3, "parent": 1, "pieces": 9, "rows": ["IIIIZZZZZZ"], "routeName": "Deep Route"},
          {"id": 7, "parent": 99, "pieces": 4, "rows": ["IIIII_____"], "routeName": "Orphan Route"}
        ]
      }]
    }"#;

    fn tie_record() -> OpenerRecord {
        let catalog: OpenerCatalog = match serde_json::from_str(TIE_CATALOG) {
            Ok(catalog) => catalog,
            Err(error) => panic!("tie catalog should parse: {error}"),
        };
        match catalog.openers.into_iter().next() {
            Some(record) => record,
            None => panic!("tie catalog should contain its record"),
        }
    }

    #[test]
    fn equal_piece_tie_keeps_the_first_encountered_name() {
        let record = tie_record();

        assert_eq!(
            resolve_report_route(&record, &[1, 2]).route_name,
            Some("First Route")
        );
        assert_eq!(
            resolve_report_route(&record, &[2, 1]).route_name,
            Some("Second Route")
        );
    }

    #[test]
    fn deeper_route_name_wins_regardless_of_input_order() {
        let record = tie_record();

        assert_eq!(
            resolve_report_route(&record, &[3]).route_name,
            Some("Deep Route")
        );
        assert_eq!(
            resolve_report_route(&record, &[1, 3]).route_name,
            Some("Deep Route")
        );
        assert_eq!(
            resolve_report_route(&record, &[3, 1]).route_name,
            Some("Deep Route")
        );
    }

    #[test]
    fn missing_parent_stops_the_walk_without_failing() {
        let record = tie_record();

        let route = resolve_report_route(&record, &[7]);

        assert_eq!(route.route_name, Some("Orphan Route"));
        assert_eq!(route.final_node_id, Some(7));
    }
}
