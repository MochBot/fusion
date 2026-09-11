//! Shared tree navigation over a raw `&OpenerCatalog` or `&OpenerRecord`.
//! First-match lookups, source-order child/sibling iteration, per-record IDs.

use super::{OpenerCatalog, OpenerRecord, OpenerTreeNode};

/// First record with this id in catalog order.
pub(crate) fn record_by_id<'a>(catalog: &'a OpenerCatalog, id: &str) -> Option<&'a OpenerRecord> {
    catalog.openers.iter().find(|record| record.id == id)
}

/// First node with this id in authored vector order.
pub(crate) fn node_by_id(record: &OpenerRecord, node_id: u32) -> Option<&OpenerTreeNode> {
    record.tree.iter().find(|node| node.id == node_id)
}

/// Direct parent, or `None` at roots and when the parent id is absent.
pub(crate) fn parent_node<'a>(
    record: &'a OpenerRecord,
    node: &OpenerTreeNode,
) -> Option<&'a OpenerTreeNode> {
    node.parent
        .and_then(|parent_id| node_by_id(record, parent_id))
}

/// Lazy leaf-first ancestor walk. Stops at a missing parent; the
/// `tree.len()` bound stops raw cycles without a visited set.
pub(crate) fn ancestors<'a>(
    record: &'a OpenerRecord,
    start: Option<&'a OpenerTreeNode>,
) -> Ancestors<'a> {
    Ancestors {
        record,
        start,
        remaining: record.tree.len(),
    }
}

pub(crate) struct Ancestors<'a> {
    record: &'a OpenerRecord,
    start: Option<&'a OpenerTreeNode>,
    remaining: usize,
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = &'a OpenerTreeNode;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let node = self.start?;
        self.remaining -= 1;
        self.start = parent_node(self.record, node);
        Some(node)
    }
}

/// Strict root-first chain. `None` when a required parent is missing or cyclic.
pub(crate) fn path_to<'a>(
    record: &'a OpenerRecord,
    node: &'a OpenerTreeNode,
) -> Option<Vec<&'a OpenerTreeNode>> {
    let mut path: Vec<&'a OpenerTreeNode> = ancestors(record, Some(node)).collect();
    if path.is_empty() || path.last().is_some_and(|root| root.parent.is_some()) {
        return None;
    }
    path.reverse();
    Some(path)
}

/// Direct children in authored vector order.
pub(crate) fn children_of(
    record: &OpenerRecord,
    parent_id: u32,
) -> impl Iterator<Item = &OpenerTreeNode> {
    record
        .tree
        .iter()
        .filter(move |node| node.parent == Some(parent_id))
}

/// Same-parent nodes excluding the anchor, in authored vector order.
pub(crate) fn siblings_of<'a>(
    record: &'a OpenerRecord,
    anchor: &OpenerTreeNode,
) -> impl Iterator<Item = &'a OpenerTreeNode> {
    let parent = anchor.parent;
    let anchor_id = anchor.id;
    record
        .tree
        .iter()
        .filter(move |node| node.parent == parent && node.id != anchor_id)
}

/// First root in authored vector order.
pub(crate) fn first_root(record: &OpenerRecord) -> Option<&OpenerTreeNode> {
    record.tree.iter().find(|node| node.parent.is_none())
}

/// Deepest node by pieces; ties keep the smallest id.
pub(crate) fn deepest_by_pieces<'a>(
    nodes: impl Iterator<Item = &'a OpenerTreeNode>,
) -> Option<&'a OpenerTreeNode> {
    let mut best: Option<&OpenerTreeNode> = None;
    for node in nodes {
        let replace = match best {
            None => true,
            Some(current) => {
                node.pieces > current.pieces
                    || (node.pieces == current.pieces && node.id < current.id)
            }
        };
        if replace {
            best = Some(node);
        }
    }
    best
}
