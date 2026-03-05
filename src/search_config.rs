use crate::attack::AttackConfig;
use crate::board::Board;
use crate::eval::EvalWeights;
use crate::header::{Move, Piece};
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
    pub attack_weight: f32,
    pub chain_weight: f32,
    pub context_weight: f32,
    pub board_weight: f32,
    pub max_depth_factor: f32,
    pub quiescence_max_extensions: usize,
    pub quiescence_beam_fraction: f32,
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
        }
    }
}

pub struct SearchResult {
    pub best_move: Move,
    pub hold_used: bool,
    pub score: f32,
    pub pv: Vec<Move>,
    pub coaching_state: CoachingState,
    pub pv_clear_events: Vec<ClearEvent>,
}

pub struct SearchResultFull {
    pub best: SearchResult,
    pub root_scores: Vec<(Move, f32)>,
    pub position_complexity: f32,
    pub board_score: f32,
    pub attack_score: f32,
    pub chain_score: f32,
    pub context_score: f32,
    pub path_attack: f32,
    pub path_chain: f32,
    pub path_context: f32,
}

pub(crate) struct SearchExpansionContext<'a> {
    pub config: &'a SearchConfig,
    pub weights: &'a EvalWeights,
    pub remaining_depth: usize,
    pub zobrist_keys: &'a ZobristKeys,
    pub tt: &'a mut Option<TranspositionTable>,
}

pub(crate) struct SearchIterationParams<'a> {
    pub state: &'a GameState,
    pub queue: &'a [Piece],
    pub config: &'a SearchConfig,
    pub weights: &'a EvalWeights,
    pub max_depth: usize,
    pub beam_width: usize,
    pub zobrist_keys: &'a ZobristKeys,
    pub tt: &'a mut Option<TranspositionTable>,
    pub forced_root_move: Option<Move>,
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
    pub path: SmallVec<[Move; 16]>,
    pub board_score: f32,
    pub attack_score: f32,
    pub chain_score: f32,
    pub context_score: f32,
    pub path_attack: f32,
    pub path_chain: f32,
    pub path_context: f32,
    pub path_clear_events: SmallVec<[ClearEvent; 4]>,
}

impl SearchNode {
    #[inline]
    pub fn is_loud(&self) -> bool {
        self.combo > 0 || self.b2b > 0 || !self.path_clear_events.is_empty()
    }
}
