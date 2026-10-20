use serde::{Deserialize, Serialize};

use crate::openers::analyze::{AnalyzeError, OpenerRoundInput};
use crate::openers::board::OPENER_BOARD_ROW_MASK;
use crate::openers::catalog::{installed_opener_catalog, OpenerCatalog};
use crate::openers::phase::{prepare_observations, PreparedObservation};

/// Rows above the residue whose six stack columns are full and whose well is
/// empty: the combo a player has banked. At six, rounds of players who never
/// play 4-wide give one false hit in 9,700.
const FOUR_WIDE_MIN_OPEN_ROWS: usize = 6;
const FOUR_WIDE_MAX_RESIDUE_ROWS: usize = 3;
/// 3 and 6 residue cells are the usual 4-wide setups; other residues exist.
const FOUR_WIDE_MAX_RESIDUE_CELLS: u32 = 8;

/// A catalog record recognized by its board shape alone, built with any piece
/// order and over any number of locks, instead of by a catalogued route.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub(crate) enum ShapeFamily {
    #[serde(rename = "4-wide")]
    FourWide,
}

/// The four empty columns of a 4-wide well.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Well {
    /// Columns 0-3.
    Left,
    /// Columns 3-6.
    Center,
    /// Columns 6-9.
    Right,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeFamilyMatch {
    pub opener_id: String,
    pub name: String,
    /// The first observation whose board has the family's shape.
    pub first_match_index: usize,
    pub well: Well,
}

/// Shape-family records found in a round's observations, in catalog order.
/// Unlike the route analysis it reads every observation it is given, since
/// these builds often run past the opener window.
pub fn analyze_opener_shape_families(
    input: &OpenerRoundInput,
) -> Result<Vec<ShapeFamilyMatch>, AnalyzeError> {
    let installed = installed_opener_catalog().ok_or(AnalyzeError::NoCatalog)?;
    Ok(match_shape_families(
        installed.catalog(),
        &prepare_observations(&input.observations),
    ))
}

pub(crate) fn match_shape_families(
    catalog: &OpenerCatalog,
    observations: &[Option<PreparedObservation>],
) -> Vec<ShapeFamilyMatch> {
    catalog
        .openers
        .iter()
        .filter_map(|record| {
            let ShapeFamily::FourWide = record.shape_family?;
            let (first_match_index, well) =
                observations
                    .iter()
                    .enumerate()
                    .find_map(|(index, observation)| {
                        let board = observation.as_ref()?.normalized.as_ref()?;
                        four_wide_well(&board.masks).map(|well| (index, well))
                    })?;
            Some(ShapeFamilyMatch {
                opener_id: record.id.clone(),
                name: record.aliases.en.clone(),
                first_match_index,
                well,
            })
        })
        .collect()
}

/// The well of a board that holds a 4-wide: from the floor, at most three
/// residue rows (stack columns full, 1-8 cells in the well), then at least six
/// rows with the stack columns full and the well empty. Rows are floor-up with
/// garbage rows removed.
fn four_wide_well(rows: &[u16]) -> Option<Well> {
    [(Well::Left, 0), (Well::Center, 3), (Well::Right, 6)]
        .into_iter()
        .find_map(|(well, column)| {
            let well_mask = 0b1111_u16 << column;
            let stack = OPENER_BOARD_ROW_MASK & !well_mask;
            let residue_rows = rows
                .iter()
                .take(FOUR_WIDE_MAX_RESIDUE_ROWS)
                .take_while(|&&row| {
                    row & stack == stack && row & well_mask != 0 && row != OPENER_BOARD_ROW_MASK
                })
                .count();
            let residue_cells: u32 = rows[..residue_rows]
                .iter()
                .map(|row| (row & well_mask).count_ones())
                .sum();
            let open_rows = rows[residue_rows..]
                .iter()
                .take_while(|&&row| row == stack)
                .count();
            ((1..=FOUR_WIDE_MAX_RESIDUE_CELLS).contains(&residue_cells)
                && open_rows >= FOUR_WIDE_MIN_OPEN_ROWS)
                .then_some(well)
        })
}

#[cfg(test)]
mod tests {
    use super::{four_wide_well, match_shape_families, Well};
    use crate::openers::board::{mirror_mask_10, rows_to_masks_floor_up};
    use crate::openers::catalog::parse_catalog;
    use crate::openers::phase::{prepare_observations, OpenerObservation};

    fn masks(rows_top_down: &[&str]) -> Vec<u16> {
        let rows: Vec<String> = rows_top_down.iter().map(|row| (*row).to_owned()).collect();
        rows_to_masks_floor_up(&rows)
    }

    fn open(stack: &str, rows: usize) -> Vec<&str> {
        vec![stack; rows]
    }

    #[test]
    fn recognizes_center_and_side_wells_over_their_residue() {
        let center_three = [open("XXX____XXX", 6), vec!["XXXXX__XXX", "XXXX___XXX"]].concat();
        let center_six = [open("XXX____XXX", 6), vec!["XXXX_XXXXX", "XXXX_XXXXX"]].concat();
        let side_three = [open("____XXXXXX", 6), vec!["X___XXXXXX", "XX__XXXXXX"]].concat();
        // A ragged tower top above the open rows does not matter.
        let tower_top = [vec!["X________X", "XX______XX"], center_three.clone()].concat();

        assert_eq!(four_wide_well(&masks(&center_three)), Some(Well::Center));
        assert_eq!(four_wide_well(&masks(&center_six)), Some(Well::Center));
        assert_eq!(four_wide_well(&masks(&side_three)), Some(Well::Left));
        assert_eq!(four_wide_well(&masks(&tower_top)), Some(Well::Center));
        let mirrored: Vec<u16> = masks(&side_three).into_iter().map(mirror_mask_10).collect();
        assert_eq!(four_wide_well(&mirrored), Some(Well::Right));
    }

    #[test]
    fn rejects_a_short_well_a_bare_floor_and_a_filled_well() {
        let five_rows = [open("XXX____XXX", 5), vec!["XXXX___XXX"]].concat();
        let no_residue = open("XXX____XXX", 8);
        let deep_residue = [open("XXX____XXX", 6), vec!["XXXX___XXX"; 4]].concat();
        let nine_cells = [
            open("XXX____XXX", 6),
            vec!["XXXX_XXXXX", "XXXX_XXXXX", "XXXX_XXXXX"],
        ]
        .concat();
        let holed_stack = [
            open("XXX____XXX", 3),
            vec!["XX_____XXX"],
            open("XXX____XXX", 3),
            vec!["XXXX___XXX"],
        ]
        .concat();

        for board in [five_rows, no_residue, deep_residue, nine_cells, holed_stack] {
            assert_eq!(four_wide_well(&masks(&board)), None, "{board:?}");
        }
    }

    #[test]
    fn reports_the_first_observation_with_the_shape_above_garbage() {
        let catalog = parse_catalog(
            br#"{"formatVersion":2,"sources":[],"openers":[
              {"id":"route","aliases":{"en":"Route"},"shapeKey":"route"},
              {"id":"4-wide","aliases":{"en":"4-Wide"},"shapeKey":"4-wide","shapeFamily":"4-wide"}
            ]}"#,
        )
        .expect("catalog with a shape family");
        let shaped = [open("XXX____XXX", 6), vec!["XXXXX__XXX", "XXXX___XXX"]].concat();
        let mut board = vec![0_u16; 40];
        let mut gmask = vec![0_u16; 40];
        // A garbage row under the build is removed before the shape is read.
        board[0] = 0b11_1111_1110;
        gmask[0] = board[0];
        for (y, row) in masks(&shaped).into_iter().enumerate() {
            board[y + 1] = row;
        }
        let observation = |board: Vec<u16>, gmask: Vec<u16>| {
            Some(OpenerObservation {
                post_board: Some(board),
                post_gmask: Some(gmask),
                post_letters: None,
            })
        };
        let observations = vec![
            None,
            observation(vec![0; 40], vec![0; 40]),
            observation(board.clone(), gmask.clone()),
            observation(board, gmask),
        ];

        let matches = match_shape_families(&catalog, &prepare_observations(&observations));

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].opener_id, "4-wide");
        assert_eq!(matches[0].name, "4-Wide");
        assert_eq!(matches[0].first_match_index, 2);
        assert_eq!(matches[0].well, Well::Center);
    }

    #[test]
    fn round_analysis_carries_the_family_and_omits_an_empty_list() {
        use crate::openers::analyze::{analyze_opener_round_with_catalog, OpenerRoundInput};

        let catalog = parse_catalog(
            br#"{"formatVersion":2,"sources":[],"openers":[
              {"id":"4-wide","aliases":{"en":"4-Wide"},"shapeKey":"4-wide","shapeFamily":"4-wide"}
            ]}"#,
        )
        .expect("catalog with a shape family");
        let shaped = [open("XXX____XXX", 6), vec!["XXXXX__XXX", "XXXX___XXX"]].concat();
        let mut board = vec![0_u16; 40];
        for (y, row) in masks(&shaped).into_iter().enumerate() {
            board[y] = row;
        }
        let observation = |board: Vec<u16>| {
            Some(OpenerObservation {
                post_board: Some(board),
                post_gmask: Some(vec![0; 40]),
                post_letters: None,
            })
        };
        let round = |observations| OpenerRoundInput {
            observations,
            ..OpenerRoundInput::default()
        };

        let found = analyze_opener_round_with_catalog(
            &catalog,
            &round(vec![observation(vec![0; 40]), observation(board)]),
        );
        assert_eq!(found.shape_families.len(), 1);
        assert_eq!(found.shape_families[0].first_match_index, 1);
        let json = serde_json::to_value(&found).expect("serializable");
        assert_eq!(json["shapeFamilies"][0]["openerId"], "4-wide");
        assert_eq!(json["shapeFamilies"][0]["well"], "center");

        let none = analyze_opener_round_with_catalog(
            &catalog,
            &round(vec![observation(vec![0; 40])]),
        );
        let json = serde_json::to_value(&none).expect("serializable");
        assert!(json.get("shapeFamilies").is_none(), "{json}");
    }

    #[test]
    fn rejects_an_unknown_shape_family() {
        let error = parse_catalog(
            br#"{"formatVersion":2,"sources":[],"openers":[
              {"id":"5-wide","aliases":{"en":"5-Wide"},"shapeKey":"5-wide","shapeFamily":"5-wide"}
            ]}"#,
        )
        .expect_err("unknown family");
        assert!(error.to_string().contains("unknown variant"), "{error}");
    }
}
