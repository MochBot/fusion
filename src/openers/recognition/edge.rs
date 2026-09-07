use std::collections::{HashMap, HashSet};

use super::compile::{CompileBudget, CompileError, CompileObserver, PlacementSpec};
use super::frames::{DeclaredFrame, FrameError};
#[cfg(test)]
use super::graph::{cleared_rows, EpsilonReason};
use super::graph::{
    physical_shadow_of_declared, BridgeReason, CanonicalKey, GraphBuilder, StateId, TransitionLabel,
};
use super::legality::LegalityVerdict;
use super::record::{bridge_control, control, origin};
use crate::openers::catalog::{OpenerRecord, OpenerTreeNode};

pub(super) fn compile_edge<O: CompileObserver>(
    builder: &mut GraphBuilder,
    observer: &mut O,
    input: EdgeInput<'_>,
) -> Result<Vec<StateId>, CompileError> {
    let EdgeInput {
        record,
        record_index,
        node,
        parent,
        mirrored,
        parent_states,
        placements,
        budget,
        bridge_exposed,
    } = input;
    let placement_count = match u8::try_from(placements.len()) {
        Ok(count) => count,
        Err(_) => {
            observer.budget_exceeded(
                record,
                node,
                mirrored,
                "placement count exceeds budget",
                bridge_exposed,
            );
            return Ok(compile_bridge(
                builder,
                observer,
                BridgeInput {
                    record,
                    record_index,
                    node,
                    mirrored,
                    parent_states,
                    reason: BridgeReason::BudgetExceeded,
                },
            ));
        }
    };
    if placement_count > budget.max_placements_per_edge {
        observer.budget_exceeded(
            record,
            node,
            mirrored,
            "placement count exceeds budget",
            bridge_exposed,
        );
        return Ok(compile_bridge(
            builder,
            observer,
            BridgeInput {
                record,
                record_index,
                node,
                mirrored,
                parent_states,
                reason: BridgeReason::BudgetExceeded,
            },
        ));
    }
    let declared_start = match DeclaredFrame::start(node, parent, placements, mirrored) {
        Ok(frame) => frame,
        Err(error) => {
            record_frame_inconsistency(observer, record, node, mirrored, error, bridge_exposed);
            return Ok(compile_bridge(
                builder,
                observer,
                BridgeInput {
                    record,
                    record_index,
                    node,
                    mirrored,
                    parent_states,
                    reason: BridgeReason::FrameInconsistent,
                },
            ));
        }
    };
    if placements.is_empty() {
        let completed = compile_epsilon_edge(
            builder,
            observer,
            EpsilonEdge {
                record,
                record_index,
                node,
                mirrored,
                parent_states,
                declared_start: &declared_start,
                bridge_exposed,
            },
        )?;
        return if completed.is_empty() {
            Ok(compile_bridge(
                builder,
                observer,
                BridgeInput {
                    record,
                    record_index,
                    node,
                    mirrored,
                    parent_states,
                    reason: BridgeReason::FrameInconsistent,
                },
            ))
        } else {
            Ok(completed)
        };
    }

    let complete_subset = complete_subset(placements.len());
    let wants_legal_orders = observer.wants_legal_orders();
    let mut completed = Vec::new();
    let mut legal_orders = 0u64;
    let mut support_observed = false;
    let mut visits = 0u32;
    let mut budget_reason = None;
    let mut frame_inconsistent = false;
    let mut frame_shift_rescued = false;

    for parent_state in parent_states {
        if let Err(error) = declared_start.validate_start_board(builder.board(*parent_state)) {
            record_frame_inconsistency(observer, record, node, mirrored, error, bridge_exposed);
            frame_inconsistent = true;
            continue;
        }
        let mut pending = vec![PendingState {
            state: *parent_state,
            subset: 0,
            declared: declared_start.clone(),
            shifted: false,
        }];
        let mut visited = HashSet::from([(*parent_state, 0u32)]);
        let mut edge_states = HashSet::new();
        let mut subsets = HashMap::new();
        let mut transitions = HashMap::<StateId, Vec<StateId>>::new();
        if wants_legal_orders {
            subsets.insert(*parent_state, 0u32);
        }
        let mut edge_completed = Vec::new();

        while let Some(current) = pending.pop() {
            visits = visits.saturating_add(1);
            if visits > budget.max_dfs_visits_per_edge {
                budget_reason = Some("DFS visits exceed budget");
                break;
            }
            for (placement_index, placement) in placements.iter().enumerate() {
                let bit = 1u32 << placement_index;
                if current.subset & bit != 0 {
                    continue;
                }
                let physical = match current.declared.physical_placement(placement) {
                    Ok(physical) => physical,
                    Err(error) => {
                        record_frame_inconsistency(
                            observer,
                            record,
                            node,
                            mirrored,
                            error,
                            bridge_exposed,
                        );
                        frame_inconsistent = true;
                        continue;
                    }
                };
                let legal =
                    builder.placement_legality(current.state, placement.letter, &physical.cells);
                observer.legality_attempt(legal.support_valid, &legal.verdict);
                support_observed |= legal.support_valid;
                let LegalityVerdict::Legal { target, mechanics } = legal.verdict else {
                    continue;
                };
                let mut next_declared = current.declared.clone();
                if let Err(error) = next_declared.merge(placement) {
                    record_frame_inconsistency(
                        observer,
                        record,
                        node,
                        mirrored,
                        error,
                        bridge_exposed,
                    );
                    frame_inconsistent = true;
                    continue;
                }
                let next_subset = current.subset | bit;
                let complete = next_subset == complete_subset;
                if complete {
                    if let Err(error) = next_declared.validate_endpoint(node, mirrored) {
                        record_frame_inconsistency(
                            observer,
                            record,
                            node,
                            mirrored,
                            error,
                            bridge_exposed,
                        );
                        frame_inconsistent = true;
                        continue;
                    }
                }
                let mut next_board = builder.board(current.state).clone();
                let locked = next_board.lock(&target);
                debug_assert_eq!(locked, mechanics);
                if next_board.rows != next_declared.physical_board().rows {
                    record_frame_inconsistency(
                        observer,
                        record,
                        node,
                        mirrored,
                        FrameError::PhysicalStartMismatch,
                        bridge_exposed,
                    );
                    frame_inconsistent = true;
                    continue;
                }
                let physical_letters = next_declared.physical_letters();
                let physical_key =
                    CanonicalKey::from_board_with_letters(&next_board, Some(&physical_letters));
                let to = builder.intern_with_physical_key(
                    next_board,
                    physical_key,
                    control(record_index, mirrored, node.id, next_subset),
                    origin(record, mirrored, node, next_subset, complete),
                    node.grey,
                );
                edge_states.insert(to);
                if u32::try_from(edge_states.len()).unwrap_or(u32::MAX) > budget.max_states_per_edge
                {
                    budget_reason = Some("interned states exceed budget");
                    break;
                }
                builder.add_transition(
                    current.state,
                    to,
                    TransitionLabel::Lock {
                        #[cfg(test)]
                        letter: placement.letter,
                        #[cfg(test)]
                        cells: physical.cells,
                        #[cfg(test)]
                        cleared_rows: cleared_rows(mechanics),
                        #[cfg(test)]
                        is_pc: mechanics.is_pc,
                    },
                );
                if wants_legal_orders {
                    transitions.entry(current.state).or_default().push(to);
                    subsets.insert(to, next_subset);
                }
                if complete {
                    if !edge_completed.contains(&to) {
                        edge_completed.push(to);
                    }
                    frame_shift_rescued |= current.shifted || physical.shifted;
                } else if visited.insert((to, next_subset)) {
                    pending.push(PendingState {
                        state: to,
                        subset: next_subset,
                        declared: next_declared,
                        shifted: current.shifted || physical.shifted,
                    });
                }
                let states = u32::try_from(builder.state_count()).unwrap_or(u32::MAX);
                if states > budget.max_total_states {
                    return Err(CompileError::TotalStateBudgetExceeded { states });
                }
            }
            if budget_reason.is_some() {
                break;
            }
        }
        observer.edge_state_count(u32::try_from(edge_states.len()).unwrap_or(u32::MAX));
        if budget_reason.is_some() {
            break;
        }
        if wants_legal_orders {
            legal_orders = legal_orders.saturating_add(count_paths(
                *parent_state,
                &subsets,
                &transitions,
                &edge_completed,
            ));
        }
        for state in edge_completed {
            if !completed.contains(&state) {
                completed.push(state);
            }
        }
    }
    observer.dfs_visits(visits);
    if let Some(reason) = budget_reason {
        observer.budget_exceeded(record, node, mirrored, reason, bridge_exposed);
        return Ok(compile_bridge(
            builder,
            observer,
            BridgeInput {
                record,
                record_index,
                node,
                mirrored,
                parent_states,
                reason: BridgeReason::BudgetExceeded,
            },
        ));
    }
    if wants_legal_orders {
        observer.legal_orders(legal_orders);
    }
    if support_observed {
        observer.support_observed();
    }
    if completed.is_empty() {
        if support_observed {
            observer.support_without_exact_srs();
        }
        if !frame_inconsistent {
            observer.direct_impossible(
                record,
                node,
                mirrored,
                "no SRS-valid order",
                bridge_exposed,
            );
        }
        return Ok(compile_bridge(
            builder,
            observer,
            BridgeInput {
                record,
                record_index,
                node,
                mirrored,
                parent_states,
                reason: if frame_inconsistent {
                    BridgeReason::FrameInconsistent
                } else {
                    BridgeReason::DirectImpossible
                },
            },
        ));
    } else {
        observer.srs_valid(record, node, mirrored);
        if frame_shift_rescued {
            observer.shifted_compiled(record, node, mirrored);
        }
        if placements.len() > 8 {
            observer.dfs_compiled_large(record, node, mirrored);
        }
    }
    Ok(completed)
}

pub(super) fn compile_grey_terminal<O: CompileObserver>(
    builder: &mut GraphBuilder,
    observer: &mut O,
    input: GreyTerminal<'_>,
) {
    let GreyTerminal {
        record,
        record_index,
        node,
        mirrored,
        parent_states,
        grey_board,
    } = input;
    let physical_shadow = physical_shadow_of_declared(grey_board);
    for from in parent_states {
        let to = builder.intern(
            physical_shadow.clone(),
            control(record_index, mirrored, node.id, 0),
            origin(record, mirrored, node, 0, true),
            true,
        );
        builder.add_transition(
            *from,
            to,
            TransitionLabel::Epsilon {
                #[cfg(test)]
                reason: EpsilonReason::BagBoundary,
            },
        );
        observer.epsilon_transition();
    }
}

pub(super) fn compile_bridge<O: CompileObserver>(
    builder: &mut GraphBuilder,
    observer: &mut O,
    input: BridgeInput<'_>,
) -> Vec<StateId> {
    let BridgeInput {
        record,
        record_index,
        node,
        mirrored,
        parent_states,
        reason,
    } = input;
    let declared = DeclaredFrame::for_node(node, mirrored);
    let physical_board = declared.physical_board();
    let physical_letters = declared.physical_letters();
    let physical_key =
        CanonicalKey::from_board_with_letters(&physical_board, Some(&physical_letters));
    let mut completed = Vec::new();
    for from in parent_states {
        let to = builder.intern_with_physical_key(
            physical_board.clone(),
            physical_key.clone(),
            bridge_control(record_index, mirrored, node.id),
            origin(record, mirrored, node, 0, true),
            false,
        );
        builder.mark_bridged(to);
        builder.add_transition(
            *from,
            to,
            TransitionLabel::Bridge {
                #[cfg(test)]
                reason,
            },
        );
        if !completed.contains(&to) {
            completed.push(to);
        }
    }
    observer.bridged_edge(record, node, mirrored, reason);
    completed
}

fn compile_epsilon_edge<O: CompileObserver>(
    builder: &mut GraphBuilder,
    observer: &mut O,
    input: EpsilonEdge<'_>,
) -> Result<Vec<StateId>, CompileError> {
    let EpsilonEdge {
        record,
        record_index,
        node,
        mirrored,
        parent_states,
        declared_start,
        bridge_exposed,
    } = input;
    let mut completed = Vec::new();
    for from in parent_states {
        if let Err(error) = declared_start.validate_start_board(builder.board(*from)) {
            record_frame_inconsistency(observer, record, node, mirrored, error, bridge_exposed);
            continue;
        }
        if let Err(error) = declared_start.validate_endpoint(node, mirrored) {
            record_frame_inconsistency(observer, record, node, mirrored, error, bridge_exposed);
            continue;
        }
        if node.parent.is_none() {
            observer.srs_valid(record, node, mirrored);
            if !completed.contains(from) {
                completed.push(*from);
            }
            continue;
        }
        let physical_letters = declared_start.physical_letters();
        let to = builder.intern_with_physical_key(
            builder.board(*from).clone(),
            CanonicalKey::from_board_with_letters(builder.board(*from), Some(&physical_letters)),
            control(record_index, mirrored, node.id, 0),
            origin(record, mirrored, node, 0, true),
            node.grey,
        );
        builder.add_transition(
            *from,
            to,
            TransitionLabel::Epsilon {
                #[cfg(test)]
                reason: EpsilonReason::BagBoundary,
            },
        );
        observer.epsilon_transition();
        observer.srs_valid(record, node, mirrored);
        if !completed.contains(&to) {
            completed.push(to);
        }
    }
    Ok(completed)
}

struct PendingState {
    state: StateId,
    subset: u32,
    declared: DeclaredFrame,
    shifted: bool,
}

pub(super) struct EdgeInput<'a> {
    pub(super) record: &'a OpenerRecord,
    pub(super) record_index: u32,
    pub(super) node: &'a OpenerTreeNode,
    pub(super) parent: Option<&'a OpenerTreeNode>,
    pub(super) mirrored: bool,
    pub(super) parent_states: &'a [StateId],
    pub(super) placements: &'a [PlacementSpec],
    pub(super) budget: &'a CompileBudget,
    pub(super) bridge_exposed: bool,
}

pub(super) struct GreyTerminal<'a> {
    pub(super) record: &'a OpenerRecord,
    pub(super) record_index: u32,
    pub(super) node: &'a OpenerTreeNode,
    pub(super) mirrored: bool,
    pub(super) parent_states: &'a [StateId],
    pub(super) grey_board: &'a crate::board::Board,
}

pub(super) struct BridgeInput<'a> {
    pub(super) record: &'a OpenerRecord,
    pub(super) record_index: u32,
    pub(super) node: &'a OpenerTreeNode,
    pub(super) mirrored: bool,
    pub(super) parent_states: &'a [StateId],
    pub(super) reason: BridgeReason,
}

struct EpsilonEdge<'a> {
    record: &'a OpenerRecord,
    record_index: u32,
    node: &'a OpenerTreeNode,
    mirrored: bool,
    parent_states: &'a [StateId],
    declared_start: &'a DeclaredFrame,
    bridge_exposed: bool,
}

fn complete_subset(placement_count: usize) -> u32 {
    if placement_count == u32::BITS as usize {
        u32::MAX
    } else {
        (1u32 << placement_count) - 1
    }
}

fn count_paths(
    root: StateId,
    subsets: &HashMap<StateId, u32>,
    transitions: &HashMap<StateId, Vec<StateId>>,
    completed: &[StateId],
) -> u64 {
    let mut states = subsets
        .iter()
        .map(|(state, subset)| (*state, *subset))
        .collect::<Vec<_>>();
    states.sort_unstable_by_key(|(_, subset)| subset.count_ones());
    let mut ways = HashMap::from([(root, 1u64)]);
    for (state, _) in states {
        let from_ways = ways.get(&state).copied().unwrap_or(0);
        for to in transitions.get(&state).into_iter().flatten() {
            let entry = ways.entry(*to).or_default();
            *entry = entry.saturating_add(from_ways);
        }
    }
    completed.iter().fold(0u64, |total, state| {
        total.saturating_add(ways.get(state).copied().unwrap_or(0))
    })
}

fn record_frame_inconsistency<O: CompileObserver>(
    observer: &mut O,
    record: &OpenerRecord,
    node: &OpenerTreeNode,
    mirrored: bool,
    error: FrameError,
    bridge_exposed: bool,
) {
    observer.frame_inconsistent(record, node, mirrored, error.reason(), bridge_exposed);
}
