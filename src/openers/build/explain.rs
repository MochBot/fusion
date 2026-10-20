//! Why a queue does not build the target: the reasons, the stuck point and
//! what each available piece cannot do there, what remains, and the mirrored
//! or sibling builds the queue does make.

use crate::board::FULL_ROW;
use crate::header::Piece;

use super::clears::ClearBreak;
use super::frame::{
    board_of, board_rows, fill, is_filled, letter_for_piece, letter_string, to_physical, Cells,
    Masks, CELLS, PIECES,
};
use super::search::{Choice, Search};
use super::target::BuildTarget;
use super::{
    BuildReason, BuildRemaining, BuildStatus, BuildStuck, MissingPiece, OpenerBuildInput,
    OpenerBuildPlan, PieceSource, PlacedPiece, SlotVerdict, StuckPiece, StuckSlot,
    ALTERNATIVE_BUDGET, BUILD_BUDGET,
};

/// Fills `plan` after the queue search found nothing: the reason, and the
/// furthest point the build got with what stops each available piece there.
pub(super) fn explain_failure(
    plan: &mut OpenerBuildPlan,
    input: &OpenerBuildInput,
    target: &BuildTarget,
    queue: &[Piece],
    hold: Option<Piece>,
    search: &mut Search<'_>,
) {
    if search.exhausted {
        plan.reasons.push(BuildReason::SearchLimit);
        return;
    }
    search.refill(BUILD_BUDGET);
    let buildable = search.any_order_buildable();
    if search.exhausted {
        plan.reasons.push(BuildReason::SearchLimit);
        return;
    }
    plan.status = BuildStatus::Impossible;
    if !buildable {
        let (filled, steps) = search
            .any_order_deepest()
            .cloned()
            .unwrap_or((target.start, Vec::new()));
        let mut shape =
            Search::new(target, queue, hold, input.hold_enabled, BUILD_BUDGET).shape_only();
        let shape_buildable = shape.any_order_buildable() && !shape.exhausted;
        plan.reasons.push(
            match search
                .rule()
                .next_clear(&filled)
                .filter(|_| shape_buildable)
                .map(|stage| &target.stages[stage])
            {
                Some(stage) => BuildReason::ClearUnachievable {
                    node_id: stage.node_id,
                    label: stage.label.clone().unwrap_or_default(),
                },
                None => BuildReason::Unbuildable,
            },
        );
        let pieces = remaining_letters(target, &filled)
            .into_iter()
            .map(|piece| (piece, PieceSource::Any))
            .collect::<Vec<_>>();
        stuck_at(plan, search, &filled, &steps, None, &pieces);
        return;
    }
    let Some(deepest) = search.deepest().cloned() else {
        return;
    };
    let missing = missing_pieces(target, queue, hold);
    plan.reasons.push(if !missing.is_empty() {
        BuildReason::MissingPieces { pieces: missing }
    } else {
        let lost = search
            .rule()
            .next_clear(&deepest.filled)
            .map(|stage| &target.stages[stage]);
        let mut shape =
            Search::new(target, queue, hold, input.hold_enabled, ALTERNATIVE_BUDGET).shape_only();
        match lost {
            Some(stage) if shape.run().is_some() => BuildReason::ClearsLost {
                node_id: stage.node_id,
                label: stage.label.clone().unwrap_or_default(),
            },
            _ => BuildReason::OrderConflict,
        }
    });
    let mut pieces = Vec::new();
    match queue.get(deepest.next) {
        Some(current) => {
            pieces.push((*current, PieceSource::Current));
            match deepest.hold {
                Some(held) if held != *current => pieces.push((held, PieceSource::Hold)),
                Some(_) => {}
                None if input.hold_enabled => {
                    if let Some(following) = queue.get(deepest.next + 1) {
                        if following != current {
                            pieces.push((*following, PieceSource::Next));
                        }
                    }
                }
                None => {}
            }
        }
        None => pieces.extend(
            remaining_letters(target, &deepest.filled)
                .into_iter()
                .map(|piece| (piece, PieceSource::Any)),
        ),
    }
    stuck_at(
        plan,
        search,
        &deepest.filled,
        &deepest.steps,
        deepest.hold,
        &pieces,
    );
    plan.queue_used = deepest.next;
}

fn stuck_at(
    plan: &mut OpenerBuildPlan,
    search: &mut Search<'_>,
    filled: &Masks,
    steps: &[Choice],
    hold: Option<Piece>,
    pieces: &[(Piece, PieceSource)],
) {
    let target = search.target();
    let mut before = target.start;
    let placed = steps
        .iter()
        .map(|choice| {
            let cells = to_physical(&before, &choice.cells).to_vec();
            before = fill(&before, &choice.cells);
            PlacedPiece {
                piece: letter_string(choice.piece),
                cells,
            }
        })
        .collect::<Vec<_>>();
    let letters = placed_letters(target, steps);
    plan.remaining = remaining(target, filled);
    plan.stuck = Some(BuildStuck {
        after_step: placed.len(),
        placed,
        board: board_rows(filled, &letters),
        hold: hold.map(letter_string),
        pieces: stuck_pieces(search, filled, steps, pieces),
    });
}

/// Why each piece the player could play at a stuck point fails to continue
/// the build: for every slot of its letter, whether it could lock now (so the
/// dead end is further on), would float until the pieces under it are
/// placed, is sealed off by pieces already placed, or would lock in a way
/// that breaks one of the required clears.
fn stuck_pieces(
    search: &mut Search<'_>,
    filled: &Masks,
    placed: &[Choice],
    pieces: &[(Piece, PieceSource)],
) -> Vec<StuckPiece> {
    let target = search.target();
    let board = board_of(filled);
    let mut explained = Vec::new();
    for (piece, source) in pieces {
        let letter = letter_for_piece(*piece);
        let mut slots = Vec::new();
        let candidates = search.candidates(filled, *piece);
        for (index, slot) in target
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.piece == *piece && !is_filled(filled, &slot.cells))
        {
            let cells = to_physical(filled, &slot.cells);
            let own = candidates
                .iter()
                .filter(|candidate| candidate.slot == Some(index))
                .collect::<Vec<_>>();
            let mut clear = None;
            let (verdict, waits_for, blocked_by) = if search.legal(&board, letter, &cells) {
                let breaks = own
                    .iter()
                    .map(|candidate| search.rule().check(filled, candidate))
                    .collect::<Vec<_>>();
                let verdict = if breaks.iter().any(Option::is_none) {
                    SlotVerdict::Placeable
                } else {
                    match breaks.into_iter().flatten().next() {
                        Some(ClearBreak::TooEarly) => SlotVerdict::TooEarly,
                        Some(ClearBreak::WrongClear) => SlotVerdict::BreaksClear,
                        Some(ClearBreak::OutOfOrder) => SlotVerdict::OutOfOrder,
                        // Legal, but not with the spin its clear needs.
                        Some(ClearBreak::NoSpin) | None => SlotVerdict::NoSpin,
                    }
                };
                if verdict != SlotVerdict::Placeable {
                    clear = own
                        .first()
                        .and_then(|candidate| search.rule().clear_concerned(filled, candidate))
                        .or_else(|| search.rule().spin_slot_label(index));
                }
                (verdict, Vec::new(), Vec::new())
            } else if !supported(filled, &slot.cells) {
                (
                    SlotVerdict::NeedsSupport,
                    supports(target, filled, &slot.cells),
                    Vec::new(),
                )
            } else {
                (
                    SlotVerdict::Blocked,
                    Vec::new(),
                    blockers(search, filled, placed, letter, &slot.cells),
                )
            };
            slots.push(StuckSlot {
                cells: cells.to_vec(),
                verdict,
                waits_for,
                blocked_by,
                clear,
            });
        }
        let outline_fit = target
            .outline_accepts(filled, letter)
            .then(|| !search.candidates(filled, *piece).is_empty());
        explained.push(StuckPiece {
            piece: letter_string(*piece),
            source: *source,
            in_build: !slots.is_empty() || outline_fit.is_some(),
            outline_fit,
            slots,
        });
    }
    explained
}

/// Whether some cell of the spot rests on the floor or a filled cell once the
/// full rows under it have cleared.
fn supported(filled: &Masks, cells: &Cells) -> bool {
    cells.iter().any(|[x, y]| match below(filled, *y) {
        None => true,
        Some(row) => filled[row] & (1 << x) != 0 && !cells.contains(&[*x, row as u8]),
    })
}

fn below(filled: &Masks, y: u8) -> Option<usize> {
    (0..usize::from(y))
        .rev()
        .find(|row| filled[*row] != FULL_ROW)
}

/// The unplaced slots (or single outline cells) right under a floating spot.
fn supports(target: &BuildTarget, filled: &Masks, cells: &Cells) -> Vec<Vec<[u8; 2]>> {
    let mut found: Vec<Vec<[u8; 2]>> = Vec::new();
    for [x, y] in cells {
        let Some(row) = below(filled, *y) else {
            continue;
        };
        let cell = [*x, row as u8];
        if cells.contains(&cell) || filled[row] & (1 << x) != 0 {
            continue;
        }
        let under = target
            .slots
            .iter()
            .find(|slot| slot.cells.contains(&cell))
            .map(|slot| physical(filled, &slot.cells))
            .unwrap_or_else(|| physical(filled, &[cell; 4])[..1].to_vec());
        if !found.contains(&under) {
            found.push(under);
        }
    }
    found
}

/// The placed pieces whose absence alone would open the spot.
fn blockers(
    search: &mut Search<'_>,
    filled: &Masks,
    placed: &[Choice],
    letter: u8,
    cells: &Cells,
) -> Vec<Vec<[u8; 2]>> {
    let mut found = Vec::new();
    for choice in placed {
        if choice
            .cells
            .iter()
            .any(|[_, y]| filled[usize::from(*y)] == FULL_ROW)
        {
            continue;
        }
        let mut without = *filled;
        for [x, y] in choice.cells {
            without[usize::from(y)] &= !(1 << x);
        }
        let board = board_of(&without);
        if search.legal(&board, letter, &to_physical(&without, cells)) {
            found.push(physical(filled, &choice.cells));
        }
    }
    found
}

fn physical(filled: &Masks, cells: &Cells) -> Vec<[u8; 2]> {
    to_physical(filled, cells).to_vec()
}

/// Letters of the start board and of every placed piece.
fn placed_letters(target: &BuildTarget, steps: &[Choice]) -> [u8; CELLS] {
    let mut letters = target.start_letters();
    for choice in steps {
        for [x, y] in choice.cells {
            letters[usize::from(y) * 10 + usize::from(x)] = letter_for_piece(choice.piece);
        }
    }
    letters
}

/// The slots and outline cells `filled` has not covered yet.
pub(super) fn remaining(target: &BuildTarget, filled: &Masks) -> BuildRemaining {
    BuildRemaining {
        pieces: target
            .slots
            .iter()
            .filter(|slot| !is_filled(filled, &slot.cells))
            .map(|slot| letter_string(slot.piece))
            .collect(),
        outline_pieces: target.outline_pieces(filled),
    }
}

/// Distinct letters the rest of the build could still use.
fn remaining_letters(target: &BuildTarget, filled: &Masks) -> Vec<Piece> {
    PIECES
        .into_iter()
        .filter(|piece| {
            target
                .slots
                .iter()
                .any(|slot| slot.piece == *piece && !is_filled(filled, &slot.cells))
                || target.outline_accepts(filled, letter_for_piece(*piece))
        })
        .collect()
}

/// Letters the build needs more of than the queue and hold supply, when the
/// queue is long enough for the whole build.
fn missing_pieces(target: &BuildTarget, queue: &[Piece], hold: Option<Piece>) -> Vec<MissingPiece> {
    let total = target.slots.len() as u32 + target.outline_pieces(&target.start);
    let supplied = queue.len() as u32 + u32::from(hold.is_some());
    if supplied < total {
        return Vec::new();
    }
    PIECES
        .into_iter()
        .filter_map(|piece| {
            let needed = target
                .slots
                .iter()
                .filter(|slot| slot.piece == piece)
                .count() as u32;
            let available = queue.iter().filter(|queued| **queued == piece).count() as u32
                + u32::from(hold == Some(piece));
            (needed > available).then(|| MissingPiece {
                piece: letter_string(piece),
                needed,
                available,
            })
        })
        .collect()
}
