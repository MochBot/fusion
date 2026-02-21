use crate::attack::AttackConfig;
use crate::board::Board;
use crate::eval::EvalWeights;
use crate::header::{Move, Piece};
use crate::state::{CoachingState, GameState};
use crate::transposition::{TranspositionTable, ZobristKeys};

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
    pub coaching_state: CoachingState,
}

/// Shared context for node expansion functions (`gen_and_eval_root`, `expand_node`).
/// Groups evaluation weights, attack config, depth tracking, and transposition table refs.
pub(crate) struct SearchExpansionContext<'a> {
    pub weights: &'a EvalWeights,
    pub remaining_depth: usize,
    pub zobrist_keys: &'a ZobristKeys,
    pub tt: &'a mut Option<TranspositionTable>,
}

/// Parameters for a single beam search iteration.
/// Groups game state, queue, configuration, and search infrastructure.
pub(crate) struct SearchIterationParams<'a> {
    pub state: &'a GameState,
    pub queue: &'a [Piece],
    pub config: &'a SearchConfig,
    pub weights: &'a EvalWeights,
    pub max_depth: usize,
    pub beam_width: usize,
    pub zobrist_keys: &'a ZobristKeys,
    pub tt: &'a mut Option<TranspositionTable>,
}

#[derive(Clone)]
pub struct SearchNode {
    pub board: Board,
    pub score: f32,
    pub hold: Option<Piece>,
    pub b2b: u8,
    pub combo: u32,
    pub pending_garbage: u8,
    pub coaching: CoachingState,
    pub root_move: Move,
    pub root_hold_used: bool,
    pub path: Vec<Move>,
}
