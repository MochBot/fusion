use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::openers::board::{mirror_mask_10, rows_to_masks_floor_up, strip_garbage_rows};
use crate::openers::catalog::{OpenerCatalog, OpenerRecord};
use crate::openers::phase::OpenerObservation;
use crate::openers::route::resolve_report_route;
use crate::openers::witness_catalog::RuntimeSearchShapeTarget;

/// One catalogued tree node's occupancy in both chiralities plus its resolved
/// route name, computed once per catalog so confirmation never re-parses
/// authored rows per observation.
#[derive(Clone, Debug)]
pub(crate) struct NodeBoard {
    pub record_index: u32,
    pub node_id: u32,
    pub pieces: u32,
    pub rows: Vec<u16>,
    pub mirrored: Vec<u16>,
    pub route_name: Option<String>,
}

/// Node boards grouped by locked-piece ordinal, in catalog record/tree order.
#[derive(Clone, Debug, Default)]
pub(crate) struct NodeBoards {
    by_pieces: Vec<Vec<NodeBoard>>,
}

impl NodeBoards {
    pub(crate) fn build(catalog: &OpenerCatalog) -> Self {
        let mut by_pieces: Vec<Vec<NodeBoard>> = Vec::new();
        for (record_index, record) in catalog.openers.iter().enumerate() {
            for node in &record.tree {
                let rows = rows_to_masks_floor_up(&node.rows);
                let mirrored = rows.iter().map(|row| mirror_mask_10(*row)).collect();
                let slot = node.pieces as usize;
                if by_pieces.len() <= slot {
                    by_pieces.resize_with(slot + 1, Vec::new);
                }
                by_pieces[slot].push(NodeBoard {
                    record_index: u32::try_from(record_index).unwrap_or(u32::MAX),
                    node_id: node.id,
                    pieces: node.pieces,
                    rows,
                    mirrored,
                    route_name: resolve_report_route(record, &[node.id])
                        .route_name
                        .map(ToOwned::to_owned),
                });
            }
        }
        Self { by_pieces }
    }

    fn at(&self, pieces: u32) -> &[NodeBoard] {
        self.by_pieces
            .get(pieces as usize)
            .map_or(&[], Vec::as_slice)
    }

    fn at_or_deeper(&self, pieces: u32) -> impl Iterator<Item = &NodeBoard> {
        self.by_pieces
            .iter()
            .skip(pieces as usize)
            .flat_map(Vec::as_slice)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchingOpener {
    pub id: String,
    pub name: String,
    pub deepest_pieces: u32,
    pub candidate_node_ids: Vec<u32>,
    pub route_name: Option<String>,
    pub mirrored: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundCataloguedBoardMatch {
    pub first_match_index: usize,
    pub anchor_index: usize,
    pub matching_openers: Vec<MatchingOpener>,
}

#[derive(Clone)]
struct ExactCandidate<'a> {
    record: &'a OpenerRecord,
    node_id: Option<u32>,
    pieces: u32,
    mirrored: Option<bool>,
    route_name: Option<String>,
}

#[cfg(test)]
pub(crate) fn match_catalogued_boards(
    catalog: &OpenerCatalog,
    observations: &[Option<OpenerObservation>],
) -> Option<RoundCataloguedBoardMatch> {
    match_catalogued_boards_with_targets(catalog, &NodeBoards::build(catalog), &[], observations)
}

pub(crate) fn match_catalogued_boards_with_targets(
    catalog: &OpenerCatalog,
    node_boards: &NodeBoards,
    runtime_targets: &[RuntimeSearchShapeTarget],
    observations: &[Option<OpenerObservation>],
) -> Option<RoundCataloguedBoardMatch> {
    let mut first_match_index = None;
    let mut anchor_index = 0;
    let mut survivors = BTreeSet::new();
    let mut anchor_candidates = Vec::new();

    for (index, observation) in observations.iter().enumerate() {
        let Some(observation) = observation.as_ref() else {
            continue;
        };
        let Some(mut candidates) = exact_candidates(
            catalog,
            node_boards,
            runtime_targets,
            observation,
            index,
            &survivors,
        ) else {
            continue;
        };
        if candidates.is_empty()
            && !survivors.is_empty()
            && !surviving_record_continues_at(
                catalog,
                node_boards,
                runtime_targets,
                &survivors,
                index,
            )
        {
            if let Some(all_candidates) = exact_candidates(
                catalog,
                node_boards,
                runtime_targets,
                observation,
                index,
                &BTreeSet::new(),
            ) {
                candidates = all_candidates;
            }
        }
        if candidates.is_empty() {
            continue;
        }

        if first_match_index.is_none() {
            survivors = candidates
                .iter()
                .map(|candidate| candidate.record.id.clone())
                .collect();
            first_match_index = Some(index);
        } else {
            survivors = candidates
                .iter()
                .map(|candidate| candidate.record.id.clone())
                .collect();
        }
        anchor_index = index;
        anchor_candidates = candidates;
    }

    let first_match_index = first_match_index?;
    Some(to_match(
        first_match_index,
        anchor_index,
        &survivors,
        &anchor_candidates,
    ))
}

/// False only once every survivor has ended, which is what lets a later exact
/// board re-anchor; a survivor that merely skips this ordinal still continues.
fn surviving_record_continues_at(
    catalog: &OpenerCatalog,
    node_boards: &NodeBoards,
    runtime_targets: &[RuntimeSearchShapeTarget],
    survivors: &BTreeSet<String>,
    pieces: usize,
) -> bool {
    let Ok(pieces) = u32::try_from(pieces.saturating_add(1)) else {
        return false;
    };
    node_boards.at_or_deeper(pieces).any(|node| {
        catalog
            .openers
            .get(node.record_index as usize)
            .is_some_and(|record| survivors.contains(&record.id))
    }) || runtime_targets.iter().any(|target| {
        target.locked_piece_ordinal >= pieces && survivors.contains(&target.record_id)
    })
}

fn exact_candidates<'a>(
    catalog: &'a OpenerCatalog,
    node_boards: &NodeBoards,
    runtime_targets: &[RuntimeSearchShapeTarget],
    observation: &OpenerObservation,
    index: usize,
    survivors: &BTreeSet<String>,
) -> Option<Vec<ExactCandidate<'a>>> {
    let board = observation.post_board.as_deref()?;
    let garbage = observation.post_gmask.as_deref()?;
    let normalized = strip_garbage_rows(board, garbage, observation.post_letters.as_deref());
    let pieces = u32::try_from(index.saturating_add(1)).ok()?;
    let mut candidates = Vec::new();

    for node in node_boards.at(pieces) {
        let record = &catalog.openers[node.record_index as usize];
        if !survivors.is_empty() && !survivors.contains(&record.id) {
            continue;
        }
        let symmetric = node.rows == node.mirrored;
        if same_occupancy(&normalized.masks, &node.rows) {
            candidates.push(ExactCandidate {
                record,
                node_id: Some(node.node_id),
                pieces: node.pieces,
                mirrored: Some(false),
                route_name: node.route_name.clone(),
            });
            if symmetric {
                candidates.push(ExactCandidate {
                    record,
                    node_id: Some(node.node_id),
                    pieces: node.pieces,
                    mirrored: Some(true),
                    route_name: node.route_name.clone(),
                });
            }
            continue;
        }
        if !symmetric && same_occupancy(&normalized.masks, &node.mirrored) {
            candidates.push(ExactCandidate {
                record,
                node_id: Some(node.node_id),
                pieces: node.pieces,
                mirrored: Some(true),
                route_name: node.route_name.clone(),
            });
        }
    }
    for target in runtime_targets
        .iter()
        .filter(|target| target.locked_piece_ordinal == pieces)
    {
        if !survivors.is_empty() && !survivors.contains(&target.record_id) {
            continue;
        }
        if candidates
            .iter()
            .any(|candidate| candidate.record.id == target.record_id)
        {
            continue;
        }
        if !same_occupancy(&normalized.masks, &target.rows) {
            continue;
        }
        let Some(record) = catalog
            .openers
            .iter()
            .find(|record| record.id == target.record_id)
        else {
            continue;
        };
        candidates.push(ExactCandidate {
            record,
            node_id: None,
            pieces,
            mirrored: None,
            route_name: None,
        });
    }
    candidates.sort_by(|left, right| {
        left.record
            .id
            .cmp(&right.record.id)
            .then(left.node_id.cmp(&right.node_id))
            .then(left.mirrored.cmp(&right.mirrored))
    });
    Some(candidates)
}

fn same_occupancy(left: &[u16], right: &[u16]) -> bool {
    let max_rows = left.len().max(right.len());
    (0..max_rows).all(|index| {
        left.get(index).copied().unwrap_or_default()
            == right.get(index).copied().unwrap_or_default()
    })
}

fn to_match(
    first_match_index: usize,
    anchor_index: usize,
    survivors: &BTreeSet<String>,
    candidates: &[ExactCandidate<'_>],
) -> RoundCataloguedBoardMatch {
    let matching_openers = survivors
        .iter()
        .filter_map(|id| {
            let opener_candidates: Vec<_> = candidates
                .iter()
                .filter(|candidate| candidate.record.id == *id)
                .collect();
            let candidate = opener_candidates.first()?;
            let mut candidate_node_ids: Vec<u32> = opener_candidates
                .iter()
                .filter_map(|candidate| candidate.node_id)
                .collect();
            candidate_node_ids.sort_unstable();
            candidate_node_ids.dedup();
            let orientations: BTreeSet<Option<bool>> = opener_candidates
                .iter()
                .map(|candidate| candidate.mirrored)
                .collect();
            let mirrored = (orientations.len() == 1)
                .then(|| orientations.iter().next().copied().flatten())
                .flatten();
            let routes: BTreeSet<Option<String>> = opener_candidates
                .iter()
                .map(|candidate| candidate.route_name.clone())
                .collect();
            let route_name = (routes.len() == 1)
                .then(|| routes.first().cloned().flatten())
                .flatten();

            Some(MatchingOpener {
                id: id.clone(),
                name: candidate.record.aliases.en.clone(),
                deepest_pieces: candidate.pieces,
                candidate_node_ids,
                route_name,
                mirrored,
            })
        })
        .collect();

    RoundCataloguedBoardMatch {
        first_match_index,
        anchor_index,
        matching_openers,
    }
}
