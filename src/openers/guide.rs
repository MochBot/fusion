//! The opener guide: a reference card for the opener a round built or came
//! nearest to. It shows the catalogued target shape per phase, the opener's
//! documented requirements, its follow-up variations, and the board where the
//! player's build first diverged from the catalogued route. It never shows or
//! implies a placement order; the catalog carries none.

use serde::{Deserialize, Serialize};

use crate::openers::board::{mirror_letter_row, mirror_piece_letter, strip_garbage_rows};
use crate::openers::catalog::{
    OpenerCatalog, OpenerLink, OpenerNodeEst, OpenerRecord, OpenerTreeNode,
};
use crate::openers::catalogued_match::RoundCataloguedBoardMatch;
use crate::openers::phase::OpenerObservation;
use crate::openers::recognition::round::{RoundHypothesis, RoundRecognition};
use crate::openers::segments::{derive_placements, floor_up_letters, ShowcasePlacement};

pub const GUIDE_VARIATION_LIMIT: usize = 6;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GuideBasis {
    Confirmed,
    Nearest,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideAliases {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbr: Option<String>,
    #[serde(default)]
    pub alt: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideRequirements {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_pct: Option<f64>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidePhase {
    pub node_id: u32,
    pub pieces: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_clear_rows: Option<Vec<String>>,
    #[serde(default)]
    pub clear_rows: Vec<u8>,
    pub rows: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub est: Option<OpenerNodeEst>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideVariation {
    pub node_id: u32,
    pub pieces: u32,
    pub rows: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_name: Option<String>,
    #[serde(default)]
    pub annotations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub est: Option<OpenerNodeEst>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideDeviation {
    pub divergence_lock: usize,
    pub lock_index: usize,
    pub player_rows: Vec<String>,
    pub target_rows: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_node_id: Option<u32>,
    pub missing: u32,
    pub stray: u32,
    pub wrong_letter: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerGuide {
    pub basis: GuideBasis,
    pub record_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_name: Option<String>,
    pub aliases: GuideAliases,
    pub mirrored: bool,
    pub phases: Vec<GuidePhase>,
    pub requirements: GuideRequirements,
    pub variations: Vec<GuideVariation>,
    pub variations_total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deviation: Option<GuideDeviation>,
    pub links: Vec<OpenerLink>,
}

/// The record and node the guide is about, plus how it was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuideSubject {
    pub basis: GuideBasis,
    pub record_id: String,
    pub node_id: Option<u32>,
    pub mirrored: bool,
    /// The deepest lock confirmation accepted as exact; divergence is sought
    /// only after it.
    pub anchor_lock: Option<usize>,
}

/// Chooses the guide's subject from the walk's deepest surviving point.
///
/// A singleton exact match names the record when its recognition hypothesis is
/// still viable; that hypothesis supplies the authored node and mirror state.
/// Tied confirmations and nearest matches continue to follow the top hypothesis.
/// Naming is withheld while an unconfirmed tie spans different shapes, or while
/// the selected hypothesis has crossed an opaque identity boundary.
pub(crate) fn select_subject(
    catalog: &OpenerCatalog,
    matched: Option<&RoundCataloguedBoardMatch>,
    recognition: Option<&RoundRecognition>,
) -> Option<GuideSubject> {
    let recognition = recognition?;
    let top = recognition.hypotheses.first()?;
    let singleton_confirmed = matched.and_then(|matched| {
        let [opener] = matched.matching_openers.as_slice() else {
            return None;
        };
        recognition
            .hypotheses
            .iter()
            .find(|hypothesis| hypothesis.record == opener.id)
            .filter(|hypothesis| {
                hypothesis.total_cost == top.total_cost
                    && hypothesis.total_cost < recognition.unknown_cost
                    && !hypothesis.identity_frozen
            })
    });
    let selected = singleton_confirmed.unwrap_or(top);
    if selected.total_cost >= recognition.unknown_cost || selected.identity_frozen {
        return None;
    }
    if singleton_confirmed.is_none()
        && selected.margin == 0
        && !tied_hypotheses_share_shape(catalog, recognition, selected)
    {
        return None;
    }

    let confirmed = matched.and_then(|matched| {
        matched
            .matching_openers
            .iter()
            .find(|opener| opener.id == selected.record)
    });
    let node_id = match confirmed {
        Some(opener) => match opener.candidate_node_ids.as_slice() {
            [node_id] => Some(*node_id),
            node_ids => node_ids
                .iter()
                .copied()
                .find(|node_id| *node_id == selected.node_id)
                .or_else(|| deepest_candidate(catalog, &opener.id, node_ids))
                .or_else(|| node_ids.is_empty().then_some(selected.node_id)),
        },
        None => Some(selected.node_id),
    };
    Some(GuideSubject {
        basis: if confirmed.is_some() {
            GuideBasis::Confirmed
        } else {
            GuideBasis::Nearest
        },
        record_id: selected.record.clone(),
        node_id,
        mirrored: confirmed
            .and_then(|opener| opener.mirrored)
            .unwrap_or(selected.mirrored),
        anchor_lock: matched
            .filter(|_| confirmed.is_some())
            .map(|matched| matched.anchor_index),
    })
}

fn deepest_candidate(catalog: &OpenerCatalog, record_id: &str, node_ids: &[u32]) -> Option<u32> {
    let record = record_by_id(catalog, record_id)?;
    node_ids
        .iter()
        .filter_map(|node_id| node_by_id(record, *node_id))
        .max_by_key(|node| (node.pieces, std::cmp::Reverse(node.id)))
        .map(|node| node.id)
}

fn tied_hypotheses_share_shape(
    catalog: &OpenerCatalog,
    recognition: &RoundRecognition,
    top: &RoundHypothesis,
) -> bool {
    let Some(top_shape) =
        record_by_id(catalog, &top.record).map(|record| record.shape_key.as_str())
    else {
        return false;
    };
    recognition
        .hypotheses
        .iter()
        .filter(|hypothesis| hypothesis.total_cost == top.total_cost)
        .all(|hypothesis| {
            record_by_id(catalog, &hypothesis.record)
                .is_some_and(|record| record.shape_key == top_shape)
        })
}

pub(crate) fn build_guide(
    catalog: &OpenerCatalog,
    subject: &GuideSubject,
    observations: &[Option<OpenerObservation>],
    recognition: Option<&RoundRecognition>,
) -> Option<OpenerGuide> {
    let record = record_by_id(catalog, &subject.record_id)?;
    let anchor = subject
        .node_id
        .and_then(|node_id| node_by_id(record, node_id))
        .or_else(|| deepest_lettered_root_path_node(record))?;
    let path = path_to(record, anchor)?;
    let mirrored = subject.mirrored;

    let mut phases = path
        .iter()
        .map(|node| guide_phase(record, node, mirrored))
        .collect::<Vec<_>>();
    let rounded_phase = rounded_child_phase(record, anchor, subject, observations);
    if let Some(phase) = &rounded_phase {
        phases.push(phase.clone());
    }
    let phases = drop_superseded_phases(phases);

    let children: Vec<&OpenerTreeNode> = record
        .tree
        .iter()
        .filter(|node| node.parent == Some(anchor.id))
        .collect();
    let candidates = if children.is_empty() {
        record
            .tree
            .iter()
            .filter(|node| node.parent == anchor.parent && node.id != anchor.id)
            .collect()
    } else {
        children
    };
    let variations_total = candidates.len();
    let variations = candidates
        .iter()
        .take(GUIDE_VARIATION_LIMIT)
        .map(|node| GuideVariation {
            node_id: node.id,
            pieces: node.pieces,
            rows: floor_up_letters(&node.rows, mirrored),
            route_name: nonempty(node.route_name.as_deref()),
            annotations: node.annotations.clone(),
            est: node.est.clone(),
        })
        .collect();

    let route_name = rounded_phase
        .as_ref()
        .and_then(|phase| phase.route_name.clone())
        .or_else(|| {
            path.iter()
                .rev()
                .find_map(|node| nonempty(node.route_name.as_deref()))
                .filter(|route| !is_record_display_name(record, route))
        });
    let dependencies = nonempty(Some(record.dependencies.as_str())).map(|text| {
        if mirrored {
            promote_mirror_clause(&text)
        } else {
            text
        }
    });

    let deviation = if rounded_phase.is_some() {
        None
    } else {
        deviation(
            record,
            anchor,
            subject.basis,
            mirrored,
            observations,
            recognition,
            subject.anchor_lock,
        )
    };

    Some(OpenerGuide {
        basis: subject.basis,
        record_id: record.id.clone(),
        name: record.aliases.en.clone(),
        route_name,
        aliases: GuideAliases {
            jp: record.aliases.jp.clone(),
            abbr: record.aliases.abbr.clone(),
            alt: record.aliases.alt.clone(),
        },
        mirrored,
        phases,
        requirements: GuideRequirements {
            dependencies,
            cover_pct: record.cover.map(|cover| cover.pct),
            tags: record.tags.clone(),
        },
        variations,
        variations_total,
        deviation,
        links: unique_links(&record.links),
    })
}

fn guide_phase(record: &OpenerRecord, node: &OpenerTreeNode, mirrored: bool) -> GuidePhase {
    GuidePhase {
        node_id: node.id,
        pieces: node.pieces,
        pre_clear_rows: node
            .pre_clear_rows
            .as_deref()
            .map(|rows| guide_rows(rows, mirrored)),
        clear_rows: node
            .clear_rows
            .as_deref()
            .map(|rows| {
                rows.iter()
                    .copied()
                    .filter_map(|row| u8::try_from(row).ok())
                    .collect()
            })
            .unwrap_or_default(),
        rows: guide_rows(&node.rows, mirrored),
        route_name: nonempty(node.route_name.as_deref())
            .filter(|route| !is_record_display_name(record, route)),
        est: node.est.clone(),
    }
}

fn guide_rows(rows_top_down: &[String], mirrored: bool) -> Vec<String> {
    floor_up_letters(rows_top_down, mirrored)
}

/// Where the player's build left the catalogued route, compared at the
/// locked-piece ordinal of the shape they were building toward.
///
/// `divergence_lock` is the first lock past the confirmed anchor whose
/// alignment paid a non-zero cost; a divergence past the opener phase window
/// is ordinary play, not an opener lesson, and yields no deviation. The compared board is the player's board
/// at the ordinal where the intended phase completes (its pre-clear frame
/// when the phase clears), so both boards hold the same number of placed
/// pieces. For a confirmed opener the intended phase is the anchor's
/// continuation that best overlaps the player's board; a finished opener with
/// no catalogued continuation has no deviation. For a nearest opener it is
/// the phase in progress at the divergence.
fn deviation(
    record: &OpenerRecord,
    anchor: &OpenerTreeNode,
    basis: GuideBasis,
    mirrored: bool,
    observations: &[Option<OpenerObservation>],
    recognition: Option<&RoundRecognition>,
    anchor_lock: Option<usize>,
) -> Option<GuideDeviation> {
    let recognition = recognition?;
    let present_locks = observations
        .iter()
        .enumerate()
        .filter(|(_, observation)| observation.is_some());
    let divergence_lock = recognition
        .per_lock
        .iter()
        .zip(present_locks)
        .filter(|(_, (index, _))| anchor_lock.is_none_or(|anchor| *index > anchor))
        .find(|(lock, _)| lock.best_cost.is_some_and(|cost| cost > 0))
        .map(|(_, (index, _))| index)?;

    let candidates: Vec<&OpenerTreeNode> = match basis {
        GuideBasis::Confirmed => record
            .tree
            .iter()
            .filter(|node| node.parent == Some(anchor.id) && !node.grey)
            .collect(),
        GuideBasis::Nearest => path_to(record, anchor)?
            .into_iter()
            .filter(|node| !node.grey)
            .filter(|node| node.pieces as usize > divergence_lock)
            .take(1)
            .collect(),
    };
    if candidates.is_empty() {
        return None;
    }

    struct Compared {
        lock_index: usize,
        overlap: usize,
        node_id: u32,
        player_rows: Vec<String>,
        target_rows: Vec<String>,
    }
    let mut best: Option<Compared> = None;
    for node in candidates {
        let lock_index = (node.pieces as usize).saturating_sub(1);
        let Some(Some(observation)) = observations.get(lock_index) else {
            continue;
        };
        let (Some(board), Some(garbage_mask)) = (
            observation.post_board.as_deref(),
            observation.post_gmask.as_deref(),
        ) else {
            continue;
        };
        let stripped = strip_garbage_rows(board, garbage_mask, observation.post_letters.as_deref());
        let player_rows = letter_rows(&stripped.masks, stripped.letters.as_deref());
        let target_rows = floor_up_letters(
            node.pre_clear_rows.as_deref().unwrap_or(&node.rows),
            mirrored,
        );
        let overlap = target_rows
            .iter()
            .zip(&player_rows)
            .flat_map(|(target, player)| target.bytes().zip(player.bytes()))
            .filter(|(target, player)| *target != b'_' && *player != b'_')
            .map(|(target, player)| {
                if target == player || target == b'X' || player == b'X' {
                    2
                } else {
                    1
                }
            })
            .sum::<usize>();
        if best.as_ref().is_none_or(|best| overlap > best.overlap) {
            best = Some(Compared {
                lock_index,
                overlap,
                node_id: node.id,
                player_rows,
                target_rows,
            });
        }
    }
    let Compared {
        lock_index,
        node_id: target_node_id,
        player_rows,
        target_rows,
        ..
    } = best?;
    let mut missing = 0;
    let mut stray = 0;
    let mut wrong_letter = 0;
    let height = player_rows.len().max(target_rows.len());
    for y in 0..height {
        let player = player_rows.get(y).map_or("__________", String::as_str);
        let expected = target_rows.get(y).map_or("__________", String::as_str);
        for (mine, theirs) in player.bytes().zip(expected.bytes()) {
            match (mine != b'_', theirs != b'_') {
                (false, true) => missing += 1,
                (true, false) => stray += 1,
                (true, true) if theirs != b'X' && mine != b'X' && mine != theirs => {
                    wrong_letter += 1
                }
                _ => {}
            }
        }
    }
    if missing + stray + wrong_letter == 0 {
        return None;
    }
    Some(GuideDeviation {
        divergence_lock,
        lock_index,
        player_rows,
        target_rows,
        target_node_id: Some(target_node_id),
        missing,
        stray,
        wrong_letter,
    })
}

fn rounded_child_phase(
    record: &OpenerRecord,
    anchor: &OpenerTreeNode,
    subject: &GuideSubject,
    observations: &[Option<OpenerObservation>],
) -> Option<GuidePhase> {
    if subject.basis != GuideBasis::Confirmed {
        return None;
    }
    let anchor_lock = subject.anchor_lock?;
    let mut best: Option<(&OpenerTreeNode, usize)> = None;
    for child in record
        .tree
        .iter()
        .filter(|child| child.parent == Some(anchor.id) && !child.grey)
    {
        let placements = match &child.placements {
            Some(placements) => Some(
                placements
                    .iter()
                    .map(|placement| ShowcasePlacement {
                        letter: placement.letter.clone(),
                        cells: placement.cells.clone(),
                    })
                    .collect::<Vec<_>>(),
            ),
            None => {
                let child_rows = match child.pre_clear_rows.as_deref() {
                    Some(rows) => rows,
                    None => &child.rows,
                };
                derive_placements(&anchor.rows, child_rows).map(|derived| derived.placements)
            }
        };
        let Some(placements) = placements else {
            continue;
        };
        let player_rows = observations
            .iter()
            .enumerate()
            .filter(|(lock, _)| {
                *lock > anchor_lock && u32::try_from(*lock).is_ok_and(|lock| lock < child.pieces)
            })
            .filter_map(|(_, observation)| {
                let observation = observation.as_ref()?;
                let board = observation.post_board.as_deref()?;
                let garbage_mask = observation.post_gmask.as_deref()?;
                let stripped =
                    strip_garbage_rows(board, garbage_mask, observation.post_letters.as_deref());
                Some(letter_rows(&stripped.masks, stripped.letters.as_deref()))
            })
            .collect::<Vec<_>>();
        let matched_placements = placements
            .iter()
            .filter(|placement| {
                let mut letters = placement.letter.chars();
                let Some(letter) = letters.next() else {
                    return false;
                };
                if letters.next().is_some() {
                    return false;
                }
                let letter = if subject.mirrored {
                    mirror_piece_letter(letter)
                } else {
                    letter
                };
                player_rows.iter().any(|rows| {
                    placement.cells.iter().all(|[x, y]| {
                        let x = if subject.mirrored { 9 - *x } else { *x };
                        rows.get(usize::from(*y))
                            .and_then(|row| row.chars().nth(usize::from(x)))
                            .is_some_and(|observed| observed == letter || observed == 'X')
                    })
                })
            })
            .count();
        if matched_placements > 0
            && best.is_none_or(|(best_child, best_matches)| {
                matched_placements > best_matches
                    || (matched_placements == best_matches && child.id < best_child.id)
            })
        {
            best = Some((child, matched_placements));
        }
    }
    best.map(|(child, _)| guide_phase(record, child, subject.mirrored))
}

fn letter_rows(masks: &[u16], letters: Option<&[String]>) -> Vec<String> {
    masks
        .iter()
        .enumerate()
        .map(|(y, mask)| {
            (0..10)
                .map(|x| {
                    if mask & (1 << x) == 0 {
                        '_'
                    } else {
                        letters
                            .and_then(|rows| rows.get(y))
                            .and_then(|row| row.as_bytes().get(x))
                            .map_or('X', |letter| {
                                if *letter == b'_' {
                                    'X'
                                } else {
                                    *letter as char
                                }
                            })
                    }
                })
                .collect()
        })
        .collect()
}

/// Dependency text documents the canonical chirality and parenthesises the
/// mirror form, e.g. `L/I>S + J>Z (J/I>Z + L>S for mirror)`. When the player
/// built the mirror, the mirror clause is the one that applies.
fn promote_mirror_clause(text: &str) -> String {
    let Some(open) = text.rfind('(') else {
        return mirror_letter_row(text);
    };
    let Some(close) = text[open..].find(')') else {
        return mirror_letter_row(text);
    };
    let inner = text[open + 1..open + close].trim();
    let inner = inner
        .strip_suffix("for mirror")
        .map(str::trim)
        .unwrap_or(inner);
    if inner.is_empty() {
        return mirror_letter_row(text);
    }
    inner.to_owned()
}

/// Ingest names a record's root node after the record itself in some sources;
/// that is the opener's name, not a route within it.
/// Only the names rendered beside the route count as redundant. `alt` is a
/// synonym list that also carries this record's variation names, so matching
/// against it suppresses the very routes the guide exists to show.
fn is_record_display_name(record: &OpenerRecord, name: &str) -> bool {
    let aliases = &record.aliases;
    std::iter::once(aliases.en.as_str())
        .chain(aliases.jp.as_deref())
        .chain(aliases.abbr.as_deref())
        .any(|alias| alias.eq_ignore_ascii_case(name))
}

/// An intermediate clearless phase is redrawn as the next phase's pre-clear
/// board, so showing it repeats the same stack twice. The first phase always
/// stays: it establishes the shape the opener starts from.
fn drop_superseded_phases(phases: Vec<GuidePhase>) -> Vec<GuidePhase> {
    let superseded = |index: usize| {
        index > 0
            && phases[index].clear_rows.is_empty()
            && phases
                .get(index + 1)
                .is_some_and(|next| next.pre_clear_rows.is_some())
    };
    phases
        .iter()
        .enumerate()
        .filter(|(index, _)| !superseded(*index))
        .map(|(_, phase)| phase.clone())
        .collect()
}

/// Records may cite one URL under several labels; consumers key their source
/// list by URL, so duplicates must not reach the DTO.
fn unique_links(links: &[OpenerLink]) -> Vec<OpenerLink> {
    let mut seen = std::collections::HashSet::new();
    links
        .iter()
        .filter(|link| seen.insert(link.url.as_str()))
        .cloned()
        .collect()
}

fn nonempty(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn record_by_id<'a>(catalog: &'a OpenerCatalog, id: &str) -> Option<&'a OpenerRecord> {
    catalog.openers.iter().find(|record| record.id == id)
}

fn node_by_id(record: &OpenerRecord, node_id: u32) -> Option<&OpenerTreeNode> {
    record.tree.iter().find(|node| node.id == node_id)
}

fn deepest_lettered_root_path_node(record: &OpenerRecord) -> Option<&OpenerTreeNode> {
    let mut best: Option<&OpenerTreeNode> = None;
    for node in &record.tree {
        if node.grey {
            continue;
        }
        let Some(path) = path_to(record, node) else {
            continue;
        };
        if path.iter().any(|ancestor| ancestor.grey) {
            continue;
        }
        if best.is_none_or(|best| {
            node.pieces > best.pieces || (node.pieces == best.pieces && node.id < best.id)
        }) {
            best = Some(node);
        }
    }
    best.or_else(|| record.tree.iter().find(|node| node.parent.is_none()))
}

fn path_to<'a>(
    record: &'a OpenerRecord,
    node: &'a OpenerTreeNode,
) -> Option<Vec<&'a OpenerTreeNode>> {
    let mut path = vec![node];
    let mut current = node;
    while let Some(parent_id) = current.parent {
        let parent = node_by_id(record, parent_id)?;
        path.push(parent);
        current = parent;
    }
    path.reverse();
    Some(path)
}
