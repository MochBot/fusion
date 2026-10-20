//! Plans how to build one version of a catalogued opener from a queue: which
//! of the version's shapes the queue fits, then the order, hold use and key
//! path of every piece under SRS+, performing the version's clears, or why no
//! plan exists. Catalogued diagrams rarely record a placement order, so the
//! order is searched here.

mod bag;
mod choose;
mod clears;
mod explain;
mod frame;
mod moves;
mod replay;
mod search;
mod target;
mod versions;
mod wire;

#[cfg(test)]
mod tests;

pub use wire::{
    BuildReason, BuildRemaining, BuildStage, BuildStatus, BuildStep, BuildStuck, HoldActionName,
    MissingPiece, OpenerBuildError, OpenerBuildInput, OpenerBuildPlan, PieceSource, PlacedPiece,
    PlannedClear, SlotVerdict, SpinName, StepClear, StuckPiece, StuckSlot,
};

use crate::header::Piece;

use super::catalog::validate_opener_tree;
use bag::{default_order, BagOrder};
use clears::{fit_clears_within, planned_clears, target_with_clears};
use frame::{board_rows, letter_string, piece_for_letter};
use moves::PlacementCache;
use search::Search;
use target::{BuildTarget, TargetError, MAX_OUTLINE_CELLS, SMALL_OUTLINE_CELLS};

/// Expansion budget of one planning phase, and of each shape-only check.
const BUILD_BUDGET: u32 = 40_000;
const ALTERNATIVE_BUDGET: u32 = 6_000;
/// Expansion budget of the default-order search, shared by all shapes.
const BAG_BUDGET: u32 = 10_000;
/// The default-order budget of one shape with a large outline. Its expansions
/// are the costliest, and 93% of the orders found on the live catalogue's
/// outlined versions need fewer.
const OUTLINE_BAG_BUDGET: u32 = 2_000;
const MAX_QUEUE: usize = 70;

pub fn build_opener(input: &OpenerBuildInput) -> Result<OpenerBuildPlan, OpenerBuildError> {
    validate_opener_tree(&input.tree).map_err(OpenerBuildError::InvalidTree)?;
    let (&first, rest) = input
        .targets
        .split_first()
        .ok_or(OpenerBuildError::NoTargets)?;
    let names =
        versions::clear_names(&input.tree, first).ok_or(OpenerBuildError::UnknownNode(first))?;
    for &node in rest {
        let other =
            versions::clear_names(&input.tree, node).ok_or(OpenerBuildError::UnknownNode(node))?;
        if other != names {
            return Err(OpenerBuildError::MixedVersions);
        }
    }
    let mut cache = PlacementCache::default();
    // Set when the default order search found nothing: whether it ran out.
    let mut no_default_order: Option<bool> = None;
    let (target_node, queue, hold) = if input.default_queue {
        match best_default_order(input, &mut cache) {
            (Some((node, order)), _) => (node, order.queue, None),
            (None, exhausted) => {
                no_default_order = Some(exhausted);
                (first, Vec::new(), None)
            }
        }
    } else {
        let queue = parse_pieces(&input.queue)?;
        if queue.len() > MAX_QUEUE {
            return Err(OpenerBuildError::InvalidQueue);
        }
        let hold = match input.hold.as_deref() {
            None | Some("") => None,
            Some(letter) => match parse_pieces(letter)?.as_slice() {
                [piece] => Some(*piece),
                _ => return Err(OpenerBuildError::InvalidQueue),
            },
        };
        // No shape plans when the diagrams themselves stop every one; the
        // first then reports why, as the default order does.
        let node = if rest.is_empty() {
            first
        } else {
            choose::choose(input, &queue, hold, &mut cache).unwrap_or(first)
        };
        (node, queue, hold)
    };
    let mut target =
        match target_with_clears(&input.tree, input.mirrored, input.start_node, target_node) {
            Ok(target) => target,
            Err(TargetError::SourceMismatch(node_id)) => {
                return Ok(unplannable(
                    input,
                    target_node,
                    BuildReason::SourceMismatch { node_id },
                ));
            }
            Err(TargetError::TooTall(node_id)) => {
                return Ok(unplannable(
                    input,
                    target_node,
                    BuildReason::TooLong { node_id },
                ));
            }
            Err(TargetError::UnknownNode(id)) => return Err(OpenerBuildError::UnknownNode(id)),
            Err(TargetError::StartNotAncestor) => return Err(OpenerBuildError::StartNotAncestor),
            Err(TargetError::NothingToBuild) => return Err(OpenerBuildError::NothingToBuild),
            Err(TargetError::TooManyPieces) => {
                return Ok(unplannable(
                    input,
                    target_node,
                    BuildReason::TooLong {
                        node_id: target_node,
                    },
                ));
            }
        };
    let outline = target.open_outline_cells();
    if outline > MAX_OUTLINE_CELLS {
        return Ok(unplannable(
            input,
            target_node,
            BuildReason::OutlineOnly { cells: outline },
        ));
    }
    if let Some(exhausted) =
        no_default_order.filter(|_| target.open_outline_cells() > SMALL_OUTLINE_CELLS)
    {
        // Explaining why a large outline has no order costs tens of seconds;
        // the empty queue leaves nothing else for the queue search to place.
        let mut plan = new_plan(input, target_node, &target, &queue, hold);
        if exhausted {
            plan.reasons.push(BuildReason::SearchLimit);
        } else {
            plan.status = BuildStatus::NeedsMorePieces;
        }
        return Ok(plan);
    }
    fit_clears_within(&mut target, BUILD_BUDGET, &mut cache);
    let mut plan = new_plan(input, target_node, &target, &queue, hold);
    let mut search =
        Search::new(&target, &queue, hold, input.hold_enabled, BUILD_BUDGET).with_cache(cache);
    match search.run() {
        Some(found) => replay::complete(&mut plan, &target, &found),
        None => explain::explain_failure(&mut plan, input, &target, &queue, hold, &mut search),
    }
    Ok(plan)
}

/// The shape with the cheapest default order and that order; ties keep the
/// earlier shape. Also whether some shape's search ran out of budget.
fn best_default_order(
    input: &OpenerBuildInput,
    cache: &mut PlacementCache,
) -> (Option<(u32, BagOrder)>, bool) {
    let mut best: Option<(u32, BagOrder)> = None;
    let mut exhausted = false;
    let mut budget = BAG_BUDGET;
    for &node in &input.targets {
        if budget == 0 {
            break;
        }
        let Ok(mut target) =
            target_with_clears(&input.tree, input.mirrored, input.start_node, node)
        else {
            continue;
        };
        if target.open_outline_cells() > MAX_OUTLINE_CELLS {
            continue;
        }
        fit_clears_within(&mut target, ALTERNATIVE_BUDGET, cache);
        let mut search = Search::new(&target, &[], None, input.hold_enabled, 0)
            .with_cache(std::mem::take(cache));
        let mut shape_budget = if target.open_outline_cells() > SMALL_OUTLINE_CELLS {
            budget.min(OUTLINE_BAG_BUDGET)
        } else {
            budget
        };
        let granted = shape_budget;
        let order = default_order(&mut search, input.hold_enabled, &mut shape_budget);
        budget -= granted - shape_budget;
        exhausted |= order.is_none() && shape_budget == 0;
        *cache = search.into_cache();
        if let Some(order) = order {
            if best
                .as_ref()
                .is_none_or(|(_, known)| order.cost() < known.cost())
            {
                best = Some((node, order));
            }
        }
    }
    (best, exhausted)
}

/// The plan before any search: the boards, stages and clears of the target.
fn new_plan(
    input: &OpenerBuildInput,
    target_node: u32,
    target: &BuildTarget,
    queue: &[Piece],
    hold: Option<Piece>,
) -> OpenerBuildPlan {
    OpenerBuildPlan {
        status: BuildStatus::Undetermined,
        mirrored: input.mirrored,
        start_node: input.start_node,
        target_node,
        queue: queue.iter().copied().map(letter_string).collect(),
        start_board: board_rows(&target.start, &target.letters),
        target_board: board_rows(&target.goal, &target.letters),
        stages: target
            .stages
            .iter()
            .map(|stage| BuildStage {
                node_id: stage.node_id,
                pieces: stage.pieces,
            })
            .collect(),
        clears: planned_clears(target),
        steps: Vec::new(),
        attack: 0.0,
        start_hold: hold.map(letter_string),
        end_hold: hold.map(letter_string),
        trailing_hold: false,
        queue_used: 0,
        remaining: explain::remaining(target, &target.start),
        reasons: Vec::new(),
        stuck: None,
    }
}

/// The plan for a path the catalog's diagrams cannot be lifted along.
fn unplannable(input: &OpenerBuildInput, target_node: u32, reason: BuildReason) -> OpenerBuildPlan {
    OpenerBuildPlan {
        status: BuildStatus::Impossible,
        mirrored: input.mirrored,
        start_node: input.start_node,
        target_node,
        queue: String::new(),
        start_board: Vec::new(),
        target_board: Vec::new(),
        stages: Vec::new(),
        clears: Vec::new(),
        steps: Vec::new(),
        attack: 0.0,
        start_hold: None,
        end_hold: None,
        trailing_hold: false,
        queue_used: 0,
        remaining: BuildRemaining::default(),
        reasons: vec![reason],
        stuck: None,
    }
}

fn parse_pieces(letters: &str) -> Result<Vec<Piece>, OpenerBuildError> {
    letters
        .bytes()
        .map(|letter| {
            piece_for_letter(letter.to_ascii_uppercase()).ok_or(OpenerBuildError::InvalidQueue)
        })
        .collect()
}
