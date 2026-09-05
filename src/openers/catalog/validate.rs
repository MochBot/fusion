use std::collections::{HashMap, HashSet};

use super::{CatalogError, OpenerCatalog, OpenerTreeNode};

pub(super) fn validate_catalog(catalog: &OpenerCatalog) -> Result<(), CatalogError> {
    let mut opener_ids = HashSet::new();
    for record in &catalog.openers {
        if record.id.is_empty() || !opener_ids.insert(record.id.as_str()) {
            return invalid("opener IDs must be unique and non-empty");
        }
        for rows in &record.search_shapes {
            validate_rows(rows)?;
        }
        let node_ids: HashMap<u32, &OpenerTreeNode> =
            record.tree.iter().map(|node| (node.id, node)).collect();
        if node_ids.len() != record.tree.len() {
            return invalid("tree node IDs must be unique per opener");
        }
        for node in &record.tree {
            validate_rows(&node.rows)?;
            if let Some(rows) = &node.pre_clear_rows {
                validate_rows(rows)?;
            }
            if node
                .clear_rows
                .as_ref()
                .is_some_and(|rows| rows.iter().any(|row| *row >= 40))
            {
                return invalid("clear rows must be within the 40-row board");
            }
            if let Some(placements) = &node.placements {
                for placement in placements {
                    if !is_piece_letter(&placement.letter) {
                        return invalid("placement letters must be one canonical tetromino");
                    }
                    if placement.cells.len() != 4 {
                        return invalid("placements must contain exactly four cells");
                    }
                    let mut cells = HashSet::new();
                    for [x, y] in &placement.cells {
                        if *x >= 10 || *y >= 40 || !cells.insert((*x, *y)) {
                            return invalid("placement cells must be unique and within the board");
                        }
                    }
                }
            }
            validate_parent_chain(node.parent, &node_ids)?;
        }
    }
    Ok(())
}

fn validate_parent_chain(
    parent: Option<u32>,
    node_ids: &HashMap<u32, &OpenerTreeNode>,
) -> Result<(), CatalogError> {
    let mut current = parent;
    let mut ancestors = HashSet::new();
    while let Some(parent) = current {
        if !ancestors.insert(parent) {
            return invalid("tree parent chains must not contain cycles");
        }
        let Some(parent_node) = node_ids.get(&parent) else {
            return invalid("tree parents must refer to an existing node");
        };
        current = parent_node.parent;
    }
    Ok(())
}

fn validate_rows(rows: &[String]) -> Result<(), CatalogError> {
    if rows.len() > 40 {
        return invalid("catalog boards must fit within 40 rows");
    }
    if rows
        .iter()
        .all(|row| row.chars().count() == 10 && row.chars().all(is_row_cell))
    {
        Ok(())
    } else {
        invalid("catalog rows must use ten canonical cells")
    }
}

fn is_row_cell(cell: char) -> bool {
    matches!(cell, '_' | 'X' | 'I' | 'O' | 'T' | 'S' | 'Z' | 'J' | 'L')
}

fn is_piece_letter(letter: &str) -> bool {
    matches!(letter, "I" | "O" | "T" | "S" | "Z" | "J" | "L")
}

fn invalid<T>(reason: &str) -> Result<T, CatalogError> {
    Err(CatalogError::InvalidCatalog {
        reason: reason.to_owned(),
    })
}
