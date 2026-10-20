use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault};

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

/// The installed target set plus a packed copy of what the per-lock board scan
/// reads, so matching walks contiguous memory instead of three vectors per
/// target. Derefs to the targets, in build order.
pub(crate) struct ShapeTargets {
    targets: Vec<ShapeTarget>,
    /// Per target, its first group in `rows`; the next target's start ends it.
    starts: Vec<u32>,
    /// Per target, the fields ranking reads.
    heads: Vec<TargetHead>,
    /// Per group of four target rows (`ROWS_PER_GROUP`, row `y` in bits
    /// `16 * (y % 4)`): occupancy, the cells whose letter is a specific piece
    /// (0 for unlettered targets), then that letter's code in three bit
    /// planes (`letter_planes`).
    rows: Vec<PackedRows>,
    /// One past the largest `opener_ordinal`.
    opener_count: usize,
}

/// Copies of a target's ranking fields.
#[derive(Clone, Copy)]
pub(crate) struct TargetHead {
    pub opener_ordinal: u32,
    pub cells: u32,
    /// `letters.is_some()`.
    pub colored: bool,
}

impl ShapeTargets {
    /// Target `index`'s ranking fields and packed row groups.
    pub(crate) fn packed(&self, index: usize) -> (TargetHead, &[PackedRows]) {
        let range = self.starts[index] as usize..self.starts[index + 1] as usize;
        (self.heads[index], &self.rows[range])
    }

    pub(crate) fn opener_count(&self) -> usize {
        self.opener_count
    }
}

impl From<Vec<ShapeTarget>> for ShapeTargets {
    fn from(targets: Vec<ShapeTarget>) -> Self {
        let mut starts = Vec::with_capacity(targets.len() + 1);
        let mut rows = Vec::new();
        for target in &targets {
            starts.push(u32::try_from(rows.len()).expect("target rows fit u32"));
            for (y, row) in target.rows.iter().enumerate() {
                let [p0, p1, p2] = target
                    .letter_masks
                    .get(y)
                    .map_or([0; 3], letter_planes);
                let lettered = target.lettered_cells.get(y).copied().unwrap_or_default();
                if y % ROWS_PER_GROUP == 0 {
                    rows.push([0; 5]);
                }
                let group = rows.last_mut().expect("a group was pushed");
                let shift = 16 * (y % ROWS_PER_GROUP);
                for (lane, value) in group.iter_mut().zip([*row, lettered, p0, p1, p2]) {
                    *lane |= u64::from(value) << shift;
                }
            }
        }
        starts.push(u32::try_from(rows.len()).expect("target rows fit u32"));
        let heads = targets
            .iter()
            .map(|target| TargetHead {
                opener_ordinal: target.opener_ordinal,
                cells: target.cells,
                colored: target.letters.is_some(),
            })
            .collect::<Vec<_>>();
        let opener_count = heads
            .iter()
            .map(|head| head.opener_ordinal as usize + 1)
            .max()
            .unwrap_or_default();
        Self {
            targets,
            starts,
            heads,
            rows,
            opener_count,
        }
    }
}

impl std::ops::Deref for ShapeTargets {
    type Target = [ShapeTarget];

    fn deref(&self) -> &Self::Target {
        &self.targets
    }
}

type Fx = BuildHasherDefault<crate::coach_beam::FxHasher64>;

/// The targets pushed so far by the hash of their (opener, rows, letters), so
/// a duplicate is found against the stored target without owning a key copy.
/// Only membership is read, never the iteration order.
#[derive(Default)]
struct TargetKeys {
    by_hash: HashMap<u64, Vec<u32>, Fx>,
}

impl TargetKeys {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            by_hash: HashMap::with_capacity_and_hasher(capacity, Fx::default()),
        }
    }

    /// Whether a target with these fields is already in `targets`.
    fn contains(
        &self,
        targets: &[ShapeTarget],
        hash: u64,
        opener_id: &str,
        rows: &[u16],
        letters: Option<&[String]>,
    ) -> bool {
        self.by_hash.get(&hash).is_some_and(|indices| {
            indices.iter().any(|&index| {
                let target = &targets[index as usize];
                target.opener_id == opener_id
                    && target.rows == rows
                    && target.letters.as_deref() == letters
            })
        })
    }
}

fn key_hash(opener_id: &str, rows: &[u16], letters: Option<&[String]>) -> u64 {
    Fx::default().hash_one((opener_id, rows, letters))
}

struct TargetSource<'a> {
    opener_id: &'a str,
    opener_ordinal: u32,
    label: String,
    rows_top_down: &'a [String],
    node_id: Option<u32>,
    route_name: Option<&'a str>,
}

pub(crate) fn build_targets(catalog: &OpenerCatalog) -> ShapeTargets {
    let mut targets = Vec::new();
    // At most two targets (plain and mirrored) per board of every node.
    let boards: usize = catalog
        .openers
        .iter()
        .flat_map(|opener| &opener.tree)
        .map(|node| 1 + usize::from(node.pre_clear_rows.is_some()))
        .sum();
    let mut seen = TargetKeys::with_capacity(2 * boards);

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

    targets.into()
}

fn push_target(targets: &mut Vec<ShapeTarget>, seen: &mut TargetKeys, source: TargetSource<'_>) {
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
        let hash = key_hash(source.opener_id, &target_rows, target_letters.as_deref());
        if seen.contains(
            targets,
            hash,
            source.opener_id,
            &target_rows,
            target_letters.as_deref(),
        ) {
            continue;
        }
        let index = u32::try_from(targets.len()).expect("fewer than 2^32 targets");
        seen.by_hash.entry(hash).or_default().push(index);

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

/// Target rows packed together for the board scan, one per 16-bit lane.
pub(crate) const ROWS_PER_GROUP: usize = 4;

/// Four packed target rows: occupancy, lettered cells, three letter planes.
pub(crate) type PackedRows = [u64; 5];

/// Each cell's letter as a code, its `PIECE_LETTERS` index plus one (0 for
/// none), split into three bit planes: two cells carry the same specific
/// letter exactly where both are lettered and every plane agrees.
pub(crate) fn letter_planes(masks: &[u16; PIECE_LETTERS.len()]) -> [u16; 3] {
    let mut planes = [0u16; 3];
    for (index, mask) in masks.iter().enumerate() {
        let code = index + 1;
        for (bit, plane) in planes.iter_mut().enumerate() {
            if code >> bit & 1 != 0 {
                *plane |= mask;
            }
        }
    }
    planes
}

/// One occupancy mask per piece letter for a row of letter cells.
pub(crate) fn letter_masks_of(row: &str) -> [u16; PIECE_LETTERS.len()] {
    let mut masks = [0u16; PIECE_LETTERS.len()];
    for (x, letter) in row.bytes().take(10).enumerate() {
        // `PIECE_LETTERS` order.
        let index = match letter {
            b'I' => 0,
            b'J' => 1,
            b'L' => 2,
            b'O' => 3,
            b'S' => 4,
            b'T' => 5,
            b'Z' => 6,
            _ => continue,
        };
        masks[index] |= 1 << x;
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
    fn letter_planes_agree_exactly_where_letters_match() {
        // Every pair of cell letters, as the matcher compares them: a lettered
        // target cell agrees with the board cell iff the per-letter masks
        // share it.
        let letters = [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)];
        for board in letters {
            for target in letters {
                let masks = |letter: Option<usize>| {
                    let mut masks = [0u16; super::PIECE_LETTERS.len()];
                    if let Some(index) = letter {
                        masks[index] = 1;
                    }
                    masks
                };
                let (board_masks, target_masks) = (masks(board), masks(target));
                let by_masks = board_masks
                    .iter()
                    .zip(&target_masks)
                    .fold(0, |kept, (b, t)| kept | (b & t));
                let [b0, b1, b2] = super::letter_planes(&board_masks);
                let [t0, t1, t2] = super::letter_planes(&target_masks);
                // As the matcher keeps a common lettered cell: lettered by any
                // character, specific or not.
                for lettered in [0u16, 1] {
                    let old = (1 & !lettered) | (lettered & by_masks);
                    let new = 1 & !(lettered & ((b0 ^ t0) | (b1 ^ t1) | (b2 ^ t2) | !(t0 | t1 | t2)));
                    assert_eq!(new, old, "board {board:?} target {target:?} lettered {lettered}");
                }
            }
        }
    }

    #[test]
    fn letter_masks_follow_piece_letter_order() {
        for (index, letter) in super::PIECE_LETTERS.iter().enumerate() {
            let row = format!("{}_X_______", *letter as char);
            let mut expected = [0u16; super::PIECE_LETTERS.len()];
            expected[index] = 1;
            assert_eq!(super::letter_masks_of(&row), expected);
        }
    }

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
