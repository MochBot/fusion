use std::collections::HashSet;

use crate::board::Board;
use crate::header::ALL_PIECES;
use crate::move_buffer::MoveBuffer;
use crate::movegen::{generate_playable, move_reachable};
use crate::openers::board::rows_to_masks_floor_up;

use super::flow::{initial_states, transitions, FlowState};
use super::frames::board_matches;
use super::model::{SearchReceipt, WitnessLock};
use super::witness::witness_lock;

pub(super) enum DirectTargetFrame {
    PostClear,
    PreClear,
}

pub(super) enum DirectTargetOutcome {
    Found {
        locks: Vec<WitnessLock>,
        frame: DirectTargetFrame,
    },
    Exhausted,
    Absent,
}

pub(super) struct DirectTargetSearch {
    target_rows: Vec<String>,
    target_masks: Vec<u16>,
    target_locks: u32,
    fuel_limit: u32,
    fuel_remaining: u32,
    explored_states: u32,
    visited: HashSet<SearchKey>,
}

#[derive(Eq, Hash, PartialEq)]
struct SearchKey {
    rows: [u16; 40],
    flows: Vec<FlowState>,
}

struct SearchFrame {
    board: Board,
    locks: u32,
    flows: Vec<FlowPath>,
}

struct FlowPath {
    state: FlowState,
    locks: Vec<WitnessLock>,
}

impl DirectTargetSearch {
    pub(super) fn new(target_rows: &[String], max_locks: u32, fuel_limit: u32) -> Self {
        let target_masks = rows_to_masks_floor_up(target_rows);
        let cells = target_masks.iter().map(|row| row.count_ones()).sum::<u32>();
        let target_locks = if cells % 4 == 0 && cells / 4 <= max_locks {
            cells / 4
        } else {
            0
        };
        Self {
            target_rows: target_rows.to_vec(),
            target_masks,
            target_locks,
            fuel_limit,
            fuel_remaining: fuel_limit,
            explored_states: 0,
            visited: HashSet::new(),
        }
    }

    pub(super) fn run(&mut self) -> DirectTargetOutcome {
        if self.target_locks == 0 {
            return DirectTargetOutcome::Absent;
        }
        match self.walk(SearchFrame {
            board: Board::new(),
            locks: 0,
            flows: initial_states()
                .into_iter()
                .map(|state| FlowPath {
                    state,
                    locks: Vec::new(),
                })
                .collect(),
        }) {
            DirectTargetOutcome::Found { locks, frame } => {
                DirectTargetOutcome::Found { locks, frame }
            }
            DirectTargetOutcome::Exhausted => DirectTargetOutcome::Exhausted,
            DirectTargetOutcome::Absent => DirectTargetOutcome::Absent,
        }
    }

    pub(super) fn receipt(&self) -> SearchReceipt {
        SearchReceipt {
            explored_states: self.explored_states,
            fuel_limit: self.fuel_limit,
            fuel_remaining: self.fuel_remaining,
        }
    }

    fn walk(&mut self, frame: SearchFrame) -> DirectTargetOutcome {
        let key = SearchKey {
            rows: frame.board.rows,
            flows: frame.flows.iter().map(|path| path.state.clone()).collect(),
        };
        if !self.visited.insert(key) {
            return DirectTargetOutcome::Absent;
        }
        let ordinal = frame.locks.saturating_add(1);
        if ordinal > self.target_locks {
            return DirectTargetOutcome::Absent;
        }
        let final_lock = ordinal == self.target_locks;
        for piece in ALL_PIECES {
            let mut playable = MoveBuffer::new();
            generate_playable(&frame.board, &mut playable, piece, false);
            for target in playable.iter().copied() {
                let mut board_before_clear = frame.board.clone();
                board_before_clear.place(&target);
                if !self.is_target_subset(&board_before_clear) {
                    continue;
                }
                if self.fuel_remaining == 0 {
                    return DirectTargetOutcome::Exhausted;
                }
                self.fuel_remaining -= 1;
                self.explored_states = self.explored_states.saturating_add(1);
                if !move_reachable(&frame.board, &target, false) {
                    continue;
                }
                let clears = board_before_clear.line_clears();
                if !final_lock && clears != 0 {
                    continue;
                }
                let mut next_board = frame.board.clone();
                next_board.lock(&target);
                let next_flows = advance_flows(
                    &frame.flows,
                    target,
                    ordinal,
                    &board_before_clear,
                    &next_board,
                    !final_lock,
                );
                if next_flows.is_empty() {
                    continue;
                }
                if final_lock {
                    let target_frame = if board_matches(&next_board, &self.target_rows, false) {
                        Some(DirectTargetFrame::PostClear)
                    } else if board_matches(&board_before_clear, &self.target_rows, false) {
                        Some(DirectTargetFrame::PreClear)
                    } else {
                        None
                    };
                    if let Some(target_frame) = target_frame {
                        return DirectTargetOutcome::Found {
                            locks: next_flows[0].locks.clone(),
                            frame: target_frame,
                        };
                    }
                    continue;
                }
                match self.walk(SearchFrame {
                    board: next_board,
                    locks: ordinal,
                    flows: next_flows,
                }) {
                    DirectTargetOutcome::Found { locks, frame } => {
                        return DirectTargetOutcome::Found { locks, frame };
                    }
                    DirectTargetOutcome::Exhausted => return DirectTargetOutcome::Exhausted,
                    DirectTargetOutcome::Absent => {}
                }
            }
        }
        DirectTargetOutcome::Absent
    }

    fn is_target_subset(&self, board: &Board) -> bool {
        board.rows.iter().enumerate().all(|(y, row)| {
            let target = self.target_masks.get(y).copied().unwrap_or(0);
            row & !target == 0
        })
    }
}

fn advance_flows(
    flows: &[FlowPath],
    target: crate::header::Move,
    ordinal: u32,
    board_before_clear: &Board,
    board_after: &Board,
    needs_next: bool,
) -> Vec<FlowPath> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for path in flows {
        for transition in transitions(&path.state, target.piece(), needs_next) {
            if !seen.insert(transition.next.clone()) {
                continue;
            }
            let Some(lock) = witness_lock(
                ordinal,
                target,
                board_before_clear.rows,
                board_after.rows,
                &transition,
            ) else {
                continue;
            };
            let mut locks = path.locks.clone();
            locks.push(lock);
            output.push(FlowPath {
                state: transition.next,
                locks,
            });
        }
    }
    output
}
