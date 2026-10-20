//! Picks which shape of one opener version to build from a queue. Every
//! shape performs the same clears in the same order, so the choice never
//! changes what the player gets, only which diagram the pieces fill.

use crate::board::FULL_ROW;
use crate::header::Piece;

use super::clears::{fit_clears_within, target_with_clears};
use super::frame::Masks;
use super::moves::PlacementCache;
use super::search::{filled_after, Ending, Search};
use super::target::{BuildTarget, MAX_OUTLINE_CELLS};
use super::OpenerBuildInput;

/// Expansion budget of each check while choosing. The chosen shape is
/// planned again with the full budget, so a shape this budget cannot settle
/// only loses the choice.
const CHOOSE_BUDGET: u32 = 4_000;
/// Budget of each label check while choosing; labels a check cannot settle
/// stay as catalogued, which at worst ranks that shape lower.
const LABEL_CHECK_BUDGET: u32 = 800;

/// How far a queue takes one shape. Ordered so the better outcome is
/// greater: built, then more clears landed, then more pieces placed, then a
/// finished board over one that still needs pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Reach {
    built: bool,
    clears: usize,
    placed: usize,
    finished: bool,
}

/// The request's shape the queue takes furthest; ties keep the earlier one.
pub(super) fn choose(
    input: &OpenerBuildInput,
    queue: &[Piece],
    hold: Option<Piece>,
    cache: &mut PlacementCache,
) -> Option<u32> {
    let mut best: Option<(Reach, u32)> = None;
    for &node in &input.targets {
        let Ok(mut target) =
            target_with_clears(&input.tree, input.mirrored, input.start_node, node)
        else {
            continue;
        };
        if target.open_outline_cells() > MAX_OUTLINE_CELLS {
            continue;
        }
        fit_clears_within(&mut target, LABEL_CHECK_BUDGET, cache);
        let reach = reach(&target, queue, hold, input.hold_enabled, cache);
        if best.is_none_or(|(known, _)| reach > known) {
            best = Some((reach, node));
        }
    }
    best.map(|(_, node)| node)
}

fn reach(
    target: &BuildTarget,
    queue: &[Piece],
    hold: Option<Piece>,
    hold_enabled: bool,
    cache: &mut PlacementCache,
) -> Reach {
    let mut search = Search::new(target, queue, hold, hold_enabled, CHOOSE_BUDGET)
        .with_cache(std::mem::take(cache));
    let reached = match search.run() {
        Some(plan) => Reach {
            built: true,
            clears: clears_landed(target, &filled_after(target.start, &plan.steps)),
            placed: plan.steps.len(),
            finished: plan.ending == Ending::Complete,
        },
        None => {
            let (placed, filled) = search.deepest().map_or((0, target.start), |deepest| {
                (deepest.steps.len(), deepest.filled)
            });
            Reach {
                built: false,
                clears: clears_landed(target, &filled),
                placed,
                finished: false,
            }
        }
    };
    *cache = search.into_cache();
    reached
}

/// Catalogued clears whose rows are all cleared on `filled`; the search only
/// clears them as the catalog requires.
fn clears_landed(target: &BuildTarget, filled: &Masks) -> usize {
    target
        .stages
        .iter()
        .filter(|stage| {
            !stage.cleared.is_empty()
                && stage
                    .cleared
                    .iter()
                    .all(|row| filled[usize::from(*row)] == FULL_ROW)
        })
        .count()
}
