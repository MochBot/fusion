// search.rs -- beam search with hold for coaching engine
// expands moves breadth-first, pruned to beam_width at each depth

use crate::attack::AttackConfig;
use crate::bag;
use crate::board::Board;
use crate::eval::{evaluate, EvalWeights};
use crate::header::*;
use crate::movegen::{generate, MoveBuffer};
use crate::state::GameState;
use crate::transposition::{TranspositionTable, ZobristKeys, DEFAULT_TT_SIZE};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

pub struct SearchConfig {
    pub beam_width: usize,
    pub depth: usize,
    pub futility_delta: f32,
    pub time_budget_ms: Option<u64>,
    pub use_tt: bool,
    pub extend_queue_7bag: bool,
    pub attack_config: AttackConfig,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            beam_width: 300,
            depth: 12,
            futility_delta: 3.0,
            time_budget_ms: None,
            use_tt: false,
            extend_queue_7bag: true,
            attack_config: AttackConfig::tetra_league(),
        }
    }
}

pub struct SearchResult {
    pub best_move: Move,
    pub hold_used: bool,
    pub score: f32,
    pub pv: Vec<Move>,
}

#[derive(Clone)]
struct SearchNode {
    board: Board,
    score: f32,
    hold: Option<Piece>,
    root_move: Move,
    root_hold_used: bool,
    path: Vec<Move>,
}

/// beam search from game state
/// returns the best move found, or None if no legal moves exist
pub fn find_best_move(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
) -> Option<SearchResult> {
    let search_queue = if config.extend_queue_7bag {
        bag::extend_queue(&state.queue, state.current, state.hold)
    } else {
        state.queue.clone()
    };

    // actual search depth — capped by queue length + 1 (current piece)
    let max_depth = config.depth.min(search_queue.len() + 1);
    if max_depth == 0 {
        return None;
    }

    let zobrist_keys = ZobristKeys::new();
    let mut tt = config
        .use_tt
        .then(|| TranspositionTable::new(DEFAULT_TT_SIZE));

    if config.time_budget_ms.is_none() {
        return run_beam_search_iteration(
            state,
            &search_queue,
            config,
            weights,
            max_depth,
            config.beam_width,
            &zobrist_keys,
            &mut tt,
        );
    }

    let max_width = config.beam_width;
    if max_width == 0 {
        return None;
    }

    let mut width = 100.min(max_width);
    let mut best_result: Option<SearchResult> = None;

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

        if let Some(result) = run_beam_search_iteration(
            state,
            &search_queue,
            config,
            weights,
            max_depth,
            width,
            &zobrist_keys,
            &mut tt,
        ) {
            let should_replace = match &best_result {
                Some(best) => result.score > best.score,
                None => true,
            };

            if should_replace {
                best_result = Some(result);
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

    best_result
}

fn run_beam_search_iteration(
    state: &GameState,
    queue: &[Piece],
    config: &SearchConfig,
    weights: &EvalWeights,
    max_depth: usize,
    beam_width: usize,
    zobrist_keys: &ZobristKeys,
    tt: &mut Option<TranspositionTable>,
) -> Option<SearchResult> {
    // expand root: generate moves for current piece, and hold piece if available
    let mut beam = expand_root(
        state,
        weights,
        &config.attack_config,
        max_depth,
        zobrist_keys,
        tt,
    );
    if beam.is_empty() {
        return None;
    }

    // sort descending by score, truncate to beam width
    apply_futility_pruning(&mut beam, config.futility_delta);
    beam.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
    beam.truncate(beam_width);

    // expand remaining depths using queue pieces
    for depth_idx in 0..max_depth.saturating_sub(1) {
        let queue_piece = match queue.get(depth_idx).copied() {
            Some(p) => p,
            None => break,
        };

        let child_depth = depth_idx + 2;
        let remaining_depth = max_depth.saturating_sub(child_depth);

        let mut next_beam: Vec<SearchNode> = Vec::with_capacity(beam_width.saturating_mul(2));

        for node in &beam {
            // figure out what piece this node plays next
            let current_piece = queue_piece;

            // generate moves for current piece (hold unchanged)
            expand_node(
                node,
                current_piece,
                node.hold,
                weights,
                &config.attack_config,
                &mut next_beam,
                remaining_depth,
                zobrist_keys,
                tt,
            );

            // also try hold swap if it gives a different piece
            if let Some(held) = node.hold {
                if held != current_piece {
                    // play held piece, queue piece goes into hold
                    expand_node(
                        node,
                        held,
                        Some(current_piece),
                        weights,
                        &config.attack_config,
                        &mut next_beam,
                        remaining_depth,
                        zobrist_keys,
                        tt,
                    );
                }
            }
        }

        if next_beam.is_empty() {
            break;
        }

        apply_futility_pruning(&mut next_beam, config.futility_delta);
        next_beam.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
        next_beam.truncate(beam_width);
        beam = next_beam;
    }

    // best node is first after final sort
    beam.first().map(|best| SearchResult {
        best_move: best.root_move,
        hold_used: best.root_hold_used,
        score: best.score,
        pv: best.path.clone(),
    })
}

fn apply_futility_pruning(nodes: &mut Vec<SearchNode>, futility_delta: f32) {
    if nodes.is_empty() {
        return;
    }

    let delta = futility_delta.max(0.0);
    let best_score = nodes
        .iter()
        .map(|node| node.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let cutoff = best_score - delta;

    nodes.retain(|node| node.score >= cutoff);
}

fn expand_root(
    state: &GameState,
    weights: &EvalWeights,
    _attack_config: &AttackConfig,
    max_depth: usize,
    zobrist_keys: &ZobristKeys,
    tt: &mut Option<TranspositionTable>,
) -> Vec<SearchNode> {
    let mut nodes = Vec::with_capacity(128);
    let remaining_depth = max_depth.saturating_sub(1);

    gen_and_eval_root(
        &state.board,
        state.current,
        state.hold,
        false,
        weights,
        &mut nodes,
        remaining_depth,
        zobrist_keys,
        tt,
    );

    match state.hold {
        Some(held) if held != state.current => {
            gen_and_eval_root(
                &state.board,
                held,
                Some(state.current),
                true,
                weights,
                &mut nodes,
                remaining_depth,
                zobrist_keys,
                tt,
            );
        }
        None if !state.queue.is_empty() => {
            let next = state.queue[0];
            if next != state.current {
                gen_and_eval_root(
                    &state.board,
                    next,
                    Some(state.current),
                    true,
                    weights,
                    &mut nodes,
                    remaining_depth,
                    zobrist_keys,
                    tt,
                );
            }
        }
        _ => {}
    }

    nodes
}

fn gen_and_eval_root(
    board: &Board,
    piece: Piece,
    new_hold: Option<Piece>,
    hold_used: bool,
    weights: &EvalWeights,
    nodes: &mut Vec<SearchNode>,
    remaining_depth: usize,
    zobrist_keys: &ZobristKeys,
    tt: &mut Option<TranspositionTable>,
) {
    let mut moves = MoveBuffer::new();
    generate(board, &mut moves, piece, false);

    for m in moves.as_slice() {
        let mut result_board = board.clone();
        result_board.do_move(m);

        let score = evaluate_with_tt(&result_board, weights, remaining_depth, zobrist_keys, tt);

        nodes.push(SearchNode {
            board: result_board,
            score,
            hold: new_hold,
            root_move: *m,
            root_hold_used: hold_used,
            path: vec![*m],
        });
    }
}

fn expand_node(
    parent: &SearchNode,
    piece: Piece,
    new_hold: Option<Piece>,
    weights: &EvalWeights,
    _attack_config: &AttackConfig,
    out: &mut Vec<SearchNode>,
    remaining_depth: usize,
    zobrist_keys: &ZobristKeys,
    tt: &mut Option<TranspositionTable>,
) {
    let mut moves = MoveBuffer::new();
    generate(&parent.board, &mut moves, piece, false);

    for m in moves.as_slice() {
        let mut result_board = parent.board.clone();
        result_board.do_move(m);

        let score = evaluate_with_tt(&result_board, weights, remaining_depth, zobrist_keys, tt);

        let mut path = parent.path.clone();
        path.push(*m);

        out.push(SearchNode {
            board: result_board,
            score,
            hold: new_hold,
            root_move: parent.root_move,
            root_hold_used: parent.root_hold_used,
            path,
        });
    }
}

fn evaluate_with_tt(
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
    use super::*;
    use crate::bag;
    use crate::board::{Board, FULL_ROW};

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
    fn test_hold_swap_considered() {
        // set up a state where holding might help
        // T piece current, I piece in hold — I piece tetris should be considered
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
        assert_eq!(
            extended.pv.len(),
            extended_config.depth.min(extended_queue.len() + 1),
            "extended depth should use 7-bag queue extension"
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
    fn test_no_moves_returns_none() {
        // fill the board nearly to the top — no valid placements
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
                score: 10.0,
                hold: None,
                root_move: Move::none(),
                root_hold_used: false,
                path: vec![Move::none()],
            },
            SearchNode {
                board: Board::new(),
                score: 8.5,
                hold: None,
                root_move: Move::none(),
                root_hold_used: false,
                path: vec![Move::none()],
            },
            SearchNode {
                board: Board::new(),
                score: 5.0,
                hold: None,
                root_move: Move::none(),
                root_hold_used: false,
                path: vec![Move::none()],
            },
        ];

        apply_futility_pruning(&mut nodes, 3.0);

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
}
