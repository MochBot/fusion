//! Replays a found plan into wire steps: each piece's key path from spawn,
//! its frames, the board after the lock, and the S2/Tetra League scoring.

use crate::attack::AttackConfig;
use crate::board::Board;
use crate::gen::canonical_r;
use crate::header::Move;
use crate::header::{piece_table, Piece, Rotation};
use crate::pathfinder::{get_input, key_name, replay_inputs, Inputs, SoftDropMode};
use crate::state::{ChainState, ClearEvent};

use super::clears::{clear_label, spin_name};
use super::explain::remaining;
use super::frame::{
    board_of, board_rows, completed_rows, fill, letter_for_piece, letter_string, to_physical, Masks,
};
use super::search::{filled_after, Choice, Ending, HoldAction, Plan};
use super::target::BuildTarget;
use super::{BuildReason, BuildStatus, BuildStep, HoldActionName, OpenerBuildPlan, StepClear};

/// Fills `plan` from a plan the queue search found.
pub(super) fn complete(plan: &mut OpenerBuildPlan, target: &BuildTarget, found: &Plan) {
    let Some(steps) = replay_steps(target, &found.steps) else {
        plan.reasons.push(BuildReason::PathReplayFailed);
        return;
    };
    plan.status = match found.ending {
        Ending::Complete => BuildStatus::Complete,
        Ending::NeedsMorePieces { .. } => BuildStatus::NeedsMorePieces,
    };
    plan.trailing_hold = matches!(
        found.ending,
        Ending::NeedsMorePieces {
            trailing_hold: true
        }
    );
    plan.attack = total_attack(&steps);
    if let Some(last) = steps.last() {
        plan.end_hold = last.hold_after.clone();
    }
    plan.steps = steps;
    plan.queue_used = found.queue_used;
    plan.remaining = remaining(target, &filled_after(target.start, &found.steps));
}

/// Replays steps from the start board: each step's key path from spawn with
/// the piece's cells after every key, the board after the lock, and the clear
/// and attack the lock scores under the S2/Tetra League rules. None when a
/// key path does not reproduce its planned lock.
fn replay_steps(target: &BuildTarget, steps: &[Choice]) -> Option<Vec<BuildStep>> {
    let config = AttackConfig::tetra_league();
    let mut chain = ChainState::default();
    let mut filled = target.start;
    let mut letters = target.start_letters();
    let mut completed = target
        .stages
        .iter()
        .map(|stage| covers(&filled, &stage.goal))
        .collect::<Vec<_>>();
    let mut replayed = Vec::with_capacity(steps.len());
    for choice in steps {
        let board = board_of(&filled);
        let cells = to_physical(&filled, &choice.cells);
        let inputs = key_path(&board, &choice.target);
        let mut frames = Vec::with_capacity(inputs.size() + 1);
        let (_, _, _, spin) = replay_inputs(
            &board,
            choice.piece,
            false,
            None,
            SoftDropMode::Row,
            &inputs,
            &mut |x, y, rotation| frames.push(piece_cells(choice.piece, x, y, rotation)),
        )?;
        if spin != choice.target.spin() {
            return None;
        }
        let last = frames.last().copied()?;
        if sorted(last) != sorted(cells.map(|[x, y]| [i16::from(x), i16::from(y)])) {
            return None;
        }
        let mut keys = Vec::with_capacity(inputs.size() + 1);
        if choice.hold != HoldAction::None {
            keys.push("hold");
        }
        keys.extend(
            inputs
                .data
                .iter()
                .map(|input| key_name(*input))
                .filter(|name| !name.is_empty()),
        );

        let mut locked = board.clone();
        let mechanics = locked.lock(&choice.target);
        let transition = chain.advance_lock(
            &choice.target,
            &mechanics,
            choice.hold != HoldAction::None,
            false,
            &config,
        );
        chain = transition.chain;
        let cleared_now = completed_rows(&filled, &choice.cells);
        let catalogued = target
            .stages
            .iter()
            .find(|stage| {
                stage.label.is_some() && cleared_now.iter().any(|row| stage.cleared.contains(row))
            })
            .map(|stage| stage.node_id);
        filled = fill(&filled, &choice.cells);
        for [x, y] in choice.cells {
            letters[usize::from(y) * 10 + usize::from(x)] = letter_for_piece(choice.piece);
        }
        if locked.rows != board_of(&filled).rows {
            return None;
        }
        let cleared_rows = (0..64u8)
            .filter(|row| mechanics.cleared_mask & (1u64 << row) != 0)
            .collect();
        let mut stage = None;
        for (index, done) in completed.iter_mut().enumerate() {
            if !*done && covers(&filled, &target.stages[index].goal) {
                *done = true;
                stage = Some(target.stages[index].node_id);
            }
        }
        replayed.push(BuildStep {
            piece: letter_string(choice.piece),
            hold_action: match choice.hold {
                HoldAction::None => HoldActionName::None,
                HoldAction::Swap => HoldActionName::Swap,
                HoldAction::Stash => HoldActionName::Stash,
                HoldAction::SwapNext => HoldActionName::SwapNext,
            },
            queue_index: choice.queue_index,
            hold_after: choice.hold_after.map(letter_string),
            cells: cells.to_vec(),
            spin: spin_name(choice.target.spin()),
            keys,
            frames: frames.into_iter().map(|frame| frame.to_vec()).collect(),
            cleared_rows,
            board: board_rows(&filled, &letters),
            stage,
            clear: transition
                .clear_event
                .map(|event| step_clear(&event, catalogued)),
        });
    }
    Some(replayed)
}

/// The keys that move a piece from spawn to `target`: the finesse path when
/// one exists, else the shortest.
pub(super) fn key_path(board: &Board, target: &Move) -> Inputs {
    let inputs = get_input(board, target, true, false);
    if inputs.size() == 0 {
        get_input(board, target, false, false)
    } else {
        inputs
    }
}

/// Key presses for `target`, hard drop included; none when unreachable.
pub(super) fn key_count(board: &Board, target: &Move) -> Option<u32> {
    let inputs = key_path(board, target);
    (inputs.size() > 0).then(|| {
        inputs
            .data
            .iter()
            .filter(|input| !key_name(**input).is_empty())
            .count() as u32
    })
}

/// Attack the steps send in total.
fn total_attack(steps: &[BuildStep]) -> f32 {
    steps
        .iter()
        .filter_map(|step| step.clear.as_ref())
        .map(|clear| clear.attack)
        .sum()
}

fn step_clear(event: &ClearEvent, catalogued: Option<u32>) -> StepClear {
    StepClear {
        label: clear_label(
            event.piece,
            event.spin_type,
            event.lines_cleared,
            event.is_perfect_clear,
        ),
        lines: event.lines_cleared,
        spin: spin_name(event.spin_type),
        attack: event.attack_sent,
        b2b: i32::from(event.b2b_after) - 1,
        combo: event.combo_after as i32 - 1,
        perfect_clear: event.is_perfect_clear,
        catalogued,
    }
}

fn covers(filled: &Masks, goal: &Masks) -> bool {
    filled
        .iter()
        .zip(goal)
        .all(|(filled, goal)| goal & !filled == 0)
}

fn piece_cells(piece: Piece, x: i32, y: i32, rotation: Rotation) -> [[i16; 2]; 4] {
    let offsets = piece_table(piece, canonical_r(piece, rotation));
    let mut cells = [[x as i16, y as i16]; 4];
    for index in 0..3 {
        cells[index + 1] = [
            (x + i32::from(offsets[index].x)) as i16,
            (y + i32::from(offsets[index].y)) as i16,
        ];
    }
    cells
}

fn sorted(mut cells: [[i16; 2]; 4]) -> [[i16; 2]; 4] {
    cells.sort_unstable();
    cells
}
