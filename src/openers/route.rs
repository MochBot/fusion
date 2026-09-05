use std::collections::HashSet;

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
        let mut node = record.tree.iter().find(|node| node.id == *matched_node_id);
        while let Some(current) = node {
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
            node = current
                .parent
                .and_then(|parent_id| record.tree.iter().find(|node| node.id == parent_id));
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
}
