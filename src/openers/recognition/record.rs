use std::collections::{BTreeSet, HashMap};

#[cfg(test)]
use super::census::EdgeRef;
use super::compile::{CompileBudget, CompileError, CompileObserver, PlacementSpec};
use super::edge::{
    compile_bridge, compile_edge, compile_grey_terminal, BridgeInput, EdgeInput, GreyTerminal,
};
use super::graph::{
    physical_shadow_of_declared, BridgeReason, ControlKey, GraphBuilder, StateId, StateOrigin,
};
use super::legality::engine_board_from_masks;
use crate::board::Board;
use crate::openers::board::{mirror_mask_10, rows_to_masks_floor_up};
use crate::openers::catalog::{OpenerRecord, OpenerTreeNode};
use crate::openers::segments::{derive_placements, ShowcasePlacement};

pub(super) fn compile_record<O: CompileObserver>(
    builder: &mut GraphBuilder,
    observer: &mut O,
    record: &OpenerRecord,
    record_index: u32,
    mirrored: bool,
    budget: &CompileBudget,
) -> Result<(), CompileError> {
    let nodes = record
        .tree
        .iter()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    let mut completed = HashMap::<u32, NodeCompletion>::new();
    let mut pending = nodes.keys().copied().collect::<BTreeSet<_>>();

    while !pending.is_empty() {
        let mut progressed = false;
        for node_id in pending.iter().copied().collect::<Vec<_>>() {
            let Some(node) = nodes.get(&node_id).copied() else {
                continue;
            };
            if node.grey && node.parent.is_none() {
                builder.intern(
                    physical_shadow_of_declared(&board_from_declared_rows(node, mirrored)),
                    control(record_index, mirrored, node.id, 0),
                    origin(record, mirrored, node, 0, true),
                    true,
                );
                completed.insert(node.id, NodeCompletion::default());
                pending.remove(&node_id);
                progressed = true;
                continue;
            }
            let parent_states = match node.parent {
                Some(parent_id) => match completed.get(&parent_id) {
                    Some(states) if states.states.is_empty() => {
                        observer.blocked_descendant(record, node, mirrored);
                        completed.insert(node.id, NodeCompletion::default());
                        pending.remove(&node_id);
                        progressed = true;
                        continue;
                    }
                    Some(states) => states.states.clone(),
                    None => continue,
                },
                None => vec![root_state(builder, record, record_index, mirrored, node)],
            };
            if node.grey {
                let grey_board = board_from_declared_rows(node, mirrored);
                compile_grey_terminal(
                    builder,
                    observer,
                    GreyTerminal {
                        record,
                        record_index,
                        node,
                        mirrored,
                        parent_states: &parent_states,
                        grey_board: &grey_board,
                    },
                );
                completed.insert(node.id, NodeCompletion::default());
                pending.remove(&node_id);
                progressed = true;
                continue;
            }
            let parent = node.parent.and_then(|id| nodes.get(&id).copied());
            let parent_bridged = node.parent.is_some_and(|parent_id| {
                completed
                    .get(&parent_id)
                    .is_some_and(|states| states.bridged)
            });
            let bridge_exposed = node.parent.is_some_and(|parent_id| {
                completed
                    .get(&parent_id)
                    .is_some_and(|states| states.bridge_exposed)
            });
            let Some(placements) = placements_for(parent, node, mirrored) else {
                observer.direct_impossible(
                    record,
                    node,
                    mirrored,
                    "placements unavailable",
                    bridge_exposed,
                );
                let states = compile_bridge(
                    builder,
                    observer,
                    BridgeInput {
                        record,
                        record_index,
                        node,
                        mirrored,
                        parent_states: &parent_states,
                        reason: BridgeReason::DirectImpossible,
                    },
                );
                completed.insert(
                    node.id,
                    NodeCompletion {
                        states,
                        bridged: true,
                        bridge_exposed: true,
                    },
                );
                pending.remove(&node_id);
                progressed = true;
                continue;
            };
            if !placements.is_empty() {
                observer.lettered_edge(bridge_exposed);
            }
            let states = compile_edge(
                builder,
                observer,
                EdgeInput {
                    record,
                    record_index,
                    node,
                    parent,
                    mirrored,
                    parent_states: &parent_states,
                    placements: &placements,
                    budget,
                    bridge_exposed,
                },
            )?;
            let bridged = states.iter().all(|state| builder.is_bridged(*state));
            if parent_bridged && !bridged {
                observer
                    .bridge_rescued_descendants(u32::try_from(states.len()).unwrap_or(u32::MAX));
            }
            completed.insert(
                node.id,
                NodeCompletion {
                    states,
                    bridged,
                    bridge_exposed: bridge_exposed || bridged,
                },
            );
            pending.remove(&node_id);
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
    Ok(())
}

#[derive(Default)]
struct NodeCompletion {
    states: Vec<StateId>,
    bridged: bool,
    bridge_exposed: bool,
}

fn root_state(
    builder: &mut GraphBuilder,
    record: &OpenerRecord,
    record_index: u32,
    mirrored: bool,
    node: &OpenerTreeNode,
) -> StateId {
    builder.intern(
        Board::new(),
        control(record_index, mirrored, node.id, 0),
        origin(record, mirrored, node, 0, false),
        false,
    )
}

fn board_from_declared_rows(node: &OpenerTreeNode, mirrored: bool) -> Board {
    let mut masks = rows_to_masks_floor_up(&node.rows);
    if mirrored {
        masks = masks.into_iter().map(mirror_mask_10).collect();
    }
    engine_board_from_masks(&masks)
}

fn placements_for(
    parent: Option<&OpenerTreeNode>,
    node: &OpenerTreeNode,
    mirrored: bool,
) -> Option<Vec<PlacementSpec>> {
    let placements = match &node.placements {
        Some(placements) => placements
            .iter()
            .map(|placement| ShowcasePlacement {
                letter: placement.letter.clone(),
                cells: placement.cells.clone(),
            })
            .collect::<Vec<_>>(),
        None => {
            let parent_rows = parent.map_or(&[][..], |parent| parent.rows.as_slice());
            let child_rows = node.pre_clear_rows.as_deref().unwrap_or(&node.rows);
            derive_placements(parent_rows, child_rows)?.placements
        }
    };
    placements
        .iter()
        .map(|placement| placement_spec(placement, mirrored))
        .collect()
}

fn placement_spec(placement: &ShowcasePlacement, mirrored: bool) -> Option<PlacementSpec> {
    let [letter] = placement.letter.as_bytes() else {
        return None;
    };
    let cells: [[u8; 2]; 4] = placement.cells.clone().try_into().ok()?;
    Some(PlacementSpec {
        letter: if mirrored {
            mirror_letter(*letter)
        } else {
            *letter
        },
        cells: if mirrored {
            cells.map(|[x, y]| [9 - x, y])
        } else {
            cells
        },
    })
}

pub(super) fn control(
    record_index: u32,
    mirrored: bool,
    node_id: u32,
    placed_subset: u32,
) -> ControlKey {
    ControlKey {
        record_index,
        mirrored,
        node_id,
        placed_subset,
        bridge: false,
    }
}

pub(super) fn bridge_control(record_index: u32, mirrored: bool, node_id: u32) -> ControlKey {
    ControlKey {
        record_index,
        mirrored,
        node_id,
        placed_subset: 0,
        bridge: true,
    }
}

pub(super) fn origin(
    record: &OpenerRecord,
    mirrored: bool,
    node: &OpenerTreeNode,
    placed_subset: u32,
    bag_complete: bool,
) -> StateOrigin {
    StateOrigin {
        record: record.id.clone().into_boxed_str(),
        mirrored,
        node_id: node.id,
        placed_subset,
        route_name: node.route_name.clone().map(String::into_boxed_str),
        bag_complete,
    }
}

#[cfg(test)]
pub(super) fn edge_ref(
    record: &OpenerRecord,
    node: &OpenerTreeNode,
    mirrored: bool,
    reason: &str,
) -> EdgeRef {
    EdgeRef {
        record: record.id.clone(),
        node_id: node.id,
        mirrored,
        reason: reason.to_owned(),
    }
}

fn mirror_letter(letter: u8) -> u8 {
    match letter {
        b'L' => b'J',
        b'J' => b'L',
        b'S' => b'Z',
        b'Z' => b'S',
        other => other,
    }
}
