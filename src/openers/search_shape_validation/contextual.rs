use std::collections::HashSet;
use std::fmt;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::default_ruleset::ACTIVE_RULES;
use crate::openers::board::{mirror_letter_row, rows_to_masks_floor_up};
use crate::openers::catalog::{parse_catalog, OpenerCatalog, OpenerRecord, OpenerTreeNode};
use crate::openers::CatalogError;

use super::direct_target::{DirectTargetFrame, DirectTargetOutcome, DirectTargetSearch};
use super::frames::node_pre_clear_rows;
use super::model::{CandidateFrame, MirrorRelation, RulesetReceipt, SearchReceipt, Witness};
use super::search::{ExactOrdinalSearch, SearchOutcome};
use super::verify::verify_witness;

#[derive(Debug)]
pub enum ContextualTargetValidationError {
    Catalog(CatalogError),
    MalformedBatch(serde_json::Error),
    UnsupportedSchemaVersion(u32),
    InvalidBatchIdentity,
}

impl fmt::Display for ContextualTargetValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalog(error) => write!(formatter, "catalog validation failed: {error}"),
            Self::MalformedBatch(error) => {
                write!(formatter, "malformed contextual-target batch: {error}")
            }
            Self::UnsupportedSchemaVersion(found) => {
                write!(
                    formatter,
                    "unsupported contextual-target schema version: {found}"
                )
            }
            Self::InvalidBatchIdentity => {
                write!(
                    formatter,
                    "runId, inputAssetSha256, and search bounds are invalid"
                )
            }
        }
    }
}

impl std::error::Error for ContextualTargetValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Catalog(error) => Some(error),
            Self::MalformedBatch(error) => Some(error),
            Self::UnsupportedSchemaVersion(_) | Self::InvalidBatchIdentity => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BatchV1 {
    schema_version: u32,
    run_id: String,
    input_asset_sha256: String,
    search_fuel: u32,
    max_locks: u32,
    candidates: Vec<CandidateInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CandidateInput {
    candidate_id: String,
    candidate_digest: String,
    record_id: String,
    search_shape_index: u32,
    target_rows: Vec<String>,
    hints: Vec<CatalogHint>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CatalogHint {
    node_id: u32,
    pieces: u32,
    frame: CandidateFrame,
    mirror_relation: MirrorRelation,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextualTargetResultsV1 {
    schema_version: u32,
    run_id: String,
    input_asset_sha256: String,
    fusion_revision: &'static str,
    ruleset: RulesetReceipt,
    results: Vec<CandidateResult>,
}

#[derive(Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum CandidateResult {
    WitnessFound {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        search_shape_index: u32,
        proof_method: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        selected_hint: Option<CatalogHint>,
        #[serde(skip_serializing_if = "Option::is_none")]
        target_frame: Option<&'static str>,
        witness: Witness,
        receipt: SearchReceipt,
    },
    Inconclusive {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        search_shape_index: u32,
        reason: InconclusiveReason,
        receipt: SearchReceipt,
    },
    InvalidInput {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        search_shape_index: u32,
        reason: &'static str,
    },
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum InconclusiveReason {
    BudgetExhausted,
    NoVerifiedTargetWitness,
}

struct Context<'a> {
    catalog: &'a OpenerCatalog,
    fuel_limit: u32,
    max_locks: u32,
}

pub fn validate_contextual_target_batch(
    catalog_json: &[u8],
    batch_json: &[u8],
) -> Result<ContextualTargetResultsV1, ContextualTargetValidationError> {
    let catalog = parse_catalog(catalog_json).map_err(ContextualTargetValidationError::Catalog)?;
    let batch: BatchV1 = serde_json::from_slice(batch_json)
        .map_err(ContextualTargetValidationError::MalformedBatch)?;
    if batch.schema_version != 1 {
        return Err(ContextualTargetValidationError::UnsupportedSchemaVersion(
            batch.schema_version,
        ));
    }
    if batch.run_id.is_empty()
        || !is_sha256(&batch.input_asset_sha256)
        || batch.search_fuel == 0
        || batch.max_locks == 0
    {
        return Err(ContextualTargetValidationError::InvalidBatchIdentity);
    }
    let context = Context {
        catalog: &catalog,
        fuel_limit: batch.search_fuel,
        max_locks: batch.max_locks,
    };
    let mut seen = HashSet::new();
    let duplicates = batch
        .candidates
        .iter()
        .map(|candidate| !seen.insert((&candidate.candidate_id, &candidate.candidate_digest)))
        .collect::<Vec<_>>();
    let results = batch
        .candidates
        .par_iter()
        .zip(duplicates.into_par_iter())
        .map(|(candidate, duplicate)| validate_candidate(&context, candidate, duplicate))
        .collect();
    Ok(ContextualTargetResultsV1 {
        schema_version: 1,
        run_id: batch.run_id,
        input_asset_sha256: batch.input_asset_sha256,
        fusion_revision: option_env!("FUSION_REVISION").unwrap_or(env!("CARGO_PKG_VERSION")),
        ruleset: RulesetReceipt {
            name: "tetraLeagueSrsPlus",
            enable_180: ACTIVE_RULES.enable_180,
            enable_tspin: ACTIVE_RULES.enable_tspin,
            enable_allspin: ACTIVE_RULES.enable_allspin,
            srs_plus: ACTIVE_RULES.srs_plus,
        },
        results,
    })
}

fn validate_candidate(
    context: &Context<'_>,
    candidate: &CandidateInput,
    duplicate: bool,
) -> CandidateResult {
    if duplicate || !candidate_is_valid(candidate) {
        return invalid(candidate, "candidate fields must be complete and unique");
    }
    let Some(record) = context
        .catalog
        .openers
        .iter()
        .find(|record| record.id == candidate.record_id)
    else {
        return invalid(candidate, "recordId is absent from the catalog");
    };
    let mut fuel_remaining = context.fuel_limit;
    let mut last_receipt = empty_receipt(context.fuel_limit);
    for hint in &candidate.hints {
        if hint.pieces > context.max_locks {
            continue;
        }
        let Some(prepared) = prepare_hint(record, candidate, hint) else {
            continue;
        };
        if fuel_remaining == 0 {
            return inconclusive(candidate, InconclusiveReason::BudgetExhausted, last_receipt);
        }
        let mut search = ExactOrdinalSearch::new(
            prepared.nodes,
            prepared.mirrored,
            hint.pieces,
            prepared.target_pre_clear_rows,
            None,
            fuel_remaining,
        );
        match search.run() {
            SearchOutcome::Found(locks) => {
                let receipt = receipt_for_batch(search.receipt(), context.fuel_limit);
                if !verify_witness(&locks) {
                    last_receipt = receipt;
                    fuel_remaining = receipt.fuel_remaining;
                    continue;
                }
                return CandidateResult::WitnessFound {
                    candidate_id: candidate.candidate_id.clone(),
                    candidate_digest: candidate.candidate_digest.clone(),
                    record_id: candidate.record_id.clone(),
                    search_shape_index: candidate.search_shape_index,
                    proof_method: "catalogHintStrictReplay",
                    selected_hint: Some(hint.clone()),
                    target_frame: None,
                    witness: Witness {
                        mirrored: prepared.mirrored,
                        confirmation_rows: locks
                            .last()
                            .map_or_else(Vec::new, |lock| lock.rows_after.clone()),
                        locks,
                    },
                    receipt,
                };
            }
            SearchOutcome::Exhausted => {
                return inconclusive(
                    candidate,
                    InconclusiveReason::BudgetExhausted,
                    receipt_for_batch(search.receipt(), context.fuel_limit),
                );
            }
            SearchOutcome::Absent => {
                last_receipt = receipt_for_batch(search.receipt(), context.fuel_limit);
                fuel_remaining = last_receipt.fuel_remaining;
            }
        }
    }
    if fuel_remaining == 0 {
        return inconclusive(candidate, InconclusiveReason::BudgetExhausted, last_receipt);
    }
    let mut direct =
        DirectTargetSearch::new(&candidate.target_rows, context.max_locks, fuel_remaining);
    match direct.run() {
        DirectTargetOutcome::Found { locks, frame } if verify_witness(&locks) => {
            let receipt = combined_receipt(last_receipt, direct.receipt(), context.fuel_limit);
            CandidateResult::WitnessFound {
                candidate_id: candidate.candidate_id.clone(),
                candidate_digest: candidate.candidate_digest.clone(),
                record_id: candidate.record_id.clone(),
                search_shape_index: candidate.search_shape_index,
                proof_method: "exactTargetOccupancySearch",
                selected_hint: None,
                target_frame: Some(match frame {
                    DirectTargetFrame::PostClear => "postClear",
                    DirectTargetFrame::PreClear => "preClear",
                }),
                witness: Witness {
                    mirrored: false,
                    confirmation_rows: candidate.target_rows.clone(),
                    locks,
                },
                receipt,
            }
        }
        DirectTargetOutcome::Exhausted => inconclusive(
            candidate,
            InconclusiveReason::BudgetExhausted,
            combined_receipt(last_receipt, direct.receipt(), context.fuel_limit),
        ),
        DirectTargetOutcome::Found { .. } | DirectTargetOutcome::Absent => inconclusive(
            candidate,
            InconclusiveReason::NoVerifiedTargetWitness,
            combined_receipt(last_receipt, direct.receipt(), context.fuel_limit),
        ),
    }
}

struct PreparedHint<'a> {
    nodes: Vec<&'a OpenerTreeNode>,
    mirrored: bool,
    target_pre_clear_rows: Option<Vec<String>>,
}

fn prepare_hint<'a>(
    record: &'a OpenerRecord,
    candidate: &CandidateInput,
    hint: &CatalogHint,
) -> Option<PreparedHint<'a>> {
    let node = record.tree.iter().find(|node| node.id == hint.node_id)?;
    if node.pieces != hint.pieces {
        return None;
    }
    let mirrored = matches!(hint.mirror_relation, MirrorRelation::Mirrored);
    let target_pre_clear_rows = match hint.frame {
        CandidateFrame::PreClear => {
            same_rows(&candidate.target_rows, node_pre_clear_rows(node), mirrored)
                .then(|| candidate.target_rows.clone())
        }
        CandidateFrame::PostClear => {
            if !same_rows(&candidate.target_rows, &node.rows, mirrored) {
                return None;
            }
            None
        }
        CandidateFrame::SourcePreClear => return None,
    };
    if matches!(hint.frame, CandidateFrame::PreClear) && target_pre_clear_rows.is_none() {
        return None;
    }
    Some(PreparedHint {
        nodes: lineage(record, node.id)?,
        mirrored,
        target_pre_clear_rows,
    })
}

fn lineage(record: &OpenerRecord, node_id: u32) -> Option<Vec<&OpenerTreeNode>> {
    let mut nodes = Vec::new();
    let mut current = record.tree.iter().find(|node| node.id == node_id)?;
    loop {
        nodes.push(current);
        let Some(parent_id) = current.parent else {
            break;
        };
        current = record.tree.iter().find(|node| node.id == parent_id)?;
    }
    nodes.reverse();
    Some(nodes)
}

fn same_rows(left: &[String], right: &[String], mirrored: bool) -> bool {
    let right = if mirrored {
        right.iter().map(|row| mirror_letter_row(row)).collect()
    } else {
        right.to_vec()
    };
    rows_to_masks_floor_up(left) == rows_to_masks_floor_up(&right)
}

fn candidate_is_valid(candidate: &CandidateInput) -> bool {
    !candidate.candidate_id.is_empty()
        && is_sha256(&candidate.candidate_digest)
        && !candidate.record_id.is_empty()
        && rows_are_valid(&candidate.target_rows)
        && candidate.hints.iter().all(|hint| hint.pieces > 0)
}

fn rows_are_valid(rows: &[String]) -> bool {
    rows.len() <= 40
        && rows
            .iter()
            .all(|row| row.len() == 10 && row.bytes().all(|cell| cell == b'_' || cell == b'X'))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn empty_receipt(fuel_limit: u32) -> SearchReceipt {
    SearchReceipt {
        explored_states: 0,
        fuel_limit,
        fuel_remaining: fuel_limit,
    }
}

fn receipt_for_batch(receipt: SearchReceipt, fuel_limit: u32) -> SearchReceipt {
    SearchReceipt {
        fuel_limit,
        ..receipt
    }
}

fn combined_receipt(
    prior: SearchReceipt,
    current: SearchReceipt,
    fuel_limit: u32,
) -> SearchReceipt {
    SearchReceipt {
        explored_states: prior
            .explored_states
            .saturating_add(current.explored_states),
        fuel_limit,
        fuel_remaining: current.fuel_remaining,
    }
}

fn inconclusive(
    candidate: &CandidateInput,
    reason: InconclusiveReason,
    receipt: SearchReceipt,
) -> CandidateResult {
    CandidateResult::Inconclusive {
        candidate_id: candidate.candidate_id.clone(),
        candidate_digest: candidate.candidate_digest.clone(),
        record_id: candidate.record_id.clone(),
        search_shape_index: candidate.search_shape_index,
        reason,
        receipt,
    }
}

fn invalid(candidate: &CandidateInput, reason: &'static str) -> CandidateResult {
    CandidateResult::InvalidInput {
        candidate_id: candidate.candidate_id.clone(),
        candidate_digest: candidate.candidate_digest.clone(),
        record_id: candidate.record_id.clone(),
        search_shape_index: candidate.search_shape_index,
        reason,
    }
}
