use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use super::model::{CatalogSignature, CollisionEvidence, CollisionRequest, CompileBudgetSignature};
use super::signature::record_evidence;
use crate::openers::catalog::OpenerCatalog;
use crate::openers::recognition::{compile_recognition_graph, CompileBudget, CompileError};

const EVIDENCE_FORMAT_VERSION: u32 = 1;
const REQUEST_ENV: &str = "OPENER_COLLISION_REQUEST";
const ASSET_ENV: &str = "OPENER_COLLISION_ASSET";
const OUT_ENV: &str = "OPENER_COLLISION_OUT";

#[derive(Debug)]
pub(super) enum CollisionEvidenceError {
    MissingEnv {
        name: &'static str,
    },
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    RequestJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    AssetJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    UnsupportedRequestFormat {
        found: u32,
    },
    EmptyRecordId,
    SelfPair {
        record: String,
    },
    DuplicatePair {
        a: String,
        b: String,
    },
    MissingRecord {
        id: String,
    },
    Compile {
        source: CompileError,
    },
    InvalidLetters {
        source: std::string::FromUtf8Error,
    },
    CountOverflow,
    OutputJson {
        source: serde_json::Error,
    },
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for CollisionEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnv { name } => {
                write!(formatter, "required environment variable {name} is not set")
            }
            Self::Read { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::RequestJson { path, source } => write!(
                formatter,
                "invalid collision request {}: {source}",
                path.display()
            ),
            Self::AssetJson { path, source } => write!(
                formatter,
                "invalid opener asset {}: {source}",
                path.display()
            ),
            Self::UnsupportedRequestFormat { found } => write!(
                formatter,
                "unsupported collision request formatVersion: {found}"
            ),
            Self::EmptyRecordId => {
                write!(formatter, "collision request contains an empty record id")
            }
            Self::SelfPair { record } => write!(
                formatter,
                "collision request contains self pair for {record}"
            ),
            Self::DuplicatePair { a, b } => write!(
                formatter,
                "collision request contains duplicate pair {a}/{b}"
            ),
            Self::MissingRecord { id } => write!(
                formatter,
                "collision request references missing record {id}"
            ),
            Self::Compile { source } => {
                write!(formatter, "failed to compile recognition graph: {source}")
            }
            Self::InvalidLetters { source } => write!(
                formatter,
                "compiler emitted non-UTF-8 identity letters: {source}"
            ),
            Self::CountOverflow => write!(formatter, "transition signature count overflow"),
            Self::OutputJson { source } => write!(
                formatter,
                "failed to serialize collision evidence: {source}"
            ),
            Self::Write { path, source } => {
                write!(formatter, "failed to write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for CollisionEvidenceError {}

pub(super) fn export_from_env() -> Result<CollisionEvidence, CollisionEvidenceError> {
    let request_path = env_path(REQUEST_ENV)?;
    let asset_path = env_path(ASSET_ENV)?;
    let out_path = env_path(OUT_ENV)?;
    let request = read_request(&request_path)?;
    let asset = read_asset(&asset_path)?;
    let evidence = export_catalog(&asset, &request)?;
    let output = serde_json::to_vec_pretty(&evidence)
        .map_err(|source| CollisionEvidenceError::OutputJson { source })?;
    fs::write(&out_path, output).map_err(|source| CollisionEvidenceError::Write {
        path: out_path,
        source,
    })?;
    Ok(evidence)
}

pub(super) fn export_catalog(
    catalog: &OpenerCatalog,
    request: &CollisionRequest,
) -> Result<CollisionEvidence, CollisionEvidenceError> {
    let record_ids = requested_record_ids(catalog, request)?;
    let budget = CompileBudget::default();
    let graph = compile_recognition_graph(catalog, &budget)
        .map_err(|source| CollisionEvidenceError::Compile { source })?;
    let records = record_ids
        .into_iter()
        .map(|id| record_evidence(&graph, &id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CollisionEvidence {
        format_version: EVIDENCE_FORMAT_VERSION,
        catalog: CatalogSignature {
            format_version: catalog.format_version,
            record_count: catalog.openers.len(),
        },
        compile_budget: CompileBudgetSignature {
            max_states_per_edge: budget.max_states_per_edge,
            max_placements_per_edge: budget.max_placements_per_edge,
            max_dfs_visits_per_edge: budget.max_dfs_visits_per_edge,
            max_total_states: budget.max_total_states,
        },
        records,
    })
}

fn env_path(name: &'static str) -> Result<PathBuf, CollisionEvidenceError> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(CollisionEvidenceError::MissingEnv { name })
}

fn read_request(path: &Path) -> Result<CollisionRequest, CollisionEvidenceError> {
    let bytes = fs::read(path).map_err(|source| CollisionEvidenceError::Read {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| CollisionEvidenceError::RequestJson {
        path: path.to_owned(),
        source,
    })
}

fn read_asset(path: &Path) -> Result<OpenerCatalog, CollisionEvidenceError> {
    let bytes = fs::read(path).map_err(|source| CollisionEvidenceError::Read {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| CollisionEvidenceError::AssetJson {
        path: path.to_owned(),
        source,
    })
}

fn requested_record_ids(
    catalog: &OpenerCatalog,
    request: &CollisionRequest,
) -> Result<BTreeSet<String>, CollisionEvidenceError> {
    if request.format_version != EVIDENCE_FORMAT_VERSION {
        return Err(CollisionEvidenceError::UnsupportedRequestFormat {
            found: request.format_version,
        });
    }
    let catalog_ids = catalog
        .openers
        .iter()
        .map(|record| record.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut pairs = BTreeSet::new();
    let mut record_ids = BTreeSet::new();
    for pair in &request.pairs {
        if pair.a.is_empty() || pair.b.is_empty() {
            return Err(CollisionEvidenceError::EmptyRecordId);
        }
        if pair.a == pair.b {
            return Err(CollisionEvidenceError::SelfPair {
                record: pair.a.clone(),
            });
        }
        let (a, b) = if pair.a < pair.b {
            (pair.a.as_str(), pair.b.as_str())
        } else {
            (pair.b.as_str(), pair.a.as_str())
        };
        if !pairs.insert((a, b)) {
            return Err(CollisionEvidenceError::DuplicatePair {
                a: a.to_owned(),
                b: b.to_owned(),
            });
        }
        for record_id in [a, b] {
            if !catalog_ids.contains(record_id) {
                return Err(CollisionEvidenceError::MissingRecord {
                    id: record_id.to_owned(),
                });
            }
            record_ids.insert(record_id.to_owned());
        }
    }
    Ok(record_ids)
}
