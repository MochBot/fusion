use super::frontier::Entry;
use super::FinalHypothesis;
use crate::openers::recognition::cost::EditOps;
use crate::openers::recognition::graph::{RecognitionGraph, StateId};

/// `entries` must be in `StateId` order; they are re-sorted by (cost, state).
pub(super) fn final_hypotheses<'a>(
    graph: &RecognitionGraph,
    entries: impl Iterator<Item = (StateId, &'a Entry)>,
    retained_record: Option<&str>,
) -> Vec<FinalHypothesis> {
    let mut hypotheses = Vec::new();
    let mut entries = entries.collect::<Vec<_>>();
    entries.sort_by_key(|(state, entry)| (entry.cost, state.0));
    for (state, entry) in entries {
        for origin in &graph.states[state.0].origins {
            let candidate = FinalHypothesis {
                origin: Some(origin.clone()),
                state: Some(state),
                total_cost: entry.cost,
                ops: entry.ops,
                #[cfg(test)]
                opaque_steps: entry.opaque_steps,
                identity_frozen: entry.opaque_steps > 0,
            };
            let duplicate = hypotheses
                .iter_mut()
                .find(|existing: &&mut FinalHypothesis| {
                    existing
                        .origin
                        .as_ref()
                        .is_some_and(|existing_origin| existing_origin.record == origin.record)
                });
            match duplicate {
                Some(existing) if is_better(&candidate, existing) => *existing = candidate,
                Some(_) => {}
                None => hypotheses.push(candidate),
            }
        }
    }
    hypotheses.sort_by(|left, right| {
        left.total_cost.cmp(&right.total_cost).then_with(|| {
            left.origin
                .as_ref()
                .map_or("", |origin| origin.record.as_ref())
                .cmp(
                    right
                        .origin
                        .as_ref()
                        .map_or("", |origin| origin.record.as_ref()),
                )
        })
    });
    let retained = retained_record.and_then(|record| {
        hypotheses
            .iter()
            .position(|hypothesis| {
                hypothesis
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.record.as_ref() == record)
            })
            .filter(|index| *index >= 8)
            .map(|index| hypotheses.remove(index))
    });
    hypotheses.truncate(8);
    if let Some(retained) = retained {
        hypotheses.push(retained);
    }
    hypotheses
}

fn is_better(candidate: &FinalHypothesis, current: &FinalHypothesis) -> bool {
    candidate.total_cost < current.total_cost
        || (candidate.total_cost == current.total_cost
            && (mismatch_count(candidate.ops) < mismatch_count(current.ops)
                || (mismatch_count(candidate.ops) == mismatch_count(current.ops)
                    && state_key(candidate) < state_key(current))))
}

fn state_key(hypothesis: &FinalHypothesis) -> usize {
    hypothesis.state.map_or(usize::MAX, |state| state.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_cost_same_record_ties_keep_the_lowest_state_id() {
        let lower = hypothesis(StateId(1));
        let higher = hypothesis(StateId(2));

        assert!(is_better(&lower, &higher));
        assert!(!is_better(&higher, &lower));
    }

    fn hypothesis(state: StateId) -> FinalHypothesis {
        FinalHypothesis {
            origin: None,
            state: Some(state),
            total_cost: 0,
            ops: EditOps {
                synchronous: 0,
                substitutions: 0,
                cell_mismatches: 0,
                observed_only: 0,
                model_only: 0,
            },
            opaque_steps: 0,
            identity_frozen: false,
        }
    }
}

fn mismatch_count(ops: EditOps) -> u32 {
    u32::from(ops.substitutions)
        .saturating_add(u32::from(ops.cell_mismatches))
        .saturating_add(u32::from(ops.observed_only))
}
