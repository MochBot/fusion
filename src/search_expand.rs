use crate::board::Board;
use crate::eval::{evaluate, EvalWeights};
use crate::header::Piece;
use crate::move_buffer::MoveBuffer;
use crate::movegen::generate;
use crate::state::{GameState, TransitionObservation};
use crate::transposition::{TranspositionTable, ZobristKeys};
use crate::search_config::{SearchExpansionContext, SearchNode};

pub(crate) fn gen_and_eval_root(
    state: &GameState,
    piece: Piece,
    new_hold: Option<Piece>,
    hold_used: bool,
    ctx: &mut SearchExpansionContext<'_>,
    nodes: &mut Vec<SearchNode>,
) {
    let mut moves = MoveBuffer::new();
    generate(&state.board, &mut moves, piece, false);

    for m in moves.as_slice() {
        let mut result_board = state.board.clone();
        let lines_cleared = result_board.do_move(m) as u8;
        let next_pending_garbage = state.pending_garbage.saturating_sub(lines_cleared);
        let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);

        let (next_b2b, next_combo) =
            GameState::next_chain_values(state.b2b, state.combo, m, lines_cleared);
        let coaching = state.coaching.transition(TransitionObservation {
            resulting_height: result_board.height(),
            resulting_b2b: next_b2b,
            resulting_combo: next_combo,
            lines_cleared,
            hold_used,
            pending_garbage: state.pending_garbage,
            imminent_garbage: next_pending_garbage,
            spawn_envelope_blocked,
        });

        let score = evaluate_with_tt(
            &result_board,
            ctx.weights,
            ctx.remaining_depth,
            ctx.zobrist_keys,
            ctx.tt,
        );

        nodes.push(SearchNode {
            board: result_board,
            score,
            hold: new_hold,
            b2b: next_b2b,
            combo: next_combo,
            pending_garbage: next_pending_garbage,
            coaching,
            root_move: *m,
            root_hold_used: hold_used,
            path: vec![*m],
        });
    }
}

pub(crate) fn expand_node(
    parent: &SearchNode,
    piece: Piece,
    new_hold: Option<Piece>,
    hold_used: bool,
    ctx: &mut SearchExpansionContext<'_>,
    out: &mut Vec<SearchNode>,
) {
    let mut moves = MoveBuffer::new();
    generate(&parent.board, &mut moves, piece, false);

    for m in moves.as_slice() {
        let mut result_board = parent.board.clone();
        let lines_cleared = result_board.do_move(m) as u8;
        let next_pending_garbage = parent.pending_garbage.saturating_sub(lines_cleared);
        let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);

        let (next_b2b, next_combo) =
            GameState::next_chain_values(parent.b2b, parent.combo, m, lines_cleared);
        let coaching = parent.coaching.transition(TransitionObservation {
            resulting_height: result_board.height(),
            resulting_b2b: next_b2b,
            resulting_combo: next_combo,
            lines_cleared,
            hold_used,
            pending_garbage: parent.pending_garbage,
            imminent_garbage: next_pending_garbage,
            spawn_envelope_blocked,
        });

        let score = evaluate_with_tt(
            &result_board,
            ctx.weights,
            ctx.remaining_depth,
            ctx.zobrist_keys,
            ctx.tt,
        );

        let mut path = parent.path.clone();
        path.push(*m);

        out.push(SearchNode {
            board: result_board,
            score,
            hold: new_hold,
            b2b: next_b2b,
            combo: next_combo,
            pending_garbage: next_pending_garbage,
            coaching,
            root_move: parent.root_move,
            root_hold_used: parent.root_hold_used,
            path,
        });
    }
}

pub(crate) fn evaluate_with_tt(
    board: &Board,
    weights: &EvalWeights,
    remaining_depth: usize,
    zobrist_keys: &ZobristKeys,
    tt: &mut Option<TranspositionTable>,
) -> f32 {
    if let Some(table) = tt.as_mut() {
        let depth = remaining_depth.min(u8::MAX as usize) as u8;
        let hash = zobrist_keys.hash_board(board);

        if let Some(score) = table.probe(hash, depth) {
            return score;
        }

        let score = evaluate(board, weights);
        table.store(hash, depth, score);
        return score;
    }

    evaluate(board, weights)
}
