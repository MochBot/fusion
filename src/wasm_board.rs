use wasm_bindgen::prelude::*;

use crate::board::Board;

// Board row conversion between the internal `[u16; 40]` rows and the WASM
// bridge's `u64` rows. Bit x of row y means cell (x, y) is filled in both;
// only the container width differs, so conversion masks to the board width.

pub(crate) fn board_from_row_bitmasks(rows: &[u64]) -> Board {
    let mut out = [0u16; 40];
    for (y, &row) in rows.iter().enumerate() {
        if y >= 40 {
            break;
        }
        out[y] = (row & 0x3FF) as u16;
    }
    Board::from_rows(out)
}

fn board_to_row_bitmasks(board: &Board) -> Vec<u64> {
    let mut rows = Vec::with_capacity(40);
    for y in 0..40 {
        rows.push(board.rows[y] as u64);
    }
    rows
}

// JsBoard

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
