use std::collections::{BTreeMap, BTreeSet};

use super::export::CollisionEvidenceError;
use super::model::{
    BridgeReasonSignature, ChiralityEvidence, EpsilonReasonSignature, OriginSignature,
    PhysicalStateSignature, RecordEvidence, RecordStatus, StateRoleSignature, StructuralOriginRole,
    TransitionKind, TransitionPayload, TransitionSignature, TransitionSignatureCount,
};
use crate::openers::recognition::graph::{
    BridgeReason, EpsilonReason, ModelState, RecognitionGraph, StateOrigin, TransitionLabel,
};

pub(super) fn record_evidence(
    graph: &RecognitionGraph,
    record_id: &str,
) -> Result<RecordEvidence, CollisionEvidenceError> {
    let [unmirrored, mirrored] =
        [false, true].map(|mirrored| chirality_evidence(graph, record_id, mirrored));
    let unmirrored = unmirrored?;
    let mirrored = mirrored?;
    let has_states = [&unmirrored, &mirrored]
        .iter()
        .any(|chirality| !chirality.state_role_signatures.is_empty());
    Ok(RecordEvidence {
        id: record_id.to_owned(),
        status: if has_states {
            RecordStatus::Compiled
        } else {
            RecordStatus::NoCompiledStates
        },
        chiralities: [unmirrored, mirrored],
    })
}

fn chirality_evidence(
    graph: &RecognitionGraph,
    record_id: &str,
    mirrored: bool,
) -> Result<ChiralityEvidence, CollisionEvidenceError> {
    let state_roles = graph
        .states
        .iter()
        .filter_map(|state| state_role_signature(state, record_id, mirrored).transpose())
        .collect::<Result<BTreeSet<_>, _>>()?;
    let physical_states = state_roles
        .iter()
        .map(|role| role.physical.clone())
        .collect::<BTreeSet<_>>();
    let origins = graph
        .states
        .iter()
        .flat_map(|state| state.origins.iter())
        .filter(|origin| origin.record.as_ref() == record_id && origin.mirrored == mirrored)
        .map(origin_signature)
        .collect::<BTreeSet<_>>();
    let mut transitions = BTreeMap::new();
    for transition in graph.out.iter().flatten() {
        let Some(from) =
            state_role_signature(&graph.states[transition.from.0], record_id, mirrored)?
        else {
            continue;
        };
        let Some(to) = state_role_signature(&graph.states[transition.to.0], record_id, mirrored)?
        else {
            continue;
        };
        let signature = TransitionSignature {
            kind: transition_kind(&transition.label),
            from,
            to,
            label: transition_payload(&transition.label),
        };
        let count = transitions.entry(signature).or_insert(0u32);
        *count = count
            .checked_add(1)
            .ok_or(CollisionEvidenceError::CountOverflow)?;
    }
    Ok(ChiralityEvidence {
        mirrored,
        physical_state_signatures: physical_states.into_iter().collect(),
        state_role_signatures: state_roles.into_iter().collect(),
        origin_signatures: origins.into_iter().collect(),
        transition_signatures: transitions
            .into_iter()
            .map(|(signature, count)| TransitionSignatureCount { signature, count })
            .collect(),
    })
}

fn state_role_signature(
    state: &ModelState,
    record_id: &str,
    mirrored: bool,
) -> Result<Option<StateRoleSignature>, CollisionEvidenceError> {
    let origin_roles = state
        .origins
        .iter()
        .filter(|origin| origin.record.as_ref() == record_id && origin.mirrored == mirrored)
        .map(structural_origin_role)
        .collect::<BTreeSet<_>>();
    if origin_roles.is_empty() {
        return Ok(None);
    }
    Ok(Some(StateRoleSignature {
        physical: physical_state_signature(state)?,
        identity_opaque: state.identity_opaque,
        origin_roles: origin_roles.into_iter().collect(),
    }))
}

fn physical_state_signature(
    state: &ModelState,
) -> Result<PhysicalStateSignature, CollisionEvidenceError> {
    let letters = state
        .physical_key
        .letters
        .as_ref()
        .map(|letters| String::from_utf8(letters.to_vec()))
        .transpose()
        .map_err(|source| CollisionEvidenceError::InvalidLetters { source })?;
    Ok(PhysicalStateSignature {
        masks: state.physical_key.masks.to_vec(),
        letters,
    })
}

fn origin_signature(origin: &StateOrigin) -> OriginSignature {
    OriginSignature {
        mirrored: origin.mirrored,
        node_id: origin.node_id,
        placed_subset: origin.placed_subset,
        route_name: origin.route_name.as_deref().map(str::to_owned),
        bag_complete: origin.bag_complete,
    }
}

fn structural_origin_role(origin: &StateOrigin) -> StructuralOriginRole {
    StructuralOriginRole {
        placed_subset: origin.placed_subset,
        bag_complete: origin.bag_complete,
    }
}

fn transition_kind(label: &TransitionLabel) -> TransitionKind {
    match label {
        TransitionLabel::Lock { .. } => TransitionKind::Lock,
        TransitionLabel::Epsilon { .. } => TransitionKind::Epsilon,
        TransitionLabel::Bridge { .. } => TransitionKind::Bridge,
    }
}

fn transition_payload(label: &TransitionLabel) -> TransitionPayload {
    match label {
        TransitionLabel::Lock {
            letter,
            cells,
            cleared_rows,
            is_pc,
        } => {
            let (letter, cells) = canonical_lock_payload(*letter, *cells);
            TransitionPayload::Lock {
                letter,
                cells,
                cleared_rows: cleared_rows.to_vec(),
                is_pc: *is_pc,
            }
        }
        TransitionLabel::Epsilon { reason } => TransitionPayload::Epsilon {
            reason: match reason {
                EpsilonReason::BagBoundary => EpsilonReasonSignature::BagBoundary,
            },
        },
        TransitionLabel::Bridge { reason } => TransitionPayload::Bridge {
            reason: match reason {
                BridgeReason::DirectImpossible => BridgeReasonSignature::DirectImpossible,
                BridgeReason::BudgetExceeded => BridgeReasonSignature::BudgetExceeded,
                BridgeReason::FrameInconsistent => BridgeReasonSignature::FrameInconsistent,
            },
        },
    }
}

fn canonical_lock_payload(letter: u8, cells: [[u8; 2]; 4]) -> (char, [[u8; 2]; 4]) {
    let mut direct = cells;
    direct.sort_unstable();
    let mut reflected = cells.map(|[x, y]| [9 - x, y]);
    reflected.sort_unstable();
    let direct = (char::from(letter), direct);
    let reflected = (char::from(mirror_piece(letter)), reflected);
    direct.min(reflected)
}

fn mirror_piece(letter: u8) -> u8 {
    match letter {
        b'L' => b'J',
        b'J' => b'L',
        b'S' => b'Z',
        b'Z' => b'S',
        other => other,
    }
}
