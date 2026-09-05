use std::fmt;

use serde::{Deserialize, Serialize};

use crate::openers::catalog::{installed_opener_catalog, InstalledCatalog};
use crate::openers::catalogued_match::{
    match_catalogued_boards_with_targets, RoundCataloguedBoardMatch,
};
use crate::openers::guide::{build_guide, select_subject, OpenerGuide};
use crate::openers::phase::{assess_opener_phase, OpenerAssessment, OpenerObservation};
use crate::openers::recognition::round::{recognize_round, RoundRecognition};
use crate::openers::report::{build_opener_report, OpenerPhaseReport};
use crate::openers::showcase::{ExternalPiece, ShowcaseDealtInput};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenerRoundInput {
    #[serde(default)]
    pub observations: Vec<Option<OpenerObservation>>,
    #[serde(default)]
    pub dealt_inputs: Vec<Option<ShowcaseDealtInput>>,
    #[serde(default)]
    pub tail_queue: Vec<ExternalPiece>,
    #[serde(default)]
    pub relax_depth: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerLockPolicy {
    pub b2b_break_exempt: bool,
    pub board_mess_exempt: bool,
    pub attack_gap_exempt: bool,
    pub attack_gap_included_in_tier_distributions: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerRoundAnalysis {
    pub assessments: Vec<Option<OpenerAssessment>>,
    pub policies: Vec<OpenerLockPolicy>,
    pub catalogued_board_match: Option<RoundCataloguedBoardMatch>,
    pub report: Option<OpenerPhaseReport>,
    pub guide: Option<OpenerGuide>,
    pub(crate) recognition: Option<RoundRecognition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyzeError {
    NoCatalog,
}

impl fmt::Display for AnalyzeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCatalog => formatter.write_str("no opener catalog is installed"),
        }
    }
}

impl std::error::Error for AnalyzeError {}

/// Analyzes against a catalog that is not the installed one. Derived data is
/// built for this call only; nothing is retained across calls.
#[cfg(test)]
pub(crate) fn analyze_opener_round_with_catalog(
    catalog: &crate::openers::catalog::OpenerCatalog,
    input: &OpenerRoundInput,
) -> OpenerRoundAnalysis {
    analyze_round(&InstalledCatalog::detached(catalog.clone()), input)
}

pub(crate) fn analyze_round(
    installed: &InstalledCatalog,
    input: &OpenerRoundInput,
) -> OpenerRoundAnalysis {
    let catalog = &installed.catalog;
    let assessments = assess_opener_phase(&installed.targets, &input.observations);
    let catalogued_board_match = match_catalogued_boards_with_targets(
        catalog,
        &installed.node_boards,
        &installed.runtime_search_shape_targets,
        &input.observations,
    );
    let report = build_opener_report(catalogued_board_match.as_ref());
    let policies = assessments
        .iter()
        .enumerate()
        .map(|(index, assessment)| lock_policy(assessment.as_ref(), index, input.relax_depth))
        .collect();
    let recognition = recognize_round(
        Some(catalog),
        &installed.compiled,
        &assessments,
        catalogued_board_match.as_ref(),
        &input.observations,
    );
    let guide = select_subject(
        catalog,
        catalogued_board_match.as_ref(),
        recognition.as_ref(),
    )
    .and_then(|subject| build_guide(catalog, &subject, &input.observations, recognition.as_ref()));

    OpenerRoundAnalysis {
        assessments,
        policies,
        catalogued_board_match,
        report,
        guide,
        recognition,
    }
}

pub fn analyze_opener_round(input: &OpenerRoundInput) -> Result<OpenerRoundAnalysis, AnalyzeError> {
    let installed = installed_opener_catalog().ok_or(AnalyzeError::NoCatalog)?;
    Ok(analyze_round(&installed, input))
}

fn lock_policy(
    assessment: Option<&OpenerAssessment>,
    index: usize,
    relax_depth: Option<usize>,
) -> OpenerLockPolicy {
    let on_script = assessment.is_some_and(|assessment| assessment.on_script);
    let attack_gap_exempt = on_script && relax_depth.is_none_or(|depth| index < depth);

    OpenerLockPolicy {
        b2b_break_exempt: on_script,
        board_mess_exempt: on_script,
        attack_gap_exempt,
        attack_gap_included_in_tier_distributions: !attack_gap_exempt,
    }
}
