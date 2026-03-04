use crate::attack::AttackConfig;
use crate::board::Board;
use crate::eval::EvalWeights;
use crate::header::{Move, Piece};
use crate::policy_value_runtime::{PolicyValueRuntime, PolicyValueRuntimeContext};
use crate::state::{ClearEvent, CoachingState, GameState};
use crate::transposition::{TranspositionTable, ZobristKeys};
use smallvec::SmallVec;

pub struct SearchConfig {
    pub beam_width: usize,
    pub depth: usize,
    pub futility_delta: f32,
    pub time_budget_ms: Option<u64>,
    pub use_tt: bool,
    pub extend_queue_7bag: bool,
    pub attack_config: AttackConfig,
    /// Multiplier for the offensive attack term. Determines how much weight is given
    /// to lines sent, B2B, and combo potential in the composite score.
    pub attack_weight: f32,
    /// Multiplier for the chain maintenance term. Weight given to sustaining current
    /// offensive momentum vs board cleanliness.
    pub chain_weight: f32,
    /// Multiplier for the context-sensitive term. Applies phase-specific or state-dependent
    /// score modifiers (e.g. Surge/Fatal multipliers).
    pub context_weight: f32,
    /// Multiplier for the core board evaluation term. The primary weight for structural
    /// cleanliness and height management.
    pub board_weight: f32,
    /// Cap for sqrt(depth) normalization of cumulative attack/chain terms.
    /// Prevents deep paths from over-discounting accumulated offensive value.
    /// sqrt(6) ≈ 2.45 means depths 1-6 get increasing normalization, depths 7+
    /// are treated as depth-6 for scoring purposes.
    pub max_depth_factor: f32,
    /// Maximum additional depths to extend "loud" nodes (mid-combo, mid-B2B,
    /// active setup) past the normal depth boundary. Prevents horizon effect
    /// where investment moves get evaluated before their payoff resolves.
    pub quiescence_max_extensions: usize,
    /// Fraction of beam_width allocated to quiescence extension beam.
    /// Keeps quiescence cost bounded: 0.15 = top 15% of loud nodes extended.
    pub quiescence_beam_fraction: f32,
    pub policy_bonus_weight: f32,
    pub heuristic_fallback_weight: f32,
    pub policy_guided_expansion_cap: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            beam_width: 800,
            depth: 14,
            futility_delta: 15.0,
            time_budget_ms: None,
            use_tt: false,
            extend_queue_7bag: true,
            attack_config: AttackConfig::tetra_league(),
            attack_weight: 0.50,
            chain_weight: 0.15,
            context_weight: 0.10,
            board_weight: 1.0,
            max_depth_factor: 2.45,
            quiescence_max_extensions: 3,
            quiescence_beam_fraction: 0.15,
            policy_bonus_weight: 0.10,
            heuristic_fallback_weight: 0.0,
            policy_guided_expansion_cap: 32,
        }
    }
}

pub struct SearchResult {
    pub best_move: Move,
    pub hold_used: bool,
    pub score: f32,
    pub pv: Vec<Move>,
    pub coaching_state: CoachingState,
    /// Per-move clear event history along the best PV path.
    pub pv_clear_events: Vec<ClearEvent>,
}

/// Extended search result that includes per-root-move scores from the final
/// beam iteration. Each entry maps a root move to the best leaf-node score
/// achieved by any beam path originating from that root placement.
/// This enables "free" quality scoring: the player's move score can be looked
/// up without running a second search.
pub struct SearchResultFull {
    pub best: SearchResult,
    /// (root_move, best_leaf_score) for every root move that survived to the
    /// final beam. Sorted descending by score. Typically ~34 entries (one per
    /// legal placement of the current piece).
    pub root_scores: Vec<(Move, f32)>,
    /// Position complexity: variance of top-10 root_scores.
    /// Low variance = flat position (dampen severity). High = sharp (amplify).
    pub position_complexity: f32,
    /// Static board evaluation score.
    pub board_score: f32,
    /// Strategic attack value.
    pub attack_score: f32,
    /// Chain maintenance bonus.
    pub chain_score: f32,
    /// Contextual multiplier/penalty.
    pub context_score: f32,
    /// Cumulative attack value along the best search path.
    /// Unlike attack_score (which is the leaf node's single-move attack),
    /// this accumulates all attack values from root to leaf.
    pub path_attack: f32,
    /// Cumulative chain value along the best search path.
    pub path_chain: f32,
    /// Cumulative context value along the best search path.
    pub path_context: f32,
    pub policy_score: f32,
    pub value_score: f32,
    pub fallback_used: bool,
}

/// Shared context for node expansion functions (`gen_and_eval_root`, `expand_node`).
/// Groups evaluation weights, attack config, depth tracking, and transposition table refs.
pub(crate) struct SearchExpansionContext<'a> {
    pub config: &'a SearchConfig,
    pub current_beam_width: usize,
    pub weights: &'a EvalWeights,
    pub remaining_depth: usize,
    pub zobrist_keys: &'a ZobristKeys,
    pub tt: &'a mut Option<TranspositionTable>,
    pub policy_value: Option<&'a PolicyValueRuntime>,
    pub runtime_context: Option<&'a PolicyValueRuntimeContext>,
}

/// Parameters for a single beam search iteration.
/// Groups game state, queue, configuration, and search infrastructure.
pub(crate) struct SearchIterationParams<'a> {
    pub state: &'a GameState,
    pub config: &'a SearchConfig,
    pub weights: &'a EvalWeights,
    pub max_depth: usize,
    pub beam_width: usize,
    pub zobrist_keys: &'a ZobristKeys,
    pub tt: &'a mut Option<TranspositionTable>,
    pub forced_root_move: Option<Move>,
    pub policy_value: Option<&'a PolicyValueRuntime>,
    pub runtime_context: Option<&'a PolicyValueRuntimeContext>,
}

#[derive(Clone)]
pub struct SearchNode {
    pub board: Board,
    pub current: Option<Piece>,
    pub queue: SmallVec<[Piece; 16]>,
    pub score: f32,
    pub hold: Option<Piece>,
    pub b2b: u8,
    pub combo: u32,
    pub pending_garbage: u8,
    pub lines_total: u32,
    pub bag_number: u32,
    pub pieces_into_bag: u8,
    pub coaching: CoachingState,
    pub root_move: Move,
    pub root_hold_used: bool,
    pub path: SmallVec<[Move; 16]>,
    /// Static board evaluation score (cached in TT).
    pub board_score: f32,
    /// Strategic attack value for this move/path.
    pub attack_score: f32,
    /// Chain/B2B maintenance bonus.
    pub chain_score: f32,
    /// Coaching-context dependent multiplier or penalty.
    pub context_score: f32,
    /// Cumulative attack value along the search path.
    pub path_attack: f32,
    /// Cumulative chain value along the search path.
    pub path_chain: f32,
    /// Cumulative context value along the search path.
    pub path_context: f32,
    pub policy_score: f32,
    pub value_score: f32,
    pub fallback_used: bool,
    /// Per-move clear event history along the search path (capacity >= typical clears per depth).
    pub path_clear_events: SmallVec<[ClearEvent; 4]>,
}

impl SearchNode {
    /// A node is "loud" if it has unresolved tactical activity that makes
    /// leaf evaluation unreliable — analogous to chess quiescence search
    /// refusing to evaluate mid-capture positions.
    #[inline]
    pub fn is_loud(&self) -> bool {
        self.combo > 0 || self.b2b > 0 || !self.path_clear_events.is_empty()
    }
}
