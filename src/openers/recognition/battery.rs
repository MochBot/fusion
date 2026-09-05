mod report;
mod runner;
mod walk;

use crate::openers::recognition::align::{align_round_exact, AlignBudget};
use crate::openers::recognition::cost::EditCosts;
use crate::openers::recognition::graph::RecognitionGraph;

use report::{HardOutcomes, LegalCase, MiniBatteryReport};

pub(super) use runner::run_behavioral_battery;

pub(super) fn synthesize_walk_observations(
    graph: &RecognitionGraph,
    record: &str,
    mirrored: bool,
) -> Result<Vec<Option<crate::openers::recognition::align::Observation>>, String> {
    walk::synthesize_observations(graph, record, mirrored)
}

pub(super) fn run_mini_battery(
    graph: &RecognitionGraph,
    record: &str,
    mirrored: bool,
) -> Result<MiniBatteryReport, String> {
    let observations = walk::synthesize_observations(graph, record, mirrored)?;
    let result = align_round_exact(
        graph,
        &EditCosts::default(),
        &observations,
        &AlignBudget::default(),
    );
    let best = result
        .hypotheses
        .first()
        .ok_or_else(|| "alignment returned no hypotheses".to_owned())?;
    let unknown = result
        .hypotheses
        .iter()
        .find(|hypothesis| hypothesis.origin.is_none())
        .ok_or_else(|| "alignment omitted the unknown hypothesis".to_owned())?;
    let legal = best
        .origin
        .as_ref()
        .is_some_and(|origin| origin.record.as_ref() == record)
        && best.total_cost == 0
        && unknown.total_cost > best.total_cost;

    Ok(MiniBatteryReport {
        legal: LegalCase {
            checked: 1,
            zero_cost: u32::from(legal),
            unknown_strictly_worse: u32::from(legal),
        },
        hard: HardOutcomes {
            legal_walks: legal,
            ..HardOutcomes::default()
        },
    })
}

#[cfg(test)]
mod tests;
