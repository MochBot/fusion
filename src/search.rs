// search.rs -- beam search with hold for coaching engine
// expands moves breadth-first, pruned to beam_width at each depth

use crate::bag;

use crate::eval::EvalWeights;
use crate::policy_value_runtime::{PolicyValueRuntime, PolicyValueRuntimeContext};
use crate::search_config::FUTILITY_DELTA;

use crate::state::GameState;
use crate::transposition::{get_zobrist_keys, TranspositionTable, DEFAULT_TT_SIZE};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

pub use crate::search_config::{SearchConfig, SearchNode, SearchResult, SearchResultFull};
pub(crate) use crate::search_config::{SearchExpansionContext, SearchIterationParams};
pub(crate) use crate::search_expand::{
    expand_node, gen_and_eval_root, profile_root_score_aggregation, profile_sort_prune_truncate,
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
        };
        return run_beam_search_iteration(&mut params);
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
        };
        if let Some(full) = run_beam_search_iteration(&mut params) {
            let should_replace = best_full
                .as_ref()
                .is_none_or(|prev| compare_results_desc(&full.best, &prev.best).is_lt());

            if should_replace {
                best_full = Some(full);
            }
        }

        if width >= max_width {
            break;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(budget) = time_budget {
                if start.elapsed() >= budget {
                    break;
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

fn run_beam_search_iteration(params: &mut SearchIterationParams<'_>) -> Option<SearchResultFull> {
    let mut ctx = SearchExpansionContext {
        config: params.config,
        current_beam_width: params.beam_width,
        weights: params.weights,
        remaining_depth: params.max_depth.saturating_sub(1),
        zobrist_keys: params.zobrist_keys,
        tt: params.tt,
        policy_value: params.policy_value,
        runtime_context: params.runtime_context,
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

    for depth_idx in 0..params.max_depth.saturating_sub(1) {
        let child_depth = depth_idx + 2;
        ctx.remaining_depth = params.max_depth.saturating_sub(child_depth);

        let mut next_beam: Vec<SearchNode> =
            Vec::with_capacity(params.beam_width.saturating_mul(2));

        for node in &beam {
            expand_node(node, &mut ctx, &mut next_beam);
        }

        if next_beam.is_empty() {
            break;
        }

        profile_sort_prune_truncate(|| {
            apply_futility_pruning(&mut next_beam, FUTILITY_DELTA, params.forced_root_move);
            next_beam.sort_unstable_by(compare_nodes_desc);
            truncate_with_forced(&mut next_beam, params.beam_width, params.forced_root_move);
        });
        beam = next_beam;
    }

    // Quiescence extensions: extend loud nodes past the normal depth boundary
    // so investment moves (mid-combo, active B2B) resolve before evaluation.
    let q_max = params.config.quiescence_max_extensions;
    let q_beam_width =
        ((params.beam_width as f32) * params.config.quiescence_beam_fraction).ceil() as usize;
    if q_max > 0 && q_beam_width > 0 {
        let main_depth = params.max_depth.saturating_sub(1);
        let loud_nodes: Vec<SearchNode> = beam.iter().filter(|n| n.is_loud()).cloned().collect();

        if !loud_nodes.is_empty() {
            let mut q_beam = loud_nodes;
            profile_sort_prune_truncate(|| {
                q_beam.sort_unstable_by(compare_nodes_desc);
                q_beam.truncate(q_beam_width);
            });

            for ext in 0..q_max {
                let child_depth = main_depth + ext + 2;
                ctx.remaining_depth = params
                    .max_depth
                    .saturating_sub(child_depth.min(params.max_depth));

                let mut next_q: Vec<SearchNode> = Vec::with_capacity(q_beam_width * 2);

                for node in &q_beam {
                    expand_node(node, &mut ctx, &mut next_q);
                }

                if next_q.is_empty() {
                    break;
                }

                profile_sort_prune_truncate(|| {
                    next_q.sort_unstable_by(compare_nodes_desc);
                    next_q.truncate(q_beam_width);
                });

                for node in &next_q {
                    if !node.is_loud() {
                        beam.push(node.clone());
                    }
                }

                q_beam = next_q.into_iter().filter(|n| n.is_loud()).collect();
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

    Some(SearchResultFull {
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
    })
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
    fn search_timing_is_disabled_by_default() {
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
