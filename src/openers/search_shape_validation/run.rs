use std::collections::HashSet;

use rayon::prelude::*;

use crate::default_ruleset::ACTIVE_RULES;
use crate::openers::board::{mirror_letter_row, rows_to_masks_floor_up};
use crate::openers::catalog::{parse_catalog, OpenerCatalog, OpenerRecord, OpenerTreeNode};

use super::frames::node_pre_clear_rows;
use super::model::{
    CandidateFrame, CandidateInput, CandidateResult, FrameEvidence, InconclusiveReason,
    MirrorRelation, ProofMethod, RulesetReceipt, SearchReceipt, ValidationBatchV1, ValidationError,
    ValidationResultsV1, Witness,
};
use super::search::{ExactOrdinalSearch, SearchOutcome};
use super::verify::verify_witness;

const FUSION_REVISION: &str = match option_env!("FUSION_REVISION") {
    Some(revision) => revision,
    None => env!("CARGO_PKG_VERSION"),
};

pub(super) fn validate(
    catalog_json: &[u8],
    batch_json: &[u8],
) -> Result<ValidationResultsV1, ValidationError> {
    let catalog = parse_catalog(catalog_json).map_err(ValidationError::Catalog)?;
    let batch: ValidationBatchV1 = serde_json::from_slice(batch_json)
        .map_err(|source| ValidationError::MalformedBatch { source })?;
    if batch.schema_version != 1 {
        return Err(ValidationError::UnsupportedSchemaVersion {
            found: batch.schema_version,
        });
    }
    if batch.run_id.is_empty() || !is_lower_hex_sha256(&batch.input_asset_sha256) {
        return Err(ValidationError::InvalidBatchIdentity);
    }
    let context = ValidationContext {
        catalog: &catalog,
        fuel_limit: batch.search_fuel,
    };
    let mut seen = HashSet::new();
    let duplicates = batch
        .candidates
        .iter()
        .map(|candidate| !seen.insert(candidate_key(candidate)))
        .collect::<Vec<_>>();
    let results = batch
        .candidates
        .par_iter()
        .zip(duplicates.into_par_iter())
        .map(|(candidate, duplicate)| validate_candidate(&context, candidate, duplicate))
        .collect();
    Ok(ValidationResultsV1 {
        schema_version: 1,
        run_id: batch.run_id,
        input_asset_sha256: batch.input_asset_sha256,
        fusion_revision: FUSION_REVISION,
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

struct ValidationContext<'a> {
    catalog: &'a OpenerCatalog,
    fuel_limit: u32,
}

fn candidate_key(candidate: &CandidateInput) -> (&str, &str) {
    (&candidate.candidate_id, &candidate.candidate_digest)
}

fn validate_candidate(
    context: &ValidationContext<'_>,
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
    if !evidence_ordinals_match(record, candidate) {
        return invalid(candidate, "frameEvidence must share lockedPieceOrdinal");
    }
    let mut fuel_remaining = context.fuel_limit;
    let mut last_receipt = SearchReceipt {
        explored_states: 0,
        fuel_limit: context.fuel_limit,
        fuel_remaining,
    };
    for evidence in &candidate.frame_evidence {
        let Some(prepared) = prepare_evidence(record, candidate, evidence) else {
            continue;
        };
        if fuel_remaining == 0 {
            return inconclusive(candidate, InconclusiveReason::BudgetExhausted, last_receipt);
        }
        let mut search = ExactOrdinalSearch::new(
            prepared.nodes,
            prepared.mirrored,
            prepared.ordinal,
            prepared.target_pre_clear_rows,
            prepared.target_construction_rows,
            fuel_remaining,
        );
        match search.run() {
            SearchOutcome::Found(locks) => {
                let receipt = receipt_for_batch(search.receipt(), context.fuel_limit);
                let verified = verify_witness(&locks);
                if !verified {
                    last_receipt = receipt;
                    fuel_remaining = last_receipt.fuel_remaining;
                    continue;
                }
                return CandidateResult::WitnessFound {
                    candidate_id: candidate.candidate_id.clone(),
                    candidate_digest: candidate.candidate_digest.clone(),
                    record_id: candidate.record_id.clone(),
                    proof_method: ProofMethod::SourceTreeStrictReplay,
                    selected_evidence: evidence.clone(),
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
    inconclusive(
        candidate,
        InconclusiveReason::NoVerifiedSourceWitness,
        last_receipt,
    )
}

fn receipt_for_batch(receipt: SearchReceipt, fuel_limit: u32) -> SearchReceipt {
    SearchReceipt {
        fuel_limit,
        ..receipt
    }
}

struct PreparedEvidence<'a> {
    nodes: Vec<&'a OpenerTreeNode>,
    ordinal: u32,
    mirrored: bool,
    target_pre_clear_rows: Option<Vec<String>>,
    target_construction_rows: Option<Vec<String>>,
}

fn prepare_evidence<'a>(
    record: &'a OpenerRecord,
    candidate: &CandidateInput,
    evidence: &FrameEvidence,
) -> Option<PreparedEvidence<'a>> {
    let node = record
        .tree
        .iter()
        .find(|node| node.id == evidence.node_id)?;
    let mirrored = match evidence.mirror_relation {
        MirrorRelation::Same => false,
        MirrorRelation::Mirrored => true,
    };
    if !same_rows(&evidence.confirmation_rows, &node.rows, mirrored) {
        return None;
    }
    let (target_pre_clear_rows, target_construction_rows) = match evidence.frame {
        CandidateFrame::PreClear => {
            if !same_rows(&candidate.source_rows, node_pre_clear_rows(node), mirrored) {
                return None;
            }
            (Some(candidate.source_rows.clone()), None)
        }
        CandidateFrame::SourcePreClear => (
            Some(candidate.source_rows.clone()),
            Some(evidence.construction_rows.clone()?),
        ),
        CandidateFrame::PostClear => {
            if !same_rows(&candidate.source_rows, &node.rows, mirrored) {
                return None;
            }
            (None, None)
        }
    };
    let nodes = lineage(record, node.id)?;
    Some(PreparedEvidence {
        nodes,
        ordinal: node.pieces,
        mirrored,
        target_pre_clear_rows,
        target_construction_rows,
    })
}

fn candidate_is_valid(candidate: &CandidateInput) -> bool {
    !candidate.candidate_id.is_empty()
        && is_lower_hex_sha256(&candidate.candidate_digest)
        && !candidate.record_id.is_empty()
        && candidate.locked_piece_ordinal != 0
        && rows_are_valid(&candidate.source_rows)
        && !candidate.frame_evidence.is_empty()
        && candidate.frame_evidence.iter().all(|evidence| {
            rows_are_valid(&evidence.confirmation_rows)
                && evidence
                    .construction_rows
                    .as_deref()
                    .is_none_or(rows_are_valid)
        })
}

fn evidence_ordinals_match(record: &OpenerRecord, candidate: &CandidateInput) -> bool {
    candidate.frame_evidence.iter().all(|evidence| {
        record
            .tree
            .iter()
            .find(|node| node.id == evidence.node_id)
            .is_some_and(|node| node.pieces == candidate.locked_piece_ordinal)
    })
}

fn same_rows(left: &[String], right: &[String], mirrored: bool) -> bool {
    let right = if mirrored {
        right.iter().map(|row| mirror_letter_row(row)).collect()
    } else {
        right.to_vec()
    };
    rows_to_masks_floor_up(left) == rows_to_masks_floor_up(&right)
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

fn rows_are_valid(rows: &[String]) -> bool {
    rows.len() <= 40
        && rows
            .iter()
            .all(|row| row.chars().count() == 10 && row.chars().all(valid_cell))
}

fn valid_cell(cell: char) -> bool {
    matches!(cell, '_' | 'X' | 'I' | 'O' | 'T' | 'S' | 'Z' | 'J' | 'L')
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
        reason,
        receipt,
    }
}

fn invalid(candidate: &CandidateInput, reason: &'static str) -> CandidateResult {
    CandidateResult::InvalidInput {
        candidate_id: candidate.candidate_id.clone(),
        candidate_digest: candidate.candidate_digest.clone(),
        record_id: candidate.record_id.clone(),
        reason,
    }
}
