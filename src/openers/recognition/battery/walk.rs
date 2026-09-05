use std::collections::HashSet;

use crate::openers::recognition::align::Observation;
use crate::openers::recognition::graph::{RecognitionGraph, StateId, TransitionLabel};

pub(super) const MAX_SYNTHESIS_VISITS_PER_RECORD: u32 = 50_000;
pub(super) const MAX_WALK_LOCKS: usize = 16;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SynthesisStats {
    pub(super) visits: u32,
    pub(super) fuel_exhausted: bool,
}

pub(super) struct SynthesisFuel {
    remaining: u32,
    stats: SynthesisStats,
}

impl SynthesisFuel {
    pub(super) fn new(limit: u32) -> Self {
        Self {
            remaining: limit,
            stats: SynthesisStats::default(),
        }
    }

    pub(super) fn stats(&self) -> SynthesisStats {
        self.stats
    }

    fn visit(&mut self) -> bool {
        if self.remaining == 0 {
            self.stats.fuel_exhausted = true;
            return false;
        }
        self.remaining -= 1;
        self.stats.visits += 1;
        true
    }
}

pub(super) struct SynthesizedRecord {
    pub(super) observations: Vec<Option<Observation>>,
    pub(super) transpositions: Vec<(Vec<Option<Observation>>, Vec<Option<Observation>>)>,
    pub(super) original_walk_locks: u32,
    pub(super) capped_walk_locks: u32,
}

#[derive(Clone, Copy)]
struct WalkTransition {
    to: StateId,
    lock: bool,
}

struct LongestFrame {
    state: StateId,
    transitions: Vec<WalkTransition>,
    next: usize,
    candidates: Vec<Vec<StateId>>,
    entered: bool,
}

impl LongestFrame {
    fn new(state: StateId) -> Self {
        Self {
            state,
            transitions: Vec::new(),
            next: 0,
            candidates: Vec::new(),
            entered: false,
        }
    }
}

pub(super) fn synthesize_observations(
    graph: &RecognitionGraph,
    record: &str,
    mirrored: bool,
) -> Result<Vec<Option<Observation>>, String> {
    let mut fuel = SynthesisFuel::new(MAX_SYNTHESIS_VISITS_PER_RECORD);
    let synthesis = synthesize_record(graph, record, mirrored, 0, &mut fuel)
        .ok_or_else(|| synthesis_error(record, mirrored, fuel.stats()))?;
    Ok(synthesis.observations)
}

pub(super) fn synthesize_record(
    graph: &RecognitionGraph,
    record: &str,
    mirrored: bool,
    maximum_transposition_pairs: usize,
    fuel: &mut SynthesisFuel,
) -> Option<SynthesizedRecord> {
    let states = synthesize_walk(graph, record, mirrored, fuel)?;
    let original_walk_locks = u32::try_from(states.len()).unwrap_or(u32::MAX);
    let states = states.into_iter().take(MAX_WALK_LOCKS).collect::<Vec<_>>();
    let capped_walk_locks = u32::try_from(states.len()).unwrap_or(u32::MAX);
    let transpositions =
        transposition_pairs(graph, record, mirrored, maximum_transposition_pairs, fuel)?
            .into_iter()
            .map(|(first, second)| (cap_walk(first), cap_walk(second)))
            .collect();
    Some(SynthesizedRecord {
        observations: observations(graph, &states),
        transpositions,
        original_walk_locks,
        capped_walk_locks,
    })
}

fn synthesize_walk(
    graph: &RecognitionGraph,
    record: &str,
    mirrored: bool,
    fuel: &mut SynthesisFuel,
) -> Option<Vec<StateId>> {
    let root = root_state(graph, record, mirrored)?;
    let path = longest_suffix(graph, root, record, mirrored, fuel)?;
    (!path.is_empty()).then_some(path)
}

fn root_state(graph: &RecognitionGraph, record: &str, mirrored: bool) -> Option<StateId> {
    graph
        .states
        .iter()
        .enumerate()
        .find(|(_, state)| {
            state.origins.iter().any(|origin| {
                origin.record.as_ref() == record
                    && origin.mirrored == mirrored
                    && origin.placed_subset == 0
                    && !origin.bag_complete
            })
        })
        .map(|(index, _)| StateId(index))
}

fn transposition_pairs(
    graph: &RecognitionGraph,
    record: &str,
    mirrored: bool,
    maximum: usize,
    fuel: &mut SynthesisFuel,
) -> Option<Vec<(Vec<Option<Observation>>, Vec<Option<Observation>>)>> {
    let mut incoming = vec![Vec::<StateId>::new(); graph.states.len()];
    for (index, transitions) in graph.out.iter().enumerate() {
        for transition in transitions {
            if matches!(transition.label, TransitionLabel::Lock { .. }) {
                incoming[transition.to.0].push(StateId(index));
            }
        }
    }
    let mut pairs = Vec::new();
    for (index, state) in graph.states.iter().enumerate() {
        if pairs.len() == maximum {
            break;
        }
        let endpoint = StateId(index);
        if !state.origins.iter().any(|origin| {
            origin.record.as_ref() == record && origin.mirrored == mirrored && origin.bag_complete
        }) {
            continue;
        }
        let sources = incoming[index]
            .iter()
            .copied()
            .filter(|source| state_matches(graph, *source, record, mirrored))
            .collect::<Vec<_>>();
        if sources.len() < 2 {
            continue;
        }
        let Some(mut first) = find_path(graph, record, mirrored, sources[0], fuel) else {
            if fuel.stats().fuel_exhausted {
                return None;
            }
            continue;
        };
        let Some(mut second) = find_path(graph, record, mirrored, sources[1], fuel) else {
            if fuel.stats().fuel_exhausted {
                return None;
            }
            continue;
        };
        first.push(endpoint);
        second.push(endpoint);
        if first != second {
            pairs.push((observations(graph, &first), observations(graph, &second)));
        }
    }
    Some(pairs)
}

fn find_path(
    graph: &RecognitionGraph,
    record: &str,
    mirrored: bool,
    target: StateId,
    fuel: &mut SynthesisFuel,
) -> Option<Vec<StateId>> {
    let root = root_state(graph, record, mirrored)?;
    let mut visited = HashSet::new();
    let mut previous = vec![None::<(StateId, bool)>; graph.states.len()];
    let mut worklist = vec![root];
    visited.insert(root);

    while let Some(state) = worklist.pop() {
        if !fuel.visit() {
            return None;
        }
        if state == target {
            let mut path = Vec::new();
            let mut current = target;
            while current != root {
                let (parent, lock) = previous[current.0]?;
                if lock {
                    path.push(current);
                }
                current = parent;
            }
            path.reverse();
            return Some(path);
        }

        let mut transitions = graph.out[state.0]
            .iter()
            .filter(|transition| !matches!(transition.label, TransitionLabel::Bridge { .. }))
            .filter(|transition| state_matches(graph, transition.to, record, mirrored))
            .map(|transition| WalkTransition {
                to: transition.to,
                lock: matches!(transition.label, TransitionLabel::Lock { .. }),
            })
            .collect::<Vec<_>>();
        transitions.sort_by(|left, right| right.to.0.cmp(&left.to.0));
        for transition in transitions {
            if visited.insert(transition.to) {
                previous[transition.to.0] = Some((state, transition.lock));
                worklist.push(transition.to);
            }
        }
    }
    None
}

fn longest_suffix(
    graph: &RecognitionGraph,
    root: StateId,
    record: &str,
    mirrored: bool,
    fuel: &mut SynthesisFuel,
) -> Option<Vec<StateId>> {
    let mut memo = vec![None::<Vec<StateId>>; graph.states.len()];
    let mut visiting = HashSet::new();
    let mut stack = vec![LongestFrame::new(root)];

    while let Some(state) = stack.last().map(|frame| frame.state) {
        if !stack.last().is_some_and(|frame| frame.entered) {
            if let Some(path) = memo[state.0].clone() {
                stack.pop();
                if let Some(parent) = stack.last_mut() {
                    let transition = parent.transitions[parent.next - 1];
                    let mut candidate = path;
                    if transition.lock {
                        candidate.insert(0, transition.to);
                    }
                    parent.candidates.push(candidate);
                    continue;
                }
                return Some(path);
            }
            if !visiting.insert(state) {
                stack.pop();
                if let Some(parent) = stack.last_mut() {
                    let transition = parent.transitions[parent.next - 1];
                    let mut candidate = Vec::new();
                    if transition.lock {
                        candidate.push(transition.to);
                    }
                    parent.candidates.push(candidate);
                    continue;
                }
                return Some(Vec::new());
            }
            if !fuel.visit() {
                return None;
            }
            let transitions = graph.out[state.0]
                .iter()
                .filter(|transition| !matches!(transition.label, TransitionLabel::Bridge { .. }))
                .filter(|transition| state_matches(graph, transition.to, record, mirrored))
                .map(|transition| WalkTransition {
                    to: transition.to,
                    lock: matches!(transition.label, TransitionLabel::Lock { .. }),
                })
                .collect();
            if let Some(frame) = stack.last_mut() {
                frame.transitions = transitions;
                frame.entered = true;
            }
            continue;
        }

        let next = stack.last().and_then(|frame| {
            (frame.next < frame.transitions.len()).then(|| frame.transitions[frame.next])
        });
        if let Some(next) = next {
            if let Some(frame) = stack.last_mut() {
                frame.next += 1;
            }
            stack.push(LongestFrame::new(next.to));
            continue;
        }

        let Some(mut frame) = stack.pop() else {
            return Some(Vec::new());
        };
        visiting.remove(&frame.state);
        frame.candidates.sort_by(|left, right| {
            right.len().cmp(&left.len()).then_with(|| {
                left.iter()
                    .map(|state| state.0)
                    .cmp(right.iter().map(|state| state.0))
            })
        });
        let path = frame.candidates.into_iter().next().unwrap_or_default();
        memo[frame.state.0] = Some(path.clone());
        if let Some(parent) = stack.last_mut() {
            let transition = parent.transitions[parent.next - 1];
            let mut candidate = path;
            if transition.lock {
                candidate.insert(0, transition.to);
            }
            parent.candidates.push(candidate);
        } else {
            return Some(path);
        }
    }
    Some(Vec::new())
}

fn observations(graph: &RecognitionGraph, states: &[StateId]) -> Vec<Option<Observation>> {
    states
        .iter()
        .map(|state| {
            Some(Observation {
                key: graph.states[state.0].physical_key.clone(),
                had_garbage: false,
            })
        })
        .collect()
}

fn cap_walk(mut observations: Vec<Option<Observation>>) -> Vec<Option<Observation>> {
    observations.truncate(MAX_WALK_LOCKS);
    observations
}

fn synthesis_error(record: &str, mirrored: bool, stats: SynthesisStats) -> String {
    if stats.fuel_exhausted {
        format!("synthesis fuel exhausted for {record} mirrored={mirrored}")
    } else {
        format!("no compiled lock walk for {record} mirrored={mirrored}")
    }
}

pub(super) fn add_lowest_column_zero_cell(observation: &Observation) -> Observation {
    let mut key = observation.key.clone();
    let row = key
        .masks
        .iter()
        .position(|mask| mask & 1 == 0)
        .unwrap_or(key.masks.len());
    let mut masks = key.masks.into_vec();
    if row == masks.len() {
        masks.push(0);
    }
    masks[row] |= 1;
    key.masks = masks.into_boxed_slice();
    Observation {
        key,
        had_garbage: false,
    }
}

pub(super) fn substitute_occupied_letter(observation: &Observation) -> Option<Observation> {
    let mut key = observation.key.clone();
    let row_width = 10;
    let mut letters = vec![b'_'; key.masks.len() * row_width];
    let (row, column) = key.masks.iter().enumerate().find_map(|(row, mask)| {
        (0..row_width)
            .find(|column| mask & (1 << column) != 0)
            .map(|column| (row, column))
    })?;
    for (row_index, mask) in key.masks.iter().copied().enumerate() {
        for column_index in 0..row_width {
            if mask & (1 << column_index) != 0 {
                letters[row_index * row_width + column_index] = b'I';
            }
        }
    }
    letters[row * row_width + column] = b'O';
    key.letters = Some(letters.into_boxed_slice());
    Some(Observation {
        key,
        had_garbage: false,
    })
}

fn state_matches(graph: &RecognitionGraph, state: StateId, record: &str, mirrored: bool) -> bool {
    graph.states[state.0]
        .origins
        .iter()
        .any(|origin| origin.record.as_ref() == record && origin.mirrored == mirrored)
}

#[cfg(test)]
mod tests {
    use super::{
        cap_walk, longest_suffix, SynthesisFuel, MAX_SYNTHESIS_VISITS_PER_RECORD, MAX_WALK_LOCKS,
    };
    use crate::board::Board;
    use crate::openers::recognition::graph::{
        BridgeReason, ControlKey, GraphBuilder, StateOrigin, TransitionLabel,
    };

    #[test]
    fn cap_walk_retains_the_opener_phase_prefix() {
        let observations = vec![None; MAX_WALK_LOCKS + 4];

        let capped = cap_walk(observations);

        assert_eq!(capped.len(), MAX_WALK_LOCKS);
    }

    #[test]
    fn bridge_free_synthesis_retains_pre_and_post_bridge_legal_segments() {
        let mut builder = GraphBuilder::new();
        let state = |builder: &mut GraphBuilder, node_id| {
            builder.intern(
                Board::new(),
                ControlKey {
                    record_index: 0,
                    mirrored: false,
                    node_id,
                    placed_subset: 0,
                    bridge: false,
                },
                StateOrigin {
                    record: "bridge-fixture".into(),
                    mirrored: false,
                    node_id,
                    placed_subset: 0,
                    route_name: None,
                    bag_complete: true,
                },
                false,
            )
        };
        let root = state(&mut builder, 0);
        let prefix = state(&mut builder, 1);
        let bridge = state(&mut builder, 2);
        let suffix = state(&mut builder, 3);
        builder.add_transition(
            root,
            prefix,
            TransitionLabel::Lock {
                letter: b'O',
                cells: [[0, 0]; 4],
                cleared_rows: Box::new([]),
                is_pc: false,
            },
        );
        builder.add_transition(
            prefix,
            bridge,
            TransitionLabel::Bridge {
                #[cfg(test)]
                reason: BridgeReason::DirectImpossible,
            },
        );
        builder.add_transition(
            bridge,
            suffix,
            TransitionLabel::Lock {
                letter: b'O',
                cells: [[0, 0]; 4],
                cleared_rows: Box::new([]),
                is_pc: false,
            },
        );
        let graph = builder.finish();
        let mut fuel = SynthesisFuel::new(MAX_SYNTHESIS_VISITS_PER_RECORD);

        let pre_bridge = longest_suffix(&graph, root, "bridge-fixture", false, &mut fuel)
            .expect("prefix should synthesize");
        let post_bridge = longest_suffix(&graph, bridge, "bridge-fixture", false, &mut fuel)
            .expect("suffix should synthesize");

        assert_eq!(pre_bridge, [prefix]);
        assert_eq!(post_bridge, [suffix]);
    }
}
