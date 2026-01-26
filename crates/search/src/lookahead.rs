use std::cmp::Ordering;

use fusion_core::{Board, GameState, Move, Piece};
use fusion_engine::{generate_moves, generate_moves_with_hold};
use fusion_eval::{evaluate, evaluate_with_clear, EvalWeights};

use crate::apply_move;

pub struct LookaheadSearch {
    pub depth: usize,
    pub beam_width: usize,
    pub weights: EvalWeights,
}

impl LookaheadSearch {
    pub fn new(depth: usize, beam_width: usize) -> Self {
        Self {
            depth: depth.clamp(1, 3),
            beam_width: beam_width.max(1),
            weights: EvalWeights::default(),
        }
    }

    pub fn search(&self, state: &GameState) -> Option<(Move, f32)> {
        let current = state.current_piece?;
        let can_hold = !state.hold_used_this_turn;

        let initial_moves = if can_hold {
            generate_moves_with_hold(&state.board, current, state.hold, &state.queue)
        } else {
            generate_moves(&state.board, current)
        };

        if initial_moves.is_empty() {
            return None;
        }

        let mut nodes: Vec<QueueNode> = initial_moves
            .into_iter()
            .map(|mv| {
                let (next_board, lines) = apply_move(&state.board, &mv);
                let score = evaluate_with_clear(&next_board, lines, &self.weights);
                let mut moves = Vec::with_capacity(1);
                moves.push(mv);
                let hold = if mv.hold_used {
                    Some(current)
                } else {
                    state.hold
                };
                let next_queue = if mv.hold_used && state.hold.is_some() {
                    state.queue.clone()
                } else {
                    state.queue.get(1..).unwrap_or(&[]).to_vec()
                };

                QueueNode {
                    node: SearchNode {
                        board: next_board,
                        score,
                        moves,
                    },
                    queue: next_queue,
                    hold,
                }
            })
            .collect();

        sort_queue_nodes(&mut nodes);
        nodes.truncate(self.beam_width);

        let mut remaining = self.depth.saturating_sub(1);
        while remaining > 0 {
            let mut expanded = false;
            let mut next_nodes = Vec::new();

            for node in nodes.into_iter() {
                if let Some(&next_piece) = node.queue.first() {
                    expanded = true;
                    for expanded_node in expand_nodes_with_hold(
                        vec![node],
                        next_piece,
                        self.beam_width,
                        &self.weights,
                    ) {
                        next_nodes.push(expanded_node);
                    }
                } else {
                    next_nodes.push(node);
                }
            }

            if !expanded {
                nodes = next_nodes;
                break;
            }

            sort_queue_nodes(&mut next_nodes);
            next_nodes.truncate(self.beam_width);
            nodes = next_nodes;
            remaining = remaining.saturating_sub(1);
        }

        let best = nodes.first()?;
        let first_move = *best.node.moves.first()?;
        Some((first_move, best.node.score))
    }

    pub fn search_partial(
        &self,
        board: &Board,
        piece: Piece,
        queue: &[Piece],
    ) -> Option<(Move, f32)> {
        let mut nodes = initial_nodes(board, piece, &self.weights);
        if nodes.is_empty() {
            return None;
        }

        nodes.truncate(self.beam_width);

        let mut remaining = self.depth.saturating_sub(1);
        let mut index = 0;
        while remaining > 0 && index < queue.len() {
            nodes = expand_nodes(nodes, queue[index], self.beam_width, &self.weights);
            if nodes.is_empty() {
                break;
            }
            remaining = remaining.saturating_sub(1);
            index += 1;
        }

        if remaining > 0 {
            for node in &mut nodes {
                node.score =
                    expected_score_unknown(&node.board, remaining, &self.weights, self.beam_width);
            }
            sort_nodes(&mut nodes);
            nodes.truncate(self.beam_width);
        }

        let best = nodes.first()?;
        let first_move = *best.moves.first()?;
        Some((first_move, best.score))
    }
}

struct QueueNode {
    node: SearchNode,
    queue: Vec<Piece>,
    hold: Option<Piece>,
}

#[derive(Clone)]
struct SearchNode {
    board: Board,
    score: f32,
    moves: Vec<Move>,
}

fn initial_nodes(board: &Board, piece: Piece, weights: &EvalWeights) -> Vec<SearchNode> {
    let mut nodes: Vec<SearchNode> = generate_moves(board, piece)
        .into_iter()
        .map(|mv| {
            let (next_board, lines) = apply_move(board, &mv);
            let score = evaluate_with_clear(&next_board, lines, weights);
            let mut moves = Vec::with_capacity(1);
            moves.push(mv);
            SearchNode {
                board: next_board,
                score,
                moves,
            }
        })
        .collect();

    sort_nodes(&mut nodes);
    nodes
}

fn expand_nodes(
    nodes: Vec<SearchNode>,
    piece: Piece,
    beam_width: usize,
    weights: &EvalWeights,
) -> Vec<SearchNode> {
    let mut next_nodes = Vec::new();

    for node in nodes {
        for mv in generate_moves(&node.board, piece) {
            let (next_board, lines) = apply_move(&node.board, &mv);
            let score = evaluate_with_clear(&next_board, lines, weights);
            let mut moves = node.moves.clone();
            moves.push(mv);
            next_nodes.push(SearchNode {
                board: next_board,
                score,
                moves,
            });
        }
    }

    sort_nodes(&mut next_nodes);
    next_nodes.truncate(beam_width);
    next_nodes
}

fn expand_nodes_with_hold(
    nodes: Vec<QueueNode>,
    piece: Piece,
    beam_width: usize,
    weights: &EvalWeights,
) -> Vec<QueueNode> {
    let mut next_nodes = Vec::new();

    for node in nodes {
        for mv in generate_moves_with_hold(&node.node.board, piece, node.hold, &node.queue) {
            let (next_board, lines) = apply_move(&node.node.board, &mv);
            let score = evaluate_with_clear(&next_board, lines, weights);
            let mut moves = node.node.moves.clone();
            moves.push(mv);
            let hold = if mv.hold_used { Some(piece) } else { node.hold };
            let next_queue = if mv.hold_used && node.hold.is_some() {
                node.queue.clone()
            } else {
                node.queue.get(1..).unwrap_or(&[]).to_vec()
            };
            next_nodes.push(QueueNode {
                node: SearchNode {
                    board: next_board,
                    score,
                    moves,
                },
                queue: next_queue,
                hold,
            });
        }
    }

    sort_queue_nodes(&mut next_nodes);
    next_nodes.truncate(beam_width);
    next_nodes
}

fn expected_score_unknown(
    board: &Board,
    depth: usize,
    weights: &EvalWeights,
    beam_width: usize,
) -> f32 {
    if depth == 0 {
        return evaluate(board, weights);
    }

    let mut total = 0.0;
    let mut count = 0usize;

    for piece in Piece::ALL {
        if let Some(score) = best_score_for_piece(board, piece, depth, weights, beam_width) {
            total += score;
            count += 1;
        }
    }

    if count == 0 {
        evaluate(board, weights)
    } else {
        total / count as f32
    }
}

fn best_score_for_piece(
    board: &Board,
    piece: Piece,
    depth: usize,
    weights: &EvalWeights,
    beam_width: usize,
) -> Option<f32> {
    let mut scored: Vec<(Board, f32)> = generate_moves(board, piece)
        .into_iter()
        .map(|mv| {
            let (next_board, lines) = apply_move(board, &mv);
            let score = evaluate_with_clear(&next_board, lines, weights);
            (next_board, score)
        })
        .collect();

    if scored.is_empty() {
        return None;
    }

    scored.sort_by(|a, b| score_cmp(a.1, b.1));
    scored.truncate(beam_width);

    if depth == 1 {
        return Some(scored[0].1);
    }

    let mut best: Option<f32> = None;
    for (next_board, _) in scored {
        let score =
            expected_score_unknown(&next_board, depth.saturating_sub(1), weights, beam_width);
        let next = match best {
            Some(current) => current.max(score),
            None => score,
        };
        best = Some(next);
    }

    best
}

fn sort_nodes(nodes: &mut Vec<SearchNode>) {
    nodes.sort_by(|a, b| score_cmp(a.score, b.score));
}

fn sort_queue_nodes(nodes: &mut Vec<QueueNode>) {
    nodes.sort_by(|a, b| score_cmp(a.node.score, b.node.score));
}

fn score_cmp(a: f32, b: f32) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beam::BeamSearch;

    fn board_with_gap() -> Board {
        let mut board = Board::new();
        for y in 0..3 {
            for x in 0..Board::WIDTH {
                if !(3..7).contains(&x) {
                    board.set(x, y, true);
                }
            }
        }
        board
    }

    fn gap_has_block(board: &Board) -> bool {
        (3..7).any(|x| board.get(x, 0))
    }

    fn tuned_weights() -> EvalWeights {
        let mut weights = EvalWeights::default();
        weights.height = -0.1;
        weights.holes = -1.0;
        weights.bumpiness = -0.1;
        weights.wells = -5.0;
        weights.i_dependency = -1.0;
        weights.lines_cleared = 5.0;
        weights
    }

    #[test]
    fn test_two_ply_preserves_i_setup() {
        let board = board_with_gap();
        let weights = tuned_weights();

        let beam = BeamSearch {
            beam_width: 200,
            weights: weights.clone(),
        };
        let lookahead = LookaheadSearch {
            depth: 2,
            beam_width: 200,
            weights,
        };

        let (beam_move, _) = beam
            .find_best_move(&board, Piece::T)
            .expect("expected a move");
        let (beam_board, _) = apply_move(&board, &beam_move);

        let (lookahead_move, _) = lookahead
            .search_partial(&board, Piece::T, &[Piece::I])
            .expect("expected a move");
        let (lookahead_board, _) = apply_move(&board, &lookahead_move);

        assert!(gap_has_block(&beam_board));
        assert!(!gap_has_block(&lookahead_board));
    }

    #[test]
    fn test_queue_affects_choice() {
        let board = board_with_gap();
        let weights = tuned_weights();

        let search = LookaheadSearch {
            depth: 2,
            beam_width: 200,
            weights,
        };

        let mut state_i = GameState::new();
        state_i.board = board.clone();
        state_i.current_piece = Some(Piece::T);
        state_i.queue = vec![Piece::I];

        let mut state_o = GameState::new();
        state_o.board = board.clone();
        state_o.current_piece = Some(Piece::T);
        state_o.queue = vec![Piece::O];

        let (move_i, _) = search.search(&state_i).expect("expected a move");
        let (move_o, _) = search.search(&state_o).expect("expected a move");

        let (board_i, _) = apply_move(&board, &move_i);
        let (board_o, _) = apply_move(&board, &move_o);

        assert!(!gap_has_block(&board_i));
        assert!(gap_has_block(&board_o));
    }

    #[test]
    fn test_search_partial_unknown_averages() {
        let search = LookaheadSearch::new(2, 80);
        let board = Board::new();

        let (mv, score) = search
            .search_partial(&board, Piece::T, &[])
            .expect("expected a move");

        let (next_board, _) = apply_move(&board, &mv);
        let expected = expected_score_unknown(&next_board, 1, &search.weights, search.beam_width);

        assert!((score - expected).abs() < 0.0001);
    }
}
