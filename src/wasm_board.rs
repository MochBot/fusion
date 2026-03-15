use wasm_bindgen::prelude::*;

use crate::board::Board;
use crate::header::COL_NB;

// rows[y] bit x = cell (x,y) filled; u16 internal, u64 over WASM boundary
pub(crate) fn board_from_row_bitmasks(rows: &[u64]) -> Board {
    let mut board = Board::new();
    for (y, &row) in rows.iter().enumerate() {
        if y >= 40 {
            break;
        }
        board.rows[y] = (row & 0x3FF) as u16;
    }
    // Rebuild cols cache from rows
    board.cols = [0; COL_NB];
    for y in 0..40 {
        let row = board.rows[y];
        if row == 0 {
            continue;
        }
        let mut bits = row as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            board.cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }
    board
}

fn board_to_row_bitmasks(board: &Board) -> Vec<u64> {
    let mut rows = Vec::with_capacity(40);
    for y in 0..40 {
        rows.push(board.rows[y] as u64);
    }
    rows
}

#[wasm_bindgen]
pub struct JsBoard {
    pub(crate) inner: Board,
}

#[wasm_bindgen]
impl JsBoard {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Board::new(),
        }
    }

    #[wasm_bindgen(js_name = "from_rows")]
    pub fn from_rows(rows: &[u64]) -> Self {
        Self {
            inner: board_from_row_bitmasks(rows),
        }
    }

    #[wasm_bindgen(js_name = "to_rows")]
    pub fn to_rows(&self) -> Vec<u64> {
        board_to_row_bitmasks(&self.inner)
    }
}

impl Default for JsBoard {
    fn default() -> Self {
        Self::new()
    }
}
