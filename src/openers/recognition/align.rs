mod evidence;
mod frontier;

#[cfg(test)]
mod tests;

use super::cost::{edit_cost, EditCosts, EditOps};
use super::graph::{CanonicalKey, RecognitionGraph, StateId, StateOrigin};
use super::retrieval::{seed_candidates, SeedBudget, Seeds};
use frontier::{empty_ops, EditCostScratch, Frontier};

#[derive(Clone, Debug)]
pub(crate) struct Observation {
    pub(crate) key: CanonicalKey,
    pub(crate) had_garbage: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct AlignBudget {
    pub(crate) seed: SeedBudget,
    pub(crate) max_active_product_states: u32,
}

impl Default for AlignBudget {
    fn default() -> Self {
        Self {
            seed: SeedBudget::default(),
            max_active_product_states: 4096,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LockAlignment {
    pub(crate) active_product_states: u32,
    pub(crate) seeded: u32,
    pub(crate) reseed_truncated: bool,
    pub(crate) best_cost: Option<u32>,
    pub(crate) unknown_cost: u32,
    pub(crate) truncated: bool,
    pub(crate) evicted_zero_cost: u32,
    pub(crate) zero_cost_excess: u32,
    pub(crate) ops: EditOps,
    pub(crate) exact_hits: u32,
    pub(crate) opaque_entries: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct FinalHypothesis {
    pub(crate) origin: Option<StateOrigin>,
    pub(crate) state: Option<StateId>,
    pub(crate) total_cost: u32,
    pub(crate) ops: EditOps,
    #[cfg(test)]
    pub(crate) opaque_steps: u16,
    pub(crate) identity_frozen: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct AlignmentResult {
    pub(crate) per_lock: Vec<LockAlignment>,
    pub(crate) hypotheses: Vec<FinalHypothesis>,
    pub(crate) best_margin: Option<u32>,
    pub(crate) truncated_any: bool,
}

#[cfg(test)]
pub(crate) fn align_round_exact(
    graph: &RecognitionGraph,
    costs: &EditCosts,
    observations: &[Option<Observation>],
    budget: &AlignBudget,
) -> AlignmentResult {
    align_round_exact_retaining_record(graph, costs, observations, budget, None)
}

pub(crate) fn align_round_exact_retaining_record(
    graph: &RecognitionGraph,
    costs: &EditCosts,
    observations: &[Option<Observation>],
    budget: &AlignBudget,
    retained_record: Option<&str>,
) -> AlignmentResult {
    let mut frontier = Frontier::sized_for(graph);
    let mut scratch_a = Frontier::sized_for(graph);
    let mut scratch_b = Frontier::sized_for(graph);
    let mut edit_scratch = EditCostScratch::sized_for(graph);
    let mut per_lock = Vec::with_capacity(observations.len());
    let mut unknown_cost = 0;
    let mut initialized = false;
    let mut truncated_any = false;

    for observation in observations {
        let Some(observation) = observation else {
            frontier.consume_missing_observation(graph, costs, &mut scratch_a);
            let (ops, opaque_steps) = frontier.best_evidence().unwrap_or((empty_ops(), 0));
            per_lock.push(LockAlignment {
                active_product_states: frontier.len(),
                seeded: 0,
                reseed_truncated: false,
                best_cost: frontier.best_cost(),
                unknown_cost,
                truncated: false,
                evicted_zero_cost: 0,
                zero_cost_excess: 0,
                ops,
                exact_hits: 0,
                opaque_entries: u32::from(opaque_steps > 0),
            });
            continue;
        };
        let _ = observation.had_garbage;
        unknown_cost = unknown_cost.saturating_add(costs.unknown_per_lock);
        let seed_cost = if initialized { unknown_cost } else { 0 };
        // Exact-seed-only initialization with per-lock rejoin is the authored recall mechanism.
        // Its coverage is measured in the battery and currently retains every sampled identity.
        let seeds = ordered_exact_seeds(graph, &observation.key, &budget.seed, seed_cost, costs);
        let seeded = u32::try_from(seeds.exact.len()).unwrap_or(u32::MAX);
        let exact_hits = graph
            .exact_index
            .get(&observation.key.occupancy_key())
            .map_or(0, |states| u32::try_from(states.len()).unwrap_or(u32::MAX));
        let opaque_entries = u32::try_from(
            seeds
                .exact
                .iter()
                .filter(|state| graph.states[state.0].identity_opaque)
                .count(),
        )
        .unwrap_or(u32::MAX);
        let reseed_truncated = initialized && seeds.truncated;
        let mut truncated = seeds.truncated;

        if !initialized {
            for state in seeds.exact {
                let (cost, ops) =
                    edit_cost(&observation.key, &graph.states[state.0].physical_key, costs);
                frontier.insert_initial(graph, state, cost, ops, costs);
            }
            initialized = true;
        } else {
            frontier.consume_observation(
                graph,
                &observation.key,
                costs,
                &mut scratch_a,
                &mut scratch_b,
                &mut edit_scratch,
            );
            for state in seeds.exact {
                frontier.insert_initial(graph, state, unknown_cost, empty_ops(), costs);
            }
        }
        frontier.close_epsilon(graph, costs);
        let truncation = frontier.truncate(budget.max_active_product_states);
        let (ops, _) = frontier.best_evidence().unwrap_or((empty_ops(), 0));
        truncated |= truncation.truncated;
        truncated_any |= truncated;
        per_lock.push(LockAlignment {
            active_product_states: frontier.len(),
            seeded,
            reseed_truncated,
            best_cost: frontier.best_cost(),
            unknown_cost,
            truncated,
            evicted_zero_cost: truncation.evicted_zero_cost,
            zero_cost_excess: truncation.zero_cost_excess,
            ops,
            exact_hits,
            opaque_entries,
        });
    }

    let mut hypotheses = frontier.final_hypotheses(graph, retained_record);
    hypotheses.push(FinalHypothesis {
        origin: None,
        state: None,
        total_cost: unknown_cost,
        ops: empty_ops(),
        #[cfg(test)]
        opaque_steps: 0,
        identity_frozen: false,
    });
    hypotheses.sort_by(|left, right| {
        left.total_cost
            .cmp(&right.total_cost)
            .then_with(|| hypothesis_name(left).cmp(hypothesis_name(right)))
            .then_with(|| hypothesis_state_key(left).cmp(&hypothesis_state_key(right)))
    });
    let best_margin = hypotheses.get(1).map(|runner_up| {
        runner_up
            .total_cost
            .saturating_sub(hypotheses[0].total_cost)
    });

    AlignmentResult {
        per_lock,
        hypotheses,
        best_margin,
        truncated_any,
    }
}

fn ordered_exact_seeds(
    graph: &RecognitionGraph,
    observation: &CanonicalKey,
    budget: &SeedBudget,
    base_cost: u32,
    costs: &EditCosts,
) -> Seeds {
    let mut seeds = seed_candidates(graph, observation, &[], budget);
    seeds.exact.sort_by_key(|state| {
        (
            base_cost.saturating_add(if graph.states[state.0].identity_opaque {
                costs.identity_opaque_entry
            } else {
                0
            }),
            state.0,
        )
    });
    seeds
}

fn hypothesis_name(hypothesis: &FinalHypothesis) -> &str {
    if hypothesis.identity_frozen {
        return "~unresolved-identity";
    }
    hypothesis
        .origin
        .as_ref()
        .map_or("~unknown", |origin| origin.record.as_ref())
}

fn hypothesis_state_key(hypothesis: &FinalHypothesis) -> usize {
    hypothesis.state.map_or(usize::MAX, |state| state.0)
}
