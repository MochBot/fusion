use crate::openers::catalogued_match::RoundCataloguedBoardMatch;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerPhaseReport {
    pub opener_id: String,
    pub name: String,
    pub route_name: Option<String>,
    pub mirrored: Option<bool>,
    pub first_match_index: usize,
    pub anchor_index: usize,
}

pub(crate) fn build_opener_report(
    catalogued_board_match: Option<&RoundCataloguedBoardMatch>,
) -> Option<OpenerPhaseReport> {
    let matched = catalogued_board_match?;
    let [opener] = matched.matching_openers.as_slice() else {
        return None;
    };
    Some(OpenerPhaseReport {
        opener_id: opener.id.clone(),
        name: opener.name.clone(),
        route_name: opener.route_name.clone(),
        mirrored: opener.mirrored,
        first_match_index: matched.first_match_index,
        anchor_index: matched.anchor_index,
    })
}
