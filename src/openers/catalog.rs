use std::fmt;
use std::sync::{Arc, LazyLock, RwLock};

#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::witness_catalog::{
    parse_witness_catalog, RuntimeSearchShapeTarget, WitnessCatalogError, WitnessCatalogStats,
};

use super::catalogued_match::NodeBoards;
use super::recognition::RecordGraphCache;
use super::target::{build_targets, ShapeTarget};

mod validate;

const OPENER_ASSET_VERSION: u32 = 2;

static INSTALLED_CATALOG: LazyLock<RwLock<Option<Arc<InstalledCatalog>>>> =
    LazyLock::new(|| RwLock::new(None));

#[cfg(test)]
static CATALOG_TEST_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenerCatalog {
    pub format_version: u32,
    pub openers: Vec<OpenerRecord>,
}

/// The install slot's contents: the validated catalog plus every derived
/// structure that depends only on it. Built once per install and shared by
/// every round analyzed against it, including compiled record graphs, which
/// are compiled lazily and retained inside the snapshot.
pub(crate) struct InstalledCatalog {
    pub catalog: OpenerCatalog,
    pub asset_sha256: String,
    pub targets: Vec<ShapeTarget>,
    pub node_boards: NodeBoards,
    pub runtime_search_shape_targets: Vec<RuntimeSearchShapeTarget>,
    pub compiled: RecordGraphCache,
}

impl InstalledCatalog {
    fn new(
        catalog: OpenerCatalog,
        asset_sha256: String,
        runtime_search_shape_targets: Vec<RuntimeSearchShapeTarget>,
    ) -> Self {
        let targets = build_targets(&catalog);
        let node_boards = NodeBoards::build(&catalog);
        Self {
            catalog,
            asset_sha256,
            targets,
            node_boards,
            runtime_search_shape_targets,
            compiled: RecordGraphCache::default(),
        }
    }

    /// A snapshot over a catalog that is not the installed one; derived data is
    /// built the same way so both paths share one analysis implementation.
    #[cfg(test)]
    pub(crate) fn detached(catalog: OpenerCatalog) -> Self {
        Self::new(catalog, String::new(), Vec::new())
    }
}

impl std::ops::Deref for InstalledCatalog {
    type Target = OpenerCatalog;

    fn deref(&self) -> &Self::Target {
        &self.catalog
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenerRecord {
    pub id: String,
    pub aliases: OpenerAliases,
    pub shape_key: String,
    #[serde(default)]
    pub search_shapes: Vec<Vec<String>>,
    #[serde(default)]
    pub tree: Vec<OpenerTreeNode>,
    #[serde(default)]
    pub dependencies: String,
    #[serde(default)]
    pub cover: Option<OpenerCover>,
    #[serde(default)]
    pub pc_chance: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<OpenerLink>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OpenerAliases {
    pub en: String,
    #[serde(default)]
    pub jp: Option<String>,
    #[serde(default)]
    pub abbr: Option<String>,
    #[serde(default)]
    pub alt: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct OpenerCover {
    pub pct: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct OpenerLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenerNodeEst {
    pub attack: u32,
    pub cum_attack: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenerTreeNode {
    pub id: u32,
    pub parent: Option<u32>,
    pub pieces: u32,
    pub rows: Vec<String>,
    pub pre_clear_rows: Option<Vec<String>>,
    pub clear_rows: Option<Vec<u32>>,
    pub placements: Option<Vec<OpenerPlacement>>,
    #[serde(default)]
    pub grey: bool,
    pub route_name: Option<String>,
    #[serde(default)]
    pub annotations: Vec<String>,
    #[serde(default)]
    pub est: Option<OpenerNodeEst>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OpenerPlacement {
    pub letter: String,
    pub cells: Vec<[u8; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStats {
    pub opener_count: usize,
    pub tree_node_count: usize,
    pub search_shape_count: usize,
}

impl CatalogStats {
    fn from_catalog(catalog: &OpenerCatalog) -> Self {
        let tree_node_count = catalog.openers.iter().map(|record| record.tree.len()).sum();
        let search_shape_count = catalog
            .openers
            .iter()
            .map(|record| record.search_shapes.len())
            .sum();

        Self {
            opener_count: catalog.openers.len(),
            tree_node_count,
            search_shape_count,
        }
    }
}

#[derive(Debug)]
pub enum CatalogError {
    MalformedJson { source: serde_json::Error },
    UnsupportedFormatVersion { found: u32 },
    InvalidCatalog { reason: String },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedJson { source } => {
                write!(formatter, "malformed opener catalog JSON: {source}")
            }
            Self::UnsupportedFormatVersion { found } => {
                write!(
                    formatter,
                    "unsupported opener catalog format version: {found}"
                )
            }
            Self::InvalidCatalog { reason } => {
                write!(formatter, "invalid opener catalog: {reason}")
            }
        }
    }
}

impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MalformedJson { source } => Some(source),
            Self::UnsupportedFormatVersion { .. } => None,
            Self::InvalidCatalog { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerInstallStats {
    pub catalog: CatalogStats,
    pub witnesses: Option<WitnessCatalogStats>,
}

#[derive(Debug)]
pub enum OpenerInstallError {
    Catalog(CatalogError),
    Witnesses(WitnessCatalogError),
}

impl fmt::Display for OpenerInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalog(error) => error.fmt(formatter),
            Self::Witnesses(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for OpenerInstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Catalog(error) => Some(error),
            Self::Witnesses(error) => Some(error),
        }
    }
}

/// Installs a catalog and its optional witness companion as one unit. Both
/// assets are parsed and validated before the install slot changes, so a
/// rejected companion leaves the previously installed runtime untouched.
pub fn install_opener_runtime(
    catalog_json_bytes: &[u8],
    witness_json_bytes: Option<&[u8]>,
) -> Result<OpenerInstallStats, OpenerInstallError> {
    let catalog = parse_catalog(catalog_json_bytes).map_err(OpenerInstallError::Catalog)?;
    let asset_sha256 = format!("{:x}", Sha256::digest(catalog_json_bytes));
    let (targets, witnesses) = match witness_json_bytes {
        Some(bytes) => {
            let (targets, stats) = parse_witness_catalog(bytes, &catalog, &asset_sha256)
                .map_err(OpenerInstallError::Witnesses)?;
            (targets, Some(stats))
        }
        None => (Vec::new(), None),
    };
    let stats = OpenerInstallStats {
        catalog: CatalogStats::from_catalog(&catalog),
        witnesses,
    };
    let installed_catalog = InstalledCatalog::new(catalog, asset_sha256, targets);
    let mut installed = match INSTALLED_CATALOG.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *installed = Some(Arc::new(installed_catalog));
    Ok(stats)
}

pub fn set_opener_catalog(catalog_json_bytes: &[u8]) -> Result<CatalogStats, CatalogError> {
    match install_opener_runtime(catalog_json_bytes, None) {
        Ok(stats) => Ok(stats.catalog),
        Err(OpenerInstallError::Catalog(error)) => Err(error),
        Err(OpenerInstallError::Witnesses(_)) => {
            unreachable!("installing without a companion cannot fail witness validation")
        }
    }
}

pub fn set_search_shape_witnesses(
    witness_json_bytes: &[u8],
) -> Result<WitnessCatalogStats, WitnessCatalogError> {
    let mut installed = match INSTALLED_CATALOG.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let previous = installed.take().ok_or(WitnessCatalogError::NoCatalog)?;
    let (targets, stats) = match parse_witness_catalog(
        witness_json_bytes,
        &previous.catalog,
        &previous.asset_sha256,
    ) {
        Ok(parsed) => parsed,
        Err(error) => {
            *installed = Some(previous);
            return Err(error);
        }
    };
    // Only the witness targets change; every other derived structure is a pure
    // function of the unchanged catalog, so it is moved rather than rebuilt.
    let updated = match Arc::try_unwrap(previous) {
        Ok(mut owned) => {
            owned.runtime_search_shape_targets = targets;
            owned
        }
        Err(shared) => InstalledCatalog {
            catalog: shared.catalog.clone(),
            asset_sha256: shared.asset_sha256.clone(),
            targets: shared.targets.clone(),
            node_boards: shared.node_boards.clone(),
            runtime_search_shape_targets: targets,
            compiled: RecordGraphCache::default(),
        },
    };
    *installed = Some(Arc::new(updated));
    Ok(stats)
}

pub(crate) fn parse_catalog(catalog_json_bytes: &[u8]) -> Result<OpenerCatalog, CatalogError> {
    let catalog: OpenerCatalog = serde_json::from_slice(catalog_json_bytes)
        .map_err(|source| CatalogError::MalformedJson { source })?;
    if catalog.format_version != OPENER_ASSET_VERSION {
        return Err(CatalogError::UnsupportedFormatVersion {
            found: catalog.format_version,
        });
    }
    validate::validate_catalog(&catalog)?;
    Ok(catalog)
}

pub(crate) fn installed_opener_catalog() -> Option<Arc<InstalledCatalog>> {
    let installed = match INSTALLED_CATALOG.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    installed.clone()
}

#[cfg(test)]
pub(crate) struct CatalogTestScope {
    previous: Option<Arc<InstalledCatalog>>,
    _lock: MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for CatalogTestScope {
    fn drop(&mut self) {
        let mut installed = match INSTALLED_CATALOG.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *installed = self.previous.take();
    }
}

#[cfg(test)]
pub(crate) fn isolated_catalog_test() -> CatalogTestScope {
    let lock = match CATALOG_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let mut installed = match INSTALLED_CATALOG.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let previous = installed.take();
    drop(installed);
    CatalogTestScope {
        previous,
        _lock: lock,
    }
}

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;
