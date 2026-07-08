use crate::attack::AttackConfig;
use crate::board::Board;
use crate::eval::EvalWeights;
use crate::header::{Move, Piece};
use crate::move_buffer::MoveBuffer;
use crate::policy_value_runtime::{PolicyValueRuntime, PolicyValueRuntimeContext};
use crate::state::{ClearEvent, CoachingState, GameState};
use crate::transposition::{TranspositionTable, ZobristKeys};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static SEARCH_MOVE_SCRATCH: RefCell<MoveBuffer> = RefCell::new(MoveBuffer::new());
}

pub const FUTILITY_DELTA: f32 = 15.0;
/// Multiplier for the offensive attack term (lines sent, B2B, combo).
pub const ATTACK_WEIGHT: f32 = 0.50;
/// Multiplier for the chain maintenance term (offensive momentum).
pub const CHAIN_WEIGHT: f32 = 0.15;
/// Multiplier for the context-sensitive term (phase/state modifiers).
pub const CONTEXT_WEIGHT: f32 = 0.10;
/// Multiplier for the core board evaluation term.
pub const BOARD_WEIGHT: f32 = 1.0;
/// Cap for sqrt(depth) normalization of cumulative attack/chain terms.
/// sqrt(6) ~= 2.45; depths 7+ treated as depth-6 for scoring.
pub const MAX_DEPTH_FACTOR: f32 = 2.45;
pub const POLICY_BONUS_WEIGHT: f32 = 0.10;

pub struct SearchConfig {
    pub beam_width: usize,
    pub depth: usize,
    pub time_budget_ms: Option<u64>,
    pub use_tt: bool,
    pub extend_queue_7bag: bool,
    pub attack_config: AttackConfig,
    /// Max additional depths to extend "loud" nodes (mid-combo, mid-B2B,
    /// active setup) past normal depth, preventing horizon effect.
    pub quiescence_max_extensions: usize,
    /// Fraction of beam_width for quiescence extension beam (0.15 = top 15%).
    pub quiescence_beam_fraction: f32,
    pub policy_guided_expansion_cap: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            beam_width: 800,
            depth: 14,
            time_budget_ms: None,
            // TT off: 23-28% hit rate, net-negative wall time (-5..-6%
            // at depths 5/7/14). Cache only pays above ~80% hit rate.
            // Results identical either way (pinned by
            // tt_does_not_change_search_result).
            use_tt: false,
            extend_queue_7bag: true,
            attack_config: AttackConfig::tetra_league(),
            quiescence_max_extensions: 3,
            quiescence_beam_fraction: 0.15,
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

/// Extended search result with per-root-move scores from the final beam
/// iteration. Enables quality scoring without a second search.
pub struct SearchResultFull {
    pub best: SearchResult,
    /// (root_move, best_leaf_score) for every root move surviving to the
    /// final beam, sorted descending. Typically ~34 entries.
    pub root_scores: Vec<(Move, f32)>,
    /// Position complexity: variance of top-10 root_scores.
    /// Low = flat position (dampen severity). High = sharp (amplify).
    pub position_complexity: f32,
    /// Static board evaluation score.
    pub board_score: f32,
    /// Strategic attack value.
    pub attack_score: f32,
    /// Chain maintenance bonus.
    pub chain_score: f32,
    /// Contextual multiplier/penalty.
    pub context_score: f32,
    /// Cumulative attack value along the best search path
    /// (leaf attack_score is single-move only).
    pub path_attack: f32,
    /// Cumulative chain value along the best search path.
    pub path_chain: f32,
    /// Cumulative context value along the best search path.
    pub path_context: f32,
    pub policy_score: f32,
    pub value_score: f32,
    pub fallback_used: bool,
}

/// Shared context for node expansion (weights, attack config, depth, TT).
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

impl SearchExpansionContext<'_> {
    #[inline]
    pub(crate) fn with_move_scratch<T>(&mut self, f: impl FnOnce(&mut MoveBuffer) -> T) -> T {
        SEARCH_MOVE_SCRATCH.with(|scratch| {
            let mut moves = scratch.borrow_mut();
            moves.clear();
            f(&mut moves)
        })
    }
}

/// Parameters for a single beam search iteration.
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
    pub path_clear_events: Arc<Vec<ClearEvent>>,
}

impl SearchNode {
    /// A node is "loud" if it has unresolved tactical activity making
    /// leaf evaluation unreliable (analogous to chess quiescence search).
    #[inline]
    pub fn is_loud(&self) -> bool {
        self.combo > 0 || self.b2b > 0 || !self.path_clear_events.is_empty()
    }
}
