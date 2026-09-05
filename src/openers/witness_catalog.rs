use std::fmt;

use serde::{Deserialize, Serialize};

use super::board::rows_to_masks_floor_up;
use super::catalog::OpenerCatalog;

const WITNESS_ASSET_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub(crate) struct RuntimeSearchShapeTarget {
    pub record_id: String,
    pub locked_piece_ordinal: u32,
    pub rows: Vec<u16>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WitnessCatalog {
    schema_version: u32,
    opener_asset_sha256: String,
    enrichment_sha256: String,
    provenance_sha256: String,
    summary: WitnessSummary,
    witnesses: Vec<WitnessEntry>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WitnessSummary {
    witnesses: usize,
    runtime_targets: usize,
    viewer_only: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WitnessEntry {
    record_id: String,
    search_shape_index: usize,
    target_frame: TargetFrame,
    queue_scope: QueueScope,
    target_rows: Vec<String>,
    runtime_target: Option<RuntimeTargetReceipt>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum TargetFrame {
    PostClear,
    PreClear,
    MidConstruction,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum QueueScope {
    FreshRound,
    NotClaimedMidConstruction,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeTargetReceipt {
    locked_piece_ordinal: u32,
    rows: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WitnessCatalogStats {
    pub witness_count: usize,
    pub runtime_target_count: usize,
    pub viewer_only_count: usize,
}

#[derive(Debug)]
pub enum WitnessCatalogError {
    NoCatalog,
    MalformedJson { source: serde_json::Error },
    UnsupportedSchemaVersion { found: u32 },
    CatalogIdentityMismatch,
    InvalidAsset { reason: String },
}

impl fmt::Display for WitnessCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCatalog => formatter.write_str("no opener catalog is installed"),
            Self::MalformedJson { source } => {
                write!(formatter, "malformed search-shape witness JSON: {source}")
            }
            Self::UnsupportedSchemaVersion { found } => {
                write!(
                    formatter,
                    "unsupported search-shape witness schema version: {found}"
                )
            }
            Self::CatalogIdentityMismatch => formatter
                .write_str("search-shape witnesses do not identify the installed opener catalog"),
            Self::InvalidAsset { reason } => {
                write!(formatter, "invalid search-shape witness asset: {reason}")
            }
        }
    }
}

impl std::error::Error for WitnessCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MalformedJson { source } => Some(source),
            Self::NoCatalog
            | Self::UnsupportedSchemaVersion { .. }
            | Self::CatalogIdentityMismatch
            | Self::InvalidAsset { .. } => None,
        }
    }
}

pub(crate) fn parse_witness_catalog(
    bytes: &[u8],
    catalog: &OpenerCatalog,
    catalog_sha256: &str,
) -> Result<(Vec<RuntimeSearchShapeTarget>, WitnessCatalogStats), WitnessCatalogError> {
    let asset: WitnessCatalog = serde_json::from_slice(bytes)
        .map_err(|source| WitnessCatalogError::MalformedJson { source })?;
    if asset.schema_version != WITNESS_ASSET_VERSION {
        return Err(WitnessCatalogError::UnsupportedSchemaVersion {
            found: asset.schema_version,
        });
    }
    if asset.opener_asset_sha256 != catalog_sha256 {
        return Err(WitnessCatalogError::CatalogIdentityMismatch);
    }
    if !is_sha256(&asset.enrichment_sha256) || !is_sha256(&asset.provenance_sha256) {
        return invalid("private evidence hashes must be lowercase SHA-256 digests");
    }
    if asset.summary.witnesses != asset.witnesses.len()
        || asset.summary.viewer_only + asset.summary.runtime_targets != asset.summary.witnesses
    {
        return invalid("summary counts do not match witness entries");
    }
    let mut targets = Vec::with_capacity(asset.summary.runtime_targets);
    for witness in &asset.witnesses {
        let record = catalog
            .openers
            .iter()
            .find(|record| record.id == witness.record_id)
            .ok_or_else(|| WitnessCatalogError::InvalidAsset {
                reason: format!(
                    "record {} is absent from the opener catalog",
                    witness.record_id
                ),
            })?;
        let public_rows = record
            .search_shapes
            .get(witness.search_shape_index)
            .ok_or_else(|| WitnessCatalogError::InvalidAsset {
                reason: format!(
                    "search shape {}:{} is absent from the opener catalog",
                    witness.record_id, witness.search_shape_index
                ),
            })?;
        if rows_to_masks_floor_up(public_rows) != rows_to_masks_floor_up(&witness.target_rows) {
            return invalid("witness target rows do not match the opener catalog");
        }
        let Some(runtime) = &witness.runtime_target else {
            continue;
        };
        if witness.target_frame != TargetFrame::PostClear
            || witness.queue_scope != QueueScope::FreshRound
            || runtime.locked_piece_ordinal == 0
            || rows_to_masks_floor_up(&runtime.rows) != rows_to_masks_floor_up(&witness.target_rows)
        {
            return invalid("runtime target is not an exact fresh-round post-clear witness");
        }
        targets.push(RuntimeSearchShapeTarget {
            record_id: witness.record_id.clone(),
            locked_piece_ordinal: runtime.locked_piece_ordinal,
            rows: rows_to_masks_floor_up(&runtime.rows),
        });
    }
    if targets.len() != asset.summary.runtime_targets {
        return invalid("runtime target count does not match the summary");
    }
    let stats = WitnessCatalogStats {
        witness_count: asset.summary.witnesses,
        runtime_target_count: targets.len(),
        viewer_only_count: asset.summary.viewer_only,
    };
    Ok((targets, stats))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid<T>(reason: &str) -> Result<T, WitnessCatalogError> {
    Err(WitnessCatalogError::InvalidAsset {
        reason: reason.to_owned(),
    })
}
