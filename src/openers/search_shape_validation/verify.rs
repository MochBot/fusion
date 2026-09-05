use crate::board::Board;
use crate::header::Move;
use crate::move_buffer::MoveBuffer;
use crate::movegen::{generate_playable, move_reachable};

use super::flow::{initial_states, transitions, FlowState};
use super::model::{piece_from_name, WitnessLock};

pub(super) fn verify_witness(locks: &[WitnessLock]) -> bool {
    let mut states = initial_states()
        .into_iter()
        .map(|flow| ReplayState {
            board: Board::new(),
            flow,
        })
        .collect::<Vec<_>>();
    for (index, lock) in locks.iter().enumerate() {
        if lock.ordinal != u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1) {
            return false;
        }
        let Some(piece) = piece_from_name(lock.piece) else {
            return false;
        };
        let needs_next = index + 1 < locks.len();
        let mut next_states = Vec::new();
        for state in states {
            for flow in transitions(&state.flow, piece, needs_next) {
                if !flow_matches(lock, &flow) {
                    continue;
                }
                let mut moves = MoveBuffer::new();
                generate_playable(&state.board, &mut moves, piece, false);
                for target in moves
                    .iter()
                    .copied()
                    .filter(|target| move_matches(lock, *target))
                {
                    if !move_reachable(&state.board, &target, false) {
                        continue;
                    }
                    let mut board_before_clear = state.board.clone();
                    board_before_clear.place(&target);
                    if super::witness::board_rows(board_before_clear.rows) != lock.rows_before_clear
                    {
                        continue;
                    }
                    let mut board = state.board.clone();
                    if !board.legal_lock_placement(&target) {
                        continue;
                    }
                    board.lock(&target);
                    if super::witness::board_rows(board.rows) != lock.rows_after {
                        continue;
                    }
                    next_states.push(ReplayState {
                        board,
                        flow: flow.next.clone(),
                    });
                }
            }
        }
        if next_states.is_empty() {
            return false;
        }
        states = next_states;
    }
    !states.is_empty()
}

struct ReplayState {
    board: Board,
    flow: FlowState,
}

fn flow_matches(lock: &WitnessLock, flow: &super::flow::FlowTransition) -> bool {
    lock.current_before == super::model::piece_name(flow.current_before)
        && lock.hold_before == flow.hold_before.map(super::model::piece_name)
        && lock.hold_used == flow.hold_used
        && lock.hold_after == flow.hold_after.map(super::model::piece_name)
        && lock.draws_before_lock.len() == flow.draws_before_lock.len()
        && lock
            .draws_before_lock
            .iter()
            .zip(&flow.draws_before_lock)
            .all(|(left, right)| {
                left.draw_ordinal == right.draw_ordinal
                    && left.bag_ordinal == right.bag_ordinal
                    && left.slot_in_bag == right.slot_in_bag
                    && left.piece == right.piece
            })
}

fn move_matches(lock: &WitnessLock, target: Move) -> bool {
    let Some(cells) = super::witness::move_cells(target) else {
        return false;
    };
    super::model::piece_name(target.piece()) == lock.piece
        && super::witness::rotation_name(target.rotation()) == lock.placement.rotation
        && target.x() == lock.placement.x
        && target.y() == lock.placement.y
        && super::witness::spin_name(target.spin()) == lock.placement.spin
        && cells == lock.placement.cells
}
