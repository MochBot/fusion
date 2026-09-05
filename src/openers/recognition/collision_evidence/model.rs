use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CollisionRequest {
    pub(super) format_version: u32,
    pub(super) pairs: Vec<CollisionPair>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct CollisionPair {
    pub(super) a: String,
    pub(super) b: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CollisionEvidence {
    pub(super) format_version: u32,
    pub(super) catalog: CatalogSignature,
    pub(super) compile_budget: CompileBudgetSignature,
    pub(super) records: Vec<RecordEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CatalogSignature {
    pub(super) format_version: u32,
    pub(super) record_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CompileBudgetSignature {
    pub(super) max_states_per_edge: u32,
    pub(super) max_placements_per_edge: u8,
    pub(super) max_dfs_visits_per_edge: u32,
    pub(super) max_total_states: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecordEvidence {
    pub(super) id: String,
    pub(super) status: RecordStatus,
    pub(super) chiralities: [ChiralityEvidence; 2],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RecordStatus {
    Compiled,
    NoCompiledStates,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChiralityEvidence {
    pub(super) mirrored: bool,
    pub(super) physical_state_signatures: Vec<PhysicalStateSignature>,
    pub(super) state_role_signatures: Vec<StateRoleSignature>,
    pub(super) origin_signatures: Vec<OriginSignature>,
    pub(super) transition_signatures: Vec<TransitionSignatureCount>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PhysicalStateSignature {
    pub(super) masks: Vec<u16>,
    pub(super) letters: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OriginSignature {
    pub(super) mirrored: bool,
    pub(super) node_id: u32,
    pub(super) placed_subset: u32,
    pub(super) route_name: Option<String>,
    pub(super) bag_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StateRoleSignature {
    pub(super) physical: PhysicalStateSignature,
    pub(super) identity_opaque: bool,
    pub(super) origin_roles: Vec<StructuralOriginRole>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StructuralOriginRole {
    pub(super) placed_subset: u32,
    pub(super) bag_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TransitionSignatureCount {
    pub(super) signature: TransitionSignature,
    pub(super) count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TransitionSignature {
    pub(super) kind: TransitionKind,
    pub(super) from: StateRoleSignature,
    pub(super) to: StateRoleSignature,
    pub(super) label: TransitionPayload,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum TransitionKind {
    Lock,
    Epsilon,
    Bridge,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub(super) enum TransitionPayload {
    Lock {
        letter: char,
        cells: [[u8; 2]; 4],
        cleared_rows: Vec<u8>,
        is_pc: bool,
    },
    Epsilon {
        reason: EpsilonReasonSignature,
    },
    Bridge {
        reason: BridgeReasonSignature,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum EpsilonReasonSignature {
    BagBoundary,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum BridgeReasonSignature {
    DirectImpossible,
    BudgetExceeded,
    FrameInconsistent,
}
