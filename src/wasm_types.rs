// wasm_types.rs -- Shared types and conversion helpers for WASM bridge
// Extracted from wasm.rs to reduce godfile complexity.

use wasm_bindgen::prelude::*;

use crate::board::Board;
use crate::header::*;
use crate::state::{
    ClearEvent, CoachingState, FatalityState, GameState, ObligationState, PhaseState, SurgeState,
};

// serde_json + js_sys bridge (serde-wasm-bindgen 0.6 has a parsing bug)

pub(crate) fn to_js<T: serde::Serialize>(val: &T) -> JsValue {
    serde_json::to_string(val)
        .ok()
        .and_then(|s| js_sys::JSON::parse(&s).ok())
        .unwrap_or(JsValue::NULL)
}

pub(crate) fn from_js<T: serde::de::DeserializeOwned>(js_val: JsValue) -> Option<T> {
    js_sys::JSON::stringify(&js_val)
        .ok()
        .and_then(|s| serde_json::from_str(&s.as_string().unwrap_or_default()).ok())
}

// piece ordering: WASM uses Fusion v1 (I O T S Z J L), internal uses Cobra (I O T L J S Z)

pub(crate) fn piece_from_external(v: u8) -> Option<Piece> {
    match v {
        0 => Some(Piece::I),
        1 => Some(Piece::O),
        2 => Some(Piece::T),
        3 => Some(Piece::S),
        4 => Some(Piece::Z),
        5 => Some(Piece::J),
        6 => Some(Piece::L),
        _ => None,
    }
}

pub(crate) fn piece_to_external(p: Piece) -> u8 {
    match p {
        Piece::I => 0,
        Piece::O => 1,
        Piece::T => 2,
        Piece::S => 3,
        Piece::Z => 4,
        Piece::J => 5,
        Piece::L => 6,
    }
}

pub(crate) fn queue_from_external(queue: Option<&[u8]>) -> Vec<Piece> {
    queue
        .map(|q| {
            q.iter()
                .filter_map(|&id| piece_from_external(id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn hold_from_external(hold: Option<u8>) -> Option<Piece> {
    hold.and_then(piece_from_external)
}

pub(crate) fn game_state_from_external_context(
    board: Board,
    current: Piece,
    queue: Option<&[u8]>,
    hold: Option<u8>,
) -> GameState {
    let mut state = GameState::new(board, current, queue_from_external(queue));
    state.hold = hold_from_external(hold);
    state
}

pub(crate) fn spin_from_u8(v: u8) -> SpinType {
    match v {
        1 => SpinType::Mini,
        2 => SpinType::Full,
        _ => SpinType::NoSpin,
    }
}

// state -> contract string mappers

pub(crate) fn fatality_to_contract(v: FatalityState) -> &'static str {
    match v {
        FatalityState::Safe => "safe",
        FatalityState::Critical => "critical",
        FatalityState::Fatal => "fatal",
    }
}

pub(crate) fn obligation_to_contract(v: ObligationState) -> &'static str {
    match v {
        ObligationState::None => "none",
        ObligationState::MustDownstack => "must_downstack",
        ObligationState::MustCancel => "must_cancel",
    }
}

pub(crate) fn surge_to_contract(v: SurgeState) -> &'static str {
    match v {
        SurgeState::Dormant => "dormant",
        SurgeState::Building => "building",
        SurgeState::Active => "active",
    }
}

pub(crate) fn phase_to_contract(v: PhaseState) -> &'static str {
    match v {
        PhaseState::Opener => "opener",
        PhaseState::Midgame => "midgame",
        PhaseState::Endgame => "endgame",
    }
}

pub(crate) fn coaching_to_contract(v: CoachingState) -> MachineDiagnosticsJson {
    MachineDiagnosticsJson {
        fatality: fatality_to_contract(v.fatality).to_string(),
        obligation: obligation_to_contract(v.obligation).to_string(),
        surge: surge_to_contract(v.surge).to_string(),
        phase: phase_to_contract(v.phase).to_string(),
    }
}

// JSON-serializable types for the WASM boundary

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MoveResultJson {
    pub piece: u8,
    pub rotation: u8,
    pub x: i8,
    pub y: i8,
    pub score: f32,
    pub spin: u8,
    pub hold_used: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MachineDiagnosticsJson {
    pub fatality: String,
    pub obligation: String,
    pub surge: String,
    pub phase: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MoveEvalResultJson {
    pub eval_before: f32,
    pub eval_after: f32,
    pub best_eval: f32,
    pub best_move: MoveResultJson,
    pub eval_loss: f32,
    pub severity: String,
    pub meter_value: f32,
    pub coaching_before: MachineDiagnosticsJson,
    pub coaching_after: MachineDiagnosticsJson,
    pub best_coaching_state: MachineDiagnosticsJson,
    pub position_complexity: f32,
    pub board_score: f32,
    pub attack_score: f32,
    pub chain_score: f32,
    pub context_score: f32,
    pub path_attack: f32,
    pub path_chain: f32,
    pub path_context: f32,
    pub insight_tags: Vec<String>,
    pub recommended_path: Vec<MoveResultJson>,
    pub best_path_attack_summary: PathAttackSummaryJson,
}

#[derive(serde::Serialize)]
pub(crate) struct CoachingStepJson {
    pub piece: u8,
    pub rotation: u8,
    pub x: i8,
    pub y: i8,
    pub inputs: Vec<u8>,
    pub board_after: Vec<u16>,
    pub clearing_rows: Vec<u8>,
    pub clear_event: Option<ClearEventJson>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ReplayFrameContextJson {
    pub queue: Option<Vec<u8>>,
    pub hold: Option<u8>,
    pub player_pps: Option<f32>,
    pub player_app: Option<f32>,
    pub player_dsp: Option<f32>,
    // Coaching state fields — actual per-move values from replay engine
    pub lines_cleared: Option<u8>,
    pub b2b: Option<i32>,
    pub combo: Option<i32>,
    pub combo_before: Option<i32>,
    pub hold_used: Option<bool>,
    pub pending_garbage: Option<u32>,
    pub imminent_garbage: Option<u32>,
}


// attack tracking types

pub(crate) fn spin_type_to_str(s: SpinType) -> &'static str {
    match s {
        SpinType::NoSpin => "none",
        SpinType::Mini => "mini",
        SpinType::Full => "full",
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ClearEventJson {
    pub clear_type: String,
    pub spin_type: String,
    pub lines_cleared: u8,
    pub attack_sent: f32,
    pub b2b_before: u8,
    pub b2b_after: u8,
    pub combo_before: u32,
    pub combo_after: u32,
    pub is_surge_release: bool,
    pub is_garbage_clear: bool,
    pub is_perfect_clear: bool,
    pub piece: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct PathAttackSummaryJson {
    pub total_attack: f32,
    pub total_lines: u32,
    pub max_combo: u32,
    pub max_b2b: u8,
    pub surge_count: u32,
    pub garbage_clear_count: u32,
    pub spin_count: u32,
    pub clear_events: Vec<ClearEventJson>,
}

pub(crate) fn clear_event_to_json(event: &ClearEvent) -> ClearEventJson {
    ClearEventJson {
        clear_type: event.clear_type.to_str().to_string(),
        spin_type: spin_type_to_str(event.spin_type).to_string(),
        lines_cleared: event.lines_cleared,
        attack_sent: event.attack_sent,
        b2b_before: event.b2b_before,
        b2b_after: event.b2b_after,
        combo_before: event.combo_before,
        combo_after: event.combo_after,
        is_surge_release: event.is_surge_release,
        is_garbage_clear: event.is_garbage_clear,
        is_perfect_clear: event.is_perfect_clear,
        piece: piece_to_external(event.piece),
    }
}

pub(crate) fn build_path_attack_summary(events: &[ClearEvent]) -> PathAttackSummaryJson {
    let mut total_attack: f32 = 0.0;
    let mut total_lines: u32 = 0;
    let mut max_combo: u32 = 0;
    let mut max_b2b: u8 = 0;
    let mut surge_count: u32 = 0;
    let mut garbage_clear_count: u32 = 0;
    let mut spin_count: u32 = 0;

    for e in events {
        total_attack += e.attack_sent;
        total_lines += e.lines_cleared as u32;
        max_combo = max_combo.max(e.combo_after);
        max_b2b = max_b2b.max(e.b2b_after);
        if e.is_surge_release {
            surge_count += 1;
        }
        if e.is_garbage_clear {
            garbage_clear_count += 1;
        }
        if e.spin_type != SpinType::NoSpin {
            spin_count += 1;
        }
    }

    PathAttackSummaryJson {
        total_attack,
        total_lines,
        max_combo,
        max_b2b,
        surge_count,
        garbage_clear_count,
        spin_count,
        clear_events: events.iter().map(clear_event_to_json).collect(),
    }
}