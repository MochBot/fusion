use crate::header::{Piece, ALL_PIECES};

use super::model::{piece_name, WitnessDraw};

const FULL_BAG: u8 = 0x7f;

#[derive(Clone, Eq, Hash, PartialEq)]
pub(super) struct FlowState {
    pub(super) current: Option<Piece>,
    pub(super) hold: Option<Piece>,
    remaining: u8,
    draw_count: u32,
    pending_draws: Vec<WitnessDraw>,
}

pub(super) struct FlowTransition {
    pub(super) next: FlowState,
    pub(super) current_before: Piece,
    pub(super) hold_before: Option<Piece>,
    pub(super) hold_used: bool,
    pub(super) hold_after: Option<Piece>,
    pub(super) draws_before_lock: Vec<WitnessDraw>,
}

pub(super) fn initial_states() -> Vec<FlowState> {
    draw_options(FULL_BAG, 0)
        .into_iter()
        .map(|draw| FlowState {
            current: Some(draw.piece),
            hold: None,
            remaining: draw.remaining,
            draw_count: 1,
            pending_draws: vec![draw.event],
        })
        .collect()
}

pub(super) fn transitions(
    state: &FlowState,
    played: Piece,
    needs_next: bool,
) -> Vec<FlowTransition> {
    let Some(current) = state.current else {
        return Vec::new();
    };
    let mut output = Vec::new();
    if current == played {
        output.extend(finish_lock(
            state,
            current,
            false,
            state.hold,
            state.remaining,
            state.draw_count,
            state.pending_draws.clone(),
            needs_next,
        ));
    }
    if state.hold == Some(played) {
        output.extend(finish_lock(
            state,
            current,
            true,
            Some(current),
            state.remaining,
            state.draw_count,
            state.pending_draws.clone(),
            needs_next,
        ));
    } else if state.hold.is_none() {
        for draw in draw_options(state.remaining, state.draw_count) {
            if draw.piece != played {
                continue;
            }
            let mut draws_before_lock = state.pending_draws.clone();
            draws_before_lock.push(draw.event);
            output.extend(finish_lock(
                state,
                current,
                true,
                Some(current),
                draw.remaining,
                state.draw_count.saturating_add(1),
                draws_before_lock,
                needs_next,
            ));
        }
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn finish_lock(
    state: &FlowState,
    current_before: Piece,
    hold_used: bool,
    hold_after: Option<Piece>,
    remaining: u8,
    draw_count: u32,
    draws_before_lock: Vec<WitnessDraw>,
    needs_next: bool,
) -> Vec<FlowTransition> {
    if !needs_next {
        return vec![FlowTransition {
            next: FlowState {
                current: None,
                hold: hold_after,
                remaining,
                draw_count,
                pending_draws: Vec::new(),
            },
            current_before,
            hold_before: state.hold,
            hold_used,
            hold_after,
            draws_before_lock,
        }];
    }
    draw_options(remaining, draw_count)
        .into_iter()
        .map(|draw| FlowTransition {
            next: FlowState {
                current: Some(draw.piece),
                hold: hold_after,
                remaining: draw.remaining,
                draw_count: draw_count.saturating_add(1),
                pending_draws: vec![draw.event],
            },
            current_before,
            hold_before: state.hold,
            hold_used,
            hold_after,
            draws_before_lock: draws_before_lock.clone(),
        })
        .collect()
}

struct DrawOption {
    piece: Piece,
    remaining: u8,
    event: WitnessDraw,
}

fn draw_options(remaining: u8, draw_count: u32) -> Vec<DrawOption> {
    let available = if remaining == 0 { FULL_BAG } else { remaining };
    ALL_PIECES
        .iter()
        .copied()
        .filter(|piece| available & piece_bit(*piece) != 0)
        .map(|piece| DrawOption {
            piece,
            remaining: available & !piece_bit(piece),
            event: WitnessDraw {
                draw_ordinal: draw_count.saturating_add(1),
                bag_ordinal: draw_count / 7 + 1,
                slot_in_bag: u8::try_from(draw_count % 7 + 1).unwrap_or(7),
                piece: piece_name(piece),
            },
        })
        .collect()
}

const fn piece_bit(piece: Piece) -> u8 {
    1 << piece as u8
}

#[cfg(test)]
mod tests {
    use super::{initial_states, transitions};
    use crate::header::Piece;

    #[test]
    fn duplicate_piece_at_lock_two_has_no_fresh_bag_hold_path() {
        let states = initial_states();
        let after_first = states
            .iter()
            .flat_map(|state| transitions(state, Piece::I, true))
            .collect::<Vec<_>>();
        assert!(after_first.iter().all(|transition| transitions(
            &transition.next,
            Piece::I,
            false
        )
        .is_empty()));
    }
}
