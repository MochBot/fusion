use fusion_core::{Board, Move};

/// Apply a move to a board and return the resulting board and lines cleared.
pub fn apply_move(board: &Board, mv: &Move) -> (Board, u8) {
    let mut next = board.clone();

    for (dx, dy) in mv.piece.minos(mv.rotation) {
        let x = mv.x + dx;
        let y = mv.y + dy;
        // Bounds check to avoid panic, though movegen should ensure validity
        if x >= 0 && y >= 0 && x < Board::WIDTH as i8 && y < Board::HEIGHT as i8 {
            next.set(x as usize, y as usize, true);
        }
    }

    let lines = next.clear_lines();
    (next, lines)
}

/// Undo info for unapply_move - stores piece cells and cleared rows
/// Stack-allocated - max 4 lines can clear from one piece
#[derive(Clone, Copy)]
pub struct UndoInfo {
    /// The move that was applied
    pub mv: Move,
    /// Rows that were cleared (y index + row bitmap as u16)
    /// Using u16 bitmap instead of [bool;10] - 10 bits needed
    pub cleared_rows: [(u8, u16); 4],
    /// How many rows were actually cleared (0-4)
    pub cleared_count: u8,
}

/// Apply move in-place, returns undo info for reversal
#[inline]
pub fn apply_move_mut(board: &mut Board, mv: &Move) -> UndoInfo {
    for (dx, dy) in mv.piece.minos(mv.rotation) {
        let x = (mv.x + dx) as usize;
        let y = (mv.y + dy) as usize;
        // movegen guarantees cells are empty — skip the branch in set()
        board.set_raw(x, y);
    }

    let mut cleared_rows = [(0u8, 0u16); 4];
    let mut cleared_count = 0u8;

    let mut full_mask = !0u64;
    for x in 0..Board::WIDTH {
        full_mask &= board.column(x);
    }

    while full_mask != 0 && cleared_count < 4 {
        let y = full_mask.trailing_zeros() as usize;

        let row_bitmap = board.row(y);
        cleared_rows[cleared_count as usize] = (y as u8, row_bitmap);
        cleared_count += 1;

        let lower_mask = (1u64 << y) - 1;
        for x in 0..Board::WIDTH {
            let col = board.column(x);
            let lower = col & lower_mask;
            let upper = col >> (y + 1);
            board.set_column(x, lower | (upper << y));
        }

        full_mask = !0u64;
        for x in 0..Board::WIDTH {
            full_mask &= board.column(x);
        }
    }

    UndoInfo {
        mv: *mv,
        cleared_rows,
        cleared_count,
    }
}

/// Undo a move - restores board to state before apply_move_mut
#[inline]
pub fn unapply_move(board: &mut Board, undo: &UndoInfo) {
    // First, undo line clears (in reverse order)
    for i in (0..undo.cleared_count as usize).rev() {
        let (row_y, row_bitmap) = undo.cleared_rows[i];
        let row_y = row_y as usize;

        // Shift rows up using column bit ops — O(10) not O(400)
        let lower_mask = (1u64 << row_y) - 1;
        for x in 0..Board::WIDTH {
            let col = board.column(x);
            let lower = col & lower_mask;
            let upper = col >> row_y;
            let row_bit = ((row_bitmap >> x) & 1) as u64;
            board.set_column(x, lower | (row_bit << row_y) | (upper << (row_y + 1)));
        }
    }

    for (dx, dy) in undo.mv.piece.minos(undo.mv.rotation) {
        let x = (undo.mv.x + dx) as usize;
        let y = (undo.mv.y + dy) as usize;
        board.clear_raw(x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::{Piece, Rotation};

    #[test]
    fn test_apply_t_piece_empty() {
        let board = Board::new();
        let mv = Move::new(Piece::T, Rotation::North, 4, 0);
        let (next, lines) = apply_move(&board, &mv);

        assert_eq!(lines, 0);
        // T North at (4,0): (3,0), (4,0), (5,0), (4,1)
        assert!(next.get(3, 0));
        assert!(next.get(4, 0));
        assert!(next.get(5, 0));
        assert!(next.get(4, 1));

        // Count filled cells
        let mut count = 0;
        for y in 0..Board::HEIGHT {
            for x in 0..Board::WIDTH {
                if next.get(x, y) {
                    count += 1;
                }
            }
        }
        assert_eq!(count, 4);
    }

    #[test]
    fn test_apply_i_piece_line_clear() {
        let mut board = Board::new();
        // Fill row 0 except for columns 4, 5, 6, 7
        for x in 0..10 {
            if !(4..=7).contains(&x) {
                board.set(x, 0, true);
            }
        }

        // I piece horizontal at (5, 0) North covers (4,0), (5,0), (6,0), (7,0)
        let mv = Move::new(Piece::I, Rotation::North, 5, 0);
        let (next, lines) = apply_move(&board, &mv);

        assert_eq!(lines, 1);
        // Board should be empty after clearing the only line
        for x in 0..10 {
            assert!(!next.get(x, 0));
        }
    }
}
