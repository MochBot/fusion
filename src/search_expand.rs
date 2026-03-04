use crate::analysis::{assemble_composite, shape_chain_value, shape_context_modifier};
use crate::attack::{calculate_attack_full, AttackContext};
use crate::board::Board;
use crate::eval::{evaluate, EvalWeights};
use crate::header::{Move, Piece};
use crate::move_buffer::MoveBuffer;
use crate::movegen::generate;
use crate::search_config::{SearchExpansionContext, SearchNode};
use crate::state::{
    ClearEvent, ClearType, CoachingState, FatalityState, GameState, ObligationState, PhaseState,
    SurgeState, TransitionObservation,
};
use crate::transposition::{TranspositionTable, ZobristKeys};
use smallvec::{smallvec, SmallVec};

#[derive(Clone)]
struct CandidateAction {
    mv: Move,
    hold_used: bool,
    next_hold: Option<Piece>,
    next_current: Option<Piece>,
    next_queue: SmallVec<[Piece; 16]>,
}

#[inline]
fn coaching_context_bias(previous: CoachingState, next: CoachingState) -> f32 {
    fn score(state: CoachingState) -> f32 {
        let fatality = match state.fatality {
            FatalityState::Safe => 0.0,
            FatalityState::Critical => -0.35,
            FatalityState::Fatal => -0.70,
        };
        let obligation = match state.obligation {
            ObligationState::None => 0.0,
            ObligationState::MustDownstack => -0.25,
            ObligationState::MustCancel => -0.45,
        };
        let surge = match state.surge {
            SurgeState::Dormant => 0.0,
            SurgeState::Building => 0.20,
            SurgeState::Active => 0.35,
        };
        let phase = match state.phase {
            PhaseState::Opener => 0.10,
            PhaseState::Midgame => 0.0,
            PhaseState::Endgame => -0.10,
        };
        fatality + obligation + surge + phase
    }

    score(next) - score(previous)
}

fn split_next_queue(queue: &[Piece], consumed: usize) -> (Option<Piece>, SmallVec<[Piece; 16]>) {
    let tail = if consumed >= queue.len() { &[][..] } else { &queue[consumed..] };
    let next_current = tail.first().copied();
    let next_queue = if tail.len() > 1 {
        SmallVec::from_slice(&tail[1..])
    } else {
        SmallVec::new()
    };
    (next_current, next_queue)
}

fn push_actions(
    actions: &mut Vec<CandidateAction>,
    board: &Board,
    piece: Piece,
    next_hold: Option<Piece>,
    hold_used: bool,
    next_current: Option<Piece>,
    next_queue: SmallVec<[Piece; 16]>,
) {
    let mut moves = MoveBuffer::new();
    generate(board, &mut moves, piece, true);
    for mv in moves.as_slice() {
        if board.legal_lock_placement(mv) {
            actions.push(CandidateAction {
                mv: *mv,
                hold_used,
                next_hold,
                next_current,
                next_queue: next_queue.clone(),
            });
        }
    }
}

fn enumerate_actions(
    board: &Board,
    current: Option<Piece>,
    hold: Option<Piece>,
    queue: &[Piece],
) -> Vec<CandidateAction> {
    let mut actions = Vec::new();
    if let Some(current_piece) = current {
        let (next_current, next_queue) = split_next_queue(queue, 0);
        push_actions(
            &mut actions,
            board,
            current_piece,
            hold,
            false,
            next_current,
            next_queue,
        );

        if let Some(held_piece) = hold {
            let (next_current, next_queue) = split_next_queue(queue, 0);
            push_actions(
                &mut actions,
                board,
                held_piece,
                Some(current_piece),
                true,
                next_current,
                next_queue,
            );
        } else if let Some(&queue_piece) = queue.first() {
            let (next_current, next_queue) = split_next_queue(queue, 1);
            push_actions(
                &mut actions,
                board,
                queue_piece,
                Some(current_piece),
                true,
                next_current,
                next_queue,
            );
        }
    }
    actions
}

fn build_runtime_state(
    board: &Board,
    current: Option<Piece>,
    hold: Option<Piece>,
    queue: &[Piece],
    b2b: u8,
    combo: u32,
    pending_garbage: u8,
    lines_total: u32,
    bag_number: u32,
    pieces_into_bag: u8,
    coaching: CoachingState,
) -> Option<GameState> {
    current.map(|piece| GameState {
        board: board.clone(),
        current: piece,
        hold,
        queue: queue.to_vec(),
        b2b,
        combo,
        pending_garbage,
        lines_total,
        bag_number,
        pieces_into_bag,
        coaching,
    })
}

fn infer_for_state(
    board: &Board,
    current: Option<Piece>,
    hold: Option<Piece>,
    queue: &[Piece],
    b2b: u8,
    combo: u32,
    pending_garbage: u8,
    lines_total: u32,
    bag_number: u32,
    pieces_into_bag: u8,
    coaching: CoachingState,
    ctx: &SearchExpansionContext<'_>,
) -> Option<(Vec<CandidateAction>, Vec<f32>, f32)> {
    let runtime = ctx.policy_value?;
    let runtime_context = ctx.runtime_context?;
    let actions = enumerate_actions(board, current, hold, queue);
    if actions.is_empty() {
        return None;
    }
    let state = build_runtime_state(
        board,
        current,
        hold,
        queue,
        b2b,
        combo,
        pending_garbage,
        lines_total,
        bag_number,
        pieces_into_bag,
        coaching,
    )?;
    let candidates: Vec<Move> = actions.iter().map(|action| action.mv).collect();
    let inference = runtime.infer(&state, runtime_context, &candidates).ok()?;
    Some((actions, inference.policy_logits, inference.value))
}

fn maybe_limit_policy_guided_actions(
    actions: Vec<CandidateAction>,
    policy_scores: Vec<f32>,
    ctx: &SearchExpansionContext<'_>,
) -> (Vec<CandidateAction>, Vec<f32>) {
    let expansion_cap = ctx
        .config
        .policy_guided_expansion_cap
        .min(ctx.current_beam_width)
        .min(actions.len());
    if expansion_cap == 0 || expansion_cap >= actions.len() {
        return (actions, policy_scores);
    }

    let mut ranked: Vec<(CandidateAction, f32)> = actions.into_iter().zip(policy_scores).collect();
    ranked.sort_unstable_by(|(_, left), (_, right)| right.total_cmp(left));
    ranked.truncate(expansion_cap);
    ranked.into_iter().unzip()
}

struct ChildEval {
    score: f32,
    board_score: f32,
    policy_score: f32,
    value_score: f32,
    fallback_used: bool,
}

fn evaluate_child_state(
    board: &Board,
    current: Option<Piece>,
    hold: Option<Piece>,
    queue: &[Piece],
    b2b: u8,
    combo: u32,
    pending_garbage: u8,
    lines_total: u32,
    bag_number: u32,
    pieces_into_bag: u8,
    coaching: CoachingState,
    policy_score: f32,
    ctx: &mut SearchExpansionContext<'_>,
    fallback_attack: f32,
    fallback_chain: f32,
    fallback_context: f32,
) -> ChildEval {
    if let Some((_, _, value_score)) = infer_for_state(
        board,
        current,
        hold,
        queue,
        b2b,
        combo,
        pending_garbage,
        lines_total,
        bag_number,
        pieces_into_bag,
        coaching,
        ctx,
    ) {
        let heuristic_tail = if ctx.config.heuristic_fallback_weight > 0.0 {
            ctx.config.heuristic_fallback_weight
                * assemble_composite(
                    0.0,
                    fallback_attack,
                    fallback_chain,
                    fallback_context,
                    ctx.config,
                )
        } else {
            0.0
        };
        let score = value_score + ctx.config.policy_bonus_weight * policy_score + heuristic_tail;
        return ChildEval {
            score,
            board_score: value_score,
            policy_score,
            value_score,
            fallback_used: false,
        };
    }

    let board_eval = evaluate_with_tt(
        board,
        ctx.weights,
        ctx.remaining_depth,
        ctx.zobrist_keys,
        ctx.tt,
    );
    let score = assemble_composite(board_eval, fallback_attack, fallback_chain, fallback_context, ctx.config);
    ChildEval {
        score,
        board_score: board_eval,
        policy_score,
        value_score: board_eval,
        fallback_used: true,
    }
}

pub(crate) fn gen_and_eval_root(
    state: &GameState,
    ctx: &mut SearchExpansionContext<'_>,
    nodes: &mut Vec<SearchNode>,
) {
    let fallback_actions = enumerate_actions(&state.board, Some(state.current), state.hold, &state.queue);
    let fallback_len = fallback_actions.len();
    let (actions, policy_scores) = if let Some((actions, policy_scores, _)) = infer_for_state(
        &state.board,
        Some(state.current),
        state.hold,
        &state.queue,
        state.b2b,
        state.combo,
        state.pending_garbage,
        state.lines_total,
        state.bag_number,
        state.pieces_into_bag,
        state.coaching,
        ctx,
    ) {
        maybe_limit_policy_guided_actions(actions, policy_scores, ctx)
    } else {
        (fallback_actions, vec![0.0; fallback_len])
    };

    for (action, policy_score) in actions.into_iter().zip(policy_scores.into_iter()) {
        let mut result_board = state.board.clone();
        let lines_cleared = result_board.do_move(&action.mv) as u8;
        let next_pending_garbage = state.pending_garbage.saturating_sub(lines_cleared);
        let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);
        let (next_b2b, next_combo) = GameState::next_chain_values(state.b2b, state.combo, &action.mv, lines_cleared);
        let coaching = state.coaching.transition(TransitionObservation {
            resulting_height: result_board.height(),
            resulting_b2b: next_b2b,
            resulting_combo: next_combo,
            lines_cleared,
            hold_used: action.hold_used,
            pending_garbage: state.pending_garbage,
            imminent_garbage: next_pending_garbage,
            spawn_envelope_blocked,
        });
        let next_pieces_into_bag = (state.pieces_into_bag + 1) % 7;
        let next_bag_number = if state.pieces_into_bag == 6 {
            state.bag_number.saturating_add(1)
        } else {
            state.bag_number
        };
        let next_lines_total = state.lines_total.saturating_add(lines_cleared as u32);

        let b2b_broken_from = if state.b2b >= 4 && next_b2b == 0 && lines_cleared > 0 {
            Some(state.b2b)
        } else {
            None
        };
        let clears_garbage = state.pending_garbage > 0 && lines_cleared > 0;
        let is_perfect_clear = result_board.is_empty();
        let attack_val = calculate_attack_full(&AttackContext {
            lines: lines_cleared,
            spin: action.mv.spin(),
            b2b: next_b2b,
            combo: next_combo as u8,
            config: &ctx.config.attack_config,
            is_perfect_clear,
            b2b_broken_from,
            clears_garbage,
        });
        let clear_event = if lines_cleared > 0 {
            Some(ClearEvent {
                clear_type: ClearType::from_lines(lines_cleared),
                spin_type: action.mv.spin(),
                lines_cleared,
                attack_sent: attack_val,
                b2b_before: state.b2b,
                b2b_after: next_b2b,
                combo_before: state.combo,
                combo_after: next_combo,
                is_surge_release: b2b_broken_from.is_some(),
                is_garbage_clear: clears_garbage,
                is_perfect_clear,
                piece: action.mv.piece(),
            })
        } else {
            None
        };
        let path_clear_events = match clear_event {
            Some(event) => smallvec![event],
            None => SmallVec::new(),
        };
        let chain_val = shape_chain_value(next_combo as f32);
        let combo_context = next_combo as f32 - state.combo as f32;
        let context_mod = shape_context_modifier(combo_context + coaching_context_bias(state.coaching, coaching));
        let child_eval = evaluate_child_state(
            &result_board,
            action.next_current,
            action.next_hold,
            action.next_queue.as_slice(),
            next_b2b,
            next_combo,
            next_pending_garbage,
            next_lines_total,
            next_bag_number,
            next_pieces_into_bag,
            coaching,
            policy_score,
            ctx,
            attack_val,
            chain_val,
            context_mod,
        );

        nodes.push(SearchNode {
            board: result_board,
            current: action.next_current,
            queue: action.next_queue,
            score: child_eval.score,
            hold: action.next_hold,
            b2b: next_b2b,
            combo: next_combo,
            pending_garbage: next_pending_garbage,
            lines_total: next_lines_total,
            bag_number: next_bag_number,
            pieces_into_bag: next_pieces_into_bag,
            coaching,
            root_move: action.mv,
            root_hold_used: action.hold_used,
            path: smallvec![action.mv],
            board_score: child_eval.board_score,
            attack_score: attack_val,
            chain_score: chain_val,
            context_score: context_mod,
            path_attack: attack_val,
            path_chain: chain_val,
            path_context: context_mod,
            policy_score: child_eval.policy_score,
            value_score: child_eval.value_score,
            fallback_used: child_eval.fallback_used,
            path_clear_events,
        });
    }
}

pub(crate) fn expand_node(
    parent: &SearchNode,
    ctx: &mut SearchExpansionContext<'_>,
    out: &mut Vec<SearchNode>,
) {
    let fallback_actions = enumerate_actions(&parent.board, parent.current, parent.hold, parent.queue.as_slice());
    let fallback_len = fallback_actions.len();
    let (actions, policy_scores) = if let Some((actions, policy_scores, _)) = infer_for_state(
        &parent.board,
        parent.current,
        parent.hold,
        parent.queue.as_slice(),
        parent.b2b,
        parent.combo,
        parent.pending_garbage,
        parent.lines_total,
        parent.bag_number,
        parent.pieces_into_bag,
        parent.coaching,
        ctx,
    ) {
        maybe_limit_policy_guided_actions(actions, policy_scores, ctx)
    } else {
        (fallback_actions, vec![0.0; fallback_len])
    };

    for (action, policy_score) in actions.into_iter().zip(policy_scores.into_iter()) {
        let mut result_board = parent.board.clone();
        let lines_cleared = result_board.do_move(&action.mv) as u8;
        let next_pending_garbage = parent.pending_garbage.saturating_sub(lines_cleared);
        let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);
        let (next_b2b, next_combo) =
            GameState::next_chain_values(parent.b2b, parent.combo, &action.mv, lines_cleared);
        let coaching = parent.coaching.transition(TransitionObservation {
            resulting_height: result_board.height(),
            resulting_b2b: next_b2b,
            resulting_combo: next_combo,
            lines_cleared,
            hold_used: action.hold_used,
            pending_garbage: parent.pending_garbage,
            imminent_garbage: next_pending_garbage,
            spawn_envelope_blocked,
        });
        let next_pieces_into_bag = (parent.pieces_into_bag + 1) % 7;
        let next_bag_number = if parent.pieces_into_bag == 6 {
            parent.bag_number.saturating_add(1)
        } else {
            parent.bag_number
        };
        let next_lines_total = parent.lines_total.saturating_add(lines_cleared as u32);

        let b2b_broken_from = if parent.b2b >= 4 && next_b2b == 0 && lines_cleared > 0 {
            Some(parent.b2b)
        } else {
            None
        };
        let clears_garbage = parent.pending_garbage > 0 && lines_cleared > 0;
        let is_perfect_clear = result_board.is_empty();
        let attack_val = calculate_attack_full(&AttackContext {
            lines: lines_cleared,
            spin: action.mv.spin(),
            b2b: next_b2b,
            combo: next_combo as u8,
            config: &ctx.config.attack_config,
            is_perfect_clear,
            b2b_broken_from,
            clears_garbage,
        });
        let clear_event = if lines_cleared > 0 {
            Some(ClearEvent {
                clear_type: ClearType::from_lines(lines_cleared),
                spin_type: action.mv.spin(),
                lines_cleared,
                attack_sent: attack_val,
                b2b_before: parent.b2b,
                b2b_after: next_b2b,
                combo_before: parent.combo,
                combo_after: next_combo,
                is_surge_release: b2b_broken_from.is_some(),
                is_garbage_clear: clears_garbage,
                is_perfect_clear,
                piece: action.mv.piece(),
            })
        } else {
            None
        };
        let mut path_clear_events = parent.path_clear_events.clone();
        if let Some(event) = clear_event {
            path_clear_events.push(event);
        }
        let chain_val = shape_chain_value(next_combo as f32);
        let combo_context = next_combo as f32 - parent.combo as f32;
        let context_mod = shape_context_modifier(combo_context + coaching_context_bias(parent.coaching, coaching));
        let cum_attack = parent.path_attack + attack_val;
        let cum_chain = parent.path_chain + chain_val;
        let depth_factor = (parent.path.len() as f32 + 1.0)
            .sqrt()
            .min(ctx.config.max_depth_factor);
        let child_eval = evaluate_child_state(
            &result_board,
            action.next_current,
            action.next_hold,
            action.next_queue.as_slice(),
            next_b2b,
            next_combo,
            next_pending_garbage,
            next_lines_total,
            next_bag_number,
            next_pieces_into_bag,
            coaching,
            policy_score,
            ctx,
            cum_attack / depth_factor,
            cum_chain / depth_factor,
            context_mod,
        );

        let mut path: SmallVec<[Move; 16]> = parent.path.clone();
        path.push(action.mv);

        out.push(SearchNode {
            board: result_board,
            current: action.next_current,
            queue: action.next_queue,
            score: child_eval.score,
            hold: action.next_hold,
            b2b: next_b2b,
            combo: next_combo,
            pending_garbage: next_pending_garbage,
            lines_total: next_lines_total,
            bag_number: next_bag_number,
            pieces_into_bag: next_pieces_into_bag,
            coaching,
            root_move: parent.root_move,
            root_hold_used: parent.root_hold_used,
            path,
            board_score: child_eval.board_score,
            attack_score: attack_val,
            chain_score: chain_val,
            context_score: context_mod,
            path_attack: cum_attack,
            path_chain: cum_chain,
            path_context: parent.path_context + context_mod,
            policy_score: child_eval.policy_score,
            value_score: child_eval.value_score,
            fallback_used: child_eval.fallback_used,
            path_clear_events,
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

#[cfg(test)]
mod tests {
    use super::{maybe_limit_policy_guided_actions, CandidateAction};
    use crate::attack::AttackConfig;
    use crate::eval::EvalWeights;
    use crate::header::{Move, Piece, Rotation};
    use crate::search_config::{SearchConfig, SearchExpansionContext};
    use crate::transposition::get_zobrist_keys;
    use smallvec::SmallVec;

    fn candidate(piece: Piece) -> CandidateAction {
        CandidateAction {
            mv: Move::new(piece, Rotation::North, 0, 0, false),
            hold_used: false,
            next_hold: None,
            next_current: None,
            next_queue: SmallVec::new(),
        }
    }

    fn context(cap: usize, beam_width: usize) -> SearchExpansionContext<'static> {
        let config = Box::leak(Box::new(SearchConfig {
            policy_guided_expansion_cap: cap,
            attack_config: AttackConfig::tetra_league(),
            ..SearchConfig::default()
        }));
        let weights = Box::leak(Box::new(EvalWeights::default()));
        let zobrist = Box::leak(Box::new(get_zobrist_keys()));
        let tt = Box::leak(Box::new(None));
        SearchExpansionContext {
            config,
            current_beam_width: beam_width,
            weights,
            remaining_depth: 0,
            zobrist_keys: zobrist,
            tt,
            policy_value: None,
            runtime_context: None,
        }
    }

    #[test]
    fn policy_guided_limit_keeps_top_scores_only() {
        let ctx = context(2, 8);
        let actions = vec![candidate(Piece::I), candidate(Piece::O), candidate(Piece::T)];
        let scores = vec![0.2, 0.9, 0.5];

        let (limited_actions, limited_scores) = maybe_limit_policy_guided_actions(actions, scores, &ctx);

        assert_eq!(limited_actions.len(), 2);
        assert_eq!(limited_scores, vec![0.9, 0.5]);
        assert_eq!(limited_actions[0].mv.piece(), Piece::O);
        assert_eq!(limited_actions[1].mv.piece(), Piece::T);
    }

    #[test]
    fn policy_guided_limit_respects_beam_width() {
        let ctx = context(10, 1);
        let actions = vec![candidate(Piece::I), candidate(Piece::O)];
        let scores = vec![0.2, 0.9];

        let (limited_actions, limited_scores) = maybe_limit_policy_guided_actions(actions, scores, &ctx);

        assert_eq!(limited_actions.len(), 1);
        assert_eq!(limited_scores, vec![0.9]);
        assert_eq!(limited_actions[0].mv.piece(), Piece::O);
    }

    #[test]
    fn zero_policy_guided_cap_disables_limiting() {
        let ctx = context(0, 1);
        let actions = vec![candidate(Piece::I), candidate(Piece::O)];
        let scores = vec![0.2, 0.9];

        let (limited_actions, limited_scores) = maybe_limit_policy_guided_actions(actions, scores, &ctx);

        assert_eq!(limited_actions.len(), 2);
        assert_eq!(limited_scores, vec![0.2, 0.9]);
        assert_eq!(limited_actions[0].mv.piece(), Piece::I);
        assert_eq!(limited_actions[1].mv.piece(), Piece::O);
    }

    #[test]
    fn zero_scores_preserve_ranked_prefix_size() {
        let ctx = context(2, 8);
        let actions = vec![candidate(Piece::I), candidate(Piece::O), candidate(Piece::T)];
        let scores = vec![0.0, 0.0, 0.0];

        let (limited_actions, limited_scores) = maybe_limit_policy_guided_actions(actions, scores, &ctx);

        assert_eq!(limited_actions.len(), 2);
        assert_eq!(limited_scores.len(), 2);
        assert!(limited_scores.iter().all(|score| *score == 0.0));
    }
}
