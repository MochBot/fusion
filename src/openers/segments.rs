mod tile;

use serde::{Deserialize, Serialize};

use crate::openers::board::mirror_letter_row;

pub const SHOWCASE_WIDTH: usize = 10;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ShowcasePlacement {
    pub letter: String,
    pub cells: Vec<[u8; 2]>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DerivedStep {
    pub placements: Vec<ShowcasePlacement>,
    pub merged: Vec<String>,
    pub clear_rows: Vec<u8>,
}

pub(crate) fn floor_up_letters(rows_top_down: &[String], mirrored: bool) -> Vec<String> {
    let mut rows = Vec::with_capacity(rows_top_down.len());
    for row in rows_top_down.iter().rev() {
        rows.push(if mirrored {
            mirror_letter_row(row)
        } else {
            row.clone()
        });
    }
    while rows
        .last()
        .is_some_and(|row| row.chars().all(|letter| letter == '_'))
    {
        rows.pop();
    }
    rows
}

#[cfg(test)]
pub(super) fn clear_full_rows(rows: &[String]) -> Vec<String> {
    rows.iter()
        .filter(|row| !is_full_row(row))
        .cloned()
        .collect()
}

pub(crate) fn derive_placements(parent: &[String], child: &[String]) -> Option<DerivedStep> {
    let (placements, merged) = tile::derive(parent, child)?;
    Some(DerivedStep {
        placements,
        clear_rows: full_rows(&merged)?,
        merged,
    })
}

fn full_rows(rows: &[String]) -> Option<Vec<u8>> {
    let mut full = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        if is_full_row(row) {
            full.push(u8::try_from(y).ok()?);
        }
    }
    Some(full)
}

fn is_full_row(row: &str) -> bool {
    row.len() == SHOWCASE_WIDTH && !row.contains('_')
}
