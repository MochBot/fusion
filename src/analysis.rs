use crate::search_config::{ATTACK_WEIGHT, BOARD_WEIGHT, CHAIN_WEIGHT, CONTEXT_WEIGHT};
use crate::state::{CoachingState, FatalityState, ObligationState, SurgeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    None,
    Inaccuracy,
    Mistake,
    Blunder,
}

pub fn normalize_meter(raw_eval: f32) -> f32 {
    let clamped = raw_eval.clamp(-15.0, 15.0);
    (clamped / 15.0) * 100.0
}

/// Player skill profile (TetraStats-style metrics).
/// Shifts the sigmoid inflection point per skill tier.
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

/// Sigmoid steepness. Higher k = sharper win-probability transitions.
pub const SIGMOID_K: f32 = 0.10;

/// Base inflection point (score where win_prob = 50%).
/// Shifted by player skill via `compute_sigmoid_c`.
const SIGMOID_C_BASE: f32 = -13.5;

/// Compute skill-adaptive sigmoid inflection point.
///
/// Higher-skilled players tolerate worse absolute positions before
/// "losing", so their inflection shifts deeper negative. The formula:
///
///   c = BASE + alpha * ln(pps) + beta * app + gamma * dsp
///
/// Calibrated against TetraStats rank data (D through X+):
///   D (pps=0.69): c ~= -13.3
///   S (pps=1.57): c ~= -17.0
///   X (pps=2.81): c ~= -20.0
///   X+(pps=3.27): c ~= -20.9
pub fn compute_sigmoid_c(skill: &PlayerSkill) -> f32 {
    const ALPHA: f32 = -3.5; // ln(pps) coefficient (attenuated for X+)
    const BETA: f32 = -2.0; // app coefficient
    const GAMMA: f32 = -5.0; // dsp coefficient (reduced from -8.0)

    SIGMOID_C_BASE + ALPHA * skill.pps.max(0.1).ln() + BETA * skill.app + GAMMA * skill.dsp
}

/// Convert search score to win/survival probability via sigmoid.
/// k = steepness, c = inflection point (50% probability).
pub fn win_prob(search_score: f32, k: f32, c: f32) -> f32 {
    1.0 / (1.0 + (-k * (search_score - c)).exp())
}

/// Classify severity by win-probability drop between best and actual move.
///
/// Thresholds calibrated for Tetris eval scale:
///   >=25% drop = Blunder
///   >=12% drop = Mistake
///   >= 6% drop = Inaccuracy
///
/// When both scores are deep in the sigmoid tail (both < c - TAIL_MARGIN
/// or both > c + TAIL_MARGIN), the sigmoid is flat and WP drop ~= 0.
/// Falls back to raw score delta classification in that region.
pub fn classify_win_prob_drop(best_score: f32, actual_score: f32, k: f32, c: f32) -> Severity {
    // Tail detection: both scores in sigmoid flat zone where WP drop
    // is uninformative (both far below or far above inflection point).
    const TAIL_MARGIN: f32 = 20.0;
    let in_lower_tail = best_score < c - TAIL_MARGIN && actual_score < c - TAIL_MARGIN;
    let in_upper_tail = best_score > c + TAIL_MARGIN && actual_score > c + TAIL_MARGIN;

    if in_lower_tail || in_upper_tail {
        // Raw score delta, linear metric where sigmoid is flat.
        // Thresholds wider than WP-drop: raw scores have larger variance
        // and small gaps in garbage states are placement-order noise.
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

/// Raw score delta classification for sigmoid tail regions.
/// Thresholds wider than WP-drop due to larger raw-score variance.
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

/// Coaching-state DP multiplier. Amplifies the WP drop based on
/// the coaching state after the player's move.
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

pub fn assemble_composite(board: f32, attack: f32, chain: f32, context: f32) -> f32 {
    board * BOARD_WEIGHT + attack * ATTACK_WEIGHT + chain * CHAIN_WEIGHT + context * CONTEXT_WEIGHT
}

/// Input for insight detection. Compares best node composite channels
/// against the player's actual move outcome.
#[derive(Debug, Clone)]
pub struct InsightDetectorInput {
    /// Best node's per-channel scores from SearchResultFull
    pub best_attack_score: f32,
    pub best_chain_score: f32,
    pub best_board_score: f32,
    /// Player's actual move aggregate score from root_scores lookup
    pub actual_score: Option<f32>,
    /// Best move's aggregate composite score
    pub best_score: f32,
    /// Combo count AFTER the player's actual move (from frame context)
    pub actual_combo_after: u32,
    /// Lines cleared by the player's actual move
    pub actual_lines_cleared: u8,
    /// Board eval delta: eval_after - eval_before (positive = board improved)
    /// Combo count BEFORE the player's actual move (0 = no active combo)
    pub actual_combo_before: u32,
    pub board_eval_delta: f32,
}

/// Minimum attack score on the best path to flag an attack window miss.
const ATTACK_WINDOW_THRESHOLD: f32 = 3.0;
/// Minimum eval loss to consider an attack window genuinely missed.
const ATTACK_WINDOW_MIN_LOSS: f32 = 0.5;
/// Minimum chain score on best path to consider combo continuation relevant.
const CHAIN_RELEVANCE_THRESHOLD: f32 = 0.3;
/// Minimum board score gap to flag a downstack efficiency miss.
const DOWNSTACK_BOARD_GAP: f32 = 2.0;

/// Run all MVP insight detectors and return any that fire.
pub fn detect_insights(input: &InsightDetectorInput) -> Vec<InsightResult> {
    let mut results = Vec::new();

    let eval_loss = input
        .actual_score
        .map(|actual| (input.best_score - actual).max(0.0))
        .unwrap_or(0.0);

    // --- AttackWindowMiss ---
    // Best path had a significant attack opportunity the player missed.
    if input.best_attack_score > ATTACK_WINDOW_THRESHOLD && eval_loss > ATTACK_WINDOW_MIN_LOSS {
        let delta = input.best_attack_score;
        let severity = (delta / 5.0).clamp(0.0, 1.0);
        results.push(InsightResult {
            tag: InsightTag::AttackWindowMiss,
            severity,
            delta,
        });
    }

    // --- ChainBreak ---
    // Best path maintained a combo (chain_score > threshold) but player
    // broke it (combo dropped to 0). Only fires with active pre-move combo.
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

    // --- DownstackEfficiencyMiss ---
    // Best path had a better board score; player's board got worse while
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let board = 2.0;
        let attack = 3.0;
        let chain = 4.0;
        let context = -1.0;
        let composite = assemble_composite(board, attack, chain, context);
        let expected = board * BOARD_WEIGHT
            + attack * ATTACK_WEIGHT
            + chain * CHAIN_WEIGHT
            + context * CONTEXT_WEIGHT;
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
