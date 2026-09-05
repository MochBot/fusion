pub const OPENER_BOARD_WIDTH: usize = 10;
pub const OPENER_BOARD_ROW_MASK: u16 = (1 << OPENER_BOARD_WIDTH) - 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedBoard {
    pub masks: Vec<u16>,
    pub letters: Option<Vec<String>>,
}

pub(crate) fn rows_to_masks_floor_up(rows_top_down: &[String]) -> Vec<u16> {
    let mut masks = Vec::with_capacity(rows_top_down.len());
    for row in rows_top_down.iter().rev() {
        let mut mask = 0;
        for (x, cell) in row.chars().take(OPENER_BOARD_WIDTH).enumerate() {
            if cell != '_' {
                mask |= 1 << x;
            }
        }
        masks.push(mask);
    }
    trim_trailing_empty_rows(&mut masks, None);
    masks
}

pub(crate) const fn mirror_mask_10(mask: u16) -> u16 {
    let mut mirrored = 0;
    let mut x = 0;
    while x < OPENER_BOARD_WIDTH {
        if mask & (1 << x) != 0 {
            mirrored |= 1 << (OPENER_BOARD_WIDTH - 1 - x);
        }
        x += 1;
    }
    mirrored
}

pub(crate) const fn mirror_piece_letter(letter: char) -> char {
    match letter {
        'L' => 'J',
        'J' => 'L',
        'S' => 'Z',
        'Z' => 'S',
        other => other,
    }
}

pub(crate) fn mirror_letter_row(row: &str) -> String {
    row.chars().rev().map(mirror_piece_letter).collect()
}

pub(crate) fn strip_garbage_rows(
    board: &[u16],
    garbage_mask: &[u16],
    letters: Option<&[String]>,
) -> NormalizedBoard {
    let mut masks = Vec::with_capacity(board.len());
    let mut kept_letters = letters.map(|_| Vec::with_capacity(board.len()));

    for (y, row) in board.iter().enumerate() {
        let is_garbage_row = match garbage_mask.get(y) {
            Some(mask) => *mask != 0,
            None => false,
        };
        if is_garbage_row {
            continue;
        }

        masks.push(*row & OPENER_BOARD_ROW_MASK);
        if let Some(kept) = kept_letters.as_mut() {
            let row_letters = match letters.and_then(|all_letters| all_letters.get(y)) {
                Some(row_letters) => row_letters.clone(),
                None => "__________".to_owned(),
            };
            kept.push(row_letters);
        }
    }

    trim_trailing_empty_rows(&mut masks, kept_letters.as_mut());
    NormalizedBoard {
        masks,
        letters: kept_letters,
    }
}

pub(crate) fn cell_count(masks: &[u16]) -> u32 {
    masks
        .iter()
        .map(|row| (*row & OPENER_BOARD_ROW_MASK).count_ones())
        .sum()
}

fn trim_trailing_empty_rows(masks: &mut Vec<u16>, letters: Option<&mut Vec<String>>) {
    let mut letters = letters;
    while matches!(masks.last(), Some(&0)) {
        masks.pop();
        if let Some(kept_letters) = letters.as_mut() {
            kept_letters.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cell_count, mirror_letter_row, mirror_mask_10, rows_to_masks_floor_up, strip_garbage_rows,
    };

    #[test]
    fn converts_top_down_rows_to_floor_up_masks_and_removes_trailing_empty_rows() {
        let rows = vec![
            "__________".to_owned(),
            "__L_______".to_owned(),
            "I_________".to_owned(),
            "__________".to_owned(),
        ];

        let masks = rows_to_masks_floor_up(&rows);

        assert_eq!(masks, vec![0, 1, 4]);
    }

    #[test]
    fn mirrors_ten_column_masks_and_piece_letters() {
        assert_eq!(mirror_mask_10(0b0000000001), 0b1000000000);
        assert_eq!(mirror_mask_10(0b1010000001), 0b1000000101);
        assert_eq!(mirror_letter_row("LJSZOITX__"), "__XTIOSZLJ");
    }

    #[test]
    fn strips_every_garbage_row_keeps_letters_aligned_and_trims_empty_rows() {
        let board = [0b0000000001, 0b1111111111, 0b0000000010, 0, 0];
        let garbage = [0, 0b0000010000, 0, 0, 0];
        let letters = vec![
            "I_________".to_owned(),
            "XXXXGXXXXX".to_owned(),
            "_O________".to_owned(),
            "__________".to_owned(),
            "__________".to_owned(),
        ];

        let normalized = strip_garbage_rows(&board, &garbage, Some(&letters));

        assert_eq!(normalized.masks, vec![0b0000000001, 0b0000000010]);
        assert_eq!(
            normalized.letters,
            Some(vec!["I_________".to_owned(), "_O________".to_owned()])
        );
        assert_eq!(cell_count(&normalized.masks), 2);
    }
}
