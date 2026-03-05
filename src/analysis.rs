// analysis.rs -- move evaluation + eval meter for coaching

use crate::calibration::{
    default_eval_thresholds, BucketThresholds, CalibrationProfile, SkillBucket,
};
use crate::eval::{evaluate, EvalWeights};
use crate::header::Move;
use crate::search::{find_best_move_with_scores, SearchConfig};
use crate::state::{CoachingState, FatalityState, GameState, ObligationState, SurgeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    None,
    Inaccuracy,
    Mistake,
    Blunder,
}

pub fn classify_eval_loss(eval_loss: f32, thresholds: BucketThresholds) -> Severity {
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

pub fn classify_major_first(
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

pub fn normalize_meter(raw_eval: f32) -> f32 {
    let clamped = raw_eval.clamp(-15.0, 15.0);
    (clamped / 15.0) * 100.0
}

/// Player skill profile derived from TetraStats-style metrics.
/// Used to shift the sigmoid inflection point so that severity
/// classifications are relative to the player's skill tier.
#[derive(Debug, Clone, Copy)]
pub struct PlayerSkill {
    /// Pieces per second (mechanical speed)
    pub pps: f32,
    /// Attack per piece (offensive efficiency)
    pub app: f32,
    /// Downstack per piece (defensive recovery)
    pub dsp: f32,
}

impl Default for PlayerSkill {
    fn default() -> Self {
        // Roughly S-rank defaults (mid-skill)
        Self {
            pps: 1.57,
            app: 0.48,
            dsp: 0.20,
        }
    }
}

pub const SIGMOID_K: f32 = 0.10;

const SIGMOID_C_BASE: f32 = -13.5;

pub fn compute_sigmoid_c(skill: &PlayerSkill) -> f32 {
    const ALPHA: f32 = -3.5; // ln(pps) coefficient (attenuated to reduce X+ false positives)
    const BETA: f32 = -2.0; // app coefficient
    const GAMMA: f32 = -5.0; // dsp coefficient (reduced from -8.0 to prevent over-shifting)

    SIGMOID_C_BASE + ALPHA * skill.pps.max(0.1).ln() + BETA * skill.app + GAMMA * skill.dsp
}

pub fn win_prob(search_score: f32, k: f32, c: f32) -> f32 {
    1.0 / (1.0 + (-k * (search_score - c)).exp())
}

pub fn classify_win_prob_drop(best_score: f32, actual_score: f32, k: f32, c: f32) -> Severity {
    // Tail detection: both scores in sigmoid flat zone where WP drop
    // is uninformative (both far below or far above inflection point c).
    const TAIL_MARGIN: f32 = 20.0;
    let in_lower_tail = best_score < c - TAIL_MARGIN && actual_score < c - TAIL_MARGIN;
    let in_upper_tail = best_score > c + TAIL_MARGIN && actual_score > c + TAIL_MARGIN;

    if in_lower_tail || in_upper_tail {
        // Raw score delta — linear metric where sigmoid is flat.
        // Thresholds wider than WP-drop because raw scores have larger
        // variance. In tail regions, score gaps of 1-2 are placement-order
        // noise yielding the same practical outcome.
        let raw_delta = (best_score - actual_score).max(0.0);
        return classify_raw_delta(raw_delta);
    }

    // Standard WP-drop classification
    let best_wp = win_prob(best_score, k, c);
    let actual_wp = win_prob(actual_score, k, c);
    let drop = (best_wp - actual_wp).max(0.0);

    if drop >= 0.25 {
        Severity::Blunder
    } else if drop >= 0.12 {
        Severity::Mistake
    } else if drop >= 0.06 {
        Severity::Inaccuracy
    } else {
        Severity::None
    }
}

fn classify_raw_delta(delta: f32) -> Severity {
    if delta >= 8.0 {
        Severity::Blunder
    } else if delta >= 4.0 {
        Severity::Mistake
    } else if delta >= 2.0 {
        Severity::Inaccuracy
    } else {
        Severity::None
    }
}

pub fn coaching_dp_multiplier(coaching_after: &CoachingState) -> f32 {
    let fatality_mul: f32 = match coaching_after.fatality {
        FatalityState::Fatal => 1.5,
        FatalityState::Critical => 1.25,
        FatalityState::Safe => 1.0,
    };
    let surge_mul = match coaching_after.surge {
        SurgeState::Active => 1.4,
        SurgeState::Building => 1.2,
        SurgeState::Dormant => 1.0,
    };
    let obligation_mul = match coaching_after.obligation {
        ObligationState::MustCancel => 1.3,
        ObligationState::MustDownstack => 1.15,
        ObligationState::None => 1.0,
    };
    // Take the maximum multiplier across all coaching dimensions
    fatality_mul.max(surge_mul).max(obligation_mul)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InsightTag {
    AttackWindowMiss,
    ChainBreak,
    DownstackEfficiencyMiss,
}

impl InsightTag {
    pub fn to_str(self) -> &'static str {
        match self {
            InsightTag::AttackWindowMiss => "attack_window_miss",
            InsightTag::ChainBreak => "chain_break",
            InsightTag::DownstackEfficiencyMiss => "downstack_efficiency_miss",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InsightResult {
    pub tag: InsightTag,
    pub severity: f32,
    pub delta: f32,
}

pub const CHAIN_SHAPE_MAX: f32 = 1.0;

pub fn shape_chain_value(raw_chain: f32) -> f32 {
    if raw_chain <= 0.0 {
        return 0.0;
    }

    (1.0 - (-0.25 * raw_chain).exp()).clamp(0.0, CHAIN_SHAPE_MAX)
}

pub fn shape_context_modifier(raw_modifier: f32) -> f32 {
    raw_modifier.clamp(-1.0, 1.0)
}

pub fn assemble_composite(
    board: f32,
    attack: f32,
    chain: f32,
    context: f32,
    config: &SearchConfig,
) -> f32 {
    board * config.board_weight
        + attack * config.attack_weight
        + chain * config.chain_weight
        + context * config.context_weight
}

#[derive(Debug, Clone)]
pub struct InsightDetectorInput {
    pub best_attack_score: f32,
    pub best_chain_score: f32,
    pub best_board_score: f32,
    pub actual_score: Option<f32>,
    pub best_score: f32,
    pub actual_combo_after: u32,
    pub actual_lines_cleared: u8,
    pub actual_combo_before: u32,
    pub board_eval_delta: f32,
}

const ATTACK_WINDOW_THRESHOLD: f32 = 3.0;
const ATTACK_WINDOW_MIN_LOSS: f32 = 0.5;
const CHAIN_RELEVANCE_THRESHOLD: f32 = 0.3;
const DOWNSTACK_BOARD_GAP: f32 = 2.0;

pub fn detect_insights(input: &InsightDetectorInput) -> Vec<InsightResult> {
    let mut results = Vec::new();

    let eval_loss = input
        .actual_score
        .map(|actual| (input.best_score - actual).max(0.0))
        .unwrap_or(0.0);

    // best path had significant attack that the player missed
    if input.best_attack_score > ATTACK_WINDOW_THRESHOLD && eval_loss > ATTACK_WINDOW_MIN_LOSS {
        let delta = input.best_attack_score;
        let severity = (delta / 5.0).clamp(0.0, 1.0);
        results.push(InsightResult {
            tag: InsightTag::AttackWindowMiss,
            severity,
            delta,
        });
    }

    // player dropped an active combo the engine would've kept
    if input.best_chain_score > CHAIN_RELEVANCE_THRESHOLD
        && input.actual_combo_after == 0
        && input.actual_combo_before > 0
    {
        let delta = input.best_chain_score;
        let severity = (delta / CHAIN_SHAPE_MAX).clamp(0.0, 1.0);
        results.push(InsightResult {
            tag: InsightTag::ChainBreak,
            severity,
            delta,
        });
    }

    // engine found a cleaner downstack path — only fires when the board got worse while
    // best would have improved it.
    let board_gap = input.best_board_score - input.board_eval_delta;
    if board_gap > DOWNSTACK_BOARD_GAP && input.board_eval_delta < 0.0 {
        let severity = (board_gap / 10.0).clamp(0.0, 1.0);
        results.push(InsightResult {
            tag: InsightTag::DownstackEfficiencyMiss,
            severity,
            delta: board_gap,
        });
    }

    results
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

    let search_result = find_best_move_with_scores(state, config, weights);

    let (best_eval, best_move, best_hold_used, best_coaching_state, eval_loss, severity) =
        match search_result {
            Some(full) => {
                let best_search_score = full.best.score;

                // Look up player's actual move in root_scores (free quality scoring).
                // root_scores contains max leaf-node score per root move from beam search.
                let actual_search_score = full
                    .root_scores
                    .iter()
                    .find(|(m, _)| m.raw() == actual_move.raw())
                    .map(|(_, s)| *s);

                let k = SIGMOID_K;
                let c = compute_sigmoid_c(&PlayerSkill::default());

                let (loss, sev) = match actual_search_score {
                    Some(actual_s) => {
                        let loss = (best_search_score - actual_s).max(0.0);
                        // Task 6: Scale ΔP by coaching state multiplier BEFORE classification
                        let dp_mul = coaching_dp_multiplier(&coaching_after);
                        let amplified_actual = best_search_score - loss * dp_mul;
                        let sev = classify_win_prob_drop(best_search_score, amplified_actual, k, c);
                        let coaching_sev = classify_major_first(
                            loss,
                            thresholds,
                            coaching_before,
                            coaching_after,
                            full.best.coaching_state,
                        );
                        (loss, max_severity(sev, coaching_sev))
                    }
                    None => {
                        // Actual move not in root_scores — can't classify quality.
                        // This can happen when forced move is dropped from beam or
                        // frame context has a piece ordering mismatch.
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[analysis] actual move (piece={:?}, raw={}) not found in root_scores ({} entries)",
                            actual_move.piece(),
                            actual_move.raw(),
                            full.root_scores.len(),
                        );
                        (0.0, Severity::None)
                    }
                };

                (
                    best_search_score,
                    full.best.best_move,
                    full.best.hold_used,
                    full.best.coaching_state,
                    loss,
                    sev,
                )
            }
            None => (
                eval_after,
                *actual_move,
                false,
                coaching_after,
                0.0,
                Severity::None,
            ),
        };

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
    use crate::header::Piece;
    use crate::movegen::{generate, MoveBuffer};
    use crate::search::find_best_move;
    use crate::state::{PhaseState, SurgeState};

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

        let full = find_best_move_with_scores(&state, &config, &weights)
            .unwrap_or_else(|| panic!("no moves"));

        // Pick the worst move that IS in root_scores (not pruned from beam)
        if full.root_scores.len() >= 2 {
            let worst = full.root_scores.last().unwrap();
            let worst_move = worst.0;
            if worst_move.raw() != full.best.best_move.raw() {
                let mut b = state.board.clone();
                let lines_cleared = b.do_move(&worst_move) as u8;
                let analysis = evaluate_move(&state, &worst_move, lines_cleared, &weights, &config);
                assert!(
                    analysis.eval_loss > 0.0,
                    "worst move in root_scores should have positive eval loss, got {}",
                    analysis.eval_loss
                );
            }
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

        let full = find_best_move_with_scores(&state, &config, &weights)
            .unwrap_or_else(|| panic!("no moves on test board"));

        // Pick the worst move that IS in root_scores (not pruned from beam)
        if full.root_scores.len() >= 2 {
            let worst = full.root_scores.last().unwrap();
            let worst_move = worst.0;
            if worst_move.raw() != full.best.best_move.raw() {
                let mut b = state.board.clone();
                let lines_cleared = b.do_move(&worst_move) as u8;
                let analysis = evaluate_move(&state, &worst_move, lines_cleared, &weights, &config);
                assert!(
                    analysis.eval_loss > 0.0,
                    "worst move in root_scores should have positive eval loss, got {}",
                    analysis.eval_loss
                );
            }
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

    #[test]
    fn test_shape_chain_value_zero_input_returns_zero() {
        assert_eq!(shape_chain_value(0.0), 0.0);
        assert_eq!(shape_chain_value(-3.0), 0.0);
    }

    #[test]
    fn test_shape_chain_value_is_monotonic_increasing() {
        let low = shape_chain_value(1.0);
        let mid = shape_chain_value(4.0);
        let high = shape_chain_value(10.0);

        assert!(low < mid, "expected low < mid, got {} >= {}", low, mid);
        assert!(mid < high, "expected mid < high, got {} >= {}", mid, high);
    }

    #[test]
    fn test_shape_chain_value_is_bounded() {
        let shaped = shape_chain_value(1_000_000.0);
        assert!(
            (0.0..=CHAIN_SHAPE_MAX).contains(&shaped),
            "shape_chain_value should be bounded in [0, {}], got {}",
            CHAIN_SHAPE_MAX,
            shaped
        );
    }

    #[test]
    fn test_shape_context_modifier_zero_passthrough() {
        assert_eq!(shape_context_modifier(0.0), 0.0);
        assert_eq!(shape_context_modifier(0.75), 0.75);
    }

    #[test]
    fn test_shape_context_modifier_clamps_upper_bound() {
        assert_eq!(shape_context_modifier(10.0), 1.0);
        assert_eq!(shape_context_modifier(1.0), 1.0);
    }

    #[test]
    fn test_shape_context_modifier_clamps_lower_bound() {
        assert_eq!(shape_context_modifier(-10.0), -1.0);
        assert_eq!(shape_context_modifier(-1.0), -1.0);
    }

    #[test]
    fn test_assemble_composite_applies_coefficients() {
        let config = SearchConfig::default();
        let board = 2.0;
        let attack = 3.0;
        let chain = 4.0;
        let context = -1.0;
        let composite = assemble_composite(board, attack, chain, context, &config);
        let expected = board * config.board_weight
            + attack * config.attack_weight
            + chain * config.chain_weight
            + context * config.context_weight;
        assert_eq!(composite, expected);
    }

    #[test]
    fn test_dual_metric_lower_tail_blunder() {
        let c = -13.5;
        let best = c - 30.0; // -43.5, deep in lower tail
        let actual = best - 10.0; // delta=10 >= 8 → Blunder
        assert_eq!(
            classify_win_prob_drop(best, actual, SIGMOID_K, c),
            Severity::Blunder
        );
    }

    #[test]
    fn test_dual_metric_lower_tail_mistake() {
        let c = -13.5;
        let best = c - 25.0;
        let actual = best - 5.0; // delta=5 >= 4 → Mistake
        assert_eq!(
            classify_win_prob_drop(best, actual, SIGMOID_K, c),
            Severity::Mistake
        );
    }

    #[test]
    fn test_dual_metric_lower_tail_inaccuracy() {
        let c = -13.5;
        let best = c - 25.0;
        let actual = best - 3.0; // delta=3 >= 2 → Inaccuracy
        assert_eq!(
            classify_win_prob_drop(best, actual, SIGMOID_K, c),
            Severity::Inaccuracy
        );
    }

    #[test]
    fn test_dual_metric_lower_tail_none() {
        let c = -13.5;
        let best = c - 25.0;
        let actual = best - 1.0; // delta=1 < 2 → None
        assert_eq!(
            classify_win_prob_drop(best, actual, SIGMOID_K, c),
            Severity::None
        );
    }

    #[test]
    fn test_dual_metric_upper_tail_blunder() {
        let c = -13.5;
        let best = c + 30.0; // 16.5, deep in upper tail
        let actual = best - 9.0; // 7.5, still > c+20=6.5, both in upper tail. delta=9 >= 8 → Blunder
        assert_eq!(
            classify_win_prob_drop(best, actual, SIGMOID_K, c),
            Severity::Blunder
        );
    }

    #[test]
    fn test_dual_metric_not_triggered_near_inflection() {
        let c = -13.5;
        let best = c + 5.0; // -8.5, within TAIL_MARGIN of c
        let actual = best - 10.0; // -18.5, also within margin
        let sev = classify_win_prob_drop(best, actual, SIGMOID_K, c);
        // Near inflection, WP-drop classification should fire (not raw delta)
        // A 10-point gap near inflection produces a large WP drop
        assert_ne!(sev, Severity::None);
    }

    #[test]
    fn test_dual_metric_mixed_regions_uses_wp() {
        let c = -13.5;
        let best = c + 5.0; // near inflection
        let actual = c - 25.0; // deep in tail
                               // One score near inflection, one in tail → NOT both in tail → uses WP-drop
        let sev = classify_win_prob_drop(best, actual, SIGMOID_K, c);
        assert_eq!(sev, Severity::Blunder); // massive WP drop crossing inflection
    }
}
