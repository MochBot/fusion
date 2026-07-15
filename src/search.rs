// search.rs -- beam search with hold for coaching engine
// expands moves breadth-first, pruned to beam_width at each depth

use crate::bag;

use crate::eval::EvalWeights;
use crate::policy_value_runtime::{PolicyValueRuntime, PolicyValueRuntimeContext};
#[cfg(not(target_arch = "wasm32"))]
use crate::search_config::DeadlineSource;
use crate::search_config::FUTILITY_DELTA;
#[cfg(not(target_arch = "wasm32"))]
use crate::search_config::{NnBatchMode, NnScoringMode};

use crate::state::GameState;
use crate::transposition::{get_zobrist_keys, TranspositionTable, DEFAULT_TT_SIZE};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

pub use crate::search_config::{SearchConfig, SearchNode, SearchResult, SearchResultFull};
pub(crate) use crate::search_config::{SearchExpansionContext, SearchIterationParams};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use crate::search_expand::record_deadline_check;
pub(crate) use crate::search_expand::{
    expand_level_batched, expand_node, gen_and_eval_root, profile_root_score_aggregation,
    profile_sort_prune_truncate, record_abandoned_level, record_completed_search,
    record_q_extension_completed, LevelExpansion,
};

/// beam search from game state
/// returns the best move found, or None if no legal moves exist
pub fn find_best_move(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
) -> Option<SearchResult> {
    find_best_move_with_scores(state, config, weights).map(|full| full.best)
}

pub fn find_best_move_runtime(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
    policy_value: &PolicyValueRuntime,
    runtime_context: &PolicyValueRuntimeContext,
) -> Option<SearchResult> {
    find_best_move_with_scores_runtime(state, config, weights, policy_value, runtime_context)
        .map(|full| full.best)
}

pub fn find_best_move_with_scores(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
) -> Option<SearchResultFull> {
    find_best_move_with_scores_forced_runtime(state, config, weights, None, None, None)
}

pub fn find_best_move_with_scores_runtime(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
    policy_value: &PolicyValueRuntime,
    runtime_context: &PolicyValueRuntimeContext,
) -> Option<SearchResultFull> {
    find_best_move_with_scores_forced_runtime(
        state,
        config,
        weights,
        Some(policy_value),
        Some(runtime_context),
        None,
    )
}

/// Beam search with optional forced root move.
/// When `forced_root_move` is Some, that move is protected from futility
/// pruning and beam truncation, so it always survives to the final beam.
pub fn find_best_move_with_scores_forced(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
    forced_root_move: Option<crate::header::Move>,
) -> Option<SearchResultFull> {
    find_best_move_with_scores_forced_runtime(state, config, weights, None, None, forced_root_move)
}

pub fn find_best_move_with_scores_forced_runtime(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
    policy_value: Option<&PolicyValueRuntime>,
    runtime_context: Option<&PolicyValueRuntimeContext>,
    forced_root_move: Option<crate::header::Move>,
) -> Option<SearchResultFull> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        find_best_move_with_scores_forced_runtime_deadline(
            state,
            config,
            weights,
            policy_value,
            runtime_context,
            forced_root_move,
            None,
        )
    }

    #[cfg(target_arch = "wasm32")]
    {
        find_best_move_with_scores_forced_runtime_impl(
            state,
            config,
            weights,
            policy_value,
            runtime_context,
            forced_root_move,
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn find_best_move_with_scores_forced_runtime_deadline(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
    policy_value: Option<&PolicyValueRuntime>,
    runtime_context: Option<&PolicyValueRuntimeContext>,
    forced_root_move: Option<crate::header::Move>,
    deadline_override: Option<&DeadlineSource>,
) -> Option<SearchResultFull> {
    find_best_move_with_scores_forced_runtime_impl(
        state,
        config,
        weights,
        policy_value,
        runtime_context,
        forced_root_move,
        deadline_override,
    )
}

fn find_best_move_with_scores_forced_runtime_impl(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
    policy_value: Option<&PolicyValueRuntime>,
    runtime_context: Option<&PolicyValueRuntimeContext>,
    forced_root_move: Option<crate::header::Move>,
    #[cfg(not(target_arch = "wasm32"))] deadline_override: Option<&DeadlineSource>,
) -> Option<SearchResultFull> {
    let search_queue = if config.extend_queue_7bag {
        bag::extend_queue(&state.queue, state.current, state.hold)
    } else {
        state.queue.clone()
    };

    let max_depth = config.depth.min(search_queue.len() + 1);
    if max_depth == 0 {
        return None;
    }

    let zobrist_keys = get_zobrist_keys();
    let mut tt = config
        .use_tt
        .then(|| TranspositionTable::new(DEFAULT_TT_SIZE));

    if config.time_budget_ms.is_none() {
        let mut params = SearchIterationParams {
            state,
            config,
            weights,
            max_depth,
            beam_width: config.beam_width,
            zobrist_keys,
            tt: &mut tt,
            forced_root_move,
            policy_value,
            runtime_context,
            #[cfg(not(target_arch = "wasm32"))]
            deadline: None,
        };
        let (full, completed_depth, completed_width) = run_beam_search_iteration(&mut params)?;
        record_completed_search(completed_depth, completed_width);
        return Some(full);
    }

    let max_width = config.beam_width;
    if max_width == 0 {
        return None;
    }

    let mut width = 200.min(max_width);
    let mut best_full: Option<SearchResultFull> = None;

    #[cfg(not(target_arch = "wasm32"))]
    let start = Instant::now();
    #[cfg(not(target_arch = "wasm32"))]
    let time_budget = config.time_budget_ms.map(Duration::from_millis);
    #[cfg(not(target_arch = "wasm32"))]
    let wall_deadline =
        if config.nn_scoring == NnScoringMode::PolicyProxy && deadline_override.is_none() {
            time_budget.map(|budget| DeadlineSource::Wall(start + budget))
        } else {
            None
        };
    #[cfg(not(target_arch = "wasm32"))]
    let deadline = if config.nn_scoring == NnScoringMode::PolicyProxy {
        deadline_override.or(wall_deadline.as_ref())
    } else {
        None
    };

    #[cfg(target_arch = "wasm32")]
    let mut iteration_count = 0usize;
    #[cfg(target_arch = "wasm32")]
    let max_iterations = config
        .time_budget_ms
        .map(|ms| ms.max(1) as usize)
        .unwrap_or(1);

    loop {
        if let Some(table) = tt.as_mut() {
            table.clear();
        }

        let mut params = SearchIterationParams {
            state,
            config,
            weights,
            max_depth,
            beam_width: width,
            zobrist_keys,
            tt: &mut tt,
            forced_root_move,
            policy_value,
            runtime_context,
            #[cfg(not(target_arch = "wasm32"))]
            deadline,
        };
        if let Some((full, completed_depth, completed_width)) =
            run_beam_search_iteration(&mut params)
        {
            let should_replace = best_full
                .as_ref()
                .is_none_or(|prev| compare_results_desc(&full.best, &prev.best).is_lt());

            if should_replace {
                best_full = Some(full);
                record_completed_search(completed_depth, completed_width);
            }
        }

        if width >= max_width {
            break;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(deadline) = deadline {
                if deadline_expired(deadline) {
                    break;
                }
            }
            if deadline.is_none() {
                if let Some(budget) = time_budget {
                    if start.elapsed() >= budget {
                        break;
                    }
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            iteration_count += 1;
            if iteration_count >= max_iterations {
                break;
            }
        }

        width = (width * 2).min(max_width);
    }

    best_full
}

fn run_beam_search_iteration(
    params: &mut SearchIterationParams<'_>,
) -> Option<(SearchResultFull, usize, usize)> {
    let mut ctx = SearchExpansionContext {
        config: params.config,
        current_beam_width: params.beam_width,
        weights: params.weights,
        remaining_depth: params.max_depth.saturating_sub(1),
        zobrist_keys: params.zobrist_keys,
        tt: params.tt,
        policy_value: params.policy_value,
        runtime_context: params.runtime_context,
        #[cfg(not(target_arch = "wasm32"))]
        deadline: params.deadline,
    };

    let mut beam = expand_root(params.state, &mut ctx);
    if beam.is_empty() {
        return None;
    }

    profile_sort_prune_truncate(|| {
        apply_futility_pruning(&mut beam, FUTILITY_DELTA, params.forced_root_move);
        beam.sort_unstable_by(compare_nodes_desc);
        truncate_with_forced(&mut beam, params.beam_width, params.forced_root_move);
    });

    let mut completed_depth = 1;
    let mut deadline_hit = false;

    for depth_idx in 0..params.max_depth.saturating_sub(1) {
        if search_deadline_expired(&ctx) {
            record_abandoned_level();
            deadline_hit = true;
            break;
        }
        let child_depth = depth_idx + 2;
        ctx.remaining_depth = params.max_depth.saturating_sub(child_depth);

        let mut next_beam =
            if level_batch_enabled(params.config, params.policy_value, params.runtime_context) {
                match expand_level_batched(&beam, &mut ctx) {
                    LevelExpansion::Completed(nodes) => nodes,
                    LevelExpansion::Abandoned => {
                        record_abandoned_level();
                        deadline_hit = true;
                        break;
                    }
                }
            } else {
                let mut nodes = Vec::with_capacity(params.beam_width.saturating_mul(2));
                let mut abandoned = false;
                for node in &beam {
                    if search_deadline_expired(&ctx) {
                        record_abandoned_level();
                        abandoned = true;
                        deadline_hit = true;
                        break;
                    }
                    expand_node(node, &mut ctx, &mut nodes);
                }
                if abandoned {
                    break;
                }
                nodes
            };

        if next_beam.is_empty() {
            break;
        }

        profile_sort_prune_truncate(|| {
            apply_futility_pruning(&mut next_beam, FUTILITY_DELTA, params.forced_root_move);
            next_beam.sort_unstable_by(compare_nodes_desc);
            truncate_with_forced(&mut next_beam, params.beam_width, params.forced_root_move);
        });
        if search_deadline_expired(&ctx) {
            record_abandoned_level();
            deadline_hit = true;
            break;
        }
        beam = next_beam;
        completed_depth += 1;
    }

    // Quiescence extensions: extend loud nodes past the normal depth boundary
    // so investment moves (mid-combo, active B2B) resolve before evaluation.
    let q_max = params.config.quiescence_max_extensions;
    let q_beam_width =
        ((params.beam_width as f32) * params.config.quiescence_beam_fraction).ceil() as usize;
    if !deadline_hit && q_max > 0 && q_beam_width > 0 && !search_deadline_expired(&ctx) {
        let main_depth = params.max_depth.saturating_sub(1);
        let loud_nodes: Vec<SearchNode> = beam.iter().filter(|n| n.is_loud()).cloned().collect();

        if !loud_nodes.is_empty() {
            let mut q_beam = loud_nodes;
            profile_sort_prune_truncate(|| {
                q_beam.sort_unstable_by(compare_nodes_desc);
                q_beam.truncate(q_beam_width);
            });

            for ext in 0..q_max {
                if search_deadline_expired(&ctx) {
                    record_abandoned_level();
                    break;
                }
                let child_depth = main_depth + ext + 2;
                ctx.remaining_depth = params
                    .max_depth
                    .saturating_sub(child_depth.min(params.max_depth));

                let mut next_q = if level_batch_enabled(
                    params.config,
                    params.policy_value,
                    params.runtime_context,
                ) {
                    match expand_level_batched(&q_beam, &mut ctx) {
                        LevelExpansion::Completed(nodes) => nodes,
                        LevelExpansion::Abandoned => {
                            record_abandoned_level();
                            break;
                        }
                    }
                } else {
                    let mut nodes = Vec::with_capacity(q_beam_width * 2);
                    let mut abandoned = false;
                    for node in &q_beam {
                        if search_deadline_expired(&ctx) {
                            record_abandoned_level();
                            abandoned = true;
                            break;
                        }
                        expand_node(node, &mut ctx, &mut nodes);
                    }
                    if abandoned {
                        break;
                    }
                    nodes
                };

                if next_q.is_empty() {
                    break;
                }

                profile_sort_prune_truncate(|| {
                    next_q.sort_unstable_by(compare_nodes_desc);
                    next_q.truncate(q_beam_width);
                });

                let mut next_main_snapshot = beam.clone();
                for node in &next_q {
                    if !node.is_loud() {
                        next_main_snapshot.push(node.clone());
                    }
                }
                let mut next_q_beam: Vec<SearchNode> =
                    next_q.into_iter().filter(|n| n.is_loud()).collect();
                profile_sort_prune_truncate(|| {
                    next_main_snapshot.sort_unstable_by(compare_nodes_desc);
                    next_q_beam.sort_unstable_by(compare_nodes_desc);
                    next_q_beam.truncate(q_beam_width);
                });
                if search_deadline_expired(&ctx) {
                    record_abandoned_level();
                    break;
                }

                beam = next_main_snapshot;
                q_beam = next_q_beam;
                record_q_extension_completed();
                if q_beam.is_empty() {
                    break;
                }
            }

            beam.extend(q_beam);
            profile_sort_prune_truncate(|| beam.sort_unstable_by(compare_nodes_desc));
        }
    }

    let best = beam.first()?;
    let result = SearchResult {
        best_move: best.root_move,
        hold_used: best.root_hold_used,
        score: best.score,
        pv: best.path.to_vec(),
        coaching_state: best.coaching,
        pv_clear_events: best.path_clear_events.to_vec(),
    };

    let (root_scores, position_complexity) = profile_root_score_aggregation(|| {
        let mut root_scores: Vec<(crate::header::Move, f32)> = Vec::new();
        for node in &beam {
            let raw = node.root_move.raw();
            match root_scores.iter_mut().find(|entry| entry.0.raw() == raw) {
                Some(entry) => {
                    if node.score > entry.1 {
                        entry.1 = node.score;
                    }
                }
                None => root_scores.push((node.root_move, node.score)),
            }
        }
        root_scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        let position_complexity = compute_position_complexity(&root_scores);
        (root_scores, position_complexity)
    });

    Some((
        SearchResultFull {
            best: result,
            root_scores,
            position_complexity,
            board_score: best.board_score,
            attack_score: best.attack_score,
            chain_score: best.chain_score,
            context_score: best.context_score,
            path_attack: best.path_attack,
            path_chain: best.path_chain,
            path_context: best.path_context,
            policy_score: best.policy_score,
            value_score: best.value_score,
            fallback_used: best.fallback_used,
            nn_parent_value: best.nn_parent_value,
        },
        completed_depth,
        params.beam_width,
    ))
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn deadline_expired(deadline: &DeadlineSource) -> bool {
    let expired = deadline.expired();
    record_deadline_check(expired);
    expired
}

#[inline]
fn search_deadline_expired(ctx: &SearchExpansionContext<'_>) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ctx.deadline.is_some_and(deadline_expired)
    }

    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        false
    }
}

#[inline]
pub(crate) fn level_batch_enabled(
    config: &SearchConfig,
    policy_value: Option<&PolicyValueRuntime>,
    runtime_context: Option<&PolicyValueRuntimeContext>,
) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        config.nn_scoring == NnScoringMode::PolicyProxy
            && config.nn_batch == NnBatchMode::Level
            && policy_value.is_some()
            && runtime_context.is_some()
    }

    #[cfg(target_arch = "wasm32")]
    {
        let _ = (config, policy_value, runtime_context);
        false
    }
}

/// Compute position complexity: variance of top-10 root move scores.
/// High variance = sharp position (clear best moves), low = flat (all moves similar).
fn compute_position_complexity(root_scores: &[(crate::header::Move, f32)]) -> f32 {
    let mut top_n = [0.0f32; 10];
    let count = root_scores.len().min(10);
    for (i, (_, s)) in root_scores.iter().take(10).enumerate() {
        top_n[i] = *s;
    }
    if count < 2 {
        return 0.0;
    }
    let scores = &top_n[..count];
    let mean = scores.iter().sum::<f32>() / count as f32;
    let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / count as f32;
    variance
}

/// Truncate beam to `max_size`, but if a forced root move would be truncated,
/// re-insert it by evicting the worst node.
fn truncate_with_forced(
    beam: &mut Vec<SearchNode>,
    max_size: usize,
    forced: Option<crate::header::Move>,
) {
    if beam.len() <= max_size {
        return;
    }

    // Extract forced node before truncation so it can't be lost
    let forced_node = forced.and_then(|fm| {
        let idx = beam.iter().position(|n| n.root_move.raw() == fm.raw());
        idx.map(|i| beam.swap_remove(i))
    });

    beam.truncate(max_size);

    // Re-insert forced node, evicting worst survivor if needed
    if let Some(node) = forced_node {
        let already_present = beam
            .iter()
            .any(|n| n.root_move.raw() == node.root_move.raw());
        if !already_present {
            if beam.len() >= max_size {
                beam.pop(); // evict worst (last after sort)
            }
            beam.push(node);
        }
    }
}

fn apply_futility_pruning(
    nodes: &mut Vec<SearchNode>,
    futility_delta: f32,
    forced: Option<crate::header::Move>,
) {
    if nodes.is_empty() {
        return;
    }

    let delta = futility_delta.max(0.0);
    let best_tier = nodes.iter().map(policy_key).max().unwrap_or((0, 0));

    // Extract forced move node before pruning (if present)
    let forced_node = forced.and_then(|fm| {
        let idx = nodes.iter().position(|n| n.root_move.raw() == fm.raw());
        idx.map(|i| nodes.swap_remove(i))
    });

    nodes.retain(|node| policy_key(node) == best_tier);

    let best_score = nodes
        .iter()
        .map(|node| node.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let cutoff = best_score - delta;

    nodes.retain(|node| node.score >= cutoff);

    // Re-insert forced move node unconditionally (it bypasses futility pruning)
    if let Some(forced_node) = forced_node {
        // Only re-insert if not already present (it might have survived pruning
        // if it was removed by swap_remove but an identical root_move node exists)
        let already_present = nodes
            .iter()
            .any(|n| n.root_move.raw() == forced_node.root_move.raw());
        if !already_present {
            nodes.push(forced_node);
        }
    }
}

fn policy_key(node: &SearchNode) -> (u8, u8) {
    let survival = match node.coaching.fatality {
        crate::state::FatalityState::Fatal => 0,
        crate::state::FatalityState::Critical => 1,
        crate::state::FatalityState::Safe => 2,
    };

    let obligation = match node.coaching.obligation {
        crate::state::ObligationState::MustCancel => 0,
        crate::state::ObligationState::MustDownstack => 1,
        crate::state::ObligationState::None => 2,
    };

    (survival, obligation)
}

fn compare_nodes_desc(a: &SearchNode, b: &SearchNode) -> std::cmp::Ordering {
    let a_key = policy_key(a);
    let b_key = policy_key(b);

    // Shipped sort order: (policy_key, score).
    b_key.cmp(&a_key).then_with(|| b.score.total_cmp(&a.score))
}

fn compare_results_desc(a: &SearchResult, b: &SearchResult) -> std::cmp::Ordering {
    let a_survival = match a.coaching_state.fatality {
        crate::state::FatalityState::Fatal => 0,
        crate::state::FatalityState::Critical => 1,
        crate::state::FatalityState::Safe => 2,
    };
    let b_survival = match b.coaching_state.fatality {
        crate::state::FatalityState::Fatal => 0,
        crate::state::FatalityState::Critical => 1,
        crate::state::FatalityState::Safe => 2,
    };

    let a_obligation = match a.coaching_state.obligation {
        crate::state::ObligationState::MustCancel => 0,
        crate::state::ObligationState::MustDownstack => 1,
        crate::state::ObligationState::None => 2,
    };
    let b_obligation = match b.coaching_state.obligation {
        crate::state::ObligationState::MustCancel => 0,
        crate::state::ObligationState::MustDownstack => 1,
        crate::state::ObligationState::None => 2,
    };

    (b_survival, b_obligation)
        .cmp(&(a_survival, a_obligation))
        .then_with(|| b.score.total_cmp(&a.score))
}

fn expand_root(state: &GameState, ctx: &mut SearchExpansionContext<'_>) -> Vec<SearchNode> {
    let mut nodes = Vec::with_capacity(128);
    gen_and_eval_root(state, ctx, &mut nodes);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bag;
    use crate::board::{Board, FULL_ROW};
    use crate::header::{Move, Piece, COL_NB};
    use crate::state::CoachingState;
    use smallvec::{smallvec, SmallVec};
    use std::sync::Arc;

    fn proxy_level_config(beam_width: usize, depth: usize) -> SearchConfig {
        SearchConfig {
            beam_width,
            depth,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: crate::search_config::NnScoringMode::PolicyProxy,
            nn_batch: crate::search_config::NnBatchMode::Level,
            ..SearchConfig::default()
        }
    }

    fn result_signature(result: &SearchResultFull) -> Vec<(u16, u32)> {
        result
            .root_scores
            .iter()
            .map(|(mv, score)| (mv.raw(), score.to_bits()))
            .collect()
    }

    fn deadline_test_state() -> GameState {
        GameState::new(
            Board::new(),
            Piece::T,
            vec![Piece::I, Piece::O, Piece::S, Piece::Z],
        )
    }

    fn deadline_test_config(depth: usize) -> SearchConfig {
        SearchConfig {
            beam_width: 1,
            depth,
            time_budget_ms: Some(1),
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: crate::search_config::NnScoringMode::PolicyProxy,
            ..SearchConfig::default()
        }
    }

    fn run_with_countdown(
        state: &GameState,
        config: &SearchConfig,
        false_queries: u32,
    ) -> (SearchResultFull, crate::search_expand::SearchExpansionStats) {
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);
        let deadline = DeadlineSource::CheckCountdown(std::cell::Cell::new(false_queries));
        let result = find_best_move_with_scores_forced_runtime_deadline(
            state,
            config,
            &EvalWeights::default(),
            None,
            None,
            None,
            Some(&deadline),
        )
        .unwrap_or_else(|| panic!("deadline search should preserve a legal root result"));
        let stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);
        (result, stats)
    }

    #[test]
    fn budget_none_performs_zero_deadline_checks() {
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);
        let state = deadline_test_state();
        let config = SearchConfig {
            time_budget_ms: None,
            nn_scoring: crate::search_config::NnScoringMode::PolicyProxy,
            beam_width: 1,
            depth: 2,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            ..SearchConfig::default()
        };

        let result = find_best_move(&state, &config, &EvalWeights::default());
        let stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);

        assert!(result.is_some());
        assert_eq!(stats.deadline_checks, 0);
        assert_eq!(stats.deadline_hits, 0);
    }

    #[test]
    fn clocked_returns_legal_move_with_completed_root() {
        let state = deadline_test_state();
        let config = deadline_test_config(4);

        let (result, stats) = run_with_countdown(&state, &config, 0);

        assert!(!state.board.obstructed_move(&result.best.best_move));
        assert!(stats.deadline_hits > 0);
        assert!(stats.completed_depth >= 1);
    }

    #[test]
    fn partial_level_discarded() {
        let state = deadline_test_state();
        let config = deadline_test_config(4);
        let expected_config = SearchConfig {
            time_budget_ms: None,
            depth: 2,
            ..deadline_test_config(2)
        };
        let expected =
            find_best_move_with_scores(&state, &expected_config, &EvalWeights::default())
                .unwrap_or_else(|| panic!("depth-two reference should return a move"));

        let (actual, stats) = run_with_countdown(&state, &config, 4);

        assert_eq!(stats.completed_depth, 2);
        assert_eq!(actual.best.best_move, expected.best.best_move);
        assert_eq!(actual.best.score.to_bits(), expected.best.score.to_bits());
        assert_eq!(actual.best.pv, expected.best.pv);
    }

    #[test]
    fn winning_snapshot_reported_across_widening() {
        let state = deadline_test_state();
        let reference_config = SearchConfig {
            beam_width: 200,
            ..deadline_test_config(2)
        };
        let (reference, reference_stats) = run_with_countdown(&state, &reference_config, u32::MAX);
        let widening_config = SearchConfig {
            beam_width: 400,
            ..deadline_test_config(2)
        };

        let (actual, stats) = run_with_countdown(
            &state,
            &widening_config,
            reference_stats.deadline_checks as u32 + 1,
        );

        assert_eq!(actual.best.best_move, reference.best.best_move);
        assert_eq!(actual.best.score.to_bits(), reference.best.score.to_bits());
        assert_eq!(stats.completed_depth, reference_stats.completed_depth);
        assert_eq!(stats.completed_width, 200);
        assert!(stats.deadline_hits > 0);
    }

    #[test]
    fn deterministic_mode_byte_identical() {
        let state = deadline_test_state();
        let config = SearchConfig {
            beam_width: 4,
            depth: 3,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            ..SearchConfig::default()
        };

        let first = find_best_move_with_scores(&state, &config, &EvalWeights::default())
            .unwrap_or_else(|| panic!("first deterministic search should return a move"));
        let second = find_best_move_with_scores(&state, &config, &EvalWeights::default())
            .unwrap_or_else(|| panic!("second deterministic search should return a move"));

        assert_eq!(first.best.best_move.raw(), second.best.best_move.raw());
        assert_eq!(first.best.score.to_bits(), second.best.score.to_bits());
        assert_eq!(first.best.pv, second.best.pv);
        assert_eq!(
            first
                .root_scores
                .iter()
                .map(|(mv, score)| (mv.raw(), score.to_bits()))
                .collect::<Vec<_>>(),
            second
                .root_scores
                .iter()
                .map(|(mv, score)| (mv.raw(), score.to_bits()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn quiescence_expiry_keeps_prior_snapshot() {
        let mut state = deadline_test_state();
        state.b2b = 1;
        let config = SearchConfig {
            depth: 1,
            quiescence_max_extensions: 2,
            quiescence_beam_fraction: 1.0,
            ..deadline_test_config(1)
        };
        let reference_config = SearchConfig {
            time_budget_ms: None,
            quiescence_max_extensions: 0,
            ..deadline_test_config(1)
        };
        let reference =
            find_best_move_with_scores(&state, &reference_config, &EvalWeights::default())
                .unwrap_or_else(|| panic!("pre-extension reference should return a move"));

        let (actual, stats) = run_with_countdown(&state, &config, 2);

        assert_eq!(actual.best.best_move, reference.best.best_move);
        assert_eq!(actual.best.score.to_bits(), reference.best.score.to_bits());
        assert_eq!(actual.best.pv, reference.best.pv);
        assert_eq!(stats.q_extensions_completed, 0);
        assert!(stats.deadline_hits > 0);
    }

    #[test]
    fn clocked_proxy_smoke_records_deadline_activity() {
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);
        let state = deadline_test_state();
        let config = SearchConfig {
            beam_width: 800,
            depth: 14,
            time_budget_ms: Some(1),
            nn_scoring: crate::search_config::NnScoringMode::PolicyProxy,
            ..SearchConfig::default()
        };

        let result = find_best_move(&state, &config, &EvalWeights::default())
            .unwrap_or_else(|| panic!("clocked proxy smoke should preserve a legal move"));
        let stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);

        assert!(!state.board.obstructed_move(&result.best_move));
        assert!(stats.deadline_checks > 0);
        assert!(stats.deadline_hits > 0);
        assert!(stats.completed_depth >= 1);
    }

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

    #[test]
    fn per_child_value_mode_results_byte_identical_to_pre_change() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O, Piece::S]);
        let config = SearchConfig {
            beam_width: 4,
            depth: 2,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: crate::search_config::NnScoringMode::PerChildValue,
            ..SearchConfig::default()
        };

        let result = find_best_move_with_scores_runtime(
            &state,
            &config,
            &EvalWeights::default(),
            &runtime,
            &runtime_context,
        )
        .unwrap_or_else(|| panic!("golden search should return a move"));

        let actual = (
            result.best.best_move.raw(),
            result.best.score.to_bits(),
            result.best.pv.iter().map(|mv| mv.raw()).collect::<Vec<_>>(),
            result
                .root_scores
                .iter()
                .map(|(mv, score)| (mv.raw(), score.to_bits()))
                .collect::<Vec<_>>(),
            (
                result.board_score.to_bits(),
                result.attack_score.to_bits(),
                result.chain_score.to_bits(),
                result.context_score.to_bits(),
                result.path_attack.to_bits(),
                result.path_chain.to_bits(),
                result.path_context.to_bits(),
                result.policy_score.to_bits(),
                result.value_score.to_bits(),
            ),
            result.fallback_used,
        );
        let expected = (
            384,
            1_063_044_931,
            vec![384, 2112],
            vec![(384, 1_063_044_931), (448, 1_063_039_862)],
            (
                1_063_214_852,
                0,
                0,
                0,
                0,
                0,
                0,
                3_184_487_499,
                1_063_214_852,
            ),
            false,
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn proxy_search_produces_valid_pv_and_coaching() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O, Piece::S]);
        let config = SearchConfig {
            beam_width: 4,
            depth: 2,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            nn_scoring: crate::search_config::NnScoringMode::PolicyProxy,
            ..SearchConfig::default()
        };

        let result = find_best_move_with_scores_runtime(
            &state,
            &config,
            &EvalWeights::default(),
            &runtime,
            &runtime_context,
        )
        .unwrap_or_else(|| panic!("proxy search should return a move"));

        assert!(!result.best.pv.is_empty());
        assert_ne!(result.best.coaching_state, CoachingState::default());
        assert!(result.position_complexity.is_finite());
        assert!(result.nn_parent_value.is_some());
    }

    #[test]
    fn proxy_batched_equals_proxy_scalar_results() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let states = [
            GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O, Piece::S]),
            GameState::new(Board::new(), Piece::L, vec![Piece::Z, Piece::T, Piece::O]),
        ];
        let candidates = [
            vec![
                Move::new(Piece::T, crate::header::Rotation::North, 0, 0, false),
                Move::new(Piece::T, crate::header::Rotation::East, 1, 0, false),
            ],
            vec![Move::new(
                Piece::L,
                crate::header::Rotation::South,
                2,
                0,
                false,
            )],
        ];
        let state_refs = [&states[0], &states[1]];
        let candidate_refs = [candidates[0].as_slice(), candidates[1].as_slice()];
        let batch = runtime
            .infer_batch_chunk(&state_refs, &runtime_context, &candidate_refs)
            .unwrap_or_else(|error| panic!("batch parity inference should succeed: {error}"));
        for (row, (state, row_candidates)) in state_refs.iter().zip(candidate_refs).enumerate() {
            let scalar = runtime
                .infer(state, &runtime_context, row_candidates)
                .unwrap_or_else(|error| panic!("scalar parity inference should succeed: {error}"));
            assert_eq!(batch[row].0.len(), scalar.policy_logits.len());
            for (batch_score, scalar_score) in batch[row].0.iter().zip(&scalar.policy_logits) {
                assert!((batch_score - scalar_score).abs() <= 1.0e-6);
            }
            assert!((batch[row].1 - scalar.value).abs() <= 1.0e-6);
        }
        let permuted = runtime
            .infer_batch_chunk(
                &[state_refs[1], state_refs[0]],
                &runtime_context,
                &[candidate_refs[1], candidate_refs[0]],
            )
            .unwrap_or_else(|error| panic!("permuted batch inference should succeed: {error}"));
        assert_eq!(batch[0], permuted[1]);
        assert_eq!(batch[1], permuted[0]);

        for state in states.iter().rev() {
            let level_config = proxy_level_config(32, 1);
            let scalar_config = SearchConfig {
                nn_batch: crate::search_config::NnBatchMode::Scalar,
                ..proxy_level_config(32, 1)
            };
            crate::search_expand::reset_search_expansion_stats();
            crate::search_expand::set_search_profiling_enabled(true);
            let level = find_best_move_with_scores_runtime(
                state,
                &level_config,
                &EvalWeights::default(),
                &runtime,
                &runtime_context,
            )
            .unwrap_or_else(|| panic!("level proxy search should return a move"));
            let level_stats = crate::search_expand::search_expansion_stats();
            crate::search_expand::set_search_profiling_enabled(false);
            let scalar = find_best_move_with_scores_runtime(
                state,
                &scalar_config,
                &EvalWeights::default(),
                &runtime,
                &runtime_context,
            )
            .unwrap_or_else(|| panic!("scalar proxy search should return a move"));

            assert!(level_stats.batch_calls > 0);
            assert_eq!(level.best.best_move, scalar.best.best_move);
            assert_eq!(level.best.pv, scalar.best.pv);
            assert_eq!(level.root_scores.len(), scalar.root_scores.len());
            for ((level_move, level_score), (scalar_move, scalar_score)) in
                level.root_scores.iter().zip(&scalar.root_scores)
            {
                assert_eq!(level_move, scalar_move);
                assert!((level_score - scalar_score).abs() <= 1.0e-6);
            }
            assert!(scalar.root_scores.len() > 1);
            assert!((scalar.root_scores[0].1 - scalar.root_scores[1].1).abs() > 1.0e-6);
        }
    }

    #[test]
    fn batch_calls_counter_equals_chunk_calls_not_nodes() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let state = deadline_test_state();
        let config = proxy_level_config(64, 3);
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);

        let result = find_best_move_with_scores_runtime(
            &state,
            &config,
            &EvalWeights::default(),
            &runtime,
            &runtime_context,
        );
        let stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);

        assert!(result.is_some());
        assert_eq!(stats.batch_calls, 3);
        assert_eq!(stats.batch_calls, stats.runtime_calls);
        assert!(stats.runtime_attempt_rows > stats.batch_calls);
    }

    #[test]
    fn level_mode_without_runtime_falls_back_to_scalar_path() {
        let state = deadline_test_state();
        let level_config = proxy_level_config(64, 3);
        let scalar_config = SearchConfig {
            nn_batch: crate::search_config::NnBatchMode::Scalar,
            ..proxy_level_config(64, 3)
        };
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::reset_level_path_markers();
        crate::search_expand::set_search_profiling_enabled(true);

        let level = find_best_move_with_scores(&state, &level_config, &EvalWeights::default())
            .unwrap_or_else(|| panic!("runtime-free Level search should return a move"));
        let level_stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::reset_search_expansion_stats();
        let scalar = find_best_move_with_scores(&state, &scalar_config, &EvalWeights::default())
            .unwrap_or_else(|| panic!("runtime-free Scalar search should return a move"));
        let scalar_stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);

        assert!(!level_batch_enabled(&level_config, None, None));
        assert_eq!(crate::search_expand::level_path_markers(), (false, false));
        assert_eq!(level.best.best_move, scalar.best.best_move);
        assert_eq!(level.best.hold_used, scalar.best.hold_used);
        assert_eq!(level.best.score.to_bits(), scalar.best.score.to_bits());
        assert_eq!(level.best.pv, scalar.best.pv);
        assert_eq!(level.best.coaching_state, scalar.best.coaching_state);
        assert_eq!(level.best.pv_clear_events, scalar.best.pv_clear_events);
        assert_eq!(result_signature(&level), result_signature(&scalar));
        assert_eq!(
            level.position_complexity.to_bits(),
            scalar.position_complexity.to_bits()
        );
        assert_eq!(level.board_score.to_bits(), scalar.board_score.to_bits());
        assert_eq!(level.attack_score.to_bits(), scalar.attack_score.to_bits());
        assert_eq!(level.chain_score.to_bits(), scalar.chain_score.to_bits());
        assert_eq!(
            level.context_score.to_bits(),
            scalar.context_score.to_bits()
        );
        assert_eq!(level.path_attack.to_bits(), scalar.path_attack.to_bits());
        assert_eq!(level.path_chain.to_bits(), scalar.path_chain.to_bits());
        assert_eq!(level.path_context.to_bits(), scalar.path_context.to_bits());
        assert_eq!(level.policy_score.to_bits(), scalar.policy_score.to_bits());
        assert_eq!(level.value_score.to_bits(), scalar.value_score.to_bits());
        assert_eq!(level.fallback_used, scalar.fallback_used);
        assert_eq!(level.nn_parent_value, scalar.nn_parent_value);
        assert_eq!(level_stats.batch_calls, 0);
        assert_eq!(level_stats.runtime_calls, 0);
        assert_eq!(
            level_stats.runtime_unavailable_nodes,
            scalar_stats.runtime_unavailable_nodes
        );
        assert!(level_stats.runtime_unavailable_nodes > 0);

        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        crate::search_expand::reset_level_path_markers();
        let result = find_best_move_with_scores_runtime(
            &state,
            &level_config,
            &EvalWeights::default(),
            &runtime,
            &runtime_context,
        );

        assert!(result.is_some());
        let (root_entered, expand_entered) = crate::search_expand::level_path_markers();
        assert!(root_entered);
        assert!(expand_entered);
    }

    #[test]
    fn expansion_slices_level_into_chunks() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let state = deadline_test_state();
        let config = proxy_level_config(300, 3);
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);

        let result = find_best_move_with_scores_runtime(
            &state,
            &config,
            &EvalWeights::default(),
            &runtime,
            &runtime_context,
        );
        let stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);

        assert!(result.is_some());
        let deepest_level_rows = stats.expanded_nodes - 1 - 32;
        assert!(
            deepest_level_rows > 256,
            "fixture must span multiple chunks: expanded={}, attempts={}, calls={}, checks={}",
            stats.expanded_nodes,
            stats.runtime_attempt_rows,
            stats.batch_calls,
            stats.deadline_checks
        );
        assert_eq!(stats.batch_calls, 2 + deepest_level_rows.div_ceil(256));
        assert_eq!(stats.max_batch_rows, 256);
        assert_eq!(stats.runtime_attempt_rows, stats.expanded_nodes);
    }

    #[test]
    fn nn_mode_in_process_determinism() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let config = proxy_level_config(16, 2);
        let side_a = crate::versus::PlayerCfg {
            search: config,
            label: "A".to_owned(),
        };
        let side_b = crate::versus::PlayerCfg {
            search: proxy_level_config(16, 2),
            label: "B".to_owned(),
        };
        let mut record_first = Vec::new();
        let first = crate::versus::play_game(
            77_000_002,
            0,
            [&side_a, &side_b],
            Some(&runtime),
            &EvalWeights::default(),
            2,
            &mut |record| record_first.push(record),
        );
        let mut record_second = Vec::new();
        let second = crate::versus::play_game(
            77_000_002,
            0,
            [&side_a, &side_b],
            Some(&runtime),
            &EvalWeights::default(),
            2,
            &mut |record| record_second.push(record),
        );
        let games_first = [crate::versus::report::RecordedGame {
            game: 0,
            seed: 77_000_002,
            slot0_label: "A",
            slot1_label: "B",
            a_slot: 0,
            result: &first,
        }];
        let games_second = [crate::versus::report::RecordedGame {
            game: 0,
            seed: 77_000_002,
            slot0_label: "A",
            slot1_label: "B",
            a_slot: 0,
            result: &second,
        }];
        let first_json = crate::versus::report::replays_jsonl(
            &games_first,
            crate::versus::report::ReportMode::Deterministic,
        );
        let second_json = crate::versus::report::replays_jsonl(
            &games_second,
            crate::versus::report::ReportMode::Deterministic,
        );

        assert_eq!(record_first, first.per_move);
        assert_eq!(record_second, second.per_move);
        assert_eq!(first_json, second_json);
        let mut corrupted = second_json.into_bytes();
        let byte = corrupted
            .iter_mut()
            .find(|byte| byte.is_ascii_digit())
            .unwrap_or_else(|| panic!("replay JSON should contain a numeric byte"));
        *byte = if *byte == b'9' { b'8' } else { *byte + 1 };
        assert_ne!(first_json.as_bytes(), corrupted);
    }

    #[test]
    fn whole_chunk_failure_falls_back_heuristically() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let state = deadline_test_state();
        let config = proxy_level_config(600, 3);
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);
        crate::search_expand::set_poison_batch_chunk(Some(1));

        let actual = find_best_move_with_scores_runtime(
            &state,
            &config,
            &EvalWeights::default(),
            &runtime,
            &runtime_context,
        )
        .unwrap_or_else(|| panic!("fallback search should return a move"));
        crate::search_expand::set_poison_batch_chunk(None);
        let stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);
        crate::search_expand::set_poison_batch_chunk(Some(0));
        let expected = find_best_move_with_scores_runtime(
            &state,
            &config,
            &EvalWeights::default(),
            &runtime,
            &runtime_context,
        )
        .unwrap_or_else(|| panic!("first-chunk fallback reference should return a move"));
        crate::search_expand::set_poison_batch_chunk(None);

        assert_eq!(stats.fallback_levels, 1);
        assert_eq!(stats.abandoned_levels, 0);
        assert!(stats.abandoned_nodes > 0);
        assert!(stats.inferred_rows < stats.runtime_attempt_rows);
        assert_eq!(
            stats.runtime_attempt_rows + stats.abandoned_nodes,
            stats.expanded_nodes
        );
        assert_eq!(stats.batch_calls, 4, "chunk 3 must remain undispatched");
        assert_eq!(actual.best.best_move, expected.best.best_move);
        assert_eq!(actual.best.score.to_bits(), expected.best.score.to_bits());
        assert_eq!(actual.best.pv, expected.best.pv);
        assert_eq!(result_signature(&actual), result_signature(&expected));
        assert_eq!(actual.policy_score.to_bits(), 0.0f32.to_bits());
        assert!(actual.nn_parent_value.is_none());
        assert!(actual.fallback_used);
    }

    #[test]
    fn deadline_between_batch_chunks_discards_level_without_fallback() {
        let Some(runtime) = load_checked_in_runtime() else {
            return;
        };
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: Board::new(),
        };
        let state = deadline_test_state();
        let config = SearchConfig {
            time_budget_ms: Some(1),
            ..proxy_level_config(300, 3)
        };
        let reference_deadline = DeadlineSource::CheckCountdown(std::cell::Cell::new(275));
        let reference = find_best_move_with_scores_forced_runtime_deadline(
            &state,
            &config,
            &EvalWeights::default(),
            Some(&runtime),
            Some(&runtime_context),
            None,
            Some(&reference_deadline),
        )
        .unwrap_or_else(|| panic!("completed-level reference should return a move"));
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);
        let deadline = DeadlineSource::CheckCountdown(std::cell::Cell::new(276));

        let actual = find_best_move_with_scores_forced_runtime_deadline(
            &state,
            &config,
            &EvalWeights::default(),
            Some(&runtime),
            Some(&runtime_context),
            None,
            Some(&deadline),
        )
        .unwrap_or_else(|| panic!("abandoned-level search should preserve a move"));
        let stats = crate::search_expand::search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);

        let deepest_level_rows = stats.expanded_nodes - 1 - 32;
        assert!(
            deepest_level_rows > 256,
            "fixture must span multiple chunks: expanded={}, attempts={}, calls={}, checks={}",
            stats.expanded_nodes,
            stats.runtime_attempt_rows,
            stats.batch_calls,
            stats.deadline_checks
        );
        assert_eq!(stats.abandoned_levels, 1);
        assert_eq!(stats.fallback_levels, 0);
        assert!(stats.abandoned_nodes > 0);
        assert!(
            stats.inferred_rows > 0,
            "chunk 1 must complete before expiry"
        );
        assert_eq!(
            stats.runtime_attempt_rows + stats.abandoned_nodes,
            stats.expanded_nodes
        );
        assert_eq!(actual.best.best_move, reference.best.best_move);
        assert_eq!(actual.best.score.to_bits(), reference.best.score.to_bits());
        assert_eq!(actual.best.pv, reference.best.pv);
    }

    fn make_node(
        score: f32,
        fatality: crate::state::FatalityState,
        obligation: crate::state::ObligationState,
    ) -> SearchNode {
        let coaching = CoachingState {
            fatality,
            obligation,
            ..CoachingState::default()
        };

        SearchNode {
            board: Board::new(),
            current: Some(Piece::T),
            queue: SmallVec::new(),
            score,
            hold: None,
            b2b: 0,
            combo: 0,
            pending_garbage: 0,
            lines_total: 0,
            bag_number: 0,
            pieces_into_bag: 0,
            coaching,
            root_move: Move::none(),
            root_hold_used: false,
            path: smallvec![Move::none()],
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
            path_clear_events: Arc::new(Vec::new()),
        }
    }

    #[test]
    fn test_find_best_move_empty_board() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let config = SearchConfig::default();
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        assert!(result.is_some(), "should find a move on empty board");

        let r = result.unwrap_or_else(|| panic!("already checked"));
        assert!(!r.pv.is_empty(), "PV should have at least one move");
    }

    #[test]
    fn test_result_move_is_valid() {
        let state = GameState::new(Board::new(), Piece::I, vec![Piece::T]);
        let config = SearchConfig {
            beam_width: 100,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights)
            .unwrap_or_else(|| panic!("should find moves"));

        // verify the move can be applied
        let m = &result.best_move;
        let board = Board::new();
        assert!(
            !board.obstructed_move(m),
            "best move should be valid placement"
        );
    }

    #[test]
    fn test_depth_1_returns_immediately() {
        let state = GameState::new(
            Board::new(),
            Piece::S,
            vec![], // no queue
        );
        let config = SearchConfig {
            beam_width: 50,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        assert!(result.is_some());
        let r = result.unwrap_or_else(|| panic!("checked"));
        assert_eq!(r.pv.len(), 1, "depth-1 search should have single-move PV");
    }

    #[test]
    fn search_records_sort_prune_truncate_timing() {
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let config = SearchConfig {
            beam_width: 20,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        let stats = crate::search_expand::search_expansion_stats();

        assert!(result.is_some());
        assert!(stats.sort_prune_truncate_nanos > 0);
        crate::search_expand::set_search_profiling_enabled(false);
    }

    #[test]
    fn new_counters_zero_when_profiling_disabled() {
        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let config = SearchConfig {
            beam_width: 20,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        let stats = crate::search_expand::search_expansion_stats();

        assert!(result.is_some());
        assert_eq!(stats.action_generation_nanos, 0);
        assert_eq!(stats.legal_filter_nanos, 0);
        assert_eq!(stats.child_eval_nanos, 0);
        assert_eq!(stats.do_move_nanos, 0);
        assert_eq!(stats.eval_fallback_nanos, 0);
        assert_eq!(stats.tt_probe_nanos, 0);
        assert_eq!(stats.sort_prune_truncate_nanos, 0);
        assert_eq!(stats.runtime_attempt_rows, 0);
        assert_eq!(stats.runtime_unavailable_nodes, 0);
        assert_eq!(stats.abandoned_nodes, 0);
        assert_eq!(stats.runtime_calls, 0);
        assert_eq!(stats.batch_calls, 0);
        assert_eq!(stats.inferred_rows, 0);
        assert_eq!(stats.max_batch_rows, 0);
        assert_eq!(stats.deadline_checks, 0);
        assert_eq!(stats.deadline_hits, 0);
        assert_eq!(stats.completed_depth, 0);
        assert_eq!(stats.completed_width, 0);
        assert_eq!(stats.fallback_levels, 0);
        assert_eq!(stats.abandoned_levels, 0);
        assert_eq!(stats.noninferable_nodes, 0);
        assert_eq!(stats.q_extensions_completed, 0);
    }

    #[test]
    fn profiling_does_not_change_search_result() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let config = SearchConfig {
            beam_width: 40,
            depth: 2,
            use_tt: true,
            extend_queue_7bag: false,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(false);
        let baseline = find_best_move_with_scores(&state, &config, &weights)
            .unwrap_or_else(|| panic!("baseline search should return a move"));

        crate::search_expand::reset_search_expansion_stats();
        crate::search_expand::set_search_profiling_enabled(true);
        let profiled = find_best_move_with_scores(&state, &config, &weights)
            .unwrap_or_else(|| panic!("profiled search should return a move"));
        crate::search_expand::set_search_profiling_enabled(false);

        assert_eq!(profiled.best.best_move, baseline.best.best_move);
        assert_eq!(profiled.best.hold_used, baseline.best.hold_used);
        assert_eq!(profiled.best.pv, baseline.best.pv);
        assert_eq!(profiled.root_scores.len(), baseline.root_scores.len());
        assert_eq!(profiled.root_scores[0].0, baseline.root_scores[0].0);
    }

    /// The TT is a pure eval cache: stored scores are `evaluate(board)` values
    /// keyed by exact 64-bit zobrist match. Search results must be identical
    /// with the cache on or off (absent hash collisions).
    #[test]
    fn tt_does_not_change_search_result() {
        let mut mid_rows = [0u16; 40];
        mid_rows[0] = 0x37F;
        mid_rows[1] = 0x3BF;
        mid_rows[2] = 0x1FF;
        mid_rows[3] = 0x3FD;
        mid_rows[4] = 0x2FF;
        mid_rows[5] = 0x07F;

        let mut tall_rows = [0u16; 40];
        for (y, row) in tall_rows.iter_mut().enumerate().take(14) {
            *row = match y {
                3 => 0x1BF,
                7 => 0x17F,
                _ => 0x1FF,
            };
        }

        let queue = vec![Piece::I, Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z];
        let weights = EvalWeights::default();
        let config = |use_tt: bool| SearchConfig {
            beam_width: 60,
            depth: 4,
            use_tt,
            extend_queue_7bag: false,
            ..SearchConfig::default()
        };

        for rows in [[0u16; 40], mid_rows, tall_rows] {
            let state = GameState::new(probe_board_from_rows(rows), Piece::T, queue.clone());

            let with_tt = find_best_move_with_scores(&state, &config(true), &weights)
                .unwrap_or_else(|| panic!("tt search should return a move"));
            let no_tt = find_best_move_with_scores(&state, &config(false), &weights)
                .unwrap_or_else(|| panic!("no-tt search should return a move"));

            assert_eq!(with_tt.best.best_move, no_tt.best.best_move);
            assert_eq!(with_tt.best.hold_used, no_tt.best.hold_used);
            assert_eq!(with_tt.best.pv, no_tt.best.pv);
            assert_eq!(with_tt.best.score.to_bits(), no_tt.best.score.to_bits());
            assert_eq!(with_tt.root_scores.len(), no_tt.root_scores.len());
            for (a, b) in with_tt.root_scores.iter().zip(no_tt.root_scores.iter()) {
                assert_eq!(a.0, b.0);
                assert_eq!(a.1.to_bits(), b.1.to_bits());
            }
        }
    }

    #[test]
    fn test_hold_swap_considered() {
        // set up a state where holding might help
        // T piece current, I piece in hold; I piece tetris should be considered
        let mut state = GameState::new(
            Board::new(),
            Piece::O, // O is least flexible
            vec![Piece::S],
        );
        state.hold = Some(Piece::I); // I is great for tetrises

        let config = SearchConfig {
            beam_width: 200,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        assert!(result.is_some(), "should find a move with hold available");
    }

    #[test]
    fn test_hold_none_uses_queue() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let config = SearchConfig {
            beam_width: 200,
            depth: 2,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        assert!(result.is_some());
        let r = result.unwrap_or_else(|| panic!("checked"));
        assert!(r.pv.len() <= 2, "PV shouldn't exceed depth");
    }

    #[test]
    fn test_beam_width_respected() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O, Piece::L]);
        // very narrow beam
        let config = SearchConfig {
            beam_width: 3,
            depth: 3,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        assert!(result.is_some(), "narrow beam should still find something");
    }

    #[test]
    fn test_bag_extends_search_depth() {
        let mut state = GameState::new(Board::new(), Piece::O, vec![Piece::T, Piece::L, Piece::J]);
        state.hold = Some(Piece::I);

        let weights = EvalWeights::default();
        let baseline_config = SearchConfig {
            beam_width: 200,
            depth: 6,
            extend_queue_7bag: false,
            ..SearchConfig::default()
        };
        let extended_config = SearchConfig {
            beam_width: 200,
            depth: 6,
            extend_queue_7bag: true,
            ..SearchConfig::default()
        };

        let baseline = find_best_move(&state, &baseline_config, &weights)
            .unwrap_or_else(|| panic!("baseline search should return a move"));
        let extended = find_best_move(&state, &extended_config, &weights)
            .unwrap_or_else(|| panic!("extended search should return a move"));
        let extended_queue = bag::extend_queue(&state.queue, state.current, state.hold);

        assert!(extended_queue.len() > state.queue.len());
        assert_eq!(
            baseline.pv.len(),
            baseline_config.depth.min(state.queue.len() + 1),
            "baseline depth should use visible queue only"
        );
        assert!(
            extended.pv.len() >= baseline.pv.len(),
            "extended search should not shorten the principal variation horizon"
        );
        assert!(
            extended.pv.len() <= extended_config.depth.min(extended_queue.len() + 1),
            "extended search should still stay within the 7-bag horizon"
        );
    }

    #[test]
    fn test_tt_deduplicates() {
        let mut state = GameState::new(
            Board::new(),
            Piece::O,
            vec![Piece::T, Piece::L, Piece::J, Piece::S],
        );
        state.hold = Some(Piece::I);

        let weights = EvalWeights::default();
        let baseline_config = SearchConfig {
            beam_width: 250,
            depth: 5,
            use_tt: false,
            extend_queue_7bag: false,
            ..SearchConfig::default()
        };
        let tt_config = SearchConfig {
            beam_width: 250,
            depth: 5,
            use_tt: true,
            extend_queue_7bag: false,
            ..SearchConfig::default()
        };

        let baseline = find_best_move(&state, &baseline_config, &weights)
            .unwrap_or_else(|| panic!("baseline search should return a move"));
        let with_tt = find_best_move(&state, &tt_config, &weights)
            .unwrap_or_else(|| panic!("tt search should return a move"));

        assert_eq!(with_tt.best_move, baseline.best_move);
        assert_eq!(with_tt.hold_used, baseline.hold_used);
    }

    #[test]
    fn tt_on_matches_tt_off_exactly_on_seeded_states() {
        // TT caches the exact f32 from evaluate(board, weights), so a hit must
        // reproduce the recompute bit-for-bit.
        let mut seed;
        let xs = |s: &mut u64| {
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            *s
        };
        for case in 0..6u64 {
            seed = 0x7757_0611_2026_0001u64.wrapping_add(case.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut board = Board::new();
            let height = 4 + (xs(&mut seed) % 8) as usize;
            for y in 0..height {
                let mut row = (xs(&mut seed) & 0x3FF) as u16;
                row &= !(1u16 << (xs(&mut seed) % 10));
                board.rows[y] = row;
            }
            for y in 0..height {
                let mut bits = board.rows[y] as u64;
                while bits != 0 {
                    let x = bits.trailing_zeros() as usize;
                    board.cols[x] |= 1u64 << y;
                    bits &= bits - 1;
                }
            }
            let pieces = [
                Piece::I,
                Piece::O,
                Piece::T,
                Piece::L,
                Piece::J,
                Piece::S,
                Piece::Z,
            ];
            let current = pieces[(xs(&mut seed) % 7) as usize];
            let queue: Vec<Piece> = (0..5)
                .map(|_| pieces[(xs(&mut seed) % 7) as usize])
                .collect();
            let mut state = GameState::new(board, current, queue);
            state.hold = Some(pieces[(xs(&mut seed) % 7) as usize]);

            let weights = EvalWeights::default();
            let off = SearchConfig {
                beam_width: 200,
                depth: 5,
                use_tt: false,
                extend_queue_7bag: false,
                ..SearchConfig::default()
            };
            let on = SearchConfig {
                beam_width: 200,
                depth: 5,
                use_tt: true,
                extend_queue_7bag: false,
                ..SearchConfig::default()
            };
            let a = find_best_move(&state, &off, &weights);
            let b = find_best_move(&state, &on, &weights);
            match (a, b) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    assert_eq!(a.best_move, b.best_move, "case={case}");
                    assert_eq!(a.hold_used, b.hold_used, "case={case}");
                    assert_eq!(a.score.to_bits(), b.score.to_bits(), "case={case}");
                    let pa: Vec<u16> = a.pv.iter().map(|m| m.raw()).collect();
                    let pb: Vec<u16> = b.pv.iter().map(|m| m.raw()).collect();
                    assert_eq!(pa, pb, "case={case}");
                }
                _ => panic!("tt presence changed move availability, case={case}"),
            }
        }
    }

    #[test]
    fn test_no_moves_returns_none() {
        // fill the board nearly to the top, no valid placements
        let mut board = Board::new();
        for y in 0..40 {
            board.rows[y] = FULL_ROW;
        }
        for x in 0..COL_NB {
            board.cols[x] = !0u64; // all bits set
        }

        let state = GameState::new(board, Piece::I, vec![]);
        let config = SearchConfig::default();
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        assert!(result.is_none(), "full board should have no moves");
    }

    #[test]
    fn test_futility_prunes_bad_moves() {
        let mut nodes = vec![
            SearchNode {
                board: Board::new(),
                current: Some(Piece::T),
                queue: SmallVec::new(),
                score: 10.0,
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
                path: smallvec![Move::none()],
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
                path_clear_events: Arc::new(Vec::new()),
            },
            SearchNode {
                board: Board::new(),
                current: Some(Piece::T),
                queue: SmallVec::new(),
                score: 8.5,
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
                path: smallvec![Move::none()],
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
                path_clear_events: Arc::new(Vec::new()),
            },
            SearchNode {
                board: Board::new(),
                current: Some(Piece::T),
                queue: SmallVec::new(),
                score: 5.0,
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
                path: smallvec![Move::none()],
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
                path_clear_events: Arc::new(Vec::new()),
            },
        ];

        apply_futility_pruning(&mut nodes, 3.0, None);

        assert_eq!(nodes.len(), 2, "score 5.0 should be pruned");
        assert!(nodes.iter().all(|node| node.score >= 7.0));
    }

    #[test]
    fn test_iterative_widening_returns_result() {
        let state = GameState::new(
            Board::new(),
            Piece::T,
            vec![Piece::I, Piece::O, Piece::L, Piece::J],
        );
        let config = SearchConfig {
            beam_width: 400,
            depth: 4,
            time_budget_ms: Some(100),
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights);
        assert!(
            result.is_some(),
            "iterative widening should return a result"
        );

        let r = result.unwrap_or_else(|| panic!("checked"));
        assert!(!r.pv.is_empty(), "PV should include at least one move");
    }

    #[test]
    fn test_compare_prefers_survival_before_raw_score() {
        let nodes = &mut [
            make_node(
                999.0,
                crate::state::FatalityState::Critical,
                crate::state::ObligationState::MustCancel,
            ),
            make_node(
                10.0,
                crate::state::FatalityState::Safe,
                crate::state::ObligationState::None,
            ),
        ];

        nodes.sort_unstable_by(compare_nodes_desc);

        assert_eq!(
            nodes[0].coaching.fatality,
            crate::state::FatalityState::Safe
        );
        assert_eq!(
            nodes[0].coaching.obligation,
            crate::state::ObligationState::None
        );
        assert_eq!(
            nodes[1].coaching.fatality,
            crate::state::FatalityState::Critical
        );
    }

    #[test]
    fn test_futility_preserves_best_survival_tier() {
        let mut nodes = vec![
            make_node(
                1000.0,
                crate::state::FatalityState::Critical,
                crate::state::ObligationState::MustCancel,
            ),
            make_node(
                12.0,
                crate::state::FatalityState::Safe,
                crate::state::ObligationState::MustDownstack,
            ),
            make_node(
                9.0,
                crate::state::FatalityState::Safe,
                crate::state::ObligationState::None,
            ),
            make_node(
                4.0,
                crate::state::FatalityState::Safe,
                crate::state::ObligationState::None,
            ),
        ];

        apply_futility_pruning(&mut nodes, 3.0, None);

        assert!(nodes.iter().all(|n| {
            n.coaching.fatality == crate::state::FatalityState::Safe
                && n.coaching.obligation == crate::state::ObligationState::None
        }));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].score, 9.0);
    }

    #[test]
    fn test_must_cancel_detected_from_imminent_garbage() {
        let mut state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        state.pending_garbage = 4;

        let config = SearchConfig {
            beam_width: 100,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights)
            .unwrap_or_else(|| panic!("expected a legal move"));

        assert_eq!(
            result.coaching_state.obligation,
            crate::state::ObligationState::MustCancel
        );
    }

    #[test]
    fn test_spawn_envelope_violation_forces_fatal_tier() {
        let mut board = Board::new();
        board.rows[crate::default_ruleset::ACTIVE_RULES.spawn_row as usize] = 1u16 << 4;
        board.cols = board.compute_cols();

        let state = GameState::new(board, Piece::T, vec![Piece::I, Piece::O]);
        let config = SearchConfig {
            beam_width: 100,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let result = find_best_move(&state, &config, &weights)
            .unwrap_or_else(|| panic!("expected a legal move"));

        assert_eq!(
            result.coaching_state.fatality,
            crate::state::FatalityState::Fatal
        );
    }

    #[test]
    fn test_position_complexity_varies() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let config = SearchConfig {
            beam_width: 200,
            depth: 2,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();

        let full = find_best_move_with_scores(&state, &config, &weights)
            .unwrap_or_else(|| panic!("should find moves"));

        // On a clean board with multiple root moves, complexity should be >= 0
        assert!(
            full.position_complexity >= 0.0,
            "position_complexity should be non-negative, got {}",
            full.position_complexity
        );
    }

    /// Build a board from raw row masks with the cols cache rebuilt to match,
    /// mirroring `Board::rebuild_cols` (private to board.rs).
    fn probe_board_from_rows(rows: [u16; 40]) -> Board {
        let mut b = Board::new();
        b.rows = rows;
        b.cols = [0; 10];
        for (y, &row) in b.rows.iter().enumerate() {
            let mut bits = row as u64;
            while bits != 0 {
                let x = bits.trailing_zeros() as usize;
                b.cols[x] |= 1u64 << y;
                bits &= bits - 1;
            }
        }
        b
    }

    /// Search latency probe for `find_best_move_with_scores`
    /// (the analysis/labeler per-position entry point) across representative
    /// board shapes and production configs. Not a correctness test.
    ///
    /// Run: RTK_DISABLED=1 cargo test --release search_latency_probe -- --ignored --nocapture
    #[test]
    #[ignore]
    fn search_latency_probe() {
        use std::time::Instant;

        // Mid-game holey fixture (same shape as bench_beam's BOARD_ROWS).
        let mut mid_rows = [0u16; 40];
        mid_rows[0] = 0x37F;
        mid_rows[1] = 0x3BF;
        mid_rows[2] = 0x1FF;
        mid_rows[3] = 0x3FD;
        mid_rows[4] = 0x2FF;
        mid_rows[5] = 0x07F;

        // Pressure stack: 14 rows, col-9 well, two buried holes.
        let mut tall_rows = [0u16; 40];
        for (y, row) in tall_rows.iter_mut().enumerate().take(14) {
            *row = match y {
                3 => 0x1BF,
                7 => 0x17F,
                _ => 0x1FF,
            };
        }

        let queue = vec![Piece::I, Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z];

        struct Scenario {
            name: &'static str,
            rows: [u16; 40],
            current: Piece,
            b2b: u8,
            combo: u32,
            pending: u8,
        }
        let scenarios = [
            Scenario {
                name: "opening_empty",
                rows: [0u16; 40],
                current: Piece::T,
                b2b: 0,
                combo: 0,
                pending: 0,
            },
            Scenario {
                name: "midgame_holey",
                rows: mid_rows,
                current: Piece::T,
                b2b: 1,
                combo: 0,
                pending: 0,
            },
            Scenario {
                name: "pressure_tall",
                rows: tall_rows,
                current: Piece::L,
                b2b: 1,
                combo: 2,
                pending: 4,
            },
        ];

        let configs: [(&str, SearchConfig); 2] = [
            ("default_800x14", SearchConfig::default()),
            (
                "width300",
                SearchConfig {
                    beam_width: 300,
                    ..SearchConfig::default()
                },
            ),
        ];

        let weights = EvalWeights::default();
        const WARMUP: usize = 2;
        const ITERS: usize = 12;

        for (cfg_name, config) in &configs {
            for sc in &scenarios {
                let mut state =
                    GameState::new(probe_board_from_rows(sc.rows), sc.current, queue.clone());
                state.b2b = sc.b2b;
                state.combo = sc.combo;
                state.pending_garbage = sc.pending;

                for _ in 0..WARMUP {
                    let r = find_best_move_with_scores(&state, config, &weights);
                    assert!(r.is_some(), "warmup search should find a move");
                }

                let mut samples_ns: Vec<u128> = Vec::with_capacity(ITERS);
                for _ in 0..ITERS {
                    let t0 = Instant::now();
                    let r = find_best_move_with_scores(&state, config, &weights);
                    samples_ns.push(t0.elapsed().as_nanos());
                    assert!(r.is_some(), "probe search should find a move");
                }
                samples_ns.sort_unstable();
                let min = samples_ns[0];
                let p50 = samples_ns[ITERS / 2];
                let p95 = samples_ns[(ITERS * 95).div_ceil(100).min(ITERS) - 1];
                println!(
                    "PROBE search_latency {cfg_name} {name} iters={ITERS} min={:.3}ms p50={:.3}ms p95={:.3}ms",
                    min as f64 / 1e6,
                    p50 as f64 / 1e6,
                    p95 as f64 / 1e6,
                    name = sc.name,
                );
            }
        }
    }
}
