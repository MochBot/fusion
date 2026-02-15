// analysis.rs -- misdrop detection + eval meter for coaching

use crate::eval::{evaluate, evaluate_move, EvalWeights};
use crate::header::Move;
use crate::search::{find_best_move, SearchConfig};
use crate::state::GameState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MisdropSeverity {
    None,
    Inaccuracy,
    Mistake,
    Blunder,
}

fn classify(eval_loss: i32) -> MisdropSeverity {
    if eval_loss < 30 {
        MisdropSeverity::None
    } else if eval_loss < 80 {
        MisdropSeverity::Inaccuracy
    } else if eval_loss < 200 {
        MisdropSeverity::Mistake
    } else {
        MisdropSeverity::Blunder
    }
}

#[derive(Debug, Clone)]
pub struct MoveAnalysis {
    pub eval_before: i32,
    pub eval_after: i32,
    pub best_eval: i32,
    pub best_move: Move,
    pub best_hold_used: bool,
    pub eval_loss: i32,
    pub severity: MisdropSeverity,
    pub meter_value: i32,
}

fn normalize_meter(raw_eval: i32) -> i32 {
    // clamp raw to [-500, 500], scale to [-1000, 1000]
    let clamped = raw_eval.clamp(-500, 500);
    clamped * 2
}

pub struct EvalMeter {
    weights: EvalWeights,
    search_config: SearchConfig,
    history: Vec<i32>,
    baseline: i32,
}

impl EvalMeter {
    pub fn new() -> Self {
        let weights = EvalWeights::default();
        let baseline = evaluate(&crate::board::Board::new(), &weights);
        Self {
            weights,
            search_config: SearchConfig::default(),
            history: Vec::new(),
            baseline,
        }
    }

    pub fn with_config(weights: EvalWeights, config: SearchConfig) -> Self {
        let baseline = evaluate(&crate::board::Board::new(), &weights);
        Self {
            weights,
            search_config: config,
            history: Vec::new(),
            baseline,
        }
    }

    pub fn analyze_move(
        &mut self,
        state: &GameState,
        actual_move: &Move,
        lines_cleared: u8,
    ) -> MoveAnalysis {
        let result = analyze_move_inner(
            state,
            actual_move,
            lines_cleared,
            &self.weights,
            &self.search_config,
        );
        self.history.push(result.meter_value);
        result
    }

    pub fn current_value(&self) -> i32 {
        self.history
            .last()
            .copied()
            .unwrap_or(normalize_meter(self.baseline))
    }

    pub fn history(&self) -> &[i32] {
        &self.history
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }
}

impl Default for EvalMeter {
    fn default() -> Self {
        Self::new()
    }
}

fn analyze_move_inner(
    state: &GameState,
    actual_move: &Move,
    lines_cleared: u8,
    weights: &EvalWeights,
    config: &SearchConfig,
) -> MoveAnalysis {
    let eval_before = evaluate(&state.board, weights);

    let mut result_board = state.board.clone();
    result_board.do_move(actual_move);
    let spin = actual_move.spin();
    let is_pc = lines_cleared > 0 && result_board.empty();

    let eval_after = evaluate_move(
        &result_board,
        actual_move,
        lines_cleared,
        spin,
        state.b2b,
        state.combo,
        is_pc,
        &config.attack_config,
        weights,
    );

    let search_result = find_best_move(state, config, weights);

    let (best_eval, best_move, best_hold_used) = match search_result {
        Some(sr) => (sr.score, sr.best_move, sr.hold_used),
        None => (eval_after, *actual_move, false),
    };

    let eval_loss = (best_eval - eval_after).max(0);
    let severity = classify(eval_loss);
    let meter_value = normalize_meter(eval_after);

    MoveAnalysis {
        eval_before,
        eval_after,
        best_eval,
        best_move,
        best_hold_used,
        eval_loss,
        severity,
        meter_value,
    }
}

pub fn detect_misdrop(
    state: &GameState,
    actual_move: &Move,
    lines_cleared: u8,
    weights: &EvalWeights,
    config: &SearchConfig,
) -> MoveAnalysis {
    analyze_move_inner(state, actual_move, lines_cleared, weights, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::header::Piece;
    use crate::movegen::{generate, MoveBuffer};

    fn find_engine_best(state: &GameState) -> (Move, bool) {
        let config = SearchConfig {
            beam_width: 200,
            depth: 1,
            ..SearchConfig::default()
        };
        let weights = EvalWeights::default();
        let sr = find_best_move(state, &config, &weights)
            .unwrap_or_else(|| panic!("no moves on test board"));
        (sr.best_move, sr.hold_used)
    }

    #[test]
    fn test_perfect_play_no_misdrop() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I]);
        let (best, _) = find_engine_best(&state);

        let mut board = state.board.clone();
        let lines = board.do_move(&best) as u8;

        let weights = EvalWeights::default();
        let config = SearchConfig {
            beam_width: 200,
            depth: 1,
            ..SearchConfig::default()
        };
        let analysis = detect_misdrop(&state, &best, lines, &weights, &config);

        assert_eq!(
            analysis.severity,
            MisdropSeverity::None,
            "playing the engine's best move should be None, loss={}",
            analysis.eval_loss
        );
    }

    #[test]
    fn test_inaccuracy_detection() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let weights = EvalWeights::default();
        let config = SearchConfig {
            beam_width: 200,
            depth: 2,
            ..SearchConfig::default()
        };

        let sr = find_best_move(&state, &config, &weights).unwrap_or_else(|| panic!("no moves"));

        let mut moves = MoveBuffer::new();
        generate(&state.board, &mut moves, state.current, false);

        let mut worst_move = sr.best_move;
        let mut worst_score = i32::MAX;
        for m in moves.as_slice() {
            let mut b = state.board.clone();
            let lines = b.do_move(m) as u8;
            let spin = m.spin();
            let is_pc = lines > 0 && b.empty();
            let score = evaluate_move(
                &b,
                m,
                lines,
                spin,
                state.b2b,
                state.combo,
                is_pc,
                &config.attack_config,
                &weights,
            );
            if score < worst_score {
                worst_score = score;
                worst_move = *m;
            }
        }

        if worst_move.raw() != sr.best_move.raw() {
            let mut b = state.board.clone();
            let lines = b.do_move(&worst_move) as u8;
            let analysis = detect_misdrop(&state, &worst_move, lines, &weights, &config);
            assert!(
                analysis.eval_loss > 0,
                "worst move should have positive eval loss"
            );
        }
    }

    #[test]
    fn test_blunder_detection() {
        let mut board = Board::new();
        for y in 0..6 {
            let row = 0x1FF;
            board.rows[y] = row;
            for x in 0..9 {
                board.cols[x] |= 1u64 << y;
            }
        }

        let state = GameState::new(board, Piece::T, vec![Piece::I, Piece::O]);
        let weights = EvalWeights::default();
        let config = SearchConfig {
            beam_width: 200,
            depth: 2,
            ..SearchConfig::default()
        };

        let sr = find_best_move(&state, &config, &weights)
            .unwrap_or_else(|| panic!("no moves on test board"));

        let mut moves = MoveBuffer::new();
        generate(&state.board, &mut moves, state.current, false);

        let mut worst_move = sr.best_move;
        let mut worst_score = i32::MAX;
        for m in moves.as_slice() {
            let mut b = state.board.clone();
            let lines = b.do_move(m) as u8;
            let spin = m.spin();
            let is_pc = lines > 0 && b.empty();
            let score = evaluate_move(
                &b,
                m,
                lines,
                spin,
                state.b2b,
                state.combo,
                is_pc,
                &config.attack_config,
                &weights,
            );
            if score < worst_score {
                worst_score = score;
                worst_move = *m;
            }
        }

        if worst_move.raw() != sr.best_move.raw() {
            let mut b = state.board.clone();
            let lines = b.do_move(&worst_move) as u8;
            let analysis = detect_misdrop(&state, &worst_move, lines, &weights, &config);
            assert!(analysis.eval_loss > 0);
        }
    }

    #[test]
    fn test_meter_value_clamped() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I]);
        let weights = EvalWeights::default();
        let config = SearchConfig {
            beam_width: 200,
            depth: 1,
            ..SearchConfig::default()
        };

        let mut moves = MoveBuffer::new();
        generate(&state.board, &mut moves, state.current, false);
        let m = &moves.as_slice()[0];

        let mut b = state.board.clone();
        let lines = b.do_move(m) as u8;
        let analysis = detect_misdrop(&state, m, lines, &weights, &config);

        assert!(
            analysis.meter_value >= -1000 && analysis.meter_value <= 1000,
            "meter_value {} out of range",
            analysis.meter_value
        );
    }

    #[test]
    fn test_history_tracks() {
        let mut meter = EvalMeter::new();
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);

        let mut moves = MoveBuffer::new();
        generate(&state.board, &mut moves, state.current, false);

        let m1 = &moves.as_slice()[0];
        let mut b1 = state.board.clone();
        let lines1 = b1.do_move(m1) as u8;
        meter.analyze_move(&state, m1, lines1);
        assert_eq!(meter.history().len(), 1);

        let state2 = GameState::new(b1, Piece::I, vec![Piece::O]);
        let mut moves2 = MoveBuffer::new();
        generate(&state2.board, &mut moves2, state2.current, false);
        let m2 = &moves2.as_slice()[0];
        let mut b2 = state2.board.clone();
        let lines2 = b2.do_move(m2) as u8;
        meter.analyze_move(&state2, m2, lines2);
        assert_eq!(meter.history().len(), 2);
    }

    #[test]
    fn test_reset_clears_history() {
        let mut meter = EvalMeter::new();
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I]);

        let mut moves = MoveBuffer::new();
        generate(&state.board, &mut moves, state.current, false);
        let m = &moves.as_slice()[0];
        let mut b = state.board.clone();
        let lines = b.do_move(m) as u8;

        meter.analyze_move(&state, m, lines);
        assert!(!meter.history().is_empty());

        meter.reset();
        assert!(meter.history().is_empty());
    }

    #[test]
    fn test_detect_misdrop_matches_meter() {
        let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
        let weights = EvalWeights::default();
        let config = SearchConfig {
            beam_width: 200,
            depth: 1,
            ..SearchConfig::default()
        };

        let mut moves = MoveBuffer::new();
        generate(&state.board, &mut moves, state.current, false);
        let m = &moves.as_slice()[0];
        let mut b = state.board.clone();
        let lines = b.do_move(m) as u8;

        let standalone = detect_misdrop(&state, m, lines, &weights, &config);

        let mut meter = EvalMeter::with_config(weights.clone(), config);
        let metered = meter.analyze_move(&state, m, lines);

        assert_eq!(standalone.eval_before, metered.eval_before);
        assert_eq!(standalone.eval_after, metered.eval_after);
        assert_eq!(standalone.eval_loss, metered.eval_loss);
        assert_eq!(standalone.severity, metered.severity);
        assert_eq!(standalone.meter_value, metered.meter_value);
    }
}
