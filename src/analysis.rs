// analysis.rs -- move evaluation + eval meter for coaching

use crate::calibration::{
    default_eval_thresholds, BucketThresholds, CalibrationProfile, SkillBucket,
};
use crate::eval::{evaluate, EvalWeights};
use crate::header::Move;
use crate::search::{find_best_move, SearchConfig};
use crate::state::{CoachingState, FatalityState, GameState, ObligationState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    None,
    Inaccuracy,
    Mistake,
    Blunder,
}

fn classify_eval_loss(eval_loss: f32, thresholds: BucketThresholds) -> Severity {
    if eval_loss < thresholds.none_max {
        Severity::None
    } else if eval_loss < thresholds.inaccuracy_max {
        Severity::Inaccuracy
    } else if eval_loss < thresholds.mistake_max {
        Severity::Mistake
    } else {
        Severity::Blunder
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::None => 0,
        Severity::Inaccuracy => 1,
        Severity::Mistake => 2,
        Severity::Blunder => 3,
    }
}

fn max_severity(a: Severity, b: Severity) -> Severity {
    if severity_rank(a) >= severity_rank(b) {
        a
    } else {
        b
    }
}

fn classify_major_first(
    eval_loss: f32,
    thresholds: BucketThresholds,
    coaching_before: CoachingState,
    coaching_after: CoachingState,
    best_coaching_state: CoachingState,
) -> Severity {
    let eval_bucket = classify_eval_loss(eval_loss, thresholds);

    let lethal_negligence = (coaching_after.fatality == FatalityState::Fatal
        && (coaching_before.fatality != FatalityState::Fatal
            || best_coaching_state.fatality != FatalityState::Fatal))
        || (coaching_after.obligation == ObligationState::MustCancel
            && (coaching_before.obligation != ObligationState::MustCancel
                || best_coaching_state.obligation != ObligationState::MustCancel));

    if lethal_negligence {
        return Severity::Blunder;
    }

    let major_obligation_fail = (coaching_after.fatality == FatalityState::Critical
        && best_coaching_state.fatality == FatalityState::Safe)
        || (coaching_after.obligation == ObligationState::MustDownstack
            && best_coaching_state.obligation == ObligationState::None);

    if major_obligation_fail {
        return max_severity(eval_bucket, Severity::Mistake);
    }

    eval_bucket
}

#[derive(Debug, Clone)]
pub struct MoveAnalysis {
    pub eval_before: f32,
    pub eval_after: f32,
    pub best_eval: f32,
    pub best_move: Move,
    pub best_hold_used: bool,
    pub coaching_before: CoachingState,
    pub coaching_after: CoachingState,
    pub best_coaching_state: CoachingState,
    pub eval_loss: f32,
    pub severity: Severity,
    pub meter_value: f32,
}

fn normalize_meter(raw_eval: f32) -> f32 {
    let clamped = raw_eval.clamp(-15.0, 15.0);
    (clamped / 15.0) * 100.0
}

pub struct EvalMeter {
    weights: EvalWeights,
    search_config: SearchConfig,
    history: Vec<f32>,
    baseline: f32,
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
            default_eval_thresholds(),
        );
        self.history.push(result.meter_value);
        result
    }

    pub fn current_value(&self) -> f32 {
        self.history
            .last()
            .copied()
            .unwrap_or(normalize_meter(self.baseline))
    }

    pub fn history(&self) -> &[f32] {
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
    thresholds: BucketThresholds,
) -> MoveAnalysis {
    let eval_before = evaluate(&state.board, weights);

    let mut result_board = state.board.clone();
    result_board.do_move(actual_move);

    let eval_after = evaluate(&result_board, weights);
    let inferred_hold_used = state.infer_hold_used_for_piece(actual_move.piece());
    let spawn_envelope_blocked = GameState::spawn_envelope_blocked(&result_board);
    let coaching_before = state.coaching;
    let coaching_after = state.transition_for_move(
        actual_move,
        lines_cleared,
        inferred_hold_used,
        result_board.height(),
        spawn_envelope_blocked,
    );

    let search_result = find_best_move(state, config, weights);

    let (best_eval, best_move, best_hold_used, best_coaching_state) = match search_result {
        Some(sr) => {
            // compare immediate board quality (depth-1) not depth-N search score
            let mut best_board = state.board.clone();
            best_board.do_move(&sr.best_move);
            let best_immediate_eval = evaluate(&best_board, weights);
            (
                best_immediate_eval,
                sr.best_move,
                sr.hold_used,
                sr.coaching_state,
            )
        }
        None => (eval_after, *actual_move, false, coaching_after),
    };

    let eval_loss = (best_eval - eval_after).max(0.0);
    let severity = classify_major_first(
        eval_loss,
        thresholds,
        coaching_before,
        coaching_after,
        best_coaching_state,
    );
    let meter_value = normalize_meter(eval_after);

    MoveAnalysis {
        eval_before,
        eval_after,
        best_eval,
        best_move,
        best_hold_used,
        coaching_before,
        coaching_after,
        best_coaching_state,
        eval_loss,
        severity,
        meter_value,
    }
}

pub fn evaluate_move(
    state: &GameState,
    actual_move: &Move,
    lines_cleared: u8,
    weights: &EvalWeights,
    config: &SearchConfig,
) -> MoveAnalysis {
    analyze_move_inner(
        state,
        actual_move,
        lines_cleared,
        weights,
        config,
        default_eval_thresholds(),
    )
}

pub fn evaluate_move_for_bucket(
    state: &GameState,
    actual_move: &Move,
    lines_cleared: u8,
    weights: &EvalWeights,
    config: &SearchConfig,
    profile: &CalibrationProfile,
    bucket: SkillBucket,
) -> MoveAnalysis {
    let thresholds = profile
        .thresholds_for(bucket)
        .unwrap_or_else(default_eval_thresholds);
    analyze_move_inner(
        state,
        actual_move,
        lines_cleared,
        weights,
        config,
        thresholds,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::calibration::{
        generate_profile_from_players_manifest, CalibrationProfile, CALIBRATION_VERSION_V1,
    };
    use crate::eval::evaluate;
    use crate::header::Piece;
    use crate::movegen::{generate, MoveBuffer};
    use crate::state::{PhaseState, PlonkState, SurgeState};

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

    fn coaching_fixture(fatality: FatalityState, obligation: ObligationState) -> CoachingState {
        CoachingState {
            fatality,
            obligation,
            surge: SurgeState::Dormant,
            phase: PhaseState::Midgame,
            plonk: PlonkState::Stable,
            plonk_streak: 0,
            ply: 12,
        }
    }

    #[test]
    fn test_perfect_play_no_eval_loss() {
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
        let analysis = evaluate_move(&state, &best, lines, &weights, &config);

        assert_eq!(
            analysis.severity,
            Severity::None,
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
        let mut worst_score = f32::INFINITY;
        for m in moves.as_slice() {
            let mut b = state.board.clone();
            b.do_move(m);
            let score = evaluate(&b, &weights);
            if score < worst_score {
                worst_score = score;
                worst_move = *m;
            }
        }

        if worst_move.raw() != sr.best_move.raw() {
            let mut b = state.board.clone();
            let lines = b.do_move(&worst_move) as u8;
            let analysis = evaluate_move(&state, &worst_move, lines, &weights, &config);
            assert!(
                analysis.eval_loss > 0.0,
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
        let mut worst_score = f32::INFINITY;
        for m in moves.as_slice() {
            let mut b = state.board.clone();
            b.do_move(m);
            let score = evaluate(&b, &weights);
            if score < worst_score {
                worst_score = score;
                worst_move = *m;
            }
        }

        if worst_move.raw() != sr.best_move.raw() {
            let mut b = state.board.clone();
            let lines = b.do_move(&worst_move) as u8;
            let analysis = evaluate_move(&state, &worst_move, lines, &weights, &config);
            assert!(analysis.eval_loss > 0.0);
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
        let analysis = evaluate_move(&state, m, lines, &weights, &config);

        assert!(
            analysis.meter_value >= -100.0 && analysis.meter_value <= 100.0,
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
    fn test_evaluate_move_matches_meter() {
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

        let standalone = evaluate_move(&state, m, lines, &weights, &config);

        let mut meter = EvalMeter::with_config(weights.clone(), config);
        let metered = meter.analyze_move(&state, m, lines);

        assert_eq!(standalone.eval_before, metered.eval_before);
        assert_eq!(standalone.eval_after, metered.eval_after);
        assert_eq!(standalone.eval_loss, metered.eval_loss);
        assert_eq!(standalone.severity, metered.severity);
        assert_eq!(standalone.meter_value, metered.meter_value);
    }

    #[test]
    fn test_major_first_severe_trigger_on_fatality_fixture() {
        let before = coaching_fixture(FatalityState::Safe, ObligationState::None);
        let after = coaching_fixture(FatalityState::Fatal, ObligationState::MustCancel);
        let best = coaching_fixture(FatalityState::Safe, ObligationState::None);

        let severity = classify_major_first(0.1, default_eval_thresholds(), before, after, best);
        assert_eq!(severity, Severity::Blunder);
    }

    #[test]
    fn test_major_first_severe_trigger_on_obligation_fixture() {
        let before = coaching_fixture(FatalityState::Safe, ObligationState::None);
        let after = coaching_fixture(FatalityState::Safe, ObligationState::MustCancel);
        let best = coaching_fixture(FatalityState::Safe, ObligationState::None);

        let severity = classify_major_first(0.2, default_eval_thresholds(), before, after, best);
        assert_eq!(severity, Severity::Blunder);
    }

    #[test]
    fn test_minor_fixture_does_not_escalate_to_severe() {
        let before = coaching_fixture(FatalityState::Safe, ObligationState::None);
        let after = coaching_fixture(FatalityState::Safe, ObligationState::None);
        let best = coaching_fixture(FatalityState::Safe, ObligationState::None);

        let severity = classify_major_first(0.2, default_eval_thresholds(), before, after, best);
        assert_eq!(severity, Severity::None);
    }

    #[test]
    fn test_calibrated_threshold_loading_and_application_is_stable() {
        let manifest = r#"{
  "players": [
    {
      "rank": "b",
      "tr": 6900.0,
      "qualified": true
    },
    {
      "rank": "u",
      "tr": 22800.0,
      "qualified": true
    }
  ]
}"#;

        let profile = generate_profile_from_players_manifest(CALIBRATION_VERSION_V1, manifest)
            .unwrap_or_else(|e| panic!("profile generation failed: {e}"));
        let artifact = profile.to_artifact_string();
        let loaded = CalibrationProfile::from_artifact_str(&artifact)
            .unwrap_or_else(|e| panic!("profile load failed: {e}"));

        let before = coaching_fixture(FatalityState::Safe, ObligationState::None);
        let after = coaching_fixture(FatalityState::Safe, ObligationState::None);
        let best = coaching_fixture(FatalityState::Safe, ObligationState::None);

        let b_thresholds = loaded
            .thresholds_for(SkillBucket::B)
            .unwrap_or_else(default_eval_thresholds);
        let u_thresholds = loaded
            .thresholds_for(SkillBucket::U)
            .unwrap_or_else(default_eval_thresholds);

        let severity_b = classify_major_first(1.3, b_thresholds, before, after, best);
        let severity_u = classify_major_first(1.3, u_thresholds, before, after, best);

        assert_eq!(severity_b, Severity::Inaccuracy);
        assert_eq!(severity_u, Severity::Mistake);
    }
}
