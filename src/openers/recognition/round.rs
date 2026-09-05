#[cfg(test)]
#[path = "round_tests.rs"]
mod tests;
use serde::{Deserialize, Serialize};

use super::align::{align_round_exact_retaining_record, AlignBudget, Observation};
use super::cache::RecordGraphCache;
use super::cost::{EditCosts, EditOps};
use super::graph::CanonicalKey;
use crate::openers::board::strip_garbage_rows;
use crate::openers::catalog::OpenerCatalog;
use crate::openers::catalogued_match::{MatchingOpener, RoundCataloguedBoardMatch};
use crate::openers::phase::{OpenerAssessment, OpenerObservation};

const MAX_SHORTLIST: usize = 24;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Consumers may key UI off the report record's hypothesis; ranked alternates are tooling evidence.
pub(crate) struct RoundRecognition {
    pub(crate) retrieval_bounded: bool,
    pub(crate) shortlist_size: usize,
    pub(crate) truncated: bool,
    pub(crate) shortlist_compile_skipped: usize,
    pub(crate) per_lock: Vec<RoundLockRecognition>,
    pub(crate) hypotheses: Vec<RoundHypothesis>,
    pub(crate) unknown_cost: u32,
    pub(crate) edit_costs: EditCosts,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoundLockRecognition {
    pub(crate) best_cost: Option<u32>,
    pub(crate) unknown_cost: u32,
    pub(crate) active_states: u32,
    pub(crate) exact_hits: u32,
    pub(crate) opaque_entries: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Recognition keys are mirror-folded, so cost is chirality-invariant; only grey-node opacity
/// freezes identity, while bridge costs preserve it for v1 evidence.
pub(crate) struct RoundHypothesis {
    pub(crate) record: String,
    /// Deepest surviving node and its route; naming needs both.
    pub(crate) node_id: u32,
    pub(crate) route_name: Option<String>,
    pub(crate) mirrored: bool,
    pub(crate) total_cost: u32,
    pub(crate) margin: u32,
    pub(crate) ops: EditOps,
    pub(crate) identity_frozen: bool,
}

/// `catalogued_board_match` must be the whole-round confirmation computed for
/// these same observations; recognition's candidate set derives from it.
pub(crate) fn recognize_round(
    catalog: Option<&OpenerCatalog>,
    compiled: &RecordGraphCache,
    assessments: &[Option<OpenerAssessment>],
    catalogued_board_match: Option<&RoundCataloguedBoardMatch>,
    observations: &[Option<OpenerObservation>],
) -> Option<RoundRecognition> {
    let catalog = catalog?;
    let confirmed_id = singleton_confirmed_id(catalogued_board_match);
    let shortlist = shortlist(assessments, catalogued_board_match);
    if shortlist.ids.is_empty() {
        return None;
    }

    let mapped_observations = observations
        .iter()
        .map(|observation| observation.as_ref().and_then(observation_for))
        .collect::<Vec<_>>();
    if mapped_observations.iter().all(Option::is_none) {
        return None;
    }

    let compiled = compiled.shortlist_graph(catalog, &shortlist.ids)?;
    let shortlist_compile_skipped = compiled.compile_skipped;

    let edit_costs = EditCosts::default();
    let alignment = align_round_exact_retaining_record(
        &compiled.graph,
        &edit_costs,
        &mapped_observations,
        &AlignBudget::default(),
        confirmed_id,
    );
    let per_lock = alignment
        .per_lock
        .iter()
        .zip(&mapped_observations)
        .filter_map(|(lock, observation)| {
            observation.as_ref().map(|_| RoundLockRecognition {
                best_cost: lock.best_cost,
                unknown_cost: lock.unknown_cost,
                active_states: lock.active_product_states,
                exact_hits: lock.exact_hits,
                opaque_entries: lock.opaque_entries,
            })
        })
        .collect();
    let hypotheses = alignment
        .hypotheses
        .iter()
        .enumerate()
        .filter_map(|(index, hypothesis)| {
            let origin = hypothesis.origin.as_ref()?;
            let margin = if index == 0 {
                alignment.best_margin.unwrap_or_default()
            } else {
                alignment
                    .hypotheses
                    .get(index + 1)
                    .map_or(hypothesis.total_cost, |next| next.total_cost)
                    .saturating_sub(hypothesis.total_cost)
            };
            Some(RoundHypothesis {
                record: origin.record.to_string(),
                node_id: origin.node_id,
                route_name: origin.route_name.as_ref().map(ToString::to_string),
                mirrored: origin.mirrored,
                total_cost: hypothesis.total_cost,
                margin,
                ops: hypothesis.ops,
                identity_frozen: hypothesis.identity_frozen,
            })
        })
        .collect();

    Some(RoundRecognition {
        retrieval_bounded: true,
        shortlist_size: shortlist.ids.len(),
        truncated: shortlist.truncated || alignment.truncated_any,
        shortlist_compile_skipped,
        per_lock,
        hypotheses,
        unknown_cost: alignment
            .per_lock
            .last()
            .map_or(0, |lock| lock.unknown_cost),
        edit_costs,
    })
}

fn singleton_confirmed_id(matched: Option<&RoundCataloguedBoardMatch>) -> Option<&str> {
    let [MatchingOpener { id, .. }] = matched?.matching_openers.as_slice() else {
        return None;
    };
    Some(id)
}

fn observation_for(observation: &OpenerObservation) -> Option<Observation> {
    let board = observation.post_board.as_deref()?;
    let garbage_mask = observation.post_gmask.as_deref()?;
    let normalized = strip_garbage_rows(board, garbage_mask, observation.post_letters.as_deref());
    Some(Observation {
        key: CanonicalKey::from_normalized_rows(&normalized.masks, normalized.letters.as_deref()),
        had_garbage: garbage_mask.iter().any(|mask| *mask != 0),
    })
}

struct Shortlist {
    ids: Vec<String>,
    truncated: bool,
}

/// An exact hit narrows the walk, it does not decide it, so confirmed records
/// are admitted alongside the assessment candidates rather than replacing them.
fn shortlist(
    assessments: &[Option<OpenerAssessment>],
    catalogued_board_match: Option<&RoundCataloguedBoardMatch>,
) -> Shortlist {
    let mut ids = Vec::new();
    if let Some(matched) = catalogued_board_match {
        for opener in &matched.matching_openers {
            push_unique(&mut ids, &opener.id);
        }
    }
    for assessment in assessments.iter().flatten() {
        if let Some(board_match) = &assessment.r#match {
            push_unique(&mut ids, &board_match.opener_id);
        }
        for runner_up in &assessment.runners_up {
            push_unique(&mut ids, &runner_up.opener_id);
        }
    }
    let truncated = ids.len() > MAX_SHORTLIST;
    ids.truncate(MAX_SHORTLIST);
    Shortlist { ids, truncated }
}

fn push_unique(ids: &mut Vec<String>, opener_id: &str) {
    if !ids.iter().any(|candidate| candidate == opener_id) {
        ids.push(opener_id.to_owned());
    }
}
