use crate::board::Board;
use crate::move_buffer::MoveBuffer;
use crate::movegen::{generate_playable, move_reachable};
use crate::openers::catalog::OpenerTreeNode;

use super::flow::{initial_states, transitions, FlowState};
use super::frames::{board_matches, node_placements, TargetPlacement};
use super::model::{SearchReceipt, WitnessLock};
use super::witness::witness_lock;

pub(super) enum SearchOutcome {
    Found(Vec<WitnessLock>),
    Exhausted,
    Absent,
}

pub(super) struct ExactOrdinalSearch<'a> {
    nodes: Vec<&'a OpenerTreeNode>,
    mirrored: bool,
    target_ordinal: u32,
    target_pre_clear_rows: Option<Vec<String>>,
    target_construction_rows: Option<Vec<String>>,
    fuel_limit: u32,
    fuel_remaining: u32,
    explored_states: u32,
}

struct SearchFrame {
    board: Board,
    locks: Vec<WitnessLock>,
    used: Vec<bool>,
    flow: FlowState,
}

impl<'a> ExactOrdinalSearch<'a> {
    pub(super) fn new(
        nodes: Vec<&'a OpenerTreeNode>,
        mirrored: bool,
        target_ordinal: u32,
        target_pre_clear_rows: Option<Vec<String>>,
        target_construction_rows: Option<Vec<String>>,
        fuel_limit: u32,
    ) -> Self {
        Self {
            nodes,
            mirrored,
            target_ordinal,
            target_pre_clear_rows,
            target_construction_rows,
            fuel_limit,
            fuel_remaining: fuel_limit,
            explored_states: 0,
        }
    }

    pub(super) fn run(&mut self) -> SearchOutcome {
        for flow in initial_states() {
            match self.walk_node(
                0,
                SearchFrame {
                    board: Board::new(),
                    locks: Vec::new(),
                    used: Vec::new(),
                    flow,
                },
            ) {
                SearchOutcome::Found(locks) => return SearchOutcome::Found(locks),
                SearchOutcome::Exhausted => return SearchOutcome::Exhausted,
                SearchOutcome::Absent => {}
            }
        }
        SearchOutcome::Absent
    }

    pub(super) fn receipt(&self) -> SearchReceipt {
        SearchReceipt {
            explored_states: self.explored_states,
            fuel_limit: self.fuel_limit,
            fuel_remaining: self.fuel_remaining,
        }
    }

    fn walk_node(&mut self, node_index: usize, frame: SearchFrame) -> SearchOutcome {
        let Some(node) = self.nodes.get(node_index).copied() else {
            return if u32::try_from(frame.locks.len()).ok() == Some(self.target_ordinal) {
                SearchOutcome::Found(frame.locks)
            } else {
                SearchOutcome::Absent
            };
        };
        let parent = node_index
            .checked_sub(1)
            .and_then(|index| self.nodes.get(index).copied());
        let construction_rows = (node.pieces == self.target_ordinal)
            .then_some(self.target_construction_rows.as_deref())
            .flatten();
        let Some(placements) = node_placements(parent, node, self.mirrored, construction_rows)
        else {
            return SearchOutcome::Absent;
        };
        self.walk_placements(
            node_index,
            &placements,
            SearchFrame {
                used: vec![false; placements.len()],
                ..frame
            },
        )
    }

    fn walk_placements(
        &mut self,
        node_index: usize,
        placements: &[TargetPlacement],
        frame: SearchFrame,
    ) -> SearchOutcome {
        if frame.used.iter().all(|used| *used) {
            let node = self.nodes[node_index];
            return if board_matches(&frame.board, &node.rows, self.mirrored) {
                self.walk_node(node_index + 1, frame)
            } else {
                SearchOutcome::Absent
            };
        }
        for (placement_index, placement) in placements.iter().enumerate() {
            if frame.used[placement_index] {
                continue;
            }
            let needs_next = next_ordinal(&frame.locks) < self.target_ordinal;
            for flow_transition in transitions(&frame.flow, placement.piece, needs_next) {
                let mut moves = MoveBuffer::new();
                generate_playable(&frame.board, &mut moves, placement.piece, false);
                for target in moves
                    .iter()
                    .copied()
                    .filter(|target| placement_matches(*target, placement))
                {
                    if self.fuel_remaining == 0 {
                        return SearchOutcome::Exhausted;
                    }
                    self.fuel_remaining -= 1;
                    self.explored_states = self.explored_states.saturating_add(1);
                    if !move_reachable(&frame.board, &target, false) {
                        continue;
                    }
                    let ordinal = next_ordinal(&frame.locks);
                    let mut board_before_clear = frame.board.clone();
                    board_before_clear.place(&target);
                    if ordinal == self.target_ordinal
                        && self
                            .target_pre_clear_rows
                            .as_deref()
                            .is_some_and(|rows| !board_matches(&board_before_clear, rows, false))
                    {
                        continue;
                    }
                    let mut next_board = frame.board.clone();
                    next_board.lock(&target);
                    let Some(lock) = witness_lock(
                        ordinal,
                        target,
                        board_before_clear.rows,
                        next_board.rows,
                        &flow_transition,
                    ) else {
                        continue;
                    };
                    let mut next_locks = frame.locks.clone();
                    next_locks.push(lock);
                    let mut next_used = frame.used.clone();
                    next_used[placement_index] = true;
                    match self.walk_placements(
                        node_index,
                        placements,
                        SearchFrame {
                            board: next_board,
                            locks: next_locks,
                            used: next_used,
                            flow: flow_transition.next.clone(),
                        },
                    ) {
                        SearchOutcome::Found(locks) => return SearchOutcome::Found(locks),
                        SearchOutcome::Exhausted => return SearchOutcome::Exhausted,
                        SearchOutcome::Absent => {}
                    }
                }
            }
        }
        SearchOutcome::Absent
    }
}

fn placement_matches(target: crate::header::Move, placement: &TargetPlacement) -> bool {
    let Some(mut actual) = super::witness::move_cells(target) else {
        return false;
    };
    let mut expected = placement.cells;
    actual.sort_unstable();
    expected.sort_unstable();
    actual == expected
}

fn next_ordinal(locks: &[WitnessLock]) -> u32 {
    match u32::try_from(locks.len()) {
        Ok(length) => length.saturating_add(1),
        Err(_) => u32::MAX,
    }
}
