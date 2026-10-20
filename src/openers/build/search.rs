//! The queue search (uniform cost over the player's queue and hold) and the
//! any-order search (any piece may come next).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::board::Board;
use crate::header::{Move, Piece};

use super::clears::ClearRule;
use super::frame::{fill, Cells, Masks, PIECES};
use super::moves::{Candidate, Moves, PlacementCache};
use super::target::BuildTarget;

/// Plan cost of a placement that needs a soft drop (a tuck or a spin), and of
/// a hold press: the search prefers plans a player can follow with hard drops,
/// then plans with fewer holds.
const SOFT_DROP_COST: u32 = 10;
const HOLD_COST: u32 = 1;
/// Plan cost of placing a piece of a later catalogued stage while an earlier
/// one is unfinished: a plan that follows the stages shows each catalogued
/// board on the way, which the reader is learning.
const STAGE_ORDER_COST: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HoldAction {
    None,
    /// The held piece is played and the current piece takes its place.
    Swap,
    /// The current piece goes to the empty hold and the next piece is played.
    Stash,
    /// After the queue: the held piece is played and the piece that follows
    /// the queue goes to hold.
    SwapNext,
}

/// One placement of a plan: frame cells, the engine move that locks them
/// from spawn, and how the piece came out of the queue.
#[derive(Clone, Copy, Debug)]
pub(super) struct Choice {
    pub piece: Piece,
    pub cells: Cells,
    pub target: Move,
    pub hold: HoldAction,
    /// Queue index of the placed piece; `None` for the piece held at the start.
    pub queue_index: Option<usize>,
    pub hold_after: Option<Piece>,
}

type StateKey = (Masks, u16, Option<Piece>);

#[derive(Clone, Copy, Debug)]
struct State {
    filled: Masks,
    next: u16,
    hold: Option<Piece>,
    /// The origin of the held piece; only needed to report where it came from.
    hold_index: Option<u16>,
}

impl State {
    fn key(&self) -> StateKey {
        (self.filled, self.next, self.hold)
    }
}

/// A search-tree entry: the state and the transition that reached it.
#[derive(Clone, Copy, Debug)]
struct Entry {
    state: State,
    parent: Option<usize>,
    choice: Option<Choice>,
    /// The transition put the last queue piece into the empty hold.
    trailing_hold: bool,
    depth: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ending {
    Complete,
    /// The queue ran out; the rest of the build still has a legal order.
    NeedsMorePieces {
        /// The last queue piece went to the empty hold without a placement.
        trailing_hold: bool,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Plan {
    pub steps: Vec<Choice>,
    pub ending: Ending,
    pub queue_used: usize,
}

/// The deepest point a failed queue search reached, for explaining why it
/// stopped.
#[derive(Clone, Debug)]
pub(super) struct Deepest {
    pub steps: Vec<Choice>,
    pub filled: Masks,
    pub next: usize,
    pub hold: Option<Piece>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Goal {
    Complete,
    /// Every queue piece is used and the remainder stays buildable.
    Partial,
}

pub(super) struct Search<'a> {
    target: &'a BuildTarget,
    queue: &'a [Piece],
    initial_hold: Option<Piece>,
    hold_enabled: bool,
    moves: Moves<'a>,
    rule: ClearRule<'a>,
    budget: u32,
    spent: u32,
    pub exhausted: bool,
    deepest: Option<Deepest>,
    /// Frame states the any-order search has proven dead or finishable.
    any_order_dead: HashSet<Masks>,
    any_order_live: HashSet<Masks>,
    any_order_deepest: Option<(Masks, Vec<Choice>)>,
}

impl<'a> Search<'a> {
    pub(super) fn new(
        target: &'a BuildTarget,
        queue: &'a [Piece],
        initial_hold: Option<Piece>,
        hold_enabled: bool,
        budget: u32,
    ) -> Self {
        Self {
            target,
            queue,
            initial_hold,
            hold_enabled,
            moves: Moves::new(target),
            rule: ClearRule::new(target),
            budget,
            spent: 0,
            exhausted: false,
            deepest: None,
            any_order_dead: HashSet::new(),
            any_order_live: HashSet::new(),
            any_order_deepest: None,
        }
    }

    /// Reuses a request's legality and spin-path lookups.
    pub(super) fn with_cache(mut self, cache: PlacementCache) -> Self {
        self.moves = Moves::with_cache(self.target, cache);
        self
    }

    pub(super) fn into_cache(self) -> PlacementCache {
        self.moves.into_cache()
    }

    /// Plans the shape alone, without required clears.
    pub(super) fn shape_only(mut self) -> Self {
        self.rule.shape_only();
        self
    }

    /// Requires the clears of the stages before `limit` only.
    pub(super) fn requiring_clears_before(mut self, limit: usize) -> Self {
        self.rule.require_before(limit);
        self
    }

    pub(super) fn target(&self) -> &'a BuildTarget {
        self.target
    }

    pub(super) fn rule(&self) -> &ClearRule<'a> {
        &self.rule
    }

    /// Legal placements of `piece` under this search's clear rule's spins.
    pub(super) fn candidates(&mut self, filled: &Masks, piece: Piece) -> Vec<Candidate> {
        self.moves
            .candidates(filled, piece, |slot| self.rule.min_spin(slot))
    }

    /// Whether a piece of `letter` can lock on these board cells from spawn.
    pub(super) fn legal(&mut self, board: &Board, letter: u8, cells: &Cells) -> bool {
        self.moves.legal(board, letter, cells)
    }

    /// The cheapest plan that completes the build within the queue, else the
    /// cheapest that plays every queue piece into the build and leaves a
    /// buildable remainder.
    pub(super) fn run(&mut self) -> Option<Plan> {
        if let Some(plan) = self.cheapest(Goal::Complete) {
            return Some(plan);
        }
        if self.exhausted {
            return None;
        }
        self.cheapest(Goal::Partial)
    }

    /// Reuses the placement and legality caches for another queue.
    #[cfg(test)]
    pub(super) fn set_queue(&mut self, queue: &'a [Piece], hold: Option<Piece>, budget: u32) {
        self.queue = queue;
        self.initial_hold = hold;
        self.deepest = None;
        self.any_order_deepest = None;
        self.refill(budget);
    }

    /// Starts a new phase with its own expansion budget.
    pub(super) fn refill(&mut self, budget: u32) {
        self.budget = budget;
        self.spent = 0;
        self.exhausted = false;
    }

    pub(super) fn deepest(&self) -> Option<&Deepest> {
        self.deepest.as_ref()
    }

    /// Whether some order of any pieces finishes the build from the start.
    pub(super) fn any_order_buildable(&mut self) -> bool {
        let mut path = Vec::new();
        self.any_order(self.target.start, &mut path)
    }

    /// An order of any pieces that finishes the build from the start.
    #[cfg(test)]
    pub(super) fn any_order_path(&mut self) -> Option<Vec<Choice>> {
        let mut path = Vec::new();
        self.any_order(self.target.start, &mut path).then_some(path)
    }

    /// The deepest frame the any-order search reached, with its order.
    pub(super) fn any_order_deepest(&self) -> Option<&(Masks, Vec<Choice>)> {
        self.any_order_deepest.as_ref()
    }

    fn cheapest(&mut self, goal: Goal) -> Option<Plan> {
        let start = State {
            filled: self.target.start,
            next: 0,
            hold: self.initial_hold,
            hold_index: None,
        };
        let mut entries = vec![Entry {
            state: start,
            parent: None,
            choice: None,
            trailing_hold: false,
            depth: 0,
        }];
        let mut best = HashMap::<StateKey, u32>::from([(start.key(), 0)]);
        let mut closed = HashSet::<StateKey>::new();
        let mut frontier = BinaryHeap::from([Reverse((0u32, 0usize))]);
        let len = self.queue.len() as u16;
        while let Some(Reverse((cost, index))) = frontier.pop() {
            let entry = entries[index];
            let state = entry.state;
            if !closed.insert(state.key()) {
                continue;
            }
            if self.spent >= self.budget {
                self.exhausted = true;
                return None;
            }
            self.spent += 1;
            self.note_deepest(&entries, index);
            let reached = match goal {
                Goal::Complete => state.filled == self.target.goal,
                Goal::Partial => {
                    state.next == len && {
                        let mut path = Vec::new();
                        let done = self.any_order(state.filled, &mut path);
                        if self.exhausted {
                            return None;
                        }
                        done
                    }
                }
            };
            if reached {
                return Some(self.plan_at(&entries, index, goal));
            }
            for (child, trailing_hold, step_cost) in self.transitions(state, goal) {
                let child_cost = cost + step_cost;
                let key = child.0.key();
                if closed.contains(&key) || best.get(&key).is_some_and(|known| *known <= child_cost)
                {
                    continue;
                }
                best.insert(key, child_cost);
                entries.push(Entry {
                    state: child.0,
                    parent: Some(index),
                    choice: child.1,
                    trailing_hold,
                    depth: entry.depth + u32::from(child.1.is_some()),
                });
                frontier.push(Reverse((child_cost, entries.len() - 1)));
            }
        }
        None
    }

    /// Successor states with their costs: play the current piece, swap it
    /// with the held one, or stash it and play the next.
    #[allow(clippy::type_complexity)]
    fn transitions(
        &mut self,
        state: State,
        goal: Goal,
    ) -> Vec<((State, Option<Choice>), bool, u32)> {
        let next = usize::from(state.next);
        let mut options = Vec::new();
        match self.queue.get(next).copied() {
            Some(current) => {
                options.push((
                    current,
                    HoldAction::None,
                    Some(next),
                    state.next + 1,
                    state.hold,
                    state.hold_index,
                ));
                if self.hold_enabled {
                    match state.hold {
                        Some(held) if held != current => options.push((
                            held,
                            HoldAction::Swap,
                            state.hold_index.map(usize::from),
                            state.next + 1,
                            Some(current),
                            Some(state.next),
                        )),
                        Some(_) => {}
                        None => {
                            if let Some(following) = self.queue.get(next + 1).copied() {
                                options.push((
                                    following,
                                    HoldAction::Stash,
                                    Some(next + 1),
                                    state.next + 2,
                                    Some(current),
                                    Some(state.next),
                                ));
                            }
                        }
                    }
                }
            }
            None => {
                if let (Goal::Complete, true, Some(held)) = (goal, self.hold_enabled, state.hold) {
                    options.push((
                        held,
                        HoldAction::SwapNext,
                        state.hold_index.map(usize::from),
                        state.next,
                        None,
                        None,
                    ));
                }
            }
        }
        let open_stage = self.target.open_stage(&state.filled).unwrap_or(usize::MAX);
        let mut found = Vec::new();
        for (piece, hold, queue_index, following, hold_after, hold_index) in options {
            let hold_cost = if hold == HoldAction::None {
                0
            } else {
                HOLD_COST
            };
            for candidate in self.candidates(&state.filled, piece) {
                if self.rule.check(&state.filled, &candidate).is_some() {
                    continue;
                }
                let filled = fill(&state.filled, &candidate.cells);
                if hold == HoldAction::SwapNext && filled != self.target.goal {
                    continue;
                }
                let step_cost = hold_cost
                    + if candidate.hard_drop {
                        0
                    } else {
                        SOFT_DROP_COST
                    }
                    + if self.target.latest_stage(&candidate.cells) > open_stage {
                        STAGE_ORDER_COST
                    } else {
                        0
                    };
                found.push((
                    (
                        State {
                            filled,
                            next: following,
                            hold: hold_after,
                            hold_index,
                        },
                        Some(Choice {
                            piece,
                            cells: candidate.cells,
                            target: candidate.target,
                            hold,
                            queue_index,
                            hold_after,
                        }),
                    ),
                    false,
                    step_cost,
                ));
            }
        }
        // The last queue piece may go to the empty hold when no placement of
        // it continues the build.
        if goal == Goal::Partial
            && self.hold_enabled
            && state.hold.is_none()
            && next + 1 == self.queue.len()
        {
            found.push((
                (
                    State {
                        filled: state.filled,
                        next: state.next + 1,
                        hold: self.queue.get(next).copied(),
                        hold_index: Some(state.next),
                    },
                    None,
                ),
                true,
                HOLD_COST,
            ));
        }
        found
    }

    fn plan_at(&self, entries: &[Entry], index: usize, goal: Goal) -> Plan {
        let entry = entries[index];
        Plan {
            steps: path_to(entries, index),
            ending: match goal {
                Goal::Complete => Ending::Complete,
                Goal::Partial => Ending::NeedsMorePieces {
                    trailing_hold: entry.trailing_hold,
                },
            },
            queue_used: usize::from(entry.state.next),
        }
    }

    fn note_deepest(&mut self, entries: &[Entry], index: usize) {
        let entry = entries[index];
        if self
            .deepest
            .as_ref()
            .is_some_and(|deepest| deepest.steps.len() >= entry.depth as usize)
        {
            return;
        }
        self.deepest = Some(Deepest {
            steps: path_to(entries, index),
            filled: entry.state.filled,
            next: usize::from(entry.state.next),
            hold: entry.state.hold,
        });
    }

    /// The any-order search: any piece may come next, finishing the
    /// catalogued stages in order. Each stage's diagram is a board the source
    /// shows being reached, so a build that exists passes through it;
    /// ordering the stages turns each boundary into one memoized state.
    fn any_order(&mut self, filled: Masks, path: &mut Vec<Choice>) -> bool {
        if filled == self.target.goal || self.any_order_live.contains(&filled) {
            return true;
        }
        if self.any_order_dead.contains(&filled) {
            return false;
        }
        if self.spent >= self.budget {
            self.exhausted = true;
            return false;
        }
        self.spent += 1;
        if self
            .any_order_deepest
            .as_ref()
            .is_none_or(|(_, steps)| steps.len() < path.len())
        {
            self.any_order_deepest = Some((filled, path.clone()));
        }
        let open = self.target.open_stage(&filled);
        for piece in PIECES {
            for candidate in self.candidates(&filled, piece) {
                if Some(self.target.earliest_stage(&candidate.cells)) != open
                    || self.rule.check(&filled, &candidate).is_some()
                {
                    continue;
                }
                path.push(Choice {
                    piece,
                    cells: candidate.cells,
                    target: candidate.target,
                    hold: HoldAction::None,
                    queue_index: None,
                    hold_after: None,
                });
                // On success the path keeps the order that finished the build.
                if self.any_order(fill(&filled, &candidate.cells), path) {
                    self.any_order_live.insert(filled);
                    return true;
                }
                path.pop();
                if self.exhausted {
                    return false;
                }
            }
        }
        self.any_order_dead.insert(filled);
        false
    }
}

/// The frame after playing `steps` from `start`.
pub(super) fn filled_after(start: Masks, steps: &[Choice]) -> Masks {
    steps
        .iter()
        .fold(start, |filled, choice| fill(&filled, &choice.cells))
}

fn path_to(entries: &[Entry], index: usize) -> Vec<Choice> {
    let mut steps = Vec::new();
    let mut current = Some(index);
    while let Some(at) = current {
        steps.extend(entries[at].choice);
        current = entries[at].parent;
    }
    steps.reverse();
    steps
}
