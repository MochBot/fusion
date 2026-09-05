mod analyze;
mod board;
mod catalog;
mod catalogued_match;
mod guide;
mod matcher;
mod phase;
mod recognition;
mod report;
mod route;
#[cfg(not(target_arch = "wasm32"))]
mod search_shape_validation;
mod segments;
mod showcase;
#[cfg(not(target_arch = "wasm32"))]
mod supplied_operation_validation;
mod target;
mod witness_catalog;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use catalog::isolated_catalog_test;

pub use analyze::{AnalyzeError, OpenerLockPolicy, OpenerRoundAnalysis, OpenerRoundInput};
pub use catalog::{
    install_opener_runtime, set_opener_catalog, set_search_shape_witnesses, CatalogError,
    CatalogStats, OpenerInstallError, OpenerInstallStats,
};
pub use catalog::{OpenerLink, OpenerNodeEst};
pub use catalogued_match::{MatchingOpener, RoundCataloguedBoardMatch};
pub use guide::{
    GuideAliases, GuideBasis, GuideDeviation, GuidePhase, GuideRequirements, GuideVariation,
    OpenerGuide, GUIDE_VARIATION_LIMIT,
};
pub use matcher::BoardMatch;
pub use phase::{OpenerAssessment, OpenerObservation};
pub use report::OpenerPhaseReport;
pub use showcase::{ExternalPiece, ShowcaseDealtInput};
pub use witness_catalog::{WitnessCatalogError, WitnessCatalogStats};

#[cfg(not(target_arch = "wasm32"))]
pub use search_shape_validation::{
    validate_contextual_target_batch, validate_search_shape_batch, ContextualTargetResultsV1,
    ContextualTargetValidationError, ValidationError, ValidationResultsV1,
};
#[cfg(not(target_arch = "wasm32"))]
pub use supplied_operation_validation::{
    validate_supplied_operation_batch, SuppliedOperationResultsV1, SuppliedOperationValidationError,
};

pub use analyze::analyze_opener_round;
