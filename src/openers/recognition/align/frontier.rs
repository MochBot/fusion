//! The alignment frontier: the set of model states reachable after consuming
//! the observations so far, each with its best edit cost.
//!
//! Model states are dense (`StateId` indexes `graph.states`), so the frontier
//! is a dense table plus a bitmap of its occupied slots. Every iteration walks
//! the bitmap in `StateId` order, which makes every tie between equal entries
//! resolve the same way on every run.

use std::collections::VecDeque;

use super::super::cost::{edit_cost, EditCosts, EditOps};
use crate::openers::board::OPENER_BOARD_WIDTH;
use super::super::graph::{CanonicalKey, RecognitionGraph, StateId, TransitionLabel};
use super::FinalHypothesis;

/// Per-alignment lookup tables derived once from the graph.
pub(super) struct AlignGraph<'g> {
    pub(super) graph: &'g RecognitionGraph,
    /// Bit `id % 64` of word `id / 64` is set when state `id` has an
    /// epsilon out-edge.
    epsilon_sources: Vec<u64>,
    /// Lock and bridge out-edges of state `id`, in `graph.out` order, are
    /// `match_edges[match_start[id]..match_start[id + 1]]`.
    match_start: Vec<u32>,
    match_edges: Vec<MatchEdge>,
    /// Distinct physical keys of the graph's states, indexed by key id.
    keys: Vec<&'g CanonicalKey>,
    key_of_state: Vec<u32>,
    /// Most mask rows of any state's physical key.
    max_rows: usize,
}

#[derive(Clone, Copy)]
struct MatchEdge {
    to: u32,
    bridge: bool,
    /// The edge leaves a non-opaque state for an identity-opaque one.
    enters_opaque: bool,
}

impl MatchEdge {
    fn to(self) -> StateId {
        StateId(self.to as usize)
    }
}

impl<'g> AlignGraph<'g> {
    pub(super) fn new(graph: &'g RecognitionGraph) -> Self {
        let mut epsilon_sources = vec![0u64; graph.states.len().div_ceil(64)];
        let mut match_start = Vec::with_capacity(graph.states.len() + 1);
        let mut match_edges = Vec::new();
        let edge_id = |len: usize| u32::try_from(len).expect("graph edges fit u32");
        let mut keys = Vec::new();
        let mut key_ids = std::collections::HashMap::<
            &CanonicalKey,
            u32,
            std::hash::BuildHasherDefault<crate::coach_beam::FxHasher64>,
        >::with_capacity_and_hasher(graph.states.len(), Default::default());
        let key_of_state = graph
            .states
            .iter()
            .map(|state| {
                *key_ids.entry(&state.physical_key).or_insert_with(|| {
                    keys.push(&state.physical_key);
                    edge_id(keys.len() - 1)
                })
            })
            .collect::<Vec<_>>();
        for (state, transitions) in graph.out.iter().enumerate() {
            match_start.push(edge_id(match_edges.len()));
            for transition in transitions {
                let bridge = match transition.label {
                    TransitionLabel::Epsilon { .. } => {
                        epsilon_sources[state / 64] |= 1 << (state % 64);
                        continue;
                    }
                    TransitionLabel::Lock { .. } => false,
                    TransitionLabel::Bridge { .. } => true,
                };
                match_edges.push(MatchEdge {
                    to: u32::try_from(transition.to.0).expect("graph states fit u32"),
                    bridge,
                    enters_opaque: !graph.states[state].identity_opaque
                        && graph.states[transition.to.0].identity_opaque,
                });
            }
        }
        match_start.push(edge_id(match_edges.len()));
        Self {
            graph,
            epsilon_sources,
            match_start,
            match_edges,
            max_rows: keys.iter().map(|key| key.masks.len()).max().unwrap_or(0),
            keys,
            key_of_state,
        }
    }

    fn key_of(&self, state: StateId) -> u32 {
        self.key_of_state[state.0]
    }

    fn match_edges(&self, state: StateId) -> &[MatchEdge] {
        &self.match_edges[self.match_start[state.0] as usize..self.match_start[state.0 + 1] as usize]
    }

    fn is_epsilon_source(&self, state: StateId) -> bool {
        self.epsilon_sources[state.0 / 64] & (1 << (state.0 % 64)) != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) cost: u32,
    pub(super) ops: EditOps,
    pub(super) opaque_steps: u16,
}

#[derive(Clone)]
pub(super) struct Frontier {
    /// `slots[id]` holds the entry of state `id` while its occupied bit is
    /// set; otherwise it is stale and never read.
    slots: Vec<Entry>,
    /// Bit `id % 64` of word `id / 64` is set exactly when state `id` is
    /// in the frontier.
    occupied: Vec<u64>,
    len: usize,
}

/// Dense per-observation cache of `edit_cost` by target key id. The cost
/// depends only on the observation, the target state's physical key, and the
/// costs, so every lock/bridge edge into a target with the same key within
/// one observation shares one computation. Cleared on entry to each
/// observation, so values never cross observations, graphs, or cost sets.
pub(super) struct EditCostScratch {
    slots: Vec<Option<(u32, EditOps)>>,
    touched: Vec<u32>,
    #[cfg(test)]
    computed: u32,
}

impl EditCostScratch {
    pub(super) fn sized_for(graph: &AlignGraph<'_>) -> Self {
        Self {
            slots: vec![None; graph.keys.len()],
            touched: Vec::new(),
            #[cfg(test)]
            computed: 0,
        }
    }

    fn clear_for(&mut self, key_count: usize) {
        if self.slots.len() != key_count {
            self.slots = vec![None; key_count];
            self.touched.clear();
            return;
        }
        for key in self.touched.drain(..) {
            self.slots[key as usize] = None;
        }
    }

    fn cost(
        &mut self,
        observation: &CanonicalKey,
        graph: &AlignGraph<'_>,
        key: u32,
        costs: &EditCosts,
    ) -> (u32, EditOps) {
        if let Some(hit) = self.slots[key as usize] {
            return hit;
        }
        let value = edit_cost(observation, graph.keys[key as usize], costs);
        self.slots[key as usize] = Some(value);
        self.touched.push(key);
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

/// Caller-owned scratch for `consume_observation`, reused across
/// observations; every part is clobbered.
pub(super) struct ConsumeScratch {
    pub(super) acc: Frontier,
    next: Frontier,
    /// Match candidates per target before their edit score is added.
    matched: Frontier,
    edit: EditCostScratch,
}

impl ConsumeScratch {
    pub(super) fn sized_for(graph: &AlignGraph<'_>) -> Self {
        Self {
            acc: Frontier::sized_for(graph.graph),
            next: Frontier::sized_for(graph.graph),
            matched: Frontier::sized_for(graph.graph),
            edit: EditCostScratch::sized_for(graph),
        }
    }

    /// Folds one depth's observed-only skips and lock/bridge matches out of
    /// `source` into the accumulator. A match out of an entry `limits`
    /// admits goes to `matched` without its edit score, which every
    /// candidate into one target shares; the rest are scored here.
    fn add_depth_contributions(
        &mut self,
        graph: &AlignGraph<'_>,
        source: &Frontier,
        observation: &CanonicalKey,
        costs: &EditCosts,
        limits: Option<&DeferLimits>,
    ) {
        let mut observed_ops = empty_ops();
        observed_ops.observed_only = 1;
        for (state, entry) in source.entries() {
            self.acc.offer(
                state,
                Entry {
                    cost: entry.cost.saturating_add(costs.observed_only),
                    ops: add_ops(entry.ops, observed_ops),
                    opaque_steps: entry.opaque_steps,
                },
            );
            let deferred = limits.is_some_and(|limits| limits.admits(entry));
            for edge in graph.match_edges(state) {
                let bridge_cost = u32::from(edge.bridge) * costs.identity_opaque_entry;
                if deferred {
                    self.matched.offer(
                        edge.to(),
                        enter(entry, bridge_cost, empty_ops(), edge.enters_opaque, costs),
                    );
                } else {
                    let (score, ops) =
                        self.edit
                            .cost(observation, graph, graph.key_of(edge.to()), costs);
                    self.acc.offer(
                        edge.to(),
                        enter(
                            entry,
                            score.saturating_add(bridge_cost),
                            ops,
                            edge.enters_opaque,
                            costs,
                        ),
                    );
                }
            }
        }
    }

    /// Adds each target's edit score to its best unscored match candidate.
    /// Adding the same score to every candidate keeps their order while no
    /// field saturates, which `DeferLimits` guarantees, so this equals
    /// scoring every candidate before taking the minimum.
    fn add_deferred_matches(
        &mut self,
        graph: &AlignGraph<'_>,
        observation: &CanonicalKey,
        costs: &EditCosts,
    ) {
        for (state, entry) in self.matched.entries() {
            let (score, ops) = self.edit.cost(observation, graph, graph.key_of(state), costs);
            self.acc.offer(state, enter(entry, score, ops, false, costs));
        }
    }
}

/// Largest entry fields whose match candidates may defer their edit score:
/// no candidate built from an admitted entry saturates any field, even after
/// its score is added.
#[derive(Clone, Copy)]
struct DeferLimits {
    cost: u32,
    synchronous: u16,
    substitutions: u16,
    cell_mismatches: u16,
}

impl DeferLimits {
    fn new(graph: &AlignGraph<'_>, observation: &CanonicalKey, costs: &EditCosts) -> Option<Self> {
        // `edit_cost` counts each cell of the longer key's rows at most once
        // as a mismatch and once as a substitution.
        let cells = u64::try_from(OPENER_BOARD_WIDTH * graph.max_rows.max(observation.masks.len()))
            .unwrap_or(u64::MAX)
            .min(u64::from(u16::MAX));
        let score = cells * u64::from(costs.cell_mismatch) + cells * u64::from(costs.substitute_letter);
        // A bridge and an opaque entry each add `identity_opaque_entry`.
        let extra = score + 2 * u64::from(costs.identity_opaque_entry);
        let ops_limit = u16::try_from(u64::from(u16::MAX).checked_sub(cells + 1)?).ok()?;
        Some(Self {
            cost: u32::try_from(u64::from(u32::MAX).checked_sub(extra + 1)?).ok()?,
            synchronous: u16::MAX - 2,
            substitutions: ops_limit,
            cell_mismatches: ops_limit,
        })
    }

    fn admits(&self, entry: &Entry) -> bool {
        entry.cost <= self.cost
            && entry.ops.synchronous <= self.synchronous
            && entry.ops.substitutions <= self.substitutions
            && entry.ops.cell_mismatches <= self.cell_mismatches
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
        Self::with_states(graph.states.len())
    }

    fn with_states(states: usize) -> Self {
        Self {
            slots: vec![
                Entry {
                    cost: 0,
                    ops: empty_ops(),
                    opaque_steps: 0,
                };
                states
            ],
            occupied: vec![0; states.div_ceil(64)],
            len: 0,
        }
    }

    /// Empties the frontier, retaining both allocations for reuse as
    /// scratch storage across observations.
    fn clear_reuse(&mut self) {
        self.occupied.fill(0);
        self.len = 0;
    }

    pub(super) fn len(&self) -> u32 {
        u32::try_from(self.len).unwrap_or(u32::MAX)
    }

    /// Occupied states in `StateId` order.
    fn states(&self) -> OccupiedStates<'_> {
        OccupiedStates {
            words: &self.occupied,
            word_index: 0,
            bits: self.occupied.first().copied().unwrap_or(0),
        }
    }

    #[cfg(test)]
    pub(super) fn get(&self, state: StateId) -> Option<&Entry> {
        let occupied = self.occupied.get(state.0 / 64)? & (1 << (state.0 % 64)) != 0;
        occupied.then(|| &self.slots[state.0])
    }

    /// Occupied states in `StateId` order with their entries.
    pub(super) fn entries(&self) -> impl Iterator<Item = (StateId, &Entry)> + '_ {
        self.states().map(|state| (state, &self.slots[state.0]))
    }

    pub(super) fn best_cost(&self) -> Option<u32> {
        self.entries().map(|(_, entry)| entry.cost).min()
    }

    pub(super) fn best_evidence(&self) -> Option<(EditOps, u16)> {
        self.entries()
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
        &mut self,
        graph: &AlignGraph<'_>,
        observation: &CanonicalKey,
        costs: &EditCosts,
        scratch: &mut ConsumeScratch,
    ) {
        #[cfg(debug_assertions)]
        let mut reference = self.clone();
        let limits = DeferLimits::new(graph, observation, costs);
        self.consume_with(graph, observation, costs, scratch, limits.as_ref());
        #[cfg(debug_assertions)]
        {
            let mut fresh = ConsumeScratch::sized_for(graph);
            reference.consume_with(graph, observation, costs, &mut fresh, None);
            debug_assert!(
                reference.len == self.len && reference.entries().eq(self.entries()),
                "deferred edit scores changed the consumed frontier"
            );
        }
    }

    fn consume_with(
        &mut self,
        graph: &AlignGraph<'_>,
        observation: &CanonicalKey,
        costs: &EditCosts,
        scratch: &mut ConsumeScratch,
        limits: Option<&DeferLimits>,
    ) {
        // Depth k is the frontier after k model-only advances (each closed
        // under epsilon). The consumed frontier is the pointwise minimum over
        // k ∈ {0, 1, 2} of skipping the observation (observed-only) or matching
        // it to a lock/bridge out of depth k. `offer` is a min-keep whose
        // exact ties are identical entries, so the result does not depend on
        // offer order: depths fold in as they are produced and no depth is
        // cloned.
        self.close_epsilon(graph, costs);
        scratch.acc.clear_reuse();
        scratch.matched.clear_reuse();
        scratch.edit.clear_for(graph.keys.len());
        for depth in 0..3 {
            scratch.add_depth_contributions(graph, self, observation, costs, limits);
            if depth == 2 {
                break;
            }
            scratch.next.clear_reuse();
            scratch.next.advance_model_only_from(graph, self, costs);
            scratch.next.close_epsilon(graph, costs);
            std::mem::swap(self, &mut scratch.next);
        }
        scratch.add_deferred_matches(graph, observation, costs);
        std::mem::swap(self, &mut scratch.acc);
    }

    pub(super) fn consume_missing_observation(
        &mut self,
        graph: &AlignGraph<'_>,
        costs: &EditCosts,
        scratch: &mut Self,
    ) {
        scratch.clear_reuse();
        for (state, entry) in self.entries() {
            scratch.offer(state, entry.clone());
            for edge in graph.match_edges(state) {
                if !edge.bridge {
                    scratch.offer(edge.to(), enter(entry, 0, empty_ops(), edge.enters_opaque, costs));
                }
            }
        }
        scratch.close_epsilon(graph, costs);
        std::mem::swap(self, scratch);
    }

    fn advance_model_only_from(
        &mut self,
        graph: &AlignGraph<'_>,
        source: &Self,
        costs: &EditCosts,
    ) {
        let mut ops = empty_ops();
        ops.model_only = 1;
        for (state, entry) in source.entries() {
            for edge in graph.match_edges(state) {
                if !edge.bridge {
                    self.offer(
                        edge.to(),
                        enter(entry, costs.model_only, ops, edge.enters_opaque, costs),
                    );
                }
            }
        }
    }

    pub(super) fn close_epsilon(&mut self, graph: &AlignGraph<'_>, costs: &EditCosts) {
        // Popping a state without epsilon edges changes nothing, so only
        // epsilon sources enter the worklist; the FIFO order of the rest is
        // unchanged.
        let mut pending = VecDeque::new();
        for (word_index, (&occupied, &sources)) in
            self.occupied.iter().zip(&graph.epsilon_sources).enumerate()
        {
            let mut bits = occupied & sources;
            while bits != 0 {
                pending.push_back(StateId(word_index * 64 + bits.trailing_zeros() as usize));
                bits &= bits - 1;
            }
        }
        while let Some(state) = pending.pop_front() {
            // Pending states are occupied: closure never removes an entry.
            let entry = self.slots[state.0].clone();
            for transition in &graph.graph.out[state.0] {
                if matches!(&transition.label, TransitionLabel::Epsilon { .. }) {
                    let next = enter_state(
                        graph.graph,
                        state,
                        transition.to,
                        &entry,
                        0,
                        empty_ops(),
                        costs,
                    );
                    if self.offer(transition.to, next) && graph.is_epsilon_source(transition.to) {
                        pending.push_back(transition.to);
                    }
                }
            }
        }
    }

    pub(super) fn truncate(&mut self, maximum: u32) -> Truncation {
        let maximum = usize::try_from(maximum).unwrap_or(usize::MAX);
        if self.len <= maximum {
            return Truncation::default();
        }
        // Keys are unique (the id breaks cost ties), so selecting the first
        // `retained` keeps exactly the prefix a full sort would keep.
        let mut keys = self
            .entries()
            .map(|(state, entry)| (entry.cost, state.0))
            .collect::<Vec<_>>();
        // Amended exactness contract: the zero-cost lane is always complete; the overall
        // optimum is exact only when `truncated_any` is false, otherwise flags disclose it.
        let zero_cost = keys.iter().filter(|(cost, _)| *cost == 0).count();
        let retained = maximum.max(zero_cost);
        if retained < keys.len() {
            keys.select_nth_unstable(retained);
            for &(_, state) in &keys[retained..] {
                self.occupied[state / 64] &= !(1 << (state % 64));
            }
        }
        let total = self.len;
        self.len = retained;
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
        let word = &mut self.occupied[state.0 / 64];
        let bit = 1 << (state.0 % 64);
        let slot = &mut self.slots[state.0];
        if *word & bit == 0 {
            *word |= bit;
            self.len += 1;
        } else if !entry_is_better(&entry, slot) {
            return false;
        }
        *slot = entry;
        true
    }
}

/// Set bits of an occupancy bitmap, lowest state first.
struct OccupiedStates<'a> {
    words: &'a [u64],
    word_index: usize,
    bits: u64,
}

impl Iterator for OccupiedStates<'_> {
    type Item = StateId;

    fn next(&mut self) -> Option<StateId> {
        while self.bits == 0 {
            self.word_index += 1;
            self.bits = *self.words.get(self.word_index)?;
        }
        let bit = self.bits.trailing_zeros() as usize;
        self.bits &= self.bits - 1;
        Some(StateId(self.word_index * 64 + bit))
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
    enter(entry, score, ops, entering_opaque, costs)
}

fn enter(
    entry: &Entry,
    score: u32,
    ops: EditOps,
    entering_opaque: bool,
    costs: &EditCosts,
) -> Entry {
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
    use std::collections::HashSet;

    fn frontier_with(entries: &[(usize, u32)]) -> Frontier {
        let mut frontier = Frontier::with_states(8);
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

    /// Fan-in fixture: 3 lock-edge visits but only 2 distinct targets, so
    /// a per-observation target cache can skip one `edit_cost` recompute.
    #[test]
    fn fan_in_walk_visits_more_edges_than_distinct_targets() {
        let graph = fan_in_graph();
        let source = frontier_with(&[(0, 0), (1, 0)]);
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
        let source = frontier_with(&[(0, 0), (1, 0)]);
        let indexed = AlignGraph::new(&graph);
        let observation = CanonicalKey {
            masks: vec![0b0000000001].into(),
            letters: None,
        };
        let costs = EditCosts::default();
        let limits = DeferLimits::new(&indexed, &observation, &costs);

        for limits in [None, limits.as_ref()] {
            let mut scratch = ConsumeScratch::sized_for(&indexed);
            scratch.add_depth_contributions(&indexed, &source, &observation, &costs, limits);
            scratch.add_deferred_matches(&indexed, &observation, &costs);

            assert_eq!(scratch.edit.computed(), 2);
            for target in [2, 3] {
                let (score, _) =
                    edit_cost(&observation, &graph.states[target].physical_key, &costs);
                assert_eq!(scratch.acc.get(StateId(target)).unwrap().cost, score);
            }
        }
    }

    #[test]
    fn scratch_reuse_across_observations_graphs_and_costs_stays_exact() {
        use crate::openers::recognition::cost::edit_cost;

        let graph = fan_in_graph();
        let indexed = AlignGraph::new(&graph);
        let costs = EditCosts::default();
        let mut edit = EditCostScratch::sized_for(&indexed);
        let first = CanonicalKey {
            masks: vec![0b0000000001].into(),
            letters: None,
        };
        let second = CanonicalKey {
            masks: vec![0b0000000100].into(),
            letters: None,
        };
        let target = indexed.key_of(StateId(2));

        edit.clear_for(indexed.keys.len());
        assert_eq!(
            edit.cost(&first, &indexed, target, &costs),
            edit_cost(&first, &graph.states[2].physical_key, &costs)
        );

        edit.clear_for(indexed.keys.len());
        assert_eq!(
            edit.cost(&second, &indexed, target, &costs),
            edit_cost(&second, &graph.states[2].physical_key, &costs)
        );

        let other_costs = EditCosts {
            cell_mismatch: 7,
            ..EditCosts::default()
        };
        edit.clear_for(indexed.keys.len());
        assert_eq!(
            edit.cost(&second, &indexed, target, &other_costs),
            edit_cost(&second, &graph.states[2].physical_key, &other_costs)
        );

        let bigger = fan_in_graph_wider();
        let bigger_indexed = AlignGraph::new(&bigger);
        edit.clear_for(bigger_indexed.keys.len());
        assert_eq!(
            edit.cost(
                &second,
                &bigger_indexed,
                bigger_indexed.key_of(StateId(4)),
                &costs
            ),
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
            let mut scratch = ConsumeScratch::sized_for(&AlignGraph::new(&graph));

            frontier.consume_observation(
                &AlignGraph::new(&graph),
                &first_observation(),
                &costs,
                &mut scratch,
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
            let mut fresh = ConsumeScratch::sized_for(&AlignGraph::new(&graph));
            let computed_before = scratch.edit.computed();
            frontier.consume_observation(
                &AlignGraph::new(&graph),
                &second_observation(),
                &costs,
                &mut scratch,
            );
            reference.consume_observation(
                &AlignGraph::new(&graph),
                &second_observation(),
                &costs,
                &mut fresh,
            );

            assert_eq!(
                snapshot(&frontier, graph.states.len()),
                snapshot(&reference, graph.states.len())
            );
            assert_eq!(
                scratch.edit.computed() - computed_before,
                fresh.edit.computed()
            );
        }

        /// A missing observation between consumes must not preserve the
        /// previous observation's cached costs either.
        #[test]
        fn missing_observation_between_consumes_still_recomputes() {
            let graph = invalidation_graph();
            let costs = EditCosts::default();
            let mut frontier = zero_frontier(&graph);
            let mut missing = Frontier::sized_for(&graph);
            let mut scratch = ConsumeScratch::sized_for(&AlignGraph::new(&graph));

            frontier.consume_observation(
                &AlignGraph::new(&graph),
                &first_observation(),
                &costs,
                &mut scratch,
            );
            frontier.consume_missing_observation(&AlignGraph::new(&graph), &costs, &mut missing);

            let mut reference = frontier.clone();
            let mut fresh = ConsumeScratch::sized_for(&AlignGraph::new(&graph));
            let computed_before = scratch.edit.computed();
            frontier.consume_observation(
                &AlignGraph::new(&graph),
                &second_observation(),
                &costs,
                &mut scratch,
            );
            reference.consume_observation(
                &AlignGraph::new(&graph),
                &second_observation(),
                &costs,
                &mut fresh,
            );

            assert_eq!(
                snapshot(&frontier, graph.states.len()),
                snapshot(&reference, graph.states.len())
            );
            assert_eq!(
                scratch.edit.computed() - computed_before,
                fresh.edit.computed()
            );
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
                exact_index: Default::default(),
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
            exact_index: Default::default(),
            epsilon_order: Box::new([]),
            census: CompileCensus::new(0, 0, 0, 0),
        }
    }
}
