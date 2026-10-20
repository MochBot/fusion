//! Legal placements into the target's unfilled cells, with the legality,
//! candidate and spin-variant caches behind them.

use std::collections::{HashMap, HashSet};

use crate::board::{Board, BOARD_HEIGHT};
use crate::header::{Move, Piece, SpinType};
use crate::move_buffer::MoveBuffer;
use crate::openers::recognition::{placement_cells, LegalityMemo, LegalityVerdict};
use crate::pathfinder::get_input;
use crate::smear_core::generate_placements;

use super::frame::{board_of, is_filled, letter_for_piece, to_frame, to_physical, Cells, Masks};
use super::target::BuildTarget;

/// One legal placement of a piece into target cells.
#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub cells: Cells,
    pub target: Move,
    /// Reachable by dropping straight down: no cell above it is filled.
    pub hard_drop: bool,
    /// The slot this places, if any; outline placements have none.
    pub slot: Option<usize>,
}

/// Legality and spin-path lookups, keyed by board and piece alone, so one
/// cache serves every target and clear rule of a request.
#[derive(Default)]
pub(super) struct PlacementCache {
    legality: LegalityMemo,
    spin_variants: HashMap<([u16; BOARD_HEIGHT], Piece, Cells), Vec<Move>>,
}

/// Placement generation for one target. A `Moves` serves one clear rule: its
/// candidate cache assumes every call passes the same minimum spins.
pub(super) struct Moves<'a> {
    target: &'a BuildTarget,
    candidates: HashMap<(Masks, Piece), Vec<Candidate>>,
    cache: PlacementCache,
}

impl<'a> Moves<'a> {
    pub(super) fn new(target: &'a BuildTarget) -> Self {
        Self::with_cache(target, PlacementCache::default())
    }

    pub(super) fn with_cache(target: &'a BuildTarget, cache: PlacementCache) -> Self {
        Self {
            target,
            candidates: HashMap::new(),
            cache,
        }
    }

    pub(super) fn into_cache(self) -> PlacementCache {
        self.cache
    }

    /// Every legal placement of `piece` into unfilled target cells, hard
    /// drops then lowest first: slots of that letter, then outline placements
    /// whose cells the source letters allow. A slot `min_spin` names yields
    /// each way it can lock with at least that spin.
    pub(super) fn candidates(
        &mut self,
        filled: &Masks,
        piece: Piece,
        min_spin: impl Fn(usize) -> Option<SpinType>,
    ) -> Vec<Candidate> {
        if let Some(cached) = self.candidates.get(&(*filled, piece)) {
            return cached.clone();
        }
        let target = self.target;
        let board = board_of(filled);
        let letter = letter_for_piece(piece);
        let mut found = Vec::new();
        for (index, slot) in target.slots.iter().enumerate() {
            if slot.piece != piece || is_filled(filled, &slot.cells) {
                continue;
            }
            let physical = to_physical(filled, &slot.cells);
            let hard_drop = drops_straight(&board, &physical);
            if let Some(min_spin) = min_spin(index) {
                for target in self.cache.spin_variants(&board, piece, &physical) {
                    if target.spin() as u8 >= min_spin as u8 {
                        found.push(Candidate {
                            cells: slot.cells,
                            target,
                            hard_drop,
                            slot: Some(index),
                        });
                    }
                }
                continue;
            }
            if let LegalityVerdict::Legal { target, .. } =
                self.cache.legality.check(&board, letter, &physical).verdict
            {
                found.push(Candidate {
                    cells: slot.cells,
                    target,
                    hard_drop,
                    slot: Some(index),
                });
            }
        }
        if target.outline_accepts(filled, letter) {
            let mut moves = MoveBuffer::new();
            generate_placements(&board, &mut moves, piece, false);
            let mut seen = HashSet::new();
            for candidate in moves.iter().copied() {
                let Some(physical) = placement_cells(candidate) else {
                    continue;
                };
                let Some(cells) = to_frame(filled, &physical) else {
                    continue;
                };
                if !target.outline_fits(filled, &cells, letter) || !seen.insert(cells) {
                    continue;
                }
                if let LegalityVerdict::Legal { target, .. } =
                    self.cache.legality.check(&board, letter, &physical).verdict
                {
                    found.push(Candidate {
                        cells,
                        target,
                        hard_drop: drops_straight(&board, &physical),
                        slot: None,
                    });
                }
            }
        }
        found.sort_by_key(|candidate| {
            let low = candidate.cells.iter().map(|[x, y]| (*y, *x)).min();
            (!candidate.hard_drop, low, candidate.cells)
        });
        self.candidates.insert((*filled, piece), found.clone());
        found
    }

    /// Whether a piece of `letter` can lock on these board cells from spawn.
    pub(super) fn legal(&mut self, board: &Board, letter: u8, cells: &Cells) -> bool {
        matches!(
            self.cache.legality.check(board, letter, cells).verdict,
            LegalityVerdict::Legal { .. }
        )
    }
}

impl PlacementCache {
    /// Every way to lock `piece` on these board cells from spawn: plain,
    /// then each spin the engine can label it with, highest first.
    fn spin_variants(&mut self, board: &Board, piece: Piece, cells: &Cells) -> Vec<Move> {
        let mut sorted = *cells;
        sorted.sort_unstable();
        let key = (board.rows, piece, sorted);
        if let Some(known) = self.spin_variants.get(&key) {
            return known.clone();
        }
        let mut moves = MoveBuffer::new();
        generate_placements(board, &mut moves, piece, false);
        let mut found: Vec<Move> = Vec::new();
        for candidate in moves.iter().copied() {
            if placement_cells(candidate) != Some(sorted) || !board.legal_lock_placement(&candidate)
            {
                continue;
            }
            let (rotation, x, y) = (candidate.rotation(), candidate.x(), candidate.y());
            let mut targets = vec![Move::new(piece, rotation, x, y, false)];
            match piece {
                Piece::T => {
                    targets.push(Move::new_tspin(rotation, x, y, true));
                    targets.push(Move::new_tspin(rotation, x, y, false));
                }
                Piece::O => {}
                _ => targets.push(Move::new_allspin_mini(piece, rotation, x, y)),
            }
            for target in targets {
                if found.iter().any(|known| known.spin() == target.spin()) {
                    continue;
                }
                if get_input(board, &target, false, false).size() > 0 {
                    found.push(target);
                }
            }
        }
        found.sort_by_key(|target| std::cmp::Reverse(target.spin() as u8));
        self.spin_variants.insert(key, found.clone());
        found
    }
}

/// Whether nothing on the board sits above any cell of the placement.
fn drops_straight(board: &Board, cells: &Cells) -> bool {
    cells.iter().all(|[x, y]| {
        ((usize::from(*y) + 1)..BOARD_HEIGHT)
            .all(|row| board.rows[row] & (1 << x) == 0 || cells.contains(&[*x, row as u8]))
    })
}
