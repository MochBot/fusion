use direct_cobra_copy::board::{Board, FULL_ROW};
use direct_cobra_copy::eval::{evaluate, EvalWeights};
use direct_cobra_copy::header::{Piece, Rotation, SpinType, COL_NB};
use direct_cobra_copy::movegen::{generate, MoveBuffer};
use direct_cobra_copy::search::{find_best_move, SearchConfig, SearchResult};
use direct_cobra_copy::state::GameState;

fn board_from_bottom_rows(bottom_rows: &[u16]) -> Board {
    assert!(
        bottom_rows.len() <= 40,
        "board helper expects at most 40 rows",
    );

    let mut board = Board::new();
    for (y, row) in bottom_rows.iter().copied().enumerate() {
        board.rows[y] = row & FULL_ROW;
    }

    board.cols = [0; COL_NB];
    for y in 0..board.rows.len() {
        let mut bits = board.rows[y] as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            board.cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }

    board
}

fn row_with_gap(gap_col: usize) -> u16 {
    assert!(gap_col < COL_NB, "gap column out of bounds");
    FULL_ROW & !(1u16 << gap_col)
}

fn fast_search_config() -> SearchConfig {
    SearchConfig {
        beam_width: 50,
        depth: 3,
        time_budget_ms: None,
        ..SearchConfig::default()
    }
}

fn run_search(state: &GameState, config: &SearchConfig, weights: &EvalWeights) -> SearchResult {
    find_best_move(state, config, weights)
        .unwrap_or_else(|| panic!("expected search to return a best move"))
}

fn assert_legal_and_sane(
    state: &GameState,
    result: &SearchResult,
    weights: &EvalWeights,
) -> (Board, Piece) {
    let played_piece = if result.hold_used {
        state
            .hold
            .or_else(|| state.queue.first().copied())
            .unwrap_or_else(|| panic!("hold_used result requires hold piece or queue fallback"))
    } else {
        state.current
    };

    let mut legal_moves = MoveBuffer::new();
    generate(&state.board, &mut legal_moves, played_piece, false);
    assert!(
        legal_moves.as_slice().contains(&result.best_move),
        "best move {:?} must exist in legal move list for {:?}",
        result.best_move,
        played_piece,
    );

    let mut board_after = state.board.clone();
    board_after.do_move(&result.best_move);
    let score = evaluate(&board_after, weights);
    assert!(
        score.is_finite(),
        "post-move eval score must be finite, got {}",
        score,
    );

    (board_after, played_piece)
}

#[test]
fn empty_board_t_piece_returns_legal_move() {
    let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O, Piece::L]);
    let config = fast_search_config();
    let weights = EvalWeights::default();

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert!(
        board_after.height() <= 4,
        "first placement on empty board should stay low"
    );
}

#[test]
fn nearly_full_single_gap_prefers_gap_for_i_piece() {
    let gap = 4;
    let board = board_from_bottom_rows(&[
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
    ]);
    let state = GameState::new(board, Piece::I, vec![Piece::T, Piece::O, Piece::L]);
    let config = fast_search_config();
    let weights = EvalWeights::default();
    let before_height = state.board.height();

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert_eq!(result.best_move.x(), gap as i32, "I should target the gap");
    assert!(
        matches!(result.best_move.rotation(), Rotation::East | Rotation::West),
        "I should be vertical when dropping into a 1-wide gap"
    );
    assert!(
        board_after.height() < before_height,
        "filling the gap should improve stack height"
    );
}

#[test]
fn tspin_shape_returns_legal_t_move() {
    let board = board_from_bottom_rows(&[
        row_with_gap(4),
        FULL_ROW & !((1u16 << 3) | (1u16 << 4) | (1u16 << 5)),
        row_with_gap(4),
    ]);
    let state = GameState::new(board, Piece::T, vec![Piece::I, Piece::O, Piece::S]);
    let config = fast_search_config();
    let weights = EvalWeights::default();

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert!(
        result.best_move.spin() != SpinType::NoSpin
            || board_after.height() <= state.board.height() + 1,
        "in T-slot shape, move should be a spin or keep stack controlled"
    );
}

#[test]
fn i_piece_well_prefers_vertical_drop() {
    let well_col = 8;
    let board = board_from_bottom_rows(&[
        row_with_gap(well_col),
        row_with_gap(well_col),
        row_with_gap(well_col),
        row_with_gap(well_col),
        row_with_gap(well_col),
        row_with_gap(well_col),
        row_with_gap(well_col),
        row_with_gap(well_col),
    ]);
    let state = GameState::new(board, Piece::I, vec![Piece::T, Piece::O, Piece::L]);
    let config = fast_search_config();
    let weights = EvalWeights::default();
    let before_height = state.board.height();

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert_eq!(
        result.best_move.x(),
        well_col as i32,
        "I should be placed in the one-wide well"
    );
    assert!(
        matches!(result.best_move.rotation(), Rotation::East | Rotation::West),
        "I should be vertical in a one-wide well"
    );
    assert!(
        board_after.height() <= before_height,
        "well fill should not raise overall stack"
    );
}

#[test]
fn flat_stack_with_garbage_hole_returns_stable_move() {
    let board = board_from_bottom_rows(&[row_with_gap(2), FULL_ROW, FULL_ROW, FULL_ROW]);
    let state = GameState::new(board, Piece::O, vec![Piece::T, Piece::I, Piece::S]);
    let config = fast_search_config();
    let weights = EvalWeights::default();
    let before_height = state.board.height();

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert!(
        board_after.height() <= before_height + 2,
        "move should not explode stack height on simple garbage board"
    );
}

#[test]
fn high_stack_danger_avoids_topout_move() {
    let gap = 4;
    let board = board_from_bottom_rows(&[
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
    ]);
    let state = GameState::new(board, Piece::I, vec![Piece::O, Piece::T, Piece::L]);
    let config = fast_search_config();
    let weights = EvalWeights::default();

    assert!(state.board.height() >= 18, "test setup must be high-stack");

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert!(
        board_after.height() < 21,
        "recommended move should avoid immediate top-out zone"
    );
}

#[test]
fn multi_piece_queue_depth_search_returns_three_ply_pv() {
    let board = board_from_bottom_rows(&[0b0000011000, 0b0001111100, 0b0011110110]);
    let state = GameState::new(
        board,
        Piece::L,
        vec![Piece::J, Piece::S, Piece::Z, Piece::T],
    );
    let config = fast_search_config();
    let weights = EvalWeights::default();

    let result = run_search(&state, &config, &weights);
    let _ = assert_legal_and_sane(&state, &result, &weights);

    assert_eq!(
        result.pv.len(),
        3,
        "depth=3 with queue should produce a 3-move principal variation"
    );
}

#[test]
fn hold_swap_uses_held_piece_legally() {
    let gap = 6;
    let board = board_from_bottom_rows(&[
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
        row_with_gap(gap),
    ]);
    let mut state = GameState::new(board, Piece::O, vec![Piece::T, Piece::Z, Piece::L]);
    state.hold = Some(Piece::I);

    let config = fast_search_config();
    let weights = EvalWeights::default();

    let result = run_search(&state, &config, &weights);
    let (_, played_piece) = assert_legal_and_sane(&state, &result, &weights);

    assert!(result.hold_used, "expected hold swap in this setup");
    assert_eq!(played_piece, Piece::I, "held I should be used");
    assert_eq!(result.best_move.x(), gap as i32, "held I should target gap");
}

#[test]
fn board_with_pending_line_clears_is_handled_correctly() {
    let board = board_from_bottom_rows(&[FULL_ROW, FULL_ROW, row_with_gap(4), row_with_gap(4)]);
    let state = GameState::new(board, Piece::I, vec![Piece::T, Piece::O, Piece::L]);
    let config = fast_search_config();
    let weights = EvalWeights::default();

    assert!(
        state.board.line_clears() != 0,
        "test setup must include pending full lines"
    );

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert_eq!(
        board_after.line_clears(),
        0,
        "post-move board should have clears applied"
    );
}

#[test]
fn random_messy_board_returns_legal_finite_move() {
    let board = board_from_bottom_rows(&[
        0b0001111000,
        0b0011011100,
        0b0110010110,
        0b0101110010,
        0b1110011101,
        0b0011100111,
        0b1100101011,
        0b0101011101,
        0b1010110011,
    ]);
    let state = GameState::new(
        board,
        Piece::S,
        vec![Piece::Z, Piece::T, Piece::I, Piece::O],
    );
    let config = fast_search_config();
    let weights = EvalWeights::default();
    let before_height = state.board.height();

    let result = run_search(&state, &config, &weights);
    let (board_after, _) = assert_legal_and_sane(&state, &result, &weights);

    assert!(
        board_after.height() <= before_height + 2,
        "messy-board recommendation should avoid immediate stack spike"
    );
}
