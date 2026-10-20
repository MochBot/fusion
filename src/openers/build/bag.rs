//! The default piece order: a 7-bag queue from a fresh bag that builds the
//! target, performing its clears, with the fewest holds, then the fewest key
//! presses. Pieces go down stage by stage, so the player passes every
//! catalogued board on the way.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::board::BOARD_HEIGHT;
use crate::header::Piece;
use crate::pathfinder::LockPresses;

use super::frame::{board_of, fill, Masks, PIECES};
use super::replay::key_count;
use super::search::Search;

/// One hold outweighs any number of key presses a build can need.
const HOLD_WEIGHT: u32 = 10_000;
const ALL_PIECES: u8 = 0b111_1111;

/// A default order: the queue, whole bags, and the holds it uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BagOrder {
    pub queue: Vec<Piece>,
    pub holds: u32,
    pub keys: u32,
}

impl BagOrder {
    pub(super) fn cost(&self) -> (u32, u32) {
        (self.holds, self.keys)
    }
}

/// Filled cells, pieces still in the current bag, and the held piece.
type State = (Masks, u8, Option<Piece>);

struct Entry {
    state: State,
    parent: Option<usize>,
    /// Pieces drawn by the transition into this state.
    drawn: Piece,
}

/// The cheapest order within `budget` expansions, which it draws down; None
/// when none exists or the budget runs out first.
pub(super) fn default_order(
    search: &mut Search<'_>,
    hold_enabled: bool,
    budget: &mut u32,
) -> Option<BagOrder> {
    let target = search.target();
    let start: State = (target.start, ALL_PIECES, None);
    let mut entries = vec![Entry {
        state: start,
        parent: None,
        drawn: Piece::I,
    }];
    let mut best = HashMap::<State, u32>::from([(start, 0)]);
    let mut frontier =
        BinaryHeap::from([Reverse((pieces_left(&target.goal, &start.0), 0u32, 0usize))]);
    // Each board and piece's key counts, from one search for all its locks.
    let mut keys = HashMap::<([u16; BOARD_HEIGHT], Piece), LockPresses>::new();
    while let Some(Reverse((_, cost, index))) = frontier.pop() {
        let (filled, left, hold) = entries[index].state;
        if best
            .get(&(filled, left, hold))
            .is_some_and(|known| *known < cost)
        {
            continue;
        }
        if filled == target.goal {
            return Some(order_at(&entries, index, cost));
        }
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let open = target.open_stage(&filled);
        let bag = if left == 0 { ALL_PIECES } else { left };
        let board = board_of(&filled);
        for (bit, drawn) in PIECES.iter().copied().enumerate() {
            if bag & (1 << bit) == 0 {
                continue;
            }
            let rest = bag & !(1 << bit);
            let mut moves = vec![(drawn, hold, 0)];
            if hold_enabled {
                match hold {
                    None => {
                        push(
                            &mut entries,
                            &mut best,
                            &mut frontier,
                            &target.goal,
                            index,
                            (filled, rest, Some(drawn)),
                            drawn,
                            cost + HOLD_WEIGHT + 1,
                        );
                    }
                    Some(held) if held != drawn => {
                        moves.push((held, Some(drawn), HOLD_WEIGHT + 1));
                    }
                    Some(_) => {}
                }
            }
            for (piece, hold_after, extra) in moves {
                for candidate in search.candidates(&filled, piece) {
                    if Some(target.earliest_stage(&candidate.cells)) != open
                        || search.rule().check(&filled, &candidate).is_some()
                    {
                        continue;
                    }
                    let presses = keys
                        .entry((board.rows, piece))
                        .or_insert_with(|| LockPresses::new(&board, piece))
                        .get(&candidate.target);
                    debug_assert_eq!(presses, key_count(&board, &candidate.target));
                    let Some(presses) = presses else {
                        continue;
                    };
                    push(
                        &mut entries,
                        &mut best,
                        &mut frontier,
                        &target.goal,
                        index,
                        (fill(&filled, &candidate.cells), rest, hold_after),
                        drawn,
                        cost + extra + presses,
                    );
                }
            }
        }
    }
    None
}

/// Pieces still to place; every piece costs at least its hard drop, so this
/// never overestimates the remaining cost.
fn pieces_left(goal: &Masks, filled: &Masks) -> u32 {
    goal.iter()
        .zip(filled)
        .map(|(goal, filled)| (goal & !filled).count_ones())
        .sum::<u32>()
        / 4
}

#[allow(clippy::too_many_arguments)]
fn push(
    entries: &mut Vec<Entry>,
    best: &mut HashMap<State, u32>,
    frontier: &mut BinaryHeap<Reverse<(u32, u32, usize)>>,
    goal: &Masks,
    parent: usize,
    state: State,
    drawn: Piece,
    cost: u32,
) {
    if best.get(&state).is_some_and(|known| *known <= cost) {
        return;
    }
    best.insert(state, cost);
    entries.push(Entry {
        state,
        parent: Some(parent),
        drawn,
    });
    frontier.push(Reverse((
        cost + pieces_left(goal, &state.0),
        cost,
        entries.len() - 1,
    )));
}

/// The pieces drawn on the way to `index`, the current bag finished with its
/// unused pieces in a fixed order.
fn order_at(entries: &[Entry], index: usize, cost: u32) -> BagOrder {
    let mut queue = Vec::new();
    let mut current = index;
    while let Some(parent) = entries[current].parent {
        queue.push(entries[current].drawn);
        current = parent;
    }
    queue.reverse();
    let left = entries[index].state.1;
    if left != ALL_PIECES {
        queue.extend(
            PIECES
                .iter()
                .enumerate()
                .filter(|(bit, _)| left & (1 << bit) != 0)
                .map(|(_, piece)| *piece),
        );
    }
    BagOrder {
        queue,
        holds: cost / HOLD_WEIGHT,
        keys: cost % HOLD_WEIGHT,
    }
}
