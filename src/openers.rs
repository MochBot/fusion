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
mod segments;
mod target;
mod witness_catalog;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use catalog::isolated_catalog_test;

pub use analyze::{AnalyzeError, OpenerLockPolicy, OpenerRoundAnalysis, OpenerRoundInput};
pub use catalog::{
    install_opener_runtime, CatalogError, CatalogStats, OpenerInstallError, OpenerInstallStats,
};
pub use catalog::{OpenerLink, OpenerNodeEst};
pub use catalogued_match::{MatchingOpener, RoundCataloguedBoardMatch};
pub use guide::{
    GuideAliases, GuideBasis, GuideDeviation, GuidePhase, GuideRequirements, GuideVariation,
    OpenerGuide, GUIDE_ALTERNATIVE_LIMIT, GUIDE_VARIATION_LIMIT,
};
pub use matcher::BoardMatch;
pub use phase::{OpenerAssessment, OpenerObservation};
pub use report::OpenerPhaseReport;
pub use witness_catalog::{WitnessCatalogError, WitnessCatalogStats};

pub use analyze::analyze_opener_round;
