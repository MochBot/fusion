use std::fmt;

use serde::{Deserialize, Serialize};

use crate::header::Piece;
use crate::openers::CatalogError;

#[derive(Debug)]
pub enum ValidationError {
    Catalog(CatalogError),
    MalformedBatch { source: serde_json::Error },
    UnsupportedSchemaVersion { found: u32 },
    InvalidBatchIdentity,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalog(error) => write!(formatter, "catalog validation failed: {error}"),
            Self::MalformedBatch { source } => {
                write!(formatter, "malformed validation batch: {source}")
            }
            Self::UnsupportedSchemaVersion { found } => {
                write!(formatter, "unsupported validation schema version: {found}")
            }
            Self::InvalidBatchIdentity => {
                write!(formatter, "runId and inputAssetSha256 are invalid")
            }
        }
    }
}

impl std::error::Error for ValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Catalog(error) => Some(error),
            Self::MalformedBatch { source } => Some(source),
            Self::UnsupportedSchemaVersion { .. } => None,
            Self::InvalidBatchIdentity => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ValidationBatchV1 {
    pub(super) schema_version: u32,
    pub(super) run_id: String,
    pub(super) input_asset_sha256: String,
    pub(super) search_fuel: u32,
    pub(super) candidates: Vec<CandidateInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct CandidateInput {
    pub(super) candidate_id: String,
    pub(super) candidate_digest: String,
    pub(super) record_id: String,
    pub(super) locked_piece_ordinal: u32,
    pub(super) source_rows: Vec<String>,
    pub(super) frame_evidence: Vec<FrameEvidence>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FrameEvidence {
    pub node_id: u32,
    pub frame: CandidateFrame,
    pub mirror_relation: MirrorRelation,
    pub confirmation_rows: Vec<String>,
    pub construction_rows: Option<Vec<String>>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CandidateFrame {
    PreClear,
    SourcePreClear,
    PostClear,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MirrorRelation {
    Same,
    Mirrored,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResultsV1 {
    pub schema_version: u32,
    pub run_id: String,
    pub input_asset_sha256: String,
    pub fusion_revision: &'static str,
    pub ruleset: RulesetReceipt,
    pub results: Vec<CandidateResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesetReceipt {
    pub name: &'static str,
    pub enable_180: bool,
    pub enable_tspin: bool,
    pub enable_allspin: bool,
    pub srs_plus: bool,
}

#[derive(Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CandidateResult {
    WitnessFound {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        proof_method: ProofMethod,
        selected_evidence: FrameEvidence,
        witness: Witness,
        receipt: SearchReceipt,
    },
    Inconclusive {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        reason: InconclusiveReason,
        receipt: SearchReceipt,
    },
    InvalidInput {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        reason: &'static str,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProofMethod {
    SourceTreeStrictReplay,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InconclusiveReason {
    BudgetExhausted,
    NoVerifiedSourceWitness,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReceipt {
    pub explored_states: u32,
    pub fuel_limit: u32,
    pub fuel_remaining: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Witness {
    pub mirrored: bool,
    pub locks: Vec<WitnessLock>,
    pub confirmation_rows: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WitnessLock {
    pub ordinal: u32,
    pub piece: &'static str,
    pub current_before: &'static str,
    pub hold_before: Option<&'static str>,
    pub hold_used: bool,
    pub hold_after: Option<&'static str>,
    pub draws_before_lock: Vec<WitnessDraw>,
    pub placement: Placement,
    pub rows_before_clear: Vec<String>,
    pub rows_after: Vec<String>,
}

#[derive(Clone, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WitnessDraw {
    pub draw_ordinal: u32,
    pub bag_ordinal: u32,
    pub slot_in_bag: u8,
    pub piece: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub rotation: &'static str,
    pub x: i32,
    pub y: i32,
    pub spin: &'static str,
    pub cells: [[u8; 2]; 4],
}

pub(super) const fn piece_name(piece: Piece) -> &'static str {
    match piece {
        Piece::I => "I",
        Piece::O => "O",
        Piece::T => "T",
        Piece::L => "L",
        Piece::J => "J",
        Piece::S => "S",
        Piece::Z => "Z",
    }
}

pub(super) fn piece_from_name(name: &str) -> Option<Piece> {
    match name {
        "I" => Some(Piece::I),
        "O" => Some(Piece::O),
        "T" => Some(Piece::T),
        "L" => Some(Piece::L),
        "J" => Some(Piece::J),
        "S" => Some(Piece::S),
        "Z" => Some(Piece::Z),
        _ => None,
    }
}
