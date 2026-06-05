use crate::analysis::{assemble_composite, shape_chain_value, shape_context_modifier};
use crate::attack::{calculate_attack_full, AttackContext};
use crate::board::{Board, BOARD_HEIGHT};
use crate::eval::{evaluate, EvalWeights};
use crate::header::{Move, Piece};
use crate::move_buffer::MoveBuffer;
use crate::movegen::generate_search;
use crate::search_config::{SearchExpansionContext, SearchNode};
use crate::state::{
    ClearEvent, ClearType, CoachingState, FatalityState, GameState, ObligationState, PhaseState,
    SurgeState, TransitionObservation,
};
use crate::transposition::{TranspositionTable, ZobristKeys};
use smallvec::{smallvec, SmallVec};
#[cfg(not(target_arch = "wasm32"))]
use std::{cell::Cell, cell::RefCell, collections::HashSet, time::Instant};

#[derive(Clone, Copy, Debug, Default)]
pub struct SearchExpansionStats {
    pub expanded_nodes: u64,
    pub movegen_calls: u64,
    pub action_builder_states: u64,
    pub duplicate_action_generations: u64,
    pub action_generation_nanos: u64,
    pub legal_filter_nanos: u64,
    pub runtime_inference_nanos: u64,
    pub child_eval_nanos: u64,
    pub do_move_nanos: u64,
    pub eval_fallback_nanos: u64,
    pub tt_hash_nanos: u64,
    pub tt_probe_nanos: u64,
    pub tt_store_nanos: u64,
    pub sort_prune_truncate_nanos: u64,
    pub candidate_copy_nanos: u64,
    pub root_score_aggregation_nanos: u64,
    pub unique_action_keys: u64,
    pub repeated_action_builds: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static EXPANDED_NODES: Cell<u64> = const { Cell::new(0) };
    static MOVEGEN_CALLS: Cell<u64> = const { Cell::new(0) };
    static ACTION_BUILDER_STATES: Cell<u64> = const { Cell::new(0) };
    static DUPLICATE_ACTION_GENERATIONS: Cell<u64> = const { Cell::new(0) };
    static ACTION_GENERATION_NANOS: Cell<u64> = const { Cell::new(0) };
    static LEGAL_FILTER_NANOS: Cell<u64> = const { Cell::new(0) };
    static RUNTIME_INFERENCE_NANOS: Cell<u64> = const { Cell::new(0) };
    static CHILD_EVAL_NANOS: Cell<u64> = const { Cell::new(0) };
    static DO_MOVE_NANOS: Cell<u64> = const { Cell::new(0) };
    static EVAL_FALLBACK_NANOS: Cell<u64> = const { Cell::new(0) };
    static TT_HASH_NANOS: Cell<u64> = const { Cell::new(0) };
    static TT_PROBE_NANOS: Cell<u64> = const { Cell::new(0) };
    static TT_STORE_NANOS: Cell<u64> = const { Cell::new(0) };
    static SORT_PRUNE_TRUNCATE_NANOS: Cell<u64> = const { Cell::new(0) };
    static CANDIDATE_COPY_NANOS: Cell<u64> = const { Cell::new(0) };
    static ROOT_SCORE_AGGREGATION_NANOS: Cell<u64> = const { Cell::new(0) };
    static UNIQUE_ACTION_KEYS: Cell<u64> = const { Cell::new(0) };
    static REPEATED_ACTION_BUILDS: Cell<u64> = const { Cell::new(0) };
    static CACHE_HITS: Cell<u64> = const { Cell::new(0) };
    static CACHE_MISSES: Cell<u64> = const { Cell::new(0) };
    static PROFILING_ENABLED: Cell<bool> = const { Cell::new(false) };
    static ACTION_KEYS: RefCell<HashSet<Vec<u16>>> = RefCell::new(HashSet::new());
}

pub fn set_search_profiling_enabled(enabled: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    PROFILING_ENABLED.with(|flag| flag.set(enabled));
}

fn search_profiling_enabled() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        PROFILING_ENABLED.with(|flag| flag.get())
    }

    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub fn reset_search_expansion_stats() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        EXPANDED_NODES.with(|count| count.set(0));
        MOVEGEN_CALLS.with(|count| count.set(0));
        ACTION_BUILDER_STATES.with(|count| count.set(0));
        DUPLICATE_ACTION_GENERATIONS.with(|count| count.set(0));
        ACTION_GENERATION_NANOS.with(|count| count.set(0));
        LEGAL_FILTER_NANOS.with(|count| count.set(0));
        RUNTIME_INFERENCE_NANOS.with(|count| count.set(0));
        CHILD_EVAL_NANOS.with(|count| count.set(0));
        DO_MOVE_NANOS.with(|count| count.set(0));
        EVAL_FALLBACK_NANOS.with(|count| count.set(0));
        TT_HASH_NANOS.with(|count| count.set(0));
        TT_PROBE_NANOS.with(|count| count.set(0));
        TT_STORE_NANOS.with(|count| count.set(0));
        SORT_PRUNE_TRUNCATE_NANOS.with(|count| count.set(0));
        CANDIDATE_COPY_NANOS.with(|count| count.set(0));
        ROOT_SCORE_AGGREGATION_NANOS.with(|count| count.set(0));
        UNIQUE_ACTION_KEYS.with(|count| count.set(0));
        REPEATED_ACTION_BUILDS.with(|count| count.set(0));
        CACHE_HITS.with(|count| count.set(0));
        CACHE_MISSES.with(|count| count.set(0));
        ACTION_KEYS.with(|keys| keys.borrow_mut().clear());
    }
}

pub fn search_expansion_stats() -> SearchExpansionStats {
    #[cfg(not(target_arch = "wasm32"))]
    {
        SearchExpansionStats {
            expanded_nodes: EXPANDED_NODES.with(|count| count.get()),
            movegen_calls: MOVEGEN_CALLS.with(|count| count.get()),
            action_builder_states: ACTION_BUILDER_STATES.with(|count| count.get()),
            duplicate_action_generations: DUPLICATE_ACTION_GENERATIONS.with(|count| count.get()),
            action_generation_nanos: ACTION_GENERATION_NANOS.with(|count| count.get()),
            legal_filter_nanos: LEGAL_FILTER_NANOS.with(|count| count.get()),
            runtime_inference_nanos: RUNTIME_INFERENCE_NANOS.with(|count| count.get()),
            child_eval_nanos: CHILD_EVAL_NANOS.with(|count| count.get()),
            do_move_nanos: DO_MOVE_NANOS.with(|count| count.get()),
            eval_fallback_nanos: EVAL_FALLBACK_NANOS.with(|count| count.get()),
            tt_hash_nanos: TT_HASH_NANOS.with(|count| count.get()),
            tt_probe_nanos: TT_PROBE_NANOS.with(|count| count.get()),
            tt_store_nanos: TT_STORE_NANOS.with(|count| count.get()),
            sort_prune_truncate_nanos: SORT_PRUNE_TRUNCATE_NANOS.with(|count| count.get()),
            candidate_copy_nanos: CANDIDATE_COPY_NANOS.with(|count| count.get()),
            root_score_aggregation_nanos: ROOT_SCORE_AGGREGATION_NANOS.with(|count| count.get()),
            unique_action_keys: UNIQUE_ACTION_KEYS.with(|count| count.get()),
            repeated_action_builds: REPEATED_ACTION_BUILDS.with(|count| count.get()),
            cache_hits: CACHE_HITS.with(|count| count.get()),
            cache_misses: CACHE_MISSES.with(|count| count.get()),
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        SearchExpansionStats::default()
    }
}

#[inline]
fn record_expanded_node() {
    #[cfg(not(target_arch = "wasm32"))]
    EXPANDED_NODES.with(|count| count.set(count.get() + 1));
}

#[inline]
fn record_movegen_call() {
    #[cfg(not(target_arch = "wasm32"))]
    MOVEGEN_CALLS.with(|count| count.set(count.get() + 1));
}

#[inline]
fn record_action_builder_state() {
    #[cfg(not(target_arch = "wasm32"))]
    ACTION_BUILDER_STATES.with(|count| count.set(count.get() + 1));
}

#[inline]
fn record_duplicate_action_generation() {
    #[cfg(not(target_arch = "wasm32"))]
    DUPLICATE_ACTION_GENERATIONS.with(|count| count.set(count.get() + 1));
}

#[inline]
fn piece_key(piece: Option<Piece>) -> u16 {
    match piece {
        Some(Piece::I) => 1,
        Some(Piece::J) => 2,
        Some(Piece::L) => 3,
        Some(Piece::O) => 4,
        Some(Piece::S) => 5,
        Some(Piece::T) => 6,
        Some(Piece::Z) => 7,
        None => 0,
    }
}

#[inline]
fn record_action_key(board: &Board, current: Option<Piece>, hold: Option<Piece>, queue: &[Piece]) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return;
        }
        let mut key = Vec::with_capacity(BOARD_HEIGHT + 2 + queue.len());
        key.extend_from_slice(&board.rows);
        key.push(piece_key(current));
        key.push(piece_key(hold));
        key.extend(queue.iter().map(|piece| piece_key(Some(*piece))));
        ACTION_KEYS.with(|keys| {
            if keys.borrow_mut().insert(key) {
                UNIQUE_ACTION_KEYS.with(|count| count.set(count.get() + 1));
            } else {
                REPEATED_ACTION_BUILDS.with(|count| count.set(count.get() + 1));
            }
        });
    }
}

#[inline]
fn record_cache_hit() {
    #[cfg(not(target_arch = "wasm32"))]
    CACHE_HITS.with(|count| count.set(count.get() + 1));
}

#[inline]
fn record_cache_miss() {
    #[cfg(not(target_arch = "wasm32"))]
    CACHE_MISSES.with(|count| count.set(count.get() + 1));
}

#[cfg(not(target_arch = "wasm32"))]
fn add_elapsed(cell: &'static std::thread::LocalKey<Cell<u64>>, started: Instant) {
    let nanos = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    cell.with(|total| total.set(total.get().saturating_add(nanos)));
}

#[inline]
fn profile_action_generation<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&ACTION_GENERATION_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_legal_filter<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&LEGAL_FILTER_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_runtime_inference<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&RUNTIME_INFERENCE_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_child_eval<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&CHILD_EVAL_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_do_move<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&DO_MOVE_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_eval_fallback<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&EVAL_FALLBACK_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_tt_probe<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&TT_PROBE_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_tt_hash<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&TT_HASH_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_tt_store<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&TT_STORE_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
pub(crate) fn profile_sort_prune_truncate<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&SORT_PRUNE_TRUNCATE_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
fn profile_candidate_copy<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&CANDIDATE_COPY_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[inline]
pub(crate) fn profile_root_score_aggregation<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !search_profiling_enabled() {
            return f();
        }
        let started = Instant::now();
        let result = f();
        add_elapsed(&ROOT_SCORE_AGGREGATION_NANOS, started);
        result
    }

    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

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
    let tail = if consumed >= queue.len() {
        &[][..]
    } else {
        &queue[consumed..]
    };
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
    record_movegen_call();
    profile_action_generation(|| generate_search(board, &mut moves, piece));
    profile_legal_filter(|| {
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
    });
}

fn enumerate_actions(
    board: &Board,
    current: Option<Piece>,
    hold: Option<Piece>,
    queue: &[Piece],
) -> Vec<CandidateAction> {
    record_action_builder_state();
    record_action_key(board, current, hold, queue);
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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
    ctx.policy_value?;
    ctx.runtime_context?;
    let actions = enumerate_actions(board, current, hold, queue);
    let (policy_logits, value) = infer_for_actions(
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
        &actions,
        ctx,
    )?;
    Some((actions, policy_logits, value))
}

#[allow(clippy::too_many_arguments)]
fn infer_for_actions(
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
    actions: &[CandidateAction],
    ctx: &SearchExpansionContext<'_>,
) -> Option<(Vec<f32>, f32)> {
    let runtime = ctx.policy_value?;
    let runtime_context = ctx.runtime_context?;
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
    let candidates: Vec<Move> =
        profile_candidate_copy(|| actions.iter().map(|action| action.mv).collect());
    let inference =
        profile_runtime_inference(|| runtime.infer(&state, runtime_context, &candidates)).ok()?;
    record_duplicate_action_generation();
    Some((inference.policy_logits, inference.value))
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

#[allow(clippy::too_many_arguments)]
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
    let score = assemble_composite(
        board_eval,
        fallback_attack,
        fallback_chain,
        fallback_context,
        ctx.config,
    );
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
    record_expanded_node();
    let actions = enumerate_actions(&state.board, Some(state.current), state.hold, &state.queue);
    let fallback_len = actions.len();
    let (actions, policy_scores) = if let Some((policy_scores, _)) = infer_for_actions(
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
        &actions,
        ctx,
    ) {
        maybe_limit_policy_guided_actions(actions, policy_scores, ctx)
    } else {
        (actions, vec![0.0; fallback_len])
    };

    for (action, policy_score) in actions.into_iter().zip(policy_scores) {
        let mut result_board = state.board.clone();
        let lines_cleared = profile_do_move(|| result_board.do_move(&action.mv)) as u8;
        let next_pending_garbage = state.pending_garbage.saturating_sub(lines_cleared);
        let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);
        let (next_b2b, next_combo) =
            GameState::next_chain_values(state.b2b, state.combo, &action.mv, lines_cleared);
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
        let context_mod =
            shape_context_modifier(combo_context + coaching_context_bias(state.coaching, coaching));
        let child_eval = profile_child_eval(|| {
            evaluate_child_state(
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
            )
        });

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
    record_expanded_node();
    let actions = enumerate_actions(
        &parent.board,
        parent.current,
        parent.hold,
        parent.queue.as_slice(),
    );
    let fallback_len = actions.len();
    let (actions, policy_scores) = if let Some((policy_scores, _)) = infer_for_actions(
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
        &actions,
        ctx,
    ) {
        maybe_limit_policy_guided_actions(actions, policy_scores, ctx)
    } else {
        (actions, vec![0.0; fallback_len])
    };

    for (action, policy_score) in actions.into_iter().zip(policy_scores) {
        let mut result_board = parent.board.clone();
        let lines_cleared = profile_do_move(|| result_board.do_move(&action.mv)) as u8;
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
        let context_mod = shape_context_modifier(
            combo_context + coaching_context_bias(parent.coaching, coaching),
        );
        let cum_attack = parent.path_attack + attack_val;
        let cum_chain = parent.path_chain + chain_val;
        let depth_factor = (parent.path.len() as f32 + 1.0)
            .sqrt()
            .min(ctx.config.max_depth_factor);
        let child_eval = profile_child_eval(|| {
            evaluate_child_state(
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
            )
        });

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
        let hash = profile_tt_hash(|| zobrist_keys.hash_board(board));

        if let Some(score) = profile_tt_probe(|| table.probe(hash, depth)) {
            record_cache_hit();
            return score;
        }
        record_cache_miss();

        let score = profile_eval_fallback(|| evaluate(board, weights));
        profile_tt_store(|| table.store(hash, depth, score));
        return score;
    }

    profile_eval_fallback(|| evaluate(board, weights))
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
        let actions = vec![
            candidate(Piece::I),
            candidate(Piece::O),
            candidate(Piece::T),
        ];
        let scores = vec![0.2, 0.9, 0.5];

        let (limited_actions, limited_scores) =
            maybe_limit_policy_guided_actions(actions, scores, &ctx);

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

        let (limited_actions, limited_scores) =
            maybe_limit_policy_guided_actions(actions, scores, &ctx);

        assert_eq!(limited_actions.len(), 1);
        assert_eq!(limited_scores, vec![0.9]);
        assert_eq!(limited_actions[0].mv.piece(), Piece::O);
    }

    #[test]
    fn zero_policy_guided_cap_disables_limiting() {
        let ctx = context(0, 1);
        let actions = vec![candidate(Piece::I), candidate(Piece::O)];
        let scores = vec![0.2, 0.9];

        let (limited_actions, limited_scores) =
            maybe_limit_policy_guided_actions(actions, scores, &ctx);

        assert_eq!(limited_actions.len(), 2);
        assert_eq!(limited_scores, vec![0.2, 0.9]);
        assert_eq!(limited_actions[0].mv.piece(), Piece::I);
        assert_eq!(limited_actions[1].mv.piece(), Piece::O);
    }

    #[test]
    fn zero_scores_preserve_ranked_prefix_size() {
        let ctx = context(2, 8);
        let actions = vec![
            candidate(Piece::I),
            candidate(Piece::O),
            candidate(Piece::T),
        ];
        let scores = vec![0.0, 0.0, 0.0];

        let (limited_actions, limited_scores) =
            maybe_limit_policy_guided_actions(actions, scores, &ctx);

        assert_eq!(limited_actions.len(), 2);
        assert_eq!(limited_scores.len(), 2);
        assert!(limited_scores.iter().all(|score| *score == 0.0));
    }

    #[test]
    fn expansion_stats_include_profiler_buckets() {
        super::reset_search_expansion_stats();
        let stats = super::search_expansion_stats();

        assert_eq!(stats.action_generation_nanos, 0);
        assert_eq!(stats.legal_filter_nanos, 0);
        assert_eq!(stats.runtime_inference_nanos, 0);
        assert_eq!(stats.child_eval_nanos, 0);
        assert_eq!(stats.do_move_nanos, 0);
        assert_eq!(stats.eval_fallback_nanos, 0);
        assert_eq!(stats.tt_hash_nanos, 0);
        assert_eq!(stats.tt_probe_nanos, 0);
        assert_eq!(stats.tt_store_nanos, 0);
        assert_eq!(stats.sort_prune_truncate_nanos, 0);
        assert_eq!(stats.candidate_copy_nanos, 0);
        assert_eq!(stats.root_score_aggregation_nanos, 0);
        assert_eq!(stats.unique_action_keys, 0);
        assert_eq!(stats.repeated_action_builds, 0);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.cache_misses, 0);
    }
}
