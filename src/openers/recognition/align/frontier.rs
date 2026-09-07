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

#[derive(Clone)]
pub(super) struct Frontier {
    slots: Vec<Option<Entry>>,
    occupied: Vec<StateId>,
    ordered: bool,
}

/// Dense per-observation cache of `edit_cost` by target state. The cost
/// depends only on the observation, the target state's key, and the costs,
/// so every lock/bridge edge into the same target within one observation
/// shares one computation. Cleared on entry to each observation, so values
/// never cross observations, graphs, or cost sets.
pub(super) struct EditCostScratch {
    slots: Vec<Option<(u32, EditOps)>>,
    touched: Vec<StateId>,
    #[cfg(test)]
    computed: u32,
}

impl EditCostScratch {
    pub(super) fn sized_for(graph: &RecognitionGraph) -> Self {
        Self {
            slots: vec![None; graph.states.len()],
            touched: Vec::new(),
            #[cfg(test)]
            computed: 0,
        }
    }

    fn clear_for(&mut self, state_count: usize) {
        if self.slots.len() != state_count {
            self.slots = vec![None; state_count];
            self.touched.clear();
            return;
        }
        for state in self.touched.drain(..) {
            self.slots[state.0] = None;
        }
    }

    fn cost(
        &mut self,
        observation: &CanonicalKey,
        graph: &RecognitionGraph,
        target: StateId,
        costs: &EditCosts,
    ) -> (u32, EditOps) {
        if let Some(hit) = self.slots[target.0] {
            return hit;
        }
        let value = edit_cost(observation, &graph.states[target.0].physical_key, costs);
        self.slots[target.0] = Some(value);
        self.touched.push(target);
        #[cfg(test)]
        {
            self.computed += 1;
        }
        value
    }

    #[cfg(test)]
    pub(super) fn computed(&self) -> u32 {
        self.computed
    }
}

impl Default for Frontier {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            occupied: Vec::new(),
            ordered: true,
        }
    }
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
            ordered: true,
        }
    }

    /// Clears only the occupied slots, retaining both allocations for reuse
    /// as scratch storage across observations.
    fn clear_reuse(&mut self) {
        for state in self.occupied.drain(..) {
            self.slots[state.0] = None;
        }
        self.ordered = true;
    }

    pub(super) fn len(&self) -> u32 {
        u32::try_from(self.occupied.len()).unwrap_or(u32::MAX)
    }

    #[cfg(test)]
    pub(super) fn get(&self, state: StateId) -> Option<&Entry> {
        self.slots.get(state.0).and_then(Option::as_ref)
    }

    /// Occupied states in `StateId` order with their entries. The occupied
    /// list is sorted in place on the first read after a membership change,
    /// so repeated walks over an unchanged frontier skip the sort.
    pub(super) fn entries(&mut self) -> impl Iterator<Item = (StateId, &Entry)> + '_ {
        if !self.ordered {
            self.occupied.sort_unstable_by_key(|state| state.0);
            self.ordered = true;
        }
        self.occupied
            .iter()
            .filter_map(|state| self.slots[state.0].as_ref().map(|entry| (*state, entry)))
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
    /// minimum over every alternative. `acc` and `next` are caller-owned
    /// scratch frontiers reused across observations; both are clobbered.
    pub(super) fn consume_observation(
        &mut self,
        graph: &RecognitionGraph,
        observation: &CanonicalKey,
        costs: &EditCosts,
        acc: &mut Self,
        next: &mut Self,
        edit: &mut EditCostScratch,
    ) {
        // Depth k is the frontier after k model-only advances (each closed
        // under epsilon). The consumed frontier is the pointwise minimum over
        // k ∈ {0, 1, 2} of skipping the observation (observed-only) or matching
        // it to a lock/bridge out of depth k. `offer` is a min-keep, so
        // depths can be folded in as they are produced; no depth is cloned.
        self.close_epsilon(graph, costs);
        acc.clear_reuse();
        edit.clear_for(graph.states.len());
        for depth in 0..3 {
            acc.add_depth_contributions(graph, self, observation, costs, edit);
            if depth == 2 {
                break;
            }
            next.clear_reuse();
            next.advance_model_only_from(graph, self, costs);
            next.close_epsilon(graph, costs);
            std::mem::swap(self, next);
        }
        std::mem::swap(self, acc);
    }

    pub(super) fn consume_missing_observation(
        &mut self,
        graph: &RecognitionGraph,
        costs: &EditCosts,
        scratch: &mut Self,
    ) {
        scratch.clear_reuse();
        for (state, entry) in self.entries() {
            scratch.offer(state, entry.clone());
            for transition in &graph.out[state.0] {
                if matches!(transition.label, TransitionLabel::Lock { .. }) {
                    scratch.offer(
                        transition.to,
                        enter_state(graph, state, transition.to, entry, 0, empty_ops(), costs),
                    );
                }
            }
        }
        scratch.close_epsilon(graph, costs);
        std::mem::swap(self, scratch);
    }

    fn advance_model_only_from(
        &mut self,
        graph: &RecognitionGraph,
        source: &mut Self,
        costs: &EditCosts,
    ) {
        for (state, entry) in source.entries() {
            for transition in &graph.out[state.0] {
                if matches!(&transition.label, TransitionLabel::Lock { .. }) {
                    let mut ops = empty_ops();
                    ops.model_only = 1;
                    self.offer(
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
    }

    /// Folds one depth's observed-only skip and lock/bridge matches into the
    /// accumulator in a single walk. Both offers are min-keeps from an
    /// unchanged source, so one pass matches two separate passes exactly.
    fn add_depth_contributions(
        &mut self,
        graph: &RecognitionGraph,
        source: &mut Self,
        observation: &CanonicalKey,
        costs: &EditCosts,
        edit: &mut EditCostScratch,
    ) {
        for (state, entry) in source.entries() {
            let mut observed_ops = empty_ops();
            observed_ops.observed_only = 1;
            self.offer(
                state,
                Entry {
                    cost: entry.cost.saturating_add(costs.observed_only),
                    ops: add_ops(entry.ops, observed_ops),
                    opaque_steps: entry.opaque_steps,
                },
            );
            for transition in &graph.out[state.0] {
                if matches!(
                    &transition.label,
                    TransitionLabel::Lock { .. } | TransitionLabel::Bridge { .. }
                ) {
                    let (score, ops) = edit.cost(observation, graph, transition.to, costs);
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
        self.ordered = false;
        Truncation {
            truncated: total > retained,
            evicted_zero_cost: 0,
            zero_cost_excess: u32::try_from(zero_cost.saturating_sub(maximum)).unwrap_or(u32::MAX),
        }
    }

    pub(super) fn final_hypotheses(
        &mut self,
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
                self.ordered = false;
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
    use crate::openers::recognition::census::CompileCensus;
    use crate::openers::recognition::graph::{ModelState, Transition};
    use std::collections::{HashMap, HashSet};

    fn frontier_with(entries: &[(usize, u32)]) -> Frontier {
        let mut frontier = Frontier {
            slots: vec![None; 8],
            occupied: Vec::new(),
            ordered: true,
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
        let mut frontier = frontier_with(&[(5, 3), (1, 2), (3, 1)]);

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

    /// Fan-in fixture: 3 lock-edge visits but only 2 distinct targets, so
    /// a per-observation target cache can skip one `edit_cost` recompute.
    #[test]
    fn fan_in_walk_visits_more_edges_than_distinct_targets() {
        let graph = fan_in_graph();
        let mut source = frontier_with(&[(0, 0), (1, 0)]);
        let observation = CanonicalKey {
            masks: vec![0b0000000001].into(),
            letters: None,
        };

        let mut edge_visits = 0;
        let mut distinct = HashSet::new();
        for (state, _) in source.entries() {
            for transition in &graph.out[state.0] {
                if matches!(
                    &transition.label,
                    TransitionLabel::Lock { .. } | TransitionLabel::Bridge { .. }
                ) {
                    edge_visits += 1;
                    distinct.insert(transition.to.0);
                    let _ = crate::openers::recognition::cost::edit_cost(
                        &observation,
                        &graph.states[transition.to.0].physical_key,
                        &EditCosts::default(),
                    );
                }
            }
        }

        assert_eq!(edge_visits, 3);
        assert_eq!(distinct.len(), 2);
    }

    #[test]
    fn depth_walk_computes_each_target_once() {
        use crate::openers::recognition::cost::edit_cost;

        let graph = fan_in_graph();
        let mut source = frontier_with(&[(0, 0), (1, 0)]);
        let mut acc = Frontier::sized_for(&graph);
        let mut edit = EditCostScratch::sized_for(&graph);
        let observation = CanonicalKey {
            masks: vec![0b0000000001].into(),
            letters: None,
        };
        let costs = EditCosts::default();

        acc.add_depth_contributions(&graph, &mut source, &observation, &costs, &mut edit);

        assert_eq!(edit.computed(), 2);
        for target in [2, 3] {
            let (score, _) = edit_cost(&observation, &graph.states[target].physical_key, &costs);
            assert_eq!(acc.get(StateId(target)).unwrap().cost, score);
        }
    }

    #[test]
    fn scratch_reuse_across_observations_graphs_and_costs_stays_exact() {
        use crate::openers::recognition::cost::edit_cost;

        let graph = fan_in_graph();
        let costs = EditCosts::default();
        let mut edit = EditCostScratch::sized_for(&graph);
        let first = CanonicalKey {
            masks: vec![0b0000000001].into(),
            letters: None,
        };
        let second = CanonicalKey {
            masks: vec![0b0000000100].into(),
            letters: None,
        };

        edit.clear_for(graph.states.len());
        assert_eq!(
            edit.cost(&first, &graph, StateId(2), &costs),
            edit_cost(&first, &graph.states[2].physical_key, &costs)
        );

        edit.clear_for(graph.states.len());
        assert_eq!(
            edit.cost(&second, &graph, StateId(2), &costs),
            edit_cost(&second, &graph.states[2].physical_key, &costs)
        );

        let other_costs = EditCosts {
            cell_mismatch: 7,
            ..EditCosts::default()
        };
        edit.clear_for(graph.states.len());
        assert_eq!(
            edit.cost(&second, &graph, StateId(2), &other_costs),
            edit_cost(&second, &graph.states[2].physical_key, &other_costs)
        );

        let bigger = fan_in_graph_wider();
        edit.clear_for(bigger.states.len());
        assert_eq!(
            edit.cost(&second, &bigger, StateId(4), &costs),
            edit_cost(&second, &bigger.states[4].physical_key, &costs)
        );
    }

    mod cache_invalidation {
        use super::*;
        use crate::openers::recognition::graph::BridgeReason;

        /// Second `consume_observation` on one shared scratch must recompute
        /// targets; it fails when the production reset is removed.
        #[test]
        fn second_observation_recomputes_targets_on_shared_scratch() {
            let graph = invalidation_graph();
            let costs = EditCosts::default();
            let mut frontier = zero_frontier(&graph);
            let mut acc = Frontier::sized_for(&graph);
            let mut next = Frontier::sized_for(&graph);
            let mut edit = EditCostScratch::sized_for(&graph);

            frontier.consume_observation(
                &graph,
                &first_observation(),
                &costs,
                &mut acc,
                &mut next,
                &mut edit,
            );

            assert_eq!(
                frontier.get(StateId(2)),
                Some(&Entry {
                    cost: 5,
                    ops: EditOps {
                        synchronous: 0,
                        substitutions: 1,
                        cell_mismatches: 0,
                        observed_only: 0,
                        model_only: 0,
                    },
                    opaque_steps: 1,
                })
            );
            assert_eq!(
                frontier.get(StateId(3)),
                Some(&Entry {
                    cost: 5,
                    ops: EditOps {
                        synchronous: 0,
                        substitutions: 1,
                        cell_mismatches: 0,
                        observed_only: 0,
                        model_only: 0,
                    },
                    opaque_steps: 0,
                })
            );

            let mut reference = frontier.clone();
            let mut fresh = EditCostScratch::sized_for(&graph);
            let computed_before = edit.computed();
            frontier.consume_observation(
                &graph,
                &second_observation(),
                &costs,
                &mut acc,
                &mut next,
                &mut edit,
            );
            reference.consume_observation(
                &graph,
                &second_observation(),
                &costs,
                &mut acc,
                &mut next,
                &mut fresh,
            );

            assert_eq!(
                snapshot(&frontier, graph.states.len()),
                snapshot(&reference, graph.states.len())
            );
            assert_eq!(edit.computed() - computed_before, fresh.computed());
        }

        /// A missing observation between consumes must not preserve the
        /// previous observation's cached costs either.
        #[test]
        fn missing_observation_between_consumes_still_recomputes() {
            let graph = invalidation_graph();
            let costs = EditCosts::default();
            let mut frontier = zero_frontier(&graph);
            let mut acc = Frontier::sized_for(&graph);
            let mut next = Frontier::sized_for(&graph);
            let mut missing = Frontier::sized_for(&graph);
            let mut edit = EditCostScratch::sized_for(&graph);

            frontier.consume_observation(
                &graph,
                &first_observation(),
                &costs,
                &mut acc,
                &mut next,
                &mut edit,
            );
            frontier.consume_missing_observation(&graph, &costs, &mut missing);

            let mut reference = frontier.clone();
            let mut fresh = EditCostScratch::sized_for(&graph);
            let computed_before = edit.computed();
            frontier.consume_observation(
                &graph,
                &second_observation(),
                &costs,
                &mut acc,
                &mut next,
                &mut edit,
            );
            reference.consume_observation(
                &graph,
                &second_observation(),
                &costs,
                &mut acc,
                &mut next,
                &mut fresh,
            );

            assert_eq!(
                snapshot(&frontier, graph.states.len()),
                snapshot(&reference, graph.states.len())
            );
            assert_eq!(edit.computed() - computed_before, fresh.computed());
        }

        fn zero_frontier(graph: &RecognitionGraph) -> Frontier {
            let mut frontier = Frontier::sized_for(graph);
            for state in [0, 1] {
                frontier.offer(
                    StateId(state),
                    Entry {
                        cost: 0,
                        ops: empty_ops(),
                        opaque_steps: 0,
                    },
                );
            }
            frontier
        }

        fn snapshot(frontier: &Frontier, states: usize) -> Vec<Option<Entry>> {
            (0..states)
                .map(|state| frontier.get(StateId(state)).cloned())
                .collect()
        }

        fn first_observation() -> CanonicalKey {
            CanonicalKey {
                masks: vec![0b0000000011].into(),
                letters: Some(b"IO________".as_slice().into()),
            }
        }

        fn second_observation() -> CanonicalKey {
            CanonicalKey {
                masks: vec![0b0000000101].into(),
                letters: Some(b"I_O_______".as_slice().into()),
            }
        }

        fn invalidation_graph() -> RecognitionGraph {
            fn state(masks: &[u16], letters: &[u8], identity_opaque: bool) -> ModelState {
                ModelState {
                    physical_key: CanonicalKey {
                        masks: masks.into(),
                        letters: Some(letters.into()),
                    },
                    origins: Vec::new(),
                    identity_opaque,
                }
            }
            fn lock(from: usize, to: usize) -> Transition {
                Transition {
                    from: StateId(from),
                    to: StateId(to),
                    label: TransitionLabel::Lock {
                        letter: b'O',
                        cells: [[0, 0], [1, 0], [0, 1], [1, 1]],
                        cleared_rows: Box::new([]),
                        is_pc: false,
                    },
                }
            }
            fn bridge(from: usize, to: usize) -> Transition {
                Transition {
                    from: StateId(from),
                    to: StateId(to),
                    label: TransitionLabel::Bridge {
                        reason: BridgeReason::DirectImpossible,
                    },
                }
            }
            RecognitionGraph {
                states: vec![
                    state(&[0b0000000011], b"IO________", false),
                    state(&[0b0000000011], b"IO________", false),
                    state(&[0b0000000011], b"OO________", true),
                    state(&[0b0000000011], b"II________", false),
                ],
                out: vec![
                    vec![lock(0, 2), bridge(0, 3)],
                    vec![lock(1, 2), bridge(1, 3)],
                    vec![],
                    vec![],
                ],
                exact_index: HashMap::new(),
                epsilon_order: Box::new([]),
                census: CompileCensus::new(0, 0, 0, 0),
            }
        }
    }

    fn fan_in_graph_wider() -> RecognitionGraph {
        let mut graph = fan_in_graph();
        graph.states.push(ModelState {
            physical_key: CanonicalKey {
                masks: vec![0b0000001000].into(),
                letters: None,
            },
            origins: Vec::new(),
            identity_opaque: false,
        });
        graph.out.push(Vec::new());
        graph
    }

    fn fan_in_graph() -> RecognitionGraph {
        fn state(masks: &[u16]) -> ModelState {
            ModelState {
                physical_key: CanonicalKey {
                    masks: masks.into(),
                    letters: None,
                },
                origins: Vec::new(),
                identity_opaque: false,
            }
        }
        fn lock(from: usize, to: usize) -> Transition {
            Transition {
                from: StateId(from),
                to: StateId(to),
                label: TransitionLabel::Lock {
                    letter: b'O',
                    cells: [[0, 0], [1, 0], [0, 1], [1, 1]],
                    cleared_rows: Box::new([]),
                    is_pc: false,
                },
            }
        }
        RecognitionGraph {
            states: vec![
                state(&[0b0000000001]),
                state(&[0b0000000010]),
                state(&[0b0000000011]),
                state(&[0b0000000100]),
            ],
            out: vec![
                vec![lock(0, 2), lock(0, 3)],
                vec![lock(1, 2)],
                vec![],
                vec![],
            ],
            exact_index: HashMap::new(),
            epsilon_order: Box::new([]),
            census: CompileCensus::new(0, 0, 0, 0),
        }
    }
}
