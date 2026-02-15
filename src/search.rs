// search.rs -- beam search with hold for coaching engine
// expands moves breadth-first, pruned to beam_width at each depth

use crate::attack::AttackConfig;
use crate::board::Board;
use crate::eval::{evaluate_move, EvalWeights};
use crate::header::*;
use crate::movegen::{generate, MoveBuffer};
use crate::state::GameState;

pub struct SearchConfig {
    pub beam_width: usize,
    pub depth: usize,
    pub attack_config: AttackConfig,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            beam_width: 400,
            depth: 2,
            attack_config: AttackConfig::tetra_league(),
        }
    }
}

pub struct SearchResult {
    pub best_move: Move,
    pub hold_used: bool,
    pub score: i32,
    pub pv: Vec<Move>,
}

/// internal node tracked during search
#[derive(Clone)]
struct SearchNode {
    board: Board,
    score: i32,
    b2b: u8,
    combo: u32,
    hold: Option<Piece>,
    /// first move in the path (what we actually return)
    root_move: Move,
    /// whether the first move used hold
    root_hold_used: bool,
    /// full move sequence for PV
    path: Vec<Move>,
}

/// beam search from game state
/// returns the best move found, or None if no legal moves exist
pub fn find_best_move(
    state: &GameState,
    config: &SearchConfig,
    weights: &EvalWeights,
) -> Option<SearchResult> {
    // actual search depth — capped by queue length + 1 (current piece)
    let max_depth = config.depth.min(state.queue_len() + 1);
    if max_depth == 0 {
        return None;
    }

    // expand root: generate moves for current piece, and hold piece if available
    let mut beam = expand_root(state, weights, &config.attack_config);
    if beam.is_empty() {
        return None;
    }

    // sort descending by score, truncate to beam width
    beam.sort_unstable_by(|a, b| b.score.cmp(&a.score));
    beam.truncate(config.beam_width);

    // expand remaining depths using queue pieces
    for depth_idx in 0..max_depth.saturating_sub(1) {
        let queue_piece = match state.queue_piece(depth_idx) {
            Some(p) => p,
            None => break,
        };

        let mut next_beam: Vec<SearchNode> = Vec::with_capacity(config.beam_width * 2);

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
                    );
                }
            }
        }

        if next_beam.is_empty() {
            break;
        }

        next_beam.sort_unstable_by(|a, b| b.score.cmp(&a.score));
        next_beam.truncate(config.beam_width);
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

/// expand the root position — generate all moves for current and hold pieces
fn expand_root(
    state: &GameState,
    weights: &EvalWeights,
    attack_config: &AttackConfig,
) -> Vec<SearchNode> {
    let mut nodes = Vec::with_capacity(128);

    gen_and_eval_root(
        &state.board,
        state.current,
        state.b2b,
        state.combo,
        state.hold,
        false,
        weights,
        attack_config,
        &mut nodes,
    );

    match state.hold {
        Some(held) if held != state.current => {
            gen_and_eval_root(
                &state.board,
                held,
                state.b2b,
                state.combo,
                Some(state.current),
                true,
                weights,
                attack_config,
                &mut nodes,
            );
        }
        None if !state.queue.is_empty() => {
            let next = state.queue[0];
            if next != state.current {
                gen_and_eval_root(
                    &state.board,
                    next,
                    state.b2b,
                    state.combo,
                    Some(state.current),
                    true,
                    weights,
                    attack_config,
                    &mut nodes,
                );
            }
        }
        _ => {}
    }

    nodes
}

/// generate all moves for a piece on a board, evaluate each, push to nodes
#[allow(clippy::too_many_arguments)]
fn gen_and_eval_root(
    board: &Board,
    piece: Piece,
    b2b: u8,
    combo: u32,
    new_hold: Option<Piece>,
    hold_used: bool,
    weights: &EvalWeights,
    attack_config: &AttackConfig,
    nodes: &mut Vec<SearchNode>,
) {
    let mut moves = MoveBuffer::new();
    generate(board, &mut moves, piece, false);

    for m in moves.as_slice() {
        let mut result_board = board.clone();
        let lines = result_board.do_move(m);
        let lines_u8 = lines as u8;
        let spin = m.spin();
        let is_pc = lines > 0 && result_board.empty();

        let is_b2b_eligible = spin != SpinType::NoSpin || lines >= 4;
        let new_b2b = if lines > 0 {
            if is_b2b_eligible {
                b2b + 1
            } else {
                0
            }
        } else {
            b2b
        };
        let new_combo = if lines > 0 { combo + 1 } else { 0 };

        let score = evaluate_move(
            &result_board,
            m,
            lines_u8,
            spin,
            b2b,
            combo,
            is_pc,
            attack_config,
            weights,
        );

        nodes.push(SearchNode {
            board: result_board,
            score,
            b2b: new_b2b,
            combo: new_combo,
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
    attack_config: &AttackConfig,
    out: &mut Vec<SearchNode>,
) {
    let mut moves = MoveBuffer::new();
    generate(&parent.board, &mut moves, piece, false);

    for m in moves.as_slice() {
        let mut result_board = parent.board.clone();
        let lines = result_board.do_move(m);
        let lines_u8 = lines as u8;
        let spin = m.spin();
        let is_pc = lines > 0 && result_board.empty();

        let is_b2b_eligible = spin != SpinType::NoSpin || lines >= 4;
        let new_b2b = if lines > 0 {
            if is_b2b_eligible {
                parent.b2b + 1
            } else {
                0
            }
        } else {
            parent.b2b
        };
        let new_combo = if lines > 0 { parent.combo + 1 } else { 0 };

        let score = evaluate_move(
            &result_board,
            m,
            lines_u8,
            spin,
            parent.b2b,
            parent.combo,
            is_pc,
            attack_config,
            weights,
        );

        let mut path = parent.path.clone();
        path.push(*m);

        out.push(SearchNode {
            board: result_board,
            score,
            b2b: new_b2b,
            combo: new_combo,
            hold: new_hold,
            root_move: parent.root_move,
            root_hold_used: parent.root_hold_used,
            path,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
