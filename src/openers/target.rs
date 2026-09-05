use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::openers::board::{
    cell_count, mirror_letter_row, mirror_mask_10, rows_to_masks_floor_up,
};
use crate::openers::catalog::OpenerCatalog;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShapeTarget {
    pub opener_id: String,
    /// Dense index of `opener_id` within the built target set; equal ids share
    /// one ordinal so per-opener bucketing needs no string hashing.
    #[serde(default)]
    pub opener_ordinal: u32,
    pub label: String,
    pub rows: Vec<u16>,
    pub cells: u32,
    pub mirrored: bool,
    pub letters: Option<Vec<String>>,
    /// Per row, the cells whose target letter is a specific piece (not the
    /// `X` wildcard), so letter comparison can skip rows and cells that
    /// cannot disagree. Derived from `letters`.
    #[serde(default)]
    pub lettered_cells: Vec<u16>,
    /// Per row, the occupancy of each specific piece letter in
    /// `PIECE_LETTERS` order, so letter agreement is a mask AND per letter.
    /// Derived from `letters`.
    #[serde(default)]
    pub letter_masks: Vec<[u16; PIECE_LETTERS.len()]>,
    pub node_id: Option<u32>,
    pub route_name: Option<String>,
}

#[derive(Hash, PartialEq, Eq)]
struct TargetKey {
    opener_id: String,
    rows: Vec<u16>,
    letters: Option<Vec<String>>,
}

struct TargetSource<'a> {
    opener_id: &'a str,
    opener_ordinal: u32,
    label: String,
    rows_top_down: &'a [String],
    node_id: Option<u32>,
    route_name: Option<&'a str>,
}

pub(crate) fn build_targets(catalog: &OpenerCatalog) -> Vec<ShapeTarget> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();

    for (ordinal, opener) in catalog.openers.iter().enumerate() {
        if opener.shape_key.starts_with("stub-") {
            continue;
        }
        let opener_ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);

        for node in &opener.tree {
            let route_name = node.route_name.as_deref().filter(|name| !name.is_empty());
            let label = format!("{} (node {})", opener.aliases.en, node.id);
            push_target(
                &mut targets,
                &mut seen,
                TargetSource {
                    opener_id: &opener.id,
                    opener_ordinal,
                    label: label.clone(),
                    rows_top_down: &node.rows,
                    node_id: Some(node.id),
                    route_name,
                },
            );
            if let Some(pre_clear_rows) = &node.pre_clear_rows {
                push_target(
                    &mut targets,
                    &mut seen,
                    TargetSource {
                        opener_id: &opener.id,
                        opener_ordinal,
                        label,
                        rows_top_down: pre_clear_rows,
                        node_id: Some(node.id),
                        route_name,
                    },
                );
            }
        }
    }

    targets
}

fn push_target(
    targets: &mut Vec<ShapeTarget>,
    seen: &mut HashSet<TargetKey>,
    source: TargetSource<'_>,
) {
    let rows = rows_to_masks_floor_up(source.rows_top_down);
    let cells = cell_count(&rows);
    if cells < 8 {
        return;
    }
    let letters = contains_piece_letters(source.rows_top_down)
        .then(|| letters_to_floor_up(source.rows_top_down, rows.len()));

    for mirrored in [false, true] {
        let target_rows = if mirrored {
            rows.iter().map(|row| mirror_mask_10(*row)).collect()
        } else {
            rows.clone()
        };
        let target_letters = match (mirrored, &letters) {
            (true, Some(letter_rows)) => Some(
                letter_rows
                    .iter()
                    .map(|row| mirror_letter_row(row))
                    .collect(),
            ),
            (false, Some(letter_rows)) => Some(letter_rows.clone()),
            (_, None) => None,
        };
        let key = TargetKey {
            opener_id: source.opener_id.to_owned(),
            rows: target_rows.clone(),
            letters: target_letters.clone(),
        };
        if !seen.insert(key) {
            continue;
        }

        let lettered_cells = target_letters
            .as_deref()
            .map(|rows| rows.iter().map(|row| lettered_cell_mask(row)).collect())
            .unwrap_or_default();
        let letter_masks = target_letters
            .as_deref()
            .map(|rows| rows.iter().map(|row| letter_masks_of(row)).collect())
            .unwrap_or_default();
        targets.push(ShapeTarget {
            opener_id: source.opener_id.to_owned(),
            opener_ordinal: source.opener_ordinal,
            label: source.label.clone(),
            rows: target_rows,
            cells,
            mirrored,
            letters: target_letters,
            lettered_cells,
            letter_masks,
            node_id: source.node_id,
            route_name: source.route_name.map(ToOwned::to_owned),
        });
    }
}

pub(crate) const PIECE_LETTERS: [u8; 7] = *b"IJLOSTZ";

/// One occupancy mask per piece letter for a row of letter cells.
pub(crate) fn letter_masks_of(row: &str) -> [u16; PIECE_LETTERS.len()] {
    let mut masks = [0u16; PIECE_LETTERS.len()];
    for (x, letter) in row.bytes().take(10).enumerate() {
        if let Some(index) = PIECE_LETTERS.iter().position(|piece| *piece == letter) {
            masks[index] |= 1 << x;
        }
    }
    masks
}

fn lettered_cell_mask(row: &str) -> u16 {
    row.bytes()
        .take(10)
        .enumerate()
        .filter(|(_, letter)| !matches!(letter, b'_' | b'X'))
        .fold(0, |mask, (x, _)| mask | (1 << x))
}

fn contains_piece_letters(rows_top_down: &[String]) -> bool {
    rows_top_down.iter().any(|row| {
        row.chars()
            .any(|letter| matches!(letter, 'I' | 'J' | 'L' | 'O' | 'S' | 'T' | 'Z'))
    })
}

fn letters_to_floor_up(rows_top_down: &[String], height: usize) -> Vec<String> {
    let mut letters = Vec::with_capacity(height);
    for row in rows_top_down.iter().rev().take(height) {
        letters.push(row.clone());
    }
    while letters.len() < height {
        letters.push("__________".to_owned());
    }
    letters
}

#[cfg(test)]
mod tests {
    use super::build_targets;
    use crate::openers::catalog::OpenerCatalog;
    #[test]
    fn build_targets_excludes_unenriched_search_shapes() {
        let catalog: OpenerCatalog = match serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/openers/catalog-mini.json"
        ))) {
            Ok(catalog) => catalog,
            Err(error) => panic!("catalog fixture should parse: {error}"),
        };
        let targets = build_targets(&catalog);

        assert!(targets.iter().all(|target| target.node_id.is_some()));
    }
}
