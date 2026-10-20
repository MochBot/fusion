//! Required clears: parsing a stage's catalog label, lowering labels no order
//! can perform, the rule that holds placements to them, and their names.

use std::ops::Range;

use crate::board::{BOARD_HEIGHT, FULL_ROW};
use crate::header::{Piece, SpinType};
use crate::openers::catalog::OpenerTreeNode;

use super::frame::{
    completed_rows, letter_for_piece, letter_string, mirror_letter, piece_for_letter, Masks,
};
use super::moves::{Candidate, PlacementCache};
use super::search::Search;
use super::target::{build_target, BuildTarget, Slot, TargetError};
#[cfg(test)]
use super::BUILD_BUDGET;
use super::{PlannedClear, SpinName};

/// One clear an opener is built for: these rows clear together, with at
/// least this spin; a spin clear is performed by its spin slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RequiredClear {
    pub rows: Vec<u8>,
    pub piece: Option<Piece>,
    /// The slot whose piece performs a spin clear.
    pub spin_slot: Option<usize>,
    pub min_spin: SpinType,
}

impl RequiredClear {
    /// One step weaker: a full T-spin may be a mini, and a mini need not
    /// spin, as long as the same rows still clear together.
    pub(super) fn relaxed(&self) -> Option<Self> {
        match self.min_spin {
            SpinType::Full => Some(Self {
                min_spin: SpinType::Mini,
                ..self.clone()
            }),
            SpinType::Mini => Some(Self {
                piece: None,
                spin_slot: None,
                min_spin: SpinType::NoSpin,
                ..self.clone()
            }),
            SpinType::NoSpin => None,
        }
    }
}

/// The catalogued path lifted into the frame, each stage's required clear
/// parsed from its label.
pub(super) fn target_with_clears(
    tree: &[OpenerTreeNode],
    mirrored: bool,
    start: Option<u32>,
    target: u32,
) -> Result<BuildTarget, TargetError> {
    let mut target = build_target(tree, mirrored, start, target)?;
    let BuildTarget { stages, slots, .. } = &mut target;
    for stage in stages {
        stage.clear = stage.label.as_deref().and_then(|label| {
            required_clear(label, mirrored, &stage.cleared, slots, stage.slots.clone())
        });
    }
    Ok(target)
}

/// The clear a catalogued label names, checked against the rows the stage
/// clears. A spin clear's spin slot is the stage's slot of that letter
/// spanning every cleared row. Non-T spins are all-spin minis under the
/// engine's rules, and a static diagram cannot tell a T-spin mini from a
/// kick-upgraded full, so only a full T-spin label demands a full spin.
fn required_clear(
    label: &str,
    mirrored: bool,
    cleared: &[u8],
    slots: &[Slot],
    stage_slots: Range<usize>,
) -> Option<RequiredClear> {
    let words = label.split(' ').collect::<Vec<_>>();
    let (mini, rest) = match words.as_slice() {
        ["mini", rest @ ..] => (true, rest),
        rest => (false, rest),
    };
    let (spin_letter, rest) = match rest {
        [spin, rest @ ..] if spin.ends_with("-spin") => {
            let [letter] = spin.trim_end_matches("-spin").as_bytes() else {
                return None;
            };
            let letter = if mirrored {
                mirror_letter(*letter)
            } else {
                *letter
            };
            (Some(piece_for_letter(letter)?), rest)
        }
        rest => (None, rest),
    };
    let lines = match rest {
        ["single", ..] => 1,
        ["double", ..] => 2,
        ["triple", ..] => 3,
        ["quad", ..] => 4,
        _ => return None,
    };
    if lines != cleared.len() {
        return None;
    }
    let Some(piece) = spin_letter else {
        return Some(RequiredClear {
            rows: cleared.to_vec(),
            piece: None,
            spin_slot: None,
            min_spin: SpinType::NoSpin,
        });
    };
    let spin_slot = stage_slots
        .filter(|index| {
            let slot = &slots[*index];
            slot.piece == piece
                && cleared
                    .iter()
                    .all(|row| slot.cells.iter().any(|[_, y]| y == row))
        })
        .max_by_key(|index| {
            slots[*index]
                .cells
                .iter()
                .filter(|[_, y]| cleared.contains(y))
                .count()
        })?;
    Some(RequiredClear {
        rows: cleared.to_vec(),
        piece: Some(piece),
        spin_slot: Some(spin_slot),
        min_spin: if piece == Piece::T && !mini {
            SpinType::Full
        } else {
            SpinType::Mini
        },
    })
}

/// Catalogued clear labels are inferred from static diagrams, which cannot
/// show whether a piece spun in. Holds each clear, in order, to the most its
/// diagram allows under SRS+: when no order of any pieces performs a clear as
/// labelled but the shape can be built, the requirement steps down (full
/// T-spin, then mini, then the same rows clearing together) and the stage is
/// marked relaxed.
#[cfg(test)]
pub(super) fn fit_clears(target: &mut BuildTarget) {
    fit_clears_within(target, BUILD_BUDGET, &mut PlacementCache::default());
}

/// `fit_clears` with its own expansion budget per check, sharing a request's
/// placement lookups; a check that runs out keeps the label as catalogued.
pub(super) fn fit_clears_within(target: &mut BuildTarget, budget: u32, cache: &mut PlacementCache) {
    let mut shape = Search::new(target, &[], None, false, budget)
        .with_cache(std::mem::take(cache))
        .shape_only();
    let buildable = shape.any_order_buildable();
    *cache = shape.into_cache();
    if !buildable {
        return;
    }
    for stage in 0..target.stages.len() {
        while target.stages[stage].clear.is_some() {
            let mut search = Search::new(target, &[], None, false, budget)
                .with_cache(std::mem::take(cache))
                .requiring_clears_before(stage + 1);
            let settled = search.any_order_buildable() || search.exhausted;
            *cache = search.into_cache();
            if settled {
                break;
            }
            let lowered = target.stages[stage]
                .clear
                .as_ref()
                .and_then(RequiredClear::relaxed);
            target.stages[stage].clear = lowered;
            target.stages[stage].relaxed = true;
        }
    }
}

/// Why a legal placement would break a required clear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClearBreak {
    /// The spin piece would lock before the rest of its rows is filled, so
    /// it would not clear them.
    TooEarly,
    /// It clears its rows, but no path from spawn gives it the spin.
    NoSpin,
    /// It would clear catalogued rows other than as the opener clears them.
    WrongClear,
    /// It would clear rows before an earlier catalogued clear.
    OutOfOrder,
}

/// Holds placements to the target's required clears.
pub(super) struct ClearRule<'a> {
    target: &'a BuildTarget,
    /// The stage whose required clear each slot performs as its spin slot.
    spin_slot_stage: Vec<Option<usize>>,
    /// The stage that clears each frame row.
    row_stage: [Option<usize>; BOARD_HEIGHT],
    /// Plans the shape without required clears.
    shape_only: bool,
    /// Only stages before this index are held to their clears.
    limit: usize,
}

impl<'a> ClearRule<'a> {
    pub(super) fn new(target: &'a BuildTarget) -> Self {
        let mut spin_slot_stage = vec![None; target.slots.len()];
        let mut row_stage = [None; BOARD_HEIGHT];
        for (index, stage) in target.stages.iter().enumerate() {
            if let Some(slot) = stage.clear.as_ref().and_then(|clear| clear.spin_slot) {
                spin_slot_stage[slot] = Some(index);
            }
            for row in &stage.cleared {
                row_stage[usize::from(*row)] = Some(index);
            }
        }
        Self {
            target,
            spin_slot_stage,
            row_stage,
            shape_only: false,
            limit: target.stages.len(),
        }
    }

    pub(super) fn shape_only(&mut self) {
        self.shape_only = true;
    }

    /// Holds only the stages before `limit` to their clears.
    pub(super) fn require_before(&mut self, limit: usize) {
        self.limit = limit;
    }

    fn required(&self, stage: usize) -> bool {
        !self.shape_only && stage < self.limit
    }

    /// The least spin a placement of `slot` must carry: set when the slot is
    /// the spin slot of a stage held to its clear.
    pub(super) fn min_spin(&self, slot: usize) -> Option<SpinType> {
        self.spin_slot_stage[slot]
            .filter(|stage| self.required(*stage))
            .and_then(|stage| self.target.stages[stage].clear.as_ref())
            .map(|clear| clear.min_spin)
    }

    /// Why placing `candidate` on `filled` would break a required clear.
    pub(super) fn check(&self, filled: &Masks, candidate: &Candidate) -> Option<ClearBreak> {
        if self.shape_only {
            return None;
        }
        let completed = completed_rows(filled, &candidate.cells);
        let spin = candidate.target.spin() as u8;
        if let Some(stage) = candidate
            .slot
            .and_then(|slot| self.spin_slot_stage[slot])
            .filter(|stage| self.required(*stage))
        {
            let clear = self.target.stages[stage].clear.as_ref()?;
            if completed != clear.rows {
                return Some(ClearBreak::TooEarly);
            }
            if spin < clear.min_spin as u8 {
                return Some(ClearBreak::NoSpin);
            }
        }
        for row in &completed {
            let Some(stage) =
                self.row_stage[usize::from(*row)].filter(|stage| self.required(*stage))
            else {
                continue;
            };
            let earlier_open = self.target.stages[..stage].iter().any(|earlier| {
                earlier
                    .cleared
                    .iter()
                    .any(|row| filled[usize::from(*row)] != FULL_ROW && !completed.contains(row))
            });
            if earlier_open {
                return Some(ClearBreak::OutOfOrder);
            }
            if let Some(clear) = &self.target.stages[stage].clear {
                if completed != clear.rows
                    || (clear.spin_slot.is_some() && candidate.slot != clear.spin_slot)
                {
                    return Some(ClearBreak::WrongClear);
                }
                if spin < clear.min_spin as u8 {
                    return Some(ClearBreak::NoSpin);
                }
            }
        }
        None
    }

    /// The first catalogued clear not performed on `filled`.
    pub(super) fn next_clear(&self, filled: &Masks) -> Option<usize> {
        self.target.stages.iter().position(|stage| {
            stage.label.is_some()
                && stage
                    .cleared
                    .iter()
                    .any(|row| filled[usize::from(*row)] != FULL_ROW)
        })
    }

    /// The catalogued clear a placement concerns: the one it performs as the
    /// spin slot, else the one whose rows it would complete.
    pub(super) fn clear_concerned(&self, filled: &Masks, candidate: &Candidate) -> Option<String> {
        if let Some(label) = candidate.slot.and_then(|slot| self.spin_slot_label(slot)) {
            return Some(label);
        }
        completed_rows(filled, &candidate.cells)
            .into_iter()
            .find_map(|row| self.row_stage[usize::from(row)])
            .or_else(|| self.next_clear(filled))
            .and_then(|stage| self.target.stages[stage].label.clone())
    }

    /// The label of the catalogued clear a slot performs as its spin slot.
    pub(super) fn spin_slot_label(&self, slot: usize) -> Option<String> {
        self.spin_slot_stage
            .get(slot)
            .copied()
            .flatten()
            .and_then(|stage| self.target.stages[stage].label.clone())
    }
}

/// The clears the path lists, as the wire reports them.
pub(super) fn planned_clears(target: &BuildTarget) -> Vec<PlannedClear> {
    target
        .stages
        .iter()
        .filter(|stage| !stage.cleared.is_empty())
        .map(|stage| PlannedClear {
            node_id: stage.node_id,
            label: stage.label.clone(),
            lines: stage.cleared.len() as u8,
            piece: stage
                .clear
                .as_ref()
                .and_then(|clear| clear.piece)
                .map(letter_string),
            spin: stage
                .clear
                .as_ref()
                .map_or(SpinName::None, |clear| spin_name(clear.min_spin)),
            enforced: stage.clear.is_some(),
            relaxed: stage.relaxed,
        })
        .collect()
}

/// The clear's name in the catalog's vocabulary, e.g. `T-spin double` or
/// `mini T-spin single`; non-T spins are all-spins.
pub(super) fn clear_label(piece: Piece, spin: SpinType, lines: u8, pc: bool) -> String {
    let name = match lines {
        1 => "single",
        2 => "double",
        3 => "triple",
        4 => "quad",
        _ => "penta",
    };
    let letter = char::from(letter_for_piece(piece));
    let mut label = match (piece, spin) {
        (_, SpinType::NoSpin) => name.to_owned(),
        (Piece::T, SpinType::Mini) => format!("mini T-spin {name}"),
        _ => format!("{letter}-spin {name}"),
    };
    if pc {
        label.push_str(" PC");
    }
    label
}

pub(super) const fn spin_name(spin: SpinType) -> SpinName {
    match spin {
        SpinType::NoSpin => SpinName::None,
        SpinType::Mini => SpinName::Mini,
        SpinType::Full => SpinName::Full,
    }
}
