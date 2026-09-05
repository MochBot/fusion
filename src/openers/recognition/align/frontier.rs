//! The alignment frontier: the set of model states reachable after consuming
//! the observations so far, each with its best edit cost.
//!
//! Model states are dense (`StateId` indexes `graph.states`), so the frontier
//! is a dense table plus the list of occupied slots. Every iteration walks the
//! occupied slots in `StateId` order, which makes every tie between equal
//! entries resolve the same way on every run.

use std::collections::VecDeque;

use super::super::cost::{edit_cost, EditCosts, EditOps};
use super::super::graph::{CanonicalKey, RecognitionGraph, StateId, TransitionLabel};
use super::FinalHypothesis;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) cost: u32,
    pub(super) ops: EditOps,
    pub(super) opaque_steps: u16,
}

#[derive(Clone, Default)]
pub(super) struct Frontier {
    slots: Vec<Option<Entry>>,
    occupied: Vec<StateId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Truncation {
    pub(super) truncated: bool,
    pub(super) evicted_zero_cost: u32,
    pub(super) zero_cost_excess: u32,
}

impl Frontier {
    pub(super) fn sized_for(graph: &RecognitionGraph) -> Self {
        Self {
            slots: vec![None; graph.states.len()],
            occupied: Vec::new(),
        }
    }

    fn empty_like(&self) -> Self {
        Self {
            slots: vec![None; self.slots.len()],
            occupied: Vec::new(),
        }
    }

    pub(super) fn len(&self) -> u32 {
        u32::try_from(self.occupied.len()).unwrap_or(u32::MAX)
    }

    #[cfg(test)]
    pub(super) fn get(&self, state: StateId) -> Option<&Entry> {
        self.slots.get(state.0).and_then(Option::as_ref)
    }

    /// Occupied states in `StateId` order with their entries.
    pub(super) fn entries(&self) -> impl Iterator<Item = (StateId, &Entry)> + '_ {
        let mut occupied = self.occupied.clone();
        occupied.sort_unstable_by_key(|state| state.0);
        occupied
            .into_iter()
            .filter_map(move |state| self.slots[state.0].as_ref().map(|entry| (state, entry)))
    }

    pub(super) fn best_cost(&self) -> Option<u32> {
        self.occupied
            .iter()
            .filter_map(|state| self.slots[state.0].as_ref())
            .map(|entry| entry.cost)
            .min()
    }

    pub(super) fn best_evidence(&self) -> Option<(EditOps, u16)> {
        self.occupied
            .iter()
            .filter_map(|state| self.slots[state.0].as_ref().map(|entry| (state, entry)))
            .min_by_key(|(state, entry)| (entry.cost, state.0))
            .map(|(_, entry)| (entry.ops, entry.opaque_steps))
    }

    pub(super) fn insert_initial(
        &mut self,
        graph: &RecognitionGraph,
        state: StateId,
        cost: u32,
        ops: EditOps,
        costs: &EditCosts,
    ) {
        let entry = if graph.states[state.0].identity_opaque {
            Entry {
                cost: cost.saturating_add(costs.identity_opaque_entry),
                ops,
                opaque_steps: 1,
            }
        } else {
            Entry {
                cost,
                ops,
                opaque_steps: 0,
            }
        };
        self.offer(state, entry);
    }

    /// Consumes one present observation. The observation may be matched to a
    /// model lock after 0, 1, or 2 model-only advances, or kept as an
    /// observed-only lock at any of those depths; the result is the pointwise
    /// minimum over every alternative.
    pub(super) fn consume_observation(
        mut self,
        graph: &RecognitionGraph,
        observation: &CanonicalKey,
        costs: &EditCosts,
    ) -> Self {
        // Depth k is the frontier after k model-only advances (each closed
        // under epsilon). The consumed frontier is the pointwise minimum over
        // k ∈ {0, 1, 2} of skipping the observation (observed-only) or matching
        // it to a lock/bridge out of depth k. `offer` is a min-keep, so
        // depths can be folded in as they are produced; no depth is cloned.
        self.close_epsilon(graph, costs);
        let mut consumed = self.empty_like();
        let mut current = self;
        for depth in 0..3 {
            consumed.add_observed_only(&current, costs);
            consumed.add_lock_advances(graph, &current, observation, costs);
            if depth == 2 {
                break;
            }
            let mut next = current.advance_model_only(graph, costs);
            next.close_epsilon(graph, costs);
            current = next;
        }
        consumed
    }

    pub(super) fn consume_missing_observation(
        &self,
        graph: &RecognitionGraph,
        costs: &EditCosts,
    ) -> Self {
        let mut missing = self.clone();
        for (state, entry) in self.entries() {
            for transition in &graph.out[state.0] {
                if matches!(transition.label, TransitionLabel::Lock { .. }) {
                    missing.offer(
                        transition.to,
                        enter_state(graph, state, transition.to, entry, 0, empty_ops(), costs),
                    );
                }
            }
        }
        missing.close_epsilon(graph, costs);
        missing
    }

    fn advance_model_only(&self, graph: &RecognitionGraph, costs: &EditCosts) -> Self {
        let mut advanced = self.empty_like();
        for (state, entry) in self.entries() {
            for transition in &graph.out[state.0] {
                if matches!(&transition.label, TransitionLabel::Lock { .. }) {
                    let mut ops = empty_ops();
                    ops.model_only = 1;
                    advanced.offer(
                        transition.to,
                        enter_state(
                            graph,
                            state,
                            transition.to,
                            entry,
                            costs.model_only,
                            ops,
                            costs,
                        ),
                    );
                }
            }
        }
        advanced
    }

    fn add_observed_only(&mut self, source: &Self, costs: &EditCosts) {
        for (state, entry) in source.entries() {
            let mut ops = empty_ops();
            ops.observed_only = 1;
            self.offer(
                state,
                Entry {
                    cost: entry.cost.saturating_add(costs.observed_only),
                    ops: add_ops(entry.ops, ops),
                    opaque_steps: entry.opaque_steps,
                },
            );
        }
    }

    fn add_lock_advances(
        &mut self,
        graph: &RecognitionGraph,
        source: &Self,
        observation: &CanonicalKey,
        costs: &EditCosts,
    ) {
        for (state, entry) in source.entries() {
            for transition in &graph.out[state.0] {
                if matches!(
                    &transition.label,
                    TransitionLabel::Lock { .. } | TransitionLabel::Bridge { .. }
                ) {
                    let (score, ops) = edit_cost(
                        observation,
                        &graph.states[transition.to.0].physical_key,
                        costs,
                    );
                    let bridge_cost =
                        u32::from(matches!(&transition.label, TransitionLabel::Bridge { .. }))
                            * costs.identity_opaque_entry;
                    self.offer(
                        transition.to,
                        enter_state(
                            graph,
                            state,
                            transition.to,
                            entry,
                            score.saturating_add(bridge_cost),
                            ops,
                            costs,
                        ),
                    );
                }
            }
        }
    }

    pub(super) fn close_epsilon(&mut self, graph: &RecognitionGraph, costs: &EditCosts) {
        let mut pending = self
            .entries()
            .map(|(state, _)| state)
            .collect::<VecDeque<_>>();
        while let Some(state) = pending.pop_front() {
            let Some(entry) = self.slots[state.0].clone() else {
                continue;
            };
            for transition in &graph.out[state.0] {
                if matches!(&transition.label, TransitionLabel::Epsilon { .. }) {
                    let next =
                        enter_state(graph, state, transition.to, &entry, 0, empty_ops(), costs);
                    if self.offer(transition.to, next) {
                        pending.push_back(transition.to);
                    }
                }
            }
        }
    }

    pub(super) fn truncate(&mut self, maximum: u32) -> Truncation {
        let maximum = usize::try_from(maximum).unwrap_or(usize::MAX);
        if self.occupied.len() <= maximum {
            return Truncation::default();
        }
        let mut states = self.occupied.clone();
        states.sort_unstable_by_key(|state| {
            (
                self.slots[state.0]
                    .as_ref()
                    .map_or(u32::MAX, |entry| entry.cost),
                state.0,
            )
        });
        // Amended exactness contract: the zero-cost lane is always complete; the overall
        // optimum is exact only when `truncated_any` is false, otherwise flags disclose it.
        let zero_cost = states
            .iter()
            .take_while(|state| {
                self.slots[state.0]
                    .as_ref()
                    .is_some_and(|entry| entry.cost == 0)
            })
            .count();
        let retained = maximum.max(zero_cost);
        for state in states.iter().copied().skip(retained) {
            self.slots[state.0] = None;
        }
        states.truncate(retained);
        let total = self.occupied.len();
        self.occupied = states;
        Truncation {
            truncated: total > retained,
            evicted_zero_cost: 0,
            zero_cost_excess: u32::try_from(zero_cost.saturating_sub(maximum)).unwrap_or(u32::MAX),
        }
    }

    pub(super) fn final_hypotheses(
        &self,
        graph: &RecognitionGraph,
        retained_record: Option<&str>,
    ) -> Vec<FinalHypothesis> {
        super::evidence::final_hypotheses(graph, self.entries(), retained_record)
    }

    /// Installs `entry` at `state` when it beats the current entry. Returns
    /// whether the slot changed.
    fn offer(&mut self, state: StateId, entry: Entry) -> bool {
        let slot = &mut self.slots[state.0];
        match slot {
            Some(current) if !entry_is_better(&entry, current) => false,
            Some(current) => {
                *current = entry;
                true
            }
            None => {
                *slot = Some(entry);
                self.occupied.push(state);
                true
            }
        }
    }
}

fn entry_is_better(candidate: &Entry, current: &Entry) -> bool {
    (
        candidate.cost,
        candidate.opaque_steps,
        candidate.ops.synchronous,
        candidate.ops.substitutions,
        candidate.ops.cell_mismatches,
        candidate.ops.observed_only,
        candidate.ops.model_only,
    ) < (
        current.cost,
        current.opaque_steps,
        current.ops.synchronous,
        current.ops.substitutions,
        current.ops.cell_mismatches,
        current.ops.observed_only,
        current.ops.model_only,
    )
}

fn enter_state(
    graph: &RecognitionGraph,
    from: StateId,
    to: StateId,
    entry: &Entry,
    score: u32,
    ops: EditOps,
    costs: &EditCosts,
) -> Entry {
    let entering_opaque =
        !graph.states[from.0].identity_opaque && graph.states[to.0].identity_opaque;
    Entry {
        cost: entry
            .cost
            .saturating_add(score)
            .saturating_add(u32::from(entering_opaque) * costs.identity_opaque_entry),
        ops: add_ops(entry.ops, ops),
        opaque_steps: entry
            .opaque_steps
            .saturating_add(u16::from(entering_opaque)),
    }
}

const fn add_ops(left: EditOps, right: EditOps) -> EditOps {
    EditOps {
        synchronous: left.synchronous.saturating_add(right.synchronous),
        substitutions: left.substitutions.saturating_add(right.substitutions),
        cell_mismatches: left.cell_mismatches.saturating_add(right.cell_mismatches),
        observed_only: left.observed_only.saturating_add(right.observed_only),
        model_only: left.model_only.saturating_add(right.model_only),
    }
}

pub(super) const fn empty_ops() -> EditOps {
    EditOps {
        synchronous: 0,
        substitutions: 0,
        cell_mismatches: 0,
        observed_only: 0,
        model_only: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frontier_with(entries: &[(usize, u32)]) -> Frontier {
        let mut frontier = Frontier {
            slots: vec![None; 8],
            occupied: Vec::new(),
        };
        for (state, cost) in entries {
            frontier.offer(
                StateId(*state),
                Entry {
                    cost: *cost,
                    ops: empty_ops(),
                    opaque_steps: 0,
                },
            );
        }
        frontier
    }

    #[test]
    fn truncate_keeps_zero_cost_entries_over_higher_cost_entries() {
        let mut frontier = frontier_with(&[(0, 0), (1, 0), (2, 1)]);

        let truncation = frontier.truncate(1);

        assert!(truncation.truncated);
        assert_eq!(truncation.evicted_zero_cost, 0);
        assert_eq!(truncation.zero_cost_excess, 1);
        assert!(frontier.get(StateId(0)).is_some());
        assert!(frontier.get(StateId(1)).is_some());
        assert!(frontier.get(StateId(2)).is_none());
        assert_eq!(frontier.len(), 2);
    }

    #[test]
    fn entries_iterate_in_state_order_regardless_of_insertion_order() {
        let frontier = frontier_with(&[(5, 3), (1, 2), (3, 1)]);

        let order = frontier
            .entries()
            .map(|(state, _)| state.0)
            .collect::<Vec<_>>();

        assert_eq!(order, vec![1, 3, 5]);
    }

    #[test]
    fn offer_keeps_the_first_entry_on_an_exact_tie() {
        let mut frontier = frontier_with(&[(2, 4)]);
        let replaced = frontier.offer(
            StateId(2),
            Entry {
                cost: 4,
                ops: empty_ops(),
                opaque_steps: 0,
            },
        );

        assert!(!replaced);
        assert_eq!(frontier.len(), 1);
    }
}
