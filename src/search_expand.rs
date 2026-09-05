use crate::analysis::{assemble_composite, shape_chain_value, shape_context_modifier};
use crate::board::{Board, BOARD_HEIGHT};
use crate::eval::evaluate;
use crate::header::{Move, Piece};
use crate::movegen::generate_search;
use crate::policy_value_runtime::{CANDIDATE_CAPACITY, MAX_INFER_BATCH};
use crate::search::level_batch_enabled;
use crate::search_config::{NnScoringMode, SearchExpansionContext, SearchNode};
use crate::search_config::{MAX_DEPTH_FACTOR, POLICY_BONUS_WEIGHT};
use crate::state::{
    ChainState, CoachingState, FatalityState, GameState, ObligationState, SurgeState,
};
use smallvec::{smallvec, SmallVec};
use std::sync::Arc;
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
    pub sort_prune_truncate_nanos: u64,
    pub candidate_copy_nanos: u64,
    pub root_score_aggregation_nanos: u64,
    pub unique_action_keys: u64,
    pub repeated_action_builds: u64,
    pub runtime_attempt_rows: u64,
    pub runtime_unavailable_nodes: u64,
    pub abandoned_nodes: u64,
    pub runtime_calls: u64,
    pub batch_calls: u64,
    pub inferred_rows: u64,
    pub max_batch_rows: u64,
    pub deadline_checks: u64,
    pub deadline_hits: u64,
    pub completed_depth: u32,
    pub completed_width: u32,
    pub fallback_levels: u64,
    pub abandoned_levels: u64,
    pub noninferable_nodes: u64,
    pub q_extensions_completed: u64,
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
    static SORT_PRUNE_TRUNCATE_NANOS: Cell<u64> = const { Cell::new(0) };
    static CANDIDATE_COPY_NANOS: Cell<u64> = const { Cell::new(0) };
    static ROOT_SCORE_AGGREGATION_NANOS: Cell<u64> = const { Cell::new(0) };
    static UNIQUE_ACTION_KEYS: Cell<u64> = const { Cell::new(0) };
    static REPEATED_ACTION_BUILDS: Cell<u64> = const { Cell::new(0) };
    static RUNTIME_ATTEMPT_ROWS: Cell<u64> = const { Cell::new(0) };
    static RUNTIME_UNAVAILABLE_NODES: Cell<u64> = const { Cell::new(0) };
    static ABANDONED_NODES: Cell<u64> = const { Cell::new(0) };
    static RUNTIME_CALLS: Cell<u64> = const { Cell::new(0) };
    static BATCH_CALLS: Cell<u64> = const { Cell::new(0) };
    static INFERRED_ROWS: Cell<u64> = const { Cell::new(0) };
    static MAX_BATCH_ROWS: Cell<u64> = const { Cell::new(0) };
    static DEADLINE_CHECKS: Cell<u64> = const { Cell::new(0) };
    static DEADLINE_HITS: Cell<u64> = const { Cell::new(0) };
    static COMPLETED_DEPTH: Cell<u32> = const { Cell::new(0) };
    static COMPLETED_WIDTH: Cell<u32> = const { Cell::new(0) };
    static FALLBACK_LEVELS: Cell<u64> = const { Cell::new(0) };
    static ABANDONED_LEVELS: Cell<u64> = const { Cell::new(0) };
    static NONINFERABLE_NODES: Cell<u64> = const { Cell::new(0) };
    static Q_EXTENSIONS_COMPLETED: Cell<u64> = const { Cell::new(0) };
    static PROFILING_ENABLED: Cell<bool> = const { Cell::new(false) };
    static ACTION_KEYS: RefCell<HashSet<Vec<u16>>> = RefCell::new(HashSet::new());
    #[cfg(test)]
    static POISON_BATCH_CHUNK: Cell<Option<usize>> = const { Cell::new(None) };
    #[cfg(test)]
    static POISON_SCALAR_INFERENCE: Cell<bool> = const { Cell::new(false) };
    #[cfg(test)]
    static LEVEL_ROOT_ENTERED: Cell<bool> = const { Cell::new(false) };
    #[cfg(test)]
    static LEVEL_EXPAND_ENTERED: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn set_poison_batch_chunk(chunk: Option<usize>) {
    POISON_BATCH_CHUNK.with(|value| value.set(chunk));
}

#[cfg(test)]
pub(crate) fn set_poison_scalar_inference(enabled: bool) {
    POISON_SCALAR_INFERENCE.with(|value| value.set(enabled));
}

#[cfg(test)]
pub(crate) fn reset_level_path_markers() {
    LEVEL_ROOT_ENTERED.with(|value| value.set(false));
    LEVEL_EXPAND_ENTERED.with(|value| value.set(false));
}

#[cfg(test)]
pub(crate) fn level_path_markers() -> (bool, bool) {
    (
        LEVEL_ROOT_ENTERED.with(Cell::get),
        LEVEL_EXPAND_ENTERED.with(Cell::get),
    )
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
        SORT_PRUNE_TRUNCATE_NANOS.with(|count| count.set(0));
        CANDIDATE_COPY_NANOS.with(|count| count.set(0));
        ROOT_SCORE_AGGREGATION_NANOS.with(|count| count.set(0));
        UNIQUE_ACTION_KEYS.with(|count| count.set(0));
        REPEATED_ACTION_BUILDS.with(|count| count.set(0));
        RUNTIME_ATTEMPT_ROWS.with(|count| count.set(0));
        RUNTIME_UNAVAILABLE_NODES.with(|count| count.set(0));
        ABANDONED_NODES.with(|count| count.set(0));
        RUNTIME_CALLS.with(|count| count.set(0));
        BATCH_CALLS.with(|count| count.set(0));
        INFERRED_ROWS.with(|count| count.set(0));
        MAX_BATCH_ROWS.with(|count| count.set(0));
        DEADLINE_CHECKS.with(|count| count.set(0));
        DEADLINE_HITS.with(|count| count.set(0));
        COMPLETED_DEPTH.with(|count| count.set(0));
        COMPLETED_WIDTH.with(|count| count.set(0));
        FALLBACK_LEVELS.with(|count| count.set(0));
        ABANDONED_LEVELS.with(|count| count.set(0));
        NONINFERABLE_NODES.with(|count| count.set(0));
        Q_EXTENSIONS_COMPLETED.with(|count| count.set(0));
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
            sort_prune_truncate_nanos: SORT_PRUNE_TRUNCATE_NANOS.with(|count| count.get()),
            candidate_copy_nanos: CANDIDATE_COPY_NANOS.with(|count| count.get()),
            root_score_aggregation_nanos: ROOT_SCORE_AGGREGATION_NANOS.with(|count| count.get()),
            unique_action_keys: UNIQUE_ACTION_KEYS.with(|count| count.get()),
            repeated_action_builds: REPEATED_ACTION_BUILDS.with(|count| count.get()),
            runtime_attempt_rows: RUNTIME_ATTEMPT_ROWS.with(|count| count.get()),
            runtime_unavailable_nodes: RUNTIME_UNAVAILABLE_NODES.with(|count| count.get()),
            abandoned_nodes: ABANDONED_NODES.with(|count| count.get()),
            runtime_calls: RUNTIME_CALLS.with(|count| count.get()),
            batch_calls: BATCH_CALLS.with(|count| count.get()),
            inferred_rows: INFERRED_ROWS.with(|count| count.get()),
            max_batch_rows: MAX_BATCH_ROWS.with(|count| count.get()),
            deadline_checks: DEADLINE_CHECKS.with(|count| count.get()),
            deadline_hits: DEADLINE_HITS.with(|count| count.get()),
            completed_depth: COMPLETED_DEPTH.with(|count| count.get()),
            completed_width: COMPLETED_WIDTH.with(|count| count.get()),
            fallback_levels: FALLBACK_LEVELS.with(|count| count.get()),
            abandoned_levels: ABANDONED_LEVELS.with(|count| count.get()),
            noninferable_nodes: NONINFERABLE_NODES.with(|count| count.get()),
            q_extensions_completed: Q_EXTENSIONS_COMPLETED.with(|count| count.get()),
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
#[cfg(not(target_arch = "wasm32"))]
fn increment_profile_counter(cell: &'static std::thread::LocalKey<Cell<u64>>) {
    if search_profiling_enabled() {
        cell.with(|count| count.set(count.get().saturating_add(1)));
    }
}

#[inline]
fn record_runtime_attempt() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        increment_profile_counter(&RUNTIME_ATTEMPT_ROWS);
        increment_profile_counter(&RUNTIME_CALLS);
    }
}

#[inline]
fn record_runtime_unavailable() {
    #[cfg(not(target_arch = "wasm32"))]
    increment_profile_counter(&RUNTIME_UNAVAILABLE_NODES);
}

#[inline]
fn record_inferred_row() {
    #[cfg(not(target_arch = "wasm32"))]
    increment_profile_counter(&INFERRED_ROWS);
}

#[inline]
fn record_batch_dispatch(rows: usize) {
    #[cfg(not(target_arch = "wasm32"))]
    if search_profiling_enabled() {
        let rows = rows.min(u64::MAX as usize) as u64;
        RUNTIME_ATTEMPT_ROWS.with(|count| count.set(count.get().saturating_add(rows)));
        RUNTIME_CALLS.with(|count| count.set(count.get().saturating_add(1)));
        BATCH_CALLS.with(|count| count.set(count.get().saturating_add(1)));
        MAX_BATCH_ROWS.with(|count| count.set(count.get().max(rows)));
    }
}

#[inline]
fn record_inferred_rows(rows: usize) {
    #[cfg(not(target_arch = "wasm32"))]
    if search_profiling_enabled() {
        let rows = rows.min(u64::MAX as usize) as u64;
        INFERRED_ROWS.with(|count| count.set(count.get().saturating_add(rows)));
    }
}

#[inline]
fn record_abandoned_nodes(nodes: usize) {
    #[cfg(not(target_arch = "wasm32"))]
    if search_profiling_enabled() {
        let nodes = nodes.min(u64::MAX as usize) as u64;
        ABANDONED_NODES.with(|count| count.set(count.get().saturating_add(nodes)));
    }
}

#[inline]
fn record_fallback_level() {
    #[cfg(not(target_arch = "wasm32"))]
    increment_profile_counter(&FALLBACK_LEVELS);
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn level_deadline_expired(ctx: &SearchExpansionContext<'_>) -> bool {
    ctx.deadline.is_some_and(|deadline| {
        let expired = deadline.expired();
        record_deadline_check(expired);
        expired
    })
}

#[cfg(target_arch = "wasm32")]
#[inline]
fn level_deadline_expired(_ctx: &SearchExpansionContext<'_>) -> bool {
    false
}

#[inline]
fn record_noninferable_node() {
    #[cfg(not(target_arch = "wasm32"))]
    increment_profile_counter(&NONINFERABLE_NODES);
}

#[inline]
pub(crate) fn record_deadline_check(expired: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        increment_profile_counter(&DEADLINE_CHECKS);
        if expired {
            increment_profile_counter(&DEADLINE_HITS);
        }
    }
}

#[inline]
pub(crate) fn record_completed_search(depth: usize, width: usize) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if search_profiling_enabled() {
            COMPLETED_DEPTH.with(|count| count.set(depth.min(u32::MAX as usize) as u32));
            COMPLETED_WIDTH.with(|count| count.set(width.min(u32::MAX as usize) as u32));
        }
    }
}

#[inline]
pub(crate) fn record_abandoned_level() {
    #[cfg(not(target_arch = "wasm32"))]
    increment_profile_counter(&ABANDONED_LEVELS);
}

#[inline]
pub(crate) fn record_q_extension_completed() {
    #[cfg(not(target_arch = "wasm32"))]
    increment_profile_counter(&Q_EXTENSIONS_COMPLETED);
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
        fatality + obligation + surge
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

struct ActionTransition {
    next_hold: Option<Piece>,
    hold_used: bool,
    next_current: Option<Piece>,
    next_queue: SmallVec<[Piece; 16]>,
}

fn push_actions(
    ctx: &mut SearchExpansionContext<'_>,
    actions: &mut Vec<CandidateAction>,
    board: &Board,
    piece: Piece,
    transition: ActionTransition,
) {
    ctx.with_move_scratch(|moves| {
        record_movegen_call();
        profile_action_generation(|| generate_search(board, moves, piece));
        profile_legal_filter(|| {
            for mv in moves.as_slice() {
                if board.legal_lock_placement(mv) {
                    actions.push(CandidateAction {
                        mv: *mv,
                        hold_used: transition.hold_used,
                        next_hold: transition.next_hold,
                        next_current: transition.next_current,
                        next_queue: transition.next_queue.clone(),
                    });
                }
            }
        });
    });
}

fn enumerate_actions(
    ctx: &mut SearchExpansionContext<'_>,
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
            ctx,
            &mut actions,
            board,
            current_piece,
            ActionTransition {
                next_hold: hold,
                hold_used: false,
                next_current,
                next_queue,
            },
        );

        if let Some(held_piece) = hold {
            let (next_current, next_queue) = split_next_queue(queue, 0);
            push_actions(
                ctx,
                &mut actions,
                board,
                held_piece,
                ActionTransition {
                    next_hold: Some(current_piece),
                    hold_used: true,
                    next_current,
                    next_queue,
                },
            );
        } else if let Some(&queue_piece) = queue.first() {
            let (next_current, next_queue) = split_next_queue(queue, 1);
            push_actions(
                ctx,
                &mut actions,
                board,
                queue_piece,
                ActionTransition {
                    next_hold: Some(current_piece),
                    hold_used: true,
                    next_current,
                    next_queue,
                },
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

enum PerChildInference {
    UnavailableRuntime,
    NonInferable,
    Succeeded(f32),
    Failed,
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
    ctx: &mut SearchExpansionContext<'_>,
) -> PerChildInference {
    if ctx.policy_value.is_none() || ctx.runtime_context.is_none() {
        return PerChildInference::UnavailableRuntime;
    }
    let actions = enumerate_actions(ctx, board, current, hold, queue);
    if actions.is_empty() || actions.len() > CANDIDATE_CAPACITY || current.is_none() {
        return PerChildInference::NonInferable;
    }
    let Some((_, value)) = infer_for_actions(
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
        false,
    ) else {
        return PerChildInference::Failed;
    };
    PerChildInference::Succeeded(value)
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
    counts_node_row: bool,
) -> Option<(Vec<f32>, f32)> {
    if actions.is_empty() || actions.len() > CANDIDATE_CAPACITY || current.is_none() {
        if counts_node_row {
            record_noninferable_node();
        }
        return None;
    }
    let (Some(runtime), Some(runtime_context)) = (ctx.policy_value, ctx.runtime_context) else {
        if counts_node_row {
            record_runtime_unavailable();
        }
        return None;
    };
    let Some(state) = build_runtime_state(
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
    ) else {
        if counts_node_row {
            record_noninferable_node();
        }
        return None;
    };
    let candidates: Vec<Move> =
        profile_candidate_copy(|| actions.iter().map(|action| action.mv).collect());
    if counts_node_row {
        record_runtime_attempt();
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        increment_profile_counter(&RUNTIME_CALLS);
    }
    let inference =
        profile_runtime_inference(|| runtime.infer(&state, runtime_context, &candidates)).ok()?;
    if counts_node_row {
        record_inferred_row();
    }
    record_duplicate_action_generation();
    Some((inference.policy_logits, inference.value))
}

enum ProxyInference {
    UnavailableRuntime,
    NonInferable,
    Succeeded {
        policy_logits: Vec<f32>,
        parent_value: f32,
    },
    Failed,
}

#[allow(clippy::too_many_arguments)]
fn infer_policy_proxy(
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
) -> ProxyInference {
    if actions.is_empty() || actions.len() > CANDIDATE_CAPACITY || current.is_none() {
        record_noninferable_node();
        return ProxyInference::NonInferable;
    }
    let (Some(runtime), Some(runtime_context)) = (ctx.policy_value, ctx.runtime_context) else {
        record_runtime_unavailable();
        return ProxyInference::UnavailableRuntime;
    };
    let Some(state) = build_runtime_state(
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
    ) else {
        record_noninferable_node();
        return ProxyInference::NonInferable;
    };
    let candidates: Vec<Move> =
        profile_candidate_copy(|| actions.iter().map(|action| action.mv).collect());
    record_runtime_attempt();
    #[cfg(test)]
    let poisoned = POISON_SCALAR_INFERENCE.with(|value| value.replace(false));
    #[cfg(not(test))]
    let poisoned = false;
    let candidate_slice = if poisoned { &[] } else { candidates.as_slice() };
    match profile_runtime_inference(|| runtime.infer(&state, runtime_context, candidate_slice)) {
        Ok(inference) => {
            record_inferred_row();
            ProxyInference::Succeeded {
                policy_logits: inference.policy_logits,
                parent_value: inference.value,
            }
        }
        Err(_) => ProxyInference::Failed,
    }
}

#[allow(clippy::too_many_arguments)]
fn infer_policy_proxy_level_root(
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
) -> ProxyInference {
    #[cfg(test)]
    LEVEL_ROOT_ENTERED.with(|value| value.set(true));
    if actions.is_empty() || actions.len() > CANDIDATE_CAPACITY || current.is_none() {
        record_noninferable_node();
        return ProxyInference::NonInferable;
    }
    let (Some(runtime), Some(runtime_context)) = (ctx.policy_value, ctx.runtime_context) else {
        record_runtime_unavailable();
        return ProxyInference::UnavailableRuntime;
    };
    let Some(state) = build_runtime_state(
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
    ) else {
        record_noninferable_node();
        return ProxyInference::NonInferable;
    };
    let candidates: Vec<Move> =
        profile_candidate_copy(|| actions.iter().map(|action| action.mv).collect());
    record_batch_dispatch(1);
    match profile_runtime_inference(|| {
        runtime.infer_batch_chunk(&[&state], runtime_context, &[candidates.as_slice()])
    }) {
        Ok(mut rows) => {
            record_inferred_rows(1);
            let Some((policy_logits, parent_value)) = rows.pop() else {
                return ProxyInference::Failed;
            };
            ProxyInference::Succeeded {
                policy_logits,
                parent_value,
            }
        }
        Err(_) => ProxyInference::Failed,
    }
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
    nn_parent_value: Option<f32>,
}

struct PreparedLevelRow {
    actions: Vec<CandidateAction>,
    runtime_state: Option<GameState>,
}

pub(crate) enum LevelExpansion {
    Completed(Vec<SearchNode>),
    Abandoned,
}

pub(crate) fn expand_level_batched(
    parents: &[SearchNode],
    ctx: &mut SearchExpansionContext<'_>,
) -> LevelExpansion {
    #[cfg(test)]
    LEVEL_EXPAND_ENTERED.with(|value| value.set(true));
    let runtime_available = ctx.policy_value.is_some() && ctx.runtime_context.is_some();
    let mut rows = Vec::with_capacity(parents.len());
    let mut inferable_indices = Vec::with_capacity(parents.len());
    for (index, parent) in parents.iter().enumerate() {
        record_expanded_node();
        let actions = enumerate_actions(
            ctx,
            &parent.board,
            parent.current,
            parent.hold,
            parent.queue.as_slice(),
        );
        let runtime_state =
            if actions.is_empty() || actions.len() > CANDIDATE_CAPACITY || parent.current.is_none()
            {
                record_noninferable_node();
                None
            } else if !runtime_available {
                record_runtime_unavailable();
                None
            } else {
                build_runtime_state(
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
                )
            };
        if runtime_state.is_some() {
            inferable_indices.push(index);
        } else if runtime_available
            && !actions.is_empty()
            && actions.len() <= CANDIDATE_CAPACITY
            && parent.current.is_some()
        {
            record_noninferable_node();
        }
        rows.push(PreparedLevelRow {
            actions,
            runtime_state,
        });
    }

    if inferable_indices.is_empty() {
        return materialize_level(parents, rows, vec![None; parents.len()], false, ctx);
    }

    let Some((runtime, runtime_context)) = ctx.policy_value.zip(ctx.runtime_context) else {
        return materialize_level(parents, rows, vec![None; parents.len()], false, ctx);
    };
    let mut outputs: Vec<Option<(Vec<f32>, f32)>> = vec![None; parents.len()];
    for (chunk_index, chunk) in inferable_indices.chunks(MAX_INFER_BATCH).enumerate() {
        if level_deadline_expired(ctx) {
            record_abandoned_nodes(inferable_indices.len() - chunk_index * MAX_INFER_BATCH);
            return LevelExpansion::Abandoned;
        }
        let states: Vec<&GameState> = chunk
            .iter()
            .map(|index| {
                rows[*index]
                    .runtime_state
                    .as_ref()
                    .unwrap_or_else(|| unreachable!("inferable row has a runtime state"))
            })
            .collect();
        let candidates: Vec<Vec<Move>> = chunk
            .iter()
            .map(|index| {
                rows[*index]
                    .actions
                    .iter()
                    .map(|action| action.mv)
                    .collect()
            })
            .collect();
        let mut candidate_slices: Vec<&[Move]> = candidates.iter().map(Vec::as_slice).collect();
        record_batch_dispatch(chunk.len());
        #[cfg(test)]
        let poisoned = POISON_BATCH_CHUNK.with(|value| {
            if inferable_indices.len() > MAX_INFER_BATCH && value.get() == Some(chunk_index) {
                value.set(None);
                true
            } else {
                false
            }
        });
        #[cfg(not(test))]
        let poisoned = false;
        if poisoned {
            candidate_slices[0] = &[];
        }
        let inference = profile_runtime_inference(|| {
            runtime.infer_batch_chunk(&states, runtime_context, &candidate_slices)
        })
        .ok();
        let Some(inference) = inference else {
            let dispatched = (chunk_index + 1) * MAX_INFER_BATCH;
            record_abandoned_nodes(inferable_indices.len().saturating_sub(dispatched));
            record_fallback_level();
            return materialize_level(parents, rows, vec![None; parents.len()], true, ctx);
        };
        record_inferred_rows(chunk.len());
        for (parent_index, output) in chunk.iter().copied().zip(inference) {
            outputs[parent_index] = Some(output);
        }
    }

    materialize_level(parents, rows, outputs, false, ctx)
}

fn materialize_level(
    parents: &[SearchNode],
    rows: Vec<PreparedLevelRow>,
    outputs: Vec<Option<(Vec<f32>, f32)>>,
    fallback: bool,
    ctx: &mut SearchExpansionContext<'_>,
) -> LevelExpansion {
    let mut children = Vec::with_capacity(ctx.current_beam_width.saturating_mul(2));
    for ((parent, row), output) in parents.iter().zip(rows).zip(outputs) {
        if level_deadline_expired(ctx) {
            return LevelExpansion::Abandoned;
        }
        let fallback_len = row.actions.len();
        let (actions, policy_scores, parent_value) = match output {
            Some((policy_scores, parent_value)) if !fallback => {
                let (actions, policy_scores) =
                    maybe_limit_policy_guided_actions(row.actions, policy_scores, ctx);
                (actions, policy_scores, Some(parent_value))
            }
            Some(_) | None => (row.actions, vec![0.0; fallback_len], None),
        };
        materialize_node_children(
            parent,
            actions,
            policy_scores,
            parent_value,
            fallback,
            ctx,
            &mut children,
        );
    }
    LevelExpansion::Completed(children)
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
    proxy_parent_value: Option<f32>,
    proxy_fallback_used: bool,
) -> ChildEval {
    if ctx.config.nn_scoring == NnScoringMode::PolicyProxy {
        let board_eval = profile_eval_fallback(|| evaluate(board, ctx.weights));
        let score = assemble_composite(
            board_eval,
            fallback_attack,
            fallback_chain,
            fallback_context,
        ) + ctx.config.policy_proxy_weight * policy_score;
        return ChildEval {
            score,
            board_score: board_eval,
            policy_score,
            value_score: board_eval,
            fallback_used: proxy_fallback_used,
            nn_parent_value: proxy_parent_value,
        };
    }
    let inference = infer_for_state(
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
    );
    if let PerChildInference::Succeeded(value_score) = inference {
        return ChildEval {
            score: value_score + POLICY_BONUS_WEIGHT * policy_score,
            board_score: value_score,
            policy_score,
            value_score,
            fallback_used: false,
            nn_parent_value: proxy_parent_value,
        };
    }

    let board_eval = profile_eval_fallback(|| evaluate(board, ctx.weights));
    let score = assemble_composite(
        board_eval,
        fallback_attack,
        fallback_chain,
        fallback_context,
    );
    ChildEval {
        score,
        board_score: board_eval,
        policy_score,
        value_score: board_eval,
        fallback_used: matches!(inference, PerChildInference::Failed),
        nn_parent_value: proxy_parent_value,
    }
}

pub(crate) fn gen_and_eval_root(
    state: &GameState,
    ctx: &mut SearchExpansionContext<'_>,
    nodes: &mut Vec<SearchNode>,
) {
    record_expanded_node();
    let actions = enumerate_actions(
        ctx,
        &state.board,
        Some(state.current),
        state.hold,
        &state.queue,
    );
    let fallback_len = actions.len();
    let (actions, policy_scores, parent_value, proxy_fallback_used) = match ctx.config.nn_scoring {
        NnScoringMode::PerChildValue => {
            if let Some((policy_scores, parent_value)) = infer_for_actions(
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
                true,
            ) {
                let (actions, policy_scores) =
                    maybe_limit_policy_guided_actions(actions, policy_scores, ctx);
                (actions, policy_scores, Some(parent_value), false)
            } else {
                (actions, vec![0.0; fallback_len], None, false)
            }
        }
        NnScoringMode::PolicyProxy => {
            match if level_batch_enabled(ctx.config, ctx.policy_value, ctx.runtime_context) {
                infer_policy_proxy_level_root(
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
                )
            } else {
                infer_policy_proxy(
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
                )
            } {
                ProxyInference::Succeeded {
                    policy_logits,
                    parent_value,
                } => {
                    let (actions, policy_scores) =
                        maybe_limit_policy_guided_actions(actions, policy_logits, ctx);
                    (actions, policy_scores, Some(parent_value), false)
                }
                ProxyInference::Failed => {
                    if level_batch_enabled(ctx.config, ctx.policy_value, ctx.runtime_context) {
                        record_fallback_level();
                    }
                    (actions, vec![0.0; fallback_len], None, true)
                }
                ProxyInference::UnavailableRuntime | ProxyInference::NonInferable => {
                    (actions, vec![0.0; fallback_len], None, false)
                }
            }
        }
    };

    for (action, policy_score) in actions.into_iter().zip(policy_scores) {
        let mut result_board = state.board.clone();
        let mechanics = profile_do_move(|| result_board.lock(&action.mv));
        let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);
        let transition = state.chain_state().advance_lock(
            &action.mv,
            &mechanics,
            action.hold_used,
            spawn_envelope_blocked,
            &ctx.config.attack_config,
        );
        let path_clear_events = match transition.clear_event {
            Some(event) => Arc::new(vec![event]),
            None => Arc::new(Vec::new()),
        };
        let chain_val = shape_chain_value(transition.chain.combo as f32);
        let combo_context = transition.chain.combo as f32 - state.combo as f32;
        let context_mod = shape_context_modifier(
            combo_context + coaching_context_bias(state.coaching, transition.chain.coaching),
        );
        let child_eval = profile_child_eval(|| {
            evaluate_child_state(
                &result_board,
                action.next_current,
                action.next_hold,
                action.next_queue.as_slice(),
                transition.chain.b2b,
                transition.chain.combo,
                transition.chain.pending_garbage,
                transition.chain.lines_total,
                transition.chain.bag_number,
                transition.chain.pieces_into_bag,
                transition.chain.coaching,
                policy_score,
                ctx,
                transition.attack,
                chain_val,
                context_mod,
                parent_value,
                proxy_fallback_used,
            )
        });

        nodes.push(SearchNode {
            board: result_board,
            current: action.next_current,
            queue: action.next_queue,
            score: child_eval.score,
            hold: action.next_hold,
            b2b: transition.chain.b2b,
            combo: transition.chain.combo,
            pending_garbage: transition.chain.pending_garbage,
            lines_total: transition.chain.lines_total,
            bag_number: transition.chain.bag_number,
            pieces_into_bag: transition.chain.pieces_into_bag,
            coaching: transition.chain.coaching,
            root_move: action.mv,
            root_hold_used: action.hold_used,
            path: smallvec![action.mv],
            board_score: child_eval.board_score,
            attack_score: transition.attack,
            chain_score: chain_val,
            context_score: context_mod,
            path_attack: transition.attack,
            path_chain: chain_val,
            path_context: context_mod,
            policy_score: child_eval.policy_score,
            value_score: child_eval.value_score,
            fallback_used: child_eval.fallback_used,
            nn_parent_value: child_eval.nn_parent_value,
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
        ctx,
        &parent.board,
        parent.current,
        parent.hold,
        parent.queue.as_slice(),
    );
    let fallback_len = actions.len();
    let (actions, policy_scores, parent_value, proxy_fallback_used) = match ctx.config.nn_scoring {
        NnScoringMode::PerChildValue => {
            if let Some((policy_scores, parent_value)) = infer_for_actions(
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
                true,
            ) {
                let (actions, policy_scores) =
                    maybe_limit_policy_guided_actions(actions, policy_scores, ctx);
                (actions, policy_scores, Some(parent_value), false)
            } else {
                (actions, vec![0.0; fallback_len], None, false)
            }
        }
        NnScoringMode::PolicyProxy => match infer_policy_proxy(
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
            ProxyInference::Succeeded {
                policy_logits,
                parent_value,
            } => {
                let (actions, policy_scores) =
                    maybe_limit_policy_guided_actions(actions, policy_logits, ctx);
                (actions, policy_scores, Some(parent_value), false)
            }
            ProxyInference::Failed => (actions, vec![0.0; fallback_len], None, true),
            ProxyInference::UnavailableRuntime | ProxyInference::NonInferable => {
                (actions, vec![0.0; fallback_len], None, false)
            }
        },
    };

    materialize_node_children(
        parent,
        actions,
        policy_scores,
        parent_value,
        proxy_fallback_used,
        ctx,
        out,
    );
}

#[allow(clippy::too_many_arguments)]
fn materialize_node_children(
    parent: &SearchNode,
    actions: Vec<CandidateAction>,
    policy_scores: Vec<f32>,
    parent_value: Option<f32>,
    proxy_fallback_used: bool,
    ctx: &mut SearchExpansionContext<'_>,
    out: &mut Vec<SearchNode>,
) {
    let depth_factor = (parent.path.len() as f32 + 1.0)
        .sqrt()
        .min(MAX_DEPTH_FACTOR);
    let parent_chain = ChainState {
        b2b: parent.b2b,
        combo: parent.combo,
        pending_garbage: parent.pending_garbage,
        lines_total: parent.lines_total,
        bag_number: parent.bag_number,
        pieces_into_bag: parent.pieces_into_bag,
        coaching: parent.coaching,
    };

    for (action, policy_score) in actions.into_iter().zip(policy_scores) {
        let mut result_board = parent.board.clone();
        let mechanics = profile_do_move(|| result_board.lock(&action.mv));
        let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);
        let transition = parent_chain.advance_lock(
            &action.mv,
            &mechanics,
            action.hold_used,
            spawn_envelope_blocked,
            &ctx.config.attack_config,
        );
        let mut path_clear_events = Arc::clone(&parent.path_clear_events);
        if let Some(event) = transition.clear_event {
            Arc::make_mut(&mut path_clear_events).push(event);
        }
        let chain_val = shape_chain_value(transition.chain.combo as f32);
        let combo_context = transition.chain.combo as f32 - parent.combo as f32;
        let context_mod = shape_context_modifier(
            combo_context + coaching_context_bias(parent.coaching, transition.chain.coaching),
        );
        let cum_attack = parent.path_attack + transition.attack;
        let cum_chain = parent.path_chain + chain_val;
        let child_eval = profile_child_eval(|| {
            evaluate_child_state(
                &result_board,
                action.next_current,
                action.next_hold,
                action.next_queue.as_slice(),
                transition.chain.b2b,
                transition.chain.combo,
                transition.chain.pending_garbage,
                transition.chain.lines_total,
                transition.chain.bag_number,
                transition.chain.pieces_into_bag,
                transition.chain.coaching,
                policy_score,
                ctx,
                cum_attack / depth_factor,
                cum_chain / depth_factor,
                context_mod,
                parent_value,
                proxy_fallback_used,
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
            b2b: transition.chain.b2b,
            combo: transition.chain.combo,
            pending_garbage: transition.chain.pending_garbage,
            lines_total: transition.chain.lines_total,
            bag_number: transition.chain.bag_number,
            pieces_into_bag: transition.chain.pieces_into_bag,
            coaching: transition.chain.coaching,
            root_move: parent.root_move,
            root_hold_used: parent.root_hold_used,
            path,
            board_score: child_eval.board_score,
            attack_score: transition.attack,
            chain_score: chain_val,
            context_score: context_mod,
            path_attack: cum_attack,
            path_chain: cum_chain,
            path_context: parent.path_context + context_mod,
            policy_score: child_eval.policy_score,
            value_score: child_eval.value_score,
            fallback_used: child_eval.fallback_used,
            nn_parent_value: child_eval.nn_parent_value,
            path_clear_events,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        assemble_composite, expand_level_batched, expand_node, gen_and_eval_root,
        infer_policy_proxy, maybe_limit_policy_guided_actions, reset_search_expansion_stats,
        search_expansion_stats, set_poison_scalar_inference, set_search_profiling_enabled,
        CandidateAction, LevelExpansion, ProxyInference,
    };
    use crate::attack::AttackConfig;
    use crate::board::{Board, BOARD_HEIGHT, FULL_ROW};
    use crate::eval::{evaluate, EvalWeights};
    use crate::header::{Move, Piece, Rotation, SpinType, COL_NB};
    use crate::policy_value_runtime::{
        PolicyValueRuntime, PolicyValueRuntimeContext, CANDIDATE_CAPACITY,
    };
    use crate::search_config::{
        NnBatchMode, NnScoringMode, SearchConfig, SearchExpansionContext, SearchNode,
    };
    use crate::state::{
        ClearEvent, ClearType, CoachingState, FatalityState, ObligationState, SurgeState,
    };
    use smallvec::{smallvec, SmallVec};
    use std::sync::Arc;

    fn load_checked_in_runtime() -> Option<PolicyValueRuntime> {
        let metadata_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json");
        if !metadata_path.exists() {
            return None;
        }
        Some(
            PolicyValueRuntime::load(metadata_path)
                .unwrap_or_else(|error| panic!("checked-in runtime should load: {error}")),
        )
    }

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
        SearchExpansionContext {
            config,
            current_beam_width: beam_width,
            weights,
            remaining_depth: 0,
            policy_value: None,
            runtime_context: None,
            deadline: None,
        }
    }

    fn board_from_rows(rows: [u16; BOARD_HEIGHT]) -> Board {
        let mut board = Board::new();
        for (y, row) in rows.iter().enumerate() {
            board.rows[y] = row & FULL_ROW;
            for x in 0..COL_NB {
                if board.rows[y] & (1u16 << x) != 0 {
                    board.cols[x] |= 1u64 << y;
                }
            }
        }
        board
    }

    fn prior_clear_event(attack_sent: f32) -> ClearEvent {
        ClearEvent {
            clear_type: ClearType::Single,
            spin_type: SpinType::NoSpin,
            lines_cleared: 1,
            attack_sent,
            b2b_before: 0,
            b2b_after: 0,
            combo_before: 0,
            combo_after: 1,
            is_surge_release: false,
            is_garbage_clear: false,
            is_perfect_clear: false,
            piece: Piece::T,
        }
    }

    fn parent_node(
        board: Board,
        current: Piece,
        queue: SmallVec<[Piece; 16]>,
        path: SmallVec<[Move; 16]>,
        path_clear_events: SmallVec<[ClearEvent; 4]>,
    ) -> SearchNode {
        SearchNode {
            board,
            current: Some(current),
            queue,
            score: 0.0,
            hold: None,
            b2b: 0,
            combo: 0,
            pending_garbage: 0,
            lines_total: 0,
            bag_number: 0,
            pieces_into_bag: 0,
            coaching: CoachingState::default(),
            root_move: Move::none(),
            root_hold_used: false,
            path,
            board_score: 0.0,
            attack_score: 0.0,
            chain_score: 0.0,
            context_score: 0.0,
            path_attack: 0.0,
            path_chain: 0.0,
            path_context: 0.0,
            policy_score: 0.0,
            value_score: 0.0,
            fallback_used: false,
            nn_parent_value: None,
            path_clear_events: Arc::new(path_clear_events.into_vec()),
        }
    }

    fn expansion_signature(label: &str, parent: SearchNode) -> String {
        let mut ctx = context(0, 64);
        let mut out = Vec::new();
        expand_node(&parent, &mut ctx, &mut out);

        let mut lines = vec![format!("{label}:{}", out.len())];
        lines.extend(out.iter().map(|node| {
            let last_attack = node
                .path_clear_events
                .last()
                .map(|event| event.attack_sent.to_bits().to_string())
                .unwrap_or_else(|| "-".to_string());
            format!(
                "{}:{}:{}:{}:{}",
                node.path.last().copied().map(Move::raw).unwrap_or(0),
                node.score.to_bits(),
                node.path_clear_events.len(),
                last_attack,
                node.path.len()
            )
        }));
        lines.join("\n")
    }

    #[test]
    fn expand_node_preserves_ordered_child_projection() {
        let empty_parent = parent_node(
            Board::new(),
            Piece::T,
            smallvec![Piece::I, Piece::O, Piece::S],
            smallvec![Move::none()],
            SmallVec::new(),
        );

        let history_parent = parent_node(
            Board::new(),
            Piece::I,
            smallvec![Piece::T, Piece::O, Piece::S],
            smallvec![Move::none(), Move::none()],
            smallvec![prior_clear_event(2.5)],
        );

        let mut clear_rows = [0u16; BOARD_HEIGHT];
        clear_rows[0] = FULL_ROW & !(1u16 << 4) & !(1u16 << 5);
        let clear_parent = parent_node(
            board_from_rows(clear_rows),
            Piece::O,
            smallvec![Piece::T, Piece::I, Piece::S],
            smallvec![Move::none(), Move::none()],
            smallvec![prior_clear_event(3.0)],
        );

        let actual = [
            expansion_signature("empty", empty_parent),
            expansion_signature("history", history_parent),
            expansion_signature("clear", clear_parent),
        ]
        .join("\n--\n");

        let expected = "\
empty:51
10241:3222483764:0:-:2
2112:3224580915:0:-:2
10305:3227516928:0:-:2
18497:3192704192:0:-:2
26689:3228355788:0:-:2
2176:3228775219:0:-:2
10369:3233598670:0:-:2
18561:3201092800:0:-:2
26753:3230662658:0:-:2
2240:3230452941:0:-:2
10433:3233598670:0:-:2
18625:3219547744:0:-:2
26817:3233598670:0:-:2
2304:3230452941:0:-:2
10497:3233598670:0:-:2
18689:3219547744:0:-:2
26881:3233598670:0:-:2
2368:3230452941:0:-:2
10561:3233598670:0:-:2
18753:3219547744:0:-:2
26945:3233598670:0:-:2
2432:3230452941:0:-:2
10625:3233598670:0:-:2
18817:3219547744:0:-:2
27009:3233598670:0:-:2
2496:3228775219:0:-:2
10689:3230662658:0:-:2
18881:3201092800:0:-:2
27073:3233598670:0:-:2
2560:3224580915:0:-:2
10753:3228355788:0:-:2
18945:3192704192:0:-:2
27137:3227516928:0:-:2
27201:3222483764:0:-:2
8194:3233808384:0:-:2
64:3214514586:0:-:2
8258:3237163826:0:-:2
128:3217870029:0:-:2
8322:3241358132:0:-:2
192:3222064333:0:-:2
8386:3241358132:0:-:2
256:3222064333:0:-:2
8450:3241358132:0:-:2
320:3222064333:0:-:2
8514:3241358132:0:-:2
384:3217870029:0:-:2
8578:3241358132:0:-:2
448:3214514586:0:-:2
8642:3241358132:0:-:2
8706:3237163826:0:-:2
8770:3233808384:0:-:2
--
history:51
8194:3233808384:1:1075838976:3
64:3214514586:1:1075838976:3
8258:3237163826:1:1075838976:3
128:3217870029:1:1075838976:3
8322:3241358132:1:1075838976:3
192:3222064333:1:1075838976:3
8386:3241358132:1:1075838976:3
256:3222064333:1:1075838976:3
8450:3241358132:1:1075838976:3
320:3222064333:1:1075838976:3
8514:3241358132:1:1075838976:3
384:3217870029:1:1075838976:3
8578:3241358132:1:1075838976:3
448:3214514586:1:1075838976:3
8642:3241358132:1:1075838976:3
8706:3237163826:1:1075838976:3
8770:3233808384:1:1075838976:3
10241:3222483764:1:1075838976:3
2112:3224580915:1:1075838976:3
10305:3227516928:1:1075838976:3
18497:3192704192:1:1075838976:3
26689:3228355788:1:1075838976:3
2176:3228775219:1:1075838976:3
10369:3233598670:1:1075838976:3
18561:3201092800:1:1075838976:3
26753:3230662658:1:1075838976:3
2240:3230452941:1:1075838976:3
10433:3233598670:1:1075838976:3
18625:3219547744:1:1075838976:3
26817:3233598670:1:1075838976:3
2304:3230452941:1:1075838976:3
10497:3233598670:1:1075838976:3
18689:3219547744:1:1075838976:3
26881:3233598670:1:1075838976:3
2368:3230452941:1:1075838976:3
10561:3233598670:1:1075838976:3
18753:3219547744:1:1075838976:3
26945:3233598670:1:1075838976:3
2432:3230452941:1:1075838976:3
10625:3233598670:1:1075838976:3
18817:3219547744:1:1075838976:3
27009:3233598670:1:1075838976:3
2496:3228775219:1:1075838976:3
10689:3230662658:1:1075838976:3
18881:3201092800:1:1075838976:3
27073:3233598670:1:1075838976:3
2560:3224580915:1:1075838976:3
10753:3228355788:1:1075838976:3
18945:3192704192:1:1075838976:3
27137:3227516928:1:1075838976:3
27201:3222483764:1:1075838976:3
--
clear:43
1025:3230033511:1:1077936128:3
1089:3231711232:1:1077936128:3
1153:3235486106:1:1077936128:3
1217:3239470695:1:1077936128:3
1280:3221564555:2:0:3
1345:3239470695:1:1077936128:3
1409:3235486106:1:1077936128:3
1473:3231711232:1:1077936128:3
1537:3230033511:1:1077936128:3
10242:3229194648:1:1077936128:3
2113:3230452942:1:1077936128:3
10306:3231920948:1:1077936128:3
18498:3219547760:1:1077936128:3
26690:3232340378:1:1077936128:3
2177:3232969524:1:1077936128:3
10370:3237792976:1:1077936128:3
18562:3222903196:1:1077936128:3
26754:3234018100:1:1077936128:3
2241:3225839208:1:1077936128:3
10434:3239575553:1:1077936128:3
18626:3232550092:1:1077936128:3
26818:3238107546:1:1077936128:3
2305:3245028147:1:1077936128:3
10497:3239994982:1:1077936128:3
18689:3236954112:1:1077936128:3
26881:3230033511:1:1077936128:3
2369:3245028147:1:1077936128:3
10561:3230033511:1:1077936128:3
18753:3236954112:1:1077936128:3
26945:3239994982:1:1077936128:3
2433:3225839208:1:1077936128:3
10626:3238107546:1:1077936128:3
18818:3232550092:1:1077936128:3
27010:3239575553:1:1077936128:3
2497:3232969524:1:1077936128:3
10690:3234018100:1:1077936128:3
18882:3222903196:1:1077936128:3
27074:3237792976:1:1077936128:3
2561:3230452942:1:1077936128:3
10754:3232340378:1:1077936128:3
18946:3219547760:1:1077936128:3
27138:3231920948:1:1077936128:3
27202:3229194648:1:1077936128:3";

        let normalize = |s: &str| -> String {
            s.split("\n--\n")
                .map(|section| {
                    let mut lines: Vec<&str> = section.lines().collect();
                    if lines.len() > 1 {
                        lines[1..].sort_unstable();
                    }
                    lines.join("\n")
                })
                .collect::<Vec<_>>()
                .join("\n--\n")
        };
        assert_eq!(normalize(&actual), normalize(&expected));
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
    fn proxy_mode_runs_one_inference_per_expanded_node() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let config = SearchConfig {
            beam_width: 64,
            depth: 2,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: NnScoringMode::PolicyProxy,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let mut ctx = SearchExpansionContext {
            config: &config,
            current_beam_width: 64,
            weights: &weights,
            remaining_depth: 1,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };
        let state = crate::state::GameState::new(Board::new(), Piece::T, Vec::new());
        let mut root = Vec::new();

        reset_search_expansion_stats();
        set_search_profiling_enabled(true);
        gen_and_eval_root(&state, &mut ctx, &mut root);
        let mut children = Vec::new();
        expand_node(&root[0], &mut ctx, &mut children);
        let stats = search_expansion_stats();
        set_search_profiling_enabled(false);

        assert!(stats.noninferable_nodes > 0);
        assert_eq!(
            stats.runtime_attempt_rows
                + stats.noninferable_nodes
                + stats.runtime_unavailable_nodes
                + stats.abandoned_nodes,
            stats.expanded_nodes
        );
        assert!(stats.inferred_rows <= stats.runtime_attempt_rows);
        assert_eq!(stats.inferred_rows, stats.runtime_attempt_rows);
        assert_eq!(stats.runtime_calls, stats.runtime_attempt_rows);
        assert_eq!(stats.abandoned_nodes, 0);
    }

    #[test]
    fn proxy_child_score_formula_matches_hand_computed() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let config = SearchConfig {
            beam_width: 4,
            depth: 1,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: NnScoringMode::PolicyProxy,
            policy_proxy_weight: 0.10,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let mut ctx = SearchExpansionContext {
            config: &config,
            current_beam_width: 4,
            weights: &weights,
            remaining_depth: 0,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };
        let state = crate::state::GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let mut nodes = Vec::new();

        gen_and_eval_root(&state, &mut ctx, &mut nodes);

        assert!(!nodes.is_empty());
        for node in nodes {
            let board_eval = evaluate(&node.board, &weights);
            let expected = assemble_composite(
                board_eval,
                node.attack_score,
                node.chain_score,
                node.context_score,
            ) + 0.10 * node.policy_score;
            assert_eq!(node.score.to_bits(), expected.to_bits());
            assert_eq!(node.board_score.to_bits(), board_eval.to_bits());
            assert_eq!(node.value_score.to_bits(), board_eval.to_bits());
        }

        let zero_config = SearchConfig {
            beam_width: 64,
            depth: 1,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            policy_guided_expansion_cap: 64,
            nn_scoring: NnScoringMode::PolicyProxy,
            policy_proxy_weight: 0.0,
            ..SearchConfig::default()
        };
        let mut runtime_ctx = SearchExpansionContext {
            config: &zero_config,
            current_beam_width: 64,
            weights: &weights,
            remaining_depth: 0,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };
        let mut runtime_nodes = Vec::new();
        gen_and_eval_root(&state, &mut runtime_ctx, &mut runtime_nodes);
        let mut heuristic_ctx = SearchExpansionContext {
            config: &zero_config,
            current_beam_width: 64,
            weights: &weights,
            remaining_depth: 0,
            policy_value: None,
            runtime_context: None,
            deadline: None,
        };
        let mut heuristic_nodes = Vec::new();
        gen_and_eval_root(&state, &mut heuristic_ctx, &mut heuristic_nodes);

        assert_eq!(runtime_nodes.len(), heuristic_nodes.len());
        for (runtime_node, heuristic_node) in runtime_nodes.iter().zip(&heuristic_nodes) {
            assert_eq!(runtime_node.root_move.raw(), heuristic_node.root_move.raw());
            assert_eq!(runtime_node.score.to_bits(), heuristic_node.score.to_bits());
        }
    }

    #[test]
    fn fallback_used_semantics_three_cases() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let weights = EvalWeights::default();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let config = SearchConfig {
            beam_width: 64,
            depth: 1,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: NnScoringMode::PolicyProxy,
            ..SearchConfig::default()
        };

        let run = |state: &crate::state::GameState,
                   supplied_runtime: Option<&PolicyValueRuntime>,
                   supplied_context: Option<&PolicyValueRuntimeContext>| {
            let mut ctx = SearchExpansionContext {
                config: &config,
                current_beam_width: 64,
                weights: &weights,
                remaining_depth: 0,
                policy_value: supplied_runtime,
                runtime_context: supplied_context,
                deadline: None,
            };
            let mut nodes = Vec::new();
            gen_and_eval_root(state, &mut ctx, &mut nodes);
            nodes
        };
        let healthy =
            crate::state::GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        assert!(run(&healthy, Some(&runtime), Some(&runtime_context))
            .iter()
            .all(|node| !node.fallback_used));
        assert!(run(&healthy, None, None)
            .iter()
            .all(|node| !node.fallback_used));
        assert!(run(&healthy, Some(&runtime), None)
            .iter()
            .all(|node| !node.fallback_used));

        let mut failing = crate::state::GameState::new(Board::new(), Piece::T, Vec::new());
        failing.hold = Some(Piece::T);
        let noninferable_nodes = run(&failing, Some(&runtime), Some(&runtime_context));
        assert!(!noninferable_nodes.is_empty());
        assert!(noninferable_nodes.iter().all(|node| !node.fallback_used));
        assert!(noninferable_nodes
            .iter()
            .all(|node| node.policy_score == 0.0));
    }

    #[test]
    fn wide_node_scalar_is_noninferable_not_failed() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let config = SearchConfig {
            beam_width: 128,
            depth: 1,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: NnScoringMode::PolicyProxy,
            nn_batch: NnBatchMode::Scalar,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let mut parent = parent_node(
            Board::new(),
            Piece::T,
            SmallVec::new(),
            smallvec![Move::none()],
            SmallVec::new(),
        );
        parent.hold = Some(Piece::T);
        let mut ctx = SearchExpansionContext {
            config: &config,
            current_beam_width: 128,
            weights: &weights,
            remaining_depth: 0,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };
        let actions = super::enumerate_actions(
            &mut ctx,
            &parent.board,
            parent.current,
            parent.hold,
            parent.queue.as_slice(),
        );
        assert!(actions.len() > CANDIDATE_CAPACITY);

        reset_search_expansion_stats();
        set_search_profiling_enabled(true);
        let outcome = infer_policy_proxy(
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
            &ctx,
        );
        assert!(matches!(outcome, ProxyInference::NonInferable));

        reset_search_expansion_stats();
        let mut children = Vec::new();
        expand_node(&parent, &mut ctx, &mut children);
        let stats = search_expansion_stats();
        set_search_profiling_enabled(false);

        assert!(children.iter().all(|node| node.score.is_finite()));
        assert!(children.iter().all(|node| !node.fallback_used));
        assert_eq!(stats.runtime_attempt_rows, 0);
        assert_eq!(stats.noninferable_nodes, 1);
        assert_eq!(stats.runtime_unavailable_nodes, 0);
        assert_eq!(
            stats.runtime_attempt_rows
                + stats.noninferable_nodes
                + stats.runtime_unavailable_nodes
                + stats.abandoned_nodes,
            stats.expanded_nodes
        );
    }

    #[test]
    fn genuine_runtime_error_sets_fallback_used_true() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let config = SearchConfig {
            beam_width: 64,
            depth: 1,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: NnScoringMode::PolicyProxy,
            nn_batch: NnBatchMode::Scalar,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let parent = parent_node(
            Board::new(),
            Piece::T,
            SmallVec::new(),
            smallvec![Move::none()],
            SmallVec::new(),
        );
        let mut ctx = SearchExpansionContext {
            config: &config,
            current_beam_width: 64,
            weights: &weights,
            remaining_depth: 0,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };
        let actions = super::enumerate_actions(
            &mut ctx,
            &parent.board,
            parent.current,
            parent.hold,
            parent.queue.as_slice(),
        );
        assert!(actions.len() <= CANDIDATE_CAPACITY);

        set_poison_scalar_inference(true);
        let outcome = infer_policy_proxy(
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
            &ctx,
        );
        assert!(matches!(outcome, ProxyInference::Failed));

        reset_search_expansion_stats();
        set_search_profiling_enabled(true);
        set_poison_scalar_inference(true);
        let mut children = Vec::new();
        expand_node(&parent, &mut ctx, &mut children);
        set_poison_scalar_inference(false);
        let stats = search_expansion_stats();
        set_search_profiling_enabled(false);

        assert!(!children.is_empty());
        assert!(children.iter().all(|node| node.fallback_used));
        assert!(children.iter().all(|node| node.score.is_finite()));
        assert_eq!(stats.runtime_attempt_rows, 1);
        assert_eq!(stats.inferred_rows, 0);
        assert_eq!(stats.noninferable_nodes, 0);
        assert_eq!(stats.runtime_unavailable_nodes, 0);
        assert_eq!(
            stats.runtime_attempt_rows
                + stats.noninferable_nodes
                + stats.runtime_unavailable_nodes
                + stats.abandoned_nodes,
            stats.expanded_nodes
        );
    }

    #[test]
    fn wide_parent_excluded_from_level_batch() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let config = SearchConfig {
            beam_width: 128,
            depth: 1,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: NnScoringMode::PolicyProxy,
            nn_batch: NnBatchMode::Level,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let mut wide = parent_node(
            Board::new(),
            Piece::T,
            SmallVec::new(),
            smallvec![Move::none()],
            SmallVec::new(),
        );
        wide.hold = Some(Piece::T);
        let narrow_one = parent_node(
            Board::new(),
            Piece::I,
            SmallVec::new(),
            smallvec![Move::none()],
            SmallVec::new(),
        );
        let narrow_two = parent_node(
            Board::new(),
            Piece::O,
            SmallVec::new(),
            smallvec![Move::none()],
            SmallVec::new(),
        );
        let parents = vec![wide, narrow_one, narrow_two];
        let mut ctx = SearchExpansionContext {
            config: &config,
            current_beam_width: 128,
            weights: &weights,
            remaining_depth: 0,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };

        reset_search_expansion_stats();
        set_search_profiling_enabled(true);
        let expansion = expand_level_batched(&parents, &mut ctx);
        let stats = search_expansion_stats();
        set_search_profiling_enabled(false);

        assert!(matches!(expansion, LevelExpansion::Completed(_)));
        assert_eq!(stats.inferred_rows, stats.runtime_attempt_rows);
        assert!(stats.inferred_rows > 0);
        assert_eq!(stats.fallback_levels, 0);
        assert!(stats.noninferable_nodes >= 1);
    }

    #[test]
    fn level_and_scalar_nn_coverage_identical_with_wide_nodes() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let make_config = |nn_batch| SearchConfig {
            beam_width: 128,
            depth: 1,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: NnScoringMode::PolicyProxy,
            nn_batch,
            ..SearchConfig::default()
        };
        let mut wide = parent_node(
            Board::new(),
            Piece::T,
            SmallVec::new(),
            smallvec![Move::none()],
            SmallVec::new(),
        );
        wide.hold = Some(Piece::T);
        let parents = vec![
            wide,
            parent_node(
                Board::new(),
                Piece::I,
                SmallVec::new(),
                smallvec![Move::none()],
                SmallVec::new(),
            ),
            parent_node(
                Board::new(),
                Piece::O,
                SmallVec::new(),
                smallvec![Move::none()],
                SmallVec::new(),
            ),
        ];
        let weights = EvalWeights::default();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };

        let scalar_config = make_config(NnBatchMode::Scalar);
        let mut scalar_ctx = SearchExpansionContext {
            config: &scalar_config,
            current_beam_width: 128,
            weights: &weights,
            remaining_depth: 0,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };
        reset_search_expansion_stats();
        set_search_profiling_enabled(true);
        let mut scalar_children = Vec::new();
        for parent in &parents {
            expand_node(parent, &mut scalar_ctx, &mut scalar_children);
        }
        let scalar_stats = search_expansion_stats();

        let level_config = make_config(NnBatchMode::Level);
        let mut level_ctx = SearchExpansionContext {
            config: &level_config,
            current_beam_width: 128,
            weights: &weights,
            remaining_depth: 0,
            policy_value: Some(&runtime),
            runtime_context: Some(&runtime_context),
            deadline: None,
        };
        reset_search_expansion_stats();
        let LevelExpansion::Completed(level_children) =
            expand_level_batched(&parents, &mut level_ctx)
        else {
            panic!("level expansion must complete");
        };
        let level_stats = search_expansion_stats();
        set_search_profiling_enabled(false);

        assert_eq!(level_stats.inferred_rows, scalar_stats.inferred_rows);
        assert_eq!(
            level_stats.runtime_attempt_rows,
            scalar_stats.runtime_attempt_rows
        );
        assert_eq!(
            level_stats.noninferable_nodes,
            scalar_stats.noninferable_nodes
        );
        assert_eq!(level_children.len(), scalar_children.len());
        for (level_child, scalar_child) in level_children.iter().zip(&scalar_children) {
            assert_eq!(level_child.root_move.raw(), scalar_child.root_move.raw());
            assert!((level_child.score - scalar_child.score).abs() <= 1e-6);
            assert!((level_child.policy_score - scalar_child.policy_score).abs() <= 1e-6);
            assert!((level_child.value_score - scalar_child.value_score).abs() <= 1e-6);
        }
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
        assert_eq!(stats.sort_prune_truncate_nanos, 0);
        assert_eq!(stats.candidate_copy_nanos, 0);
        assert_eq!(stats.root_score_aggregation_nanos, 0);
        assert_eq!(stats.unique_action_keys, 0);
        assert_eq!(stats.repeated_action_builds, 0);
    }

    #[test]
    fn coaching_context_bias_uses_active_dimensions() {
        let previous = CoachingState {
            fatality: FatalityState::Safe,
            obligation: ObligationState::None,
            surge: SurgeState::Dormant,
        };
        let next = CoachingState {
            fatality: FatalityState::Critical,
            obligation: ObligationState::MustDownstack,
            surge: SurgeState::Building,
        };

        assert!((super::coaching_context_bias(previous, next) + 0.4).abs() < 1e-6);
    }
}
