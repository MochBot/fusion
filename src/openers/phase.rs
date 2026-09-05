use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::openers::board::{cell_count, strip_garbage_rows};
use crate::openers::matcher::{match_board, BoardMatch};
use crate::openers::target::ShapeTarget;

pub(crate) const OPENER_PHASE_LOCKS: usize = 14;
const MATCH_LIMIT: usize = 8;
const GREY_MIN_PROGRESS: f64 = 0.85;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerObservation {
    pub post_board: Option<Vec<u16>>,
    pub post_gmask: Option<Vec<u16>>,
    pub post_letters: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenerAssessment {
    pub on_script: bool,
    pub board_cells: u32,
    pub r#match: Option<BoardMatch>,
    pub runners_up: Vec<BoardMatch>,
}

pub(crate) fn assess_opener_phase(
    targets: &[ShapeTarget],
    observations: &[Option<OpenerObservation>],
) -> Vec<Option<OpenerAssessment>> {
    let mut assessments: Vec<Option<OpenerAssessment>> = Vec::with_capacity(observations.len());
    let mut on_script_chain = true;

    for (index, observation) in observations.iter().enumerate() {
        if index >= OPENER_PHASE_LOCKS && !on_script_chain {
            assessments.push(None);
            continue;
        }

        let Some(observation) = observation else {
            assessments.push(None);
            continue;
        };
        let (Some(post_board), Some(post_gmask)) = (
            observation.post_board.as_deref(),
            observation.post_gmask.as_deref(),
        ) else {
            assessments.push(None);
            continue;
        };

        let stripped =
            strip_garbage_rows(post_board, post_gmask, observation.post_letters.as_deref());
        let board_cells = cell_count(&stripped.masks);
        if board_cells == 0 {
            assessments.push(Some(OpenerAssessment {
                on_script: true,
                board_cells,
                r#match: None,
                runners_up: Vec::new(),
            }));
            continue;
        }

        let placed_cells = i128::try_from(index)
            .map_or(i128::MAX, |index| index.saturating_add(1).saturating_mul(4));
        let deficit = placed_cells - i128::from(board_cells);
        let plausible_stack = deficit >= 0
            && deficit % 10 == 0
            && (deficit != 0
                || match stripped.letters.as_deref() {
                    Some(letters) => whole_piece_letters(letters),
                    None => true,
                });
        let matches = match_board(
            targets,
            &stripped.masks,
            MATCH_LIMIT,
            stripped.letters.as_deref(),
        );
        let qualifying = matches.iter().position(|board_match| {
            board_match.stray_cells == 0
                && (board_match.colored
                    || board_match.complete
                    || board_match.progress >= GREY_MIN_PROGRESS)
        });
        let best_index = qualifying.or_else(|| (!matches.is_empty()).then_some(0));
        let best = best_index.map(|match_index| matches[match_index].clone());
        let runners_up = matches
            .into_iter()
            .enumerate()
            .filter_map(|(match_index, board_match)| {
                (Some(match_index) != best_index).then_some(board_match)
            })
            .take(2)
            .collect();

        on_script_chain = plausible_stack && qualifying.is_some();
        assessments.push(Some(OpenerAssessment {
            on_script: on_script_chain,
            board_cells,
            r#match: best,
            runners_up,
        }));
    }

    assessments
}

fn whole_piece_letters(letters: &[String]) -> bool {
    let mut counts = HashMap::new();
    for letter in letters.iter().flat_map(|row| row.chars()) {
        if letter != '_' {
            *counts.entry(letter).or_insert(0_u32) += 1;
        }
    }

    counts.contains_key(&'X') || counts.values().all(|count| count % 4 == 0)
}
