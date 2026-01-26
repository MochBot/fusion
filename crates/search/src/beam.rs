use std::cmp::Ordering;

use fusion_core::{Board, Move, Piece};
use fusion_engine::generate_moves;
use fusion_eval::{evaluate_with_clear, EvalWeights};

use crate::apply_move;

pub struct BeamSearch {
    pub beam_width: usize,
    pub weights: EvalWeights,
}

impl BeamSearch {
    pub fn new(beam_width: usize) -> Self {
        Self {
            beam_width: beam_width.max(1),
            weights: EvalWeights::default(),
        }
    }

    pub fn find_best_move(&self, board: &Board, piece: Piece) -> Option<(Move, f32)> {
        self.find_top_moves(board, piece, 1).into_iter().next()
    }

    pub fn find_top_moves(&self, board: &Board, piece: Piece, n: usize) -> Vec<(Move, f32)> {
        if n == 0 {
            return Vec::new();
        }

        let mut scored: Vec<(Move, f32)> = generate_moves(board, piece)
            .into_iter()
            .map(|mv| {
                let (next_board, lines) = apply_move(board, &mv);
                let score = evaluate_with_clear(&next_board, lines, &self.weights);
                (mv, score)
            })
            .collect();

        scored.sort_by(|a, b| score_cmp(a.1, b.1));

        let limit = self.beam_width.min(scored.len());
        scored.truncate(limit);

        if scored.len() > n {
            scored.truncate(n);
        }

        scored
    }
}

impl Default for BeamSearch {
    fn default() -> Self {
        Self {
            beam_width: 400,
            weights: EvalWeights::default(),
        }
    }
}

fn score_cmp(a: f32, b: f32) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_board_returns_move() {
        let search = BeamSearch::default();
        let board = Board::new();

        let result = search.find_best_move(&board, Piece::T);
        assert!(result.is_some());

        let (mv, _) = result.expect("expected a move");
        let all_moves = generate_moves(&board, Piece::T);
        assert!(all_moves.iter().any(|candidate| *candidate == mv));
    }

    #[test]
    fn test_best_move_matches_max_score() {
        let search = BeamSearch::default();
        let board = Board::new();

        let mut best_score = f32::NEG_INFINITY;
        for mv in generate_moves(&board, Piece::T) {
            let (next_board, lines) = apply_move(&board, &mv);
            let score = evaluate_with_clear(&next_board, lines, &search.weights);
            if score > best_score {
                best_score = score;
            }
        }

        let (_, score) = search
            .find_best_move(&board, Piece::T)
            .expect("expected a move");
        assert!((score - best_score).abs() < 0.0001);
    }

    #[test]
    fn test_beam_width_limits_candidates() {
        let search = BeamSearch {
            beam_width: 2,
            weights: EvalWeights::default(),
        };
        let board = Board::new();

        let moves = search.find_top_moves(&board, Piece::T, 10);
        assert!(moves.len() <= 2);
    }

    #[test]
    fn test_default_weights_clear_line() {
        let search = BeamSearch::default();
        let mut board = Board::new();

        for x in 0..Board::WIDTH {
            if !(3..7).contains(&x) {
                board.set(x, 0, true);
            }
        }

        let (mv, _) = search
            .find_best_move(&board, Piece::I)
            .expect("expected a move");
        let (_, lines) = apply_move(&board, &mv);
        assert!(lines >= 1);
    }
}
