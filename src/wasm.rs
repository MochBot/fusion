// wasm.rs -- WASM bridge for Mosaic SvelteKit frontend
// Feature-gated behind `wasm` feature. Exposes Fusion v1-compatible API.

use wasm_bindgen::prelude::*;

use crate::analysis;
use crate::attack::{self, AttackConfig, ComboTable};
use crate::board::Board;
use crate::eval::{self, EvalWeights};
use crate::header::*;
use crate::movegen::{generate, MoveBuffer};
use crate::search::{self, SearchConfig};
use crate::state::GameState;
use crate::state::{
    CoachingState, FatalityState, ObligationState, PhaseState, PlonkState, SurgeState,
};

// ---------------------------------------------------------------------------
// Serialization helpers (serde_json + js_sys to avoid serde-wasm-bindgen 0.6 bug)
// ---------------------------------------------------------------------------

fn to_js<T: serde::Serialize>(val: &T) -> JsValue {
    serde_json::to_string(val)
        .ok()
        .and_then(|s| js_sys::JSON::parse(&s).ok())
        .unwrap_or(JsValue::NULL)
}

fn from_js<T: serde::de::DeserializeOwned>(js_val: JsValue) -> Option<T> {
    js_sys::JSON::stringify(&js_val)
        .ok()
        .and_then(|s| serde_json::from_str(&s.as_string().unwrap_or_default()).ok())
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
}

// ---------------------------------------------------------------------------
// Piece conversion helpers
// ---------------------------------------------------------------------------
// WASM API uses Fusion v1 ordering: I=0,O=1,T=2,S=3,Z=4,J=5,L=6
// Internal (Cobra) ordering:        I=0,O=1,T=2,L=3,J=4,S=5,Z=6

fn piece_from_external(v: u8) -> Option<Piece> {
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

fn piece_to_external(p: Piece) -> u8 {
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

fn queue_from_external(queue: Option<&[u8]>) -> Vec<Piece> {
    queue
        .map(|q| {
            q.iter()
                .filter_map(|&id| piece_from_external(id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn hold_from_external(hold: Option<u8>) -> Option<Piece> {
    hold.and_then(piece_from_external)
}

fn game_state_from_external_context(
    board: Board,
    current: Piece,
    queue: Option<&[u8]>,
    hold: Option<u8>,
) -> GameState {
    let mut state = GameState::new(board, current, queue_from_external(queue));
    state.hold = hold_from_external(hold);
    state
}

fn rotation_from_u8(v: u8) -> Option<Rotation> {
    match v {
        0 => Some(Rotation::North),
        1 => Some(Rotation::East),
        2 => Some(Rotation::South),
        3 => Some(Rotation::West),
        _ => None,
    }
}

fn spin_from_u8(v: u8) -> SpinType {
    match v {
        1 => SpinType::Mini,
        2 => SpinType::Full,
        _ => SpinType::NoSpin,
    }
}

fn fatality_to_contract(v: FatalityState) -> &'static str {
    match v {
        FatalityState::Safe => "safe",
        FatalityState::Critical => "critical",
        FatalityState::Fatal => "fatal",
    }
}

fn obligation_to_contract(v: ObligationState) -> &'static str {
    match v {
        ObligationState::None => "none",
        ObligationState::MustDownstack => "must_downstack",
        ObligationState::MustCancel => "must_cancel",
    }
}

fn surge_to_contract(v: SurgeState) -> &'static str {
    match v {
        SurgeState::Dormant => "dormant",
        SurgeState::Building => "building",
        SurgeState::Active => "active",
    }
}

fn phase_to_contract(v: PhaseState) -> &'static str {
    match v {
        PhaseState::Opener => "opener",
        PhaseState::Midgame => "midgame",
        PhaseState::Endgame => "endgame",
    }
}

fn plonk_to_contract(v: PlonkState) -> &'static str {
    match v {
        PlonkState::Stable => "stable",
        PlonkState::Drifting => "drifting",
        PlonkState::Spiral => "spiral",
    }
}

fn coaching_to_contract(v: CoachingState) -> MachineDiagnosticsJson {
    MachineDiagnosticsJson {
        fatality: fatality_to_contract(v.fatality).to_string(),
        obligation: obligation_to_contract(v.obligation).to_string(),
        surge: surge_to_contract(v.surge).to_string(),
        phase: phase_to_contract(v.phase).to_string(),
        plonk: plonk_to_contract(v.plonk).to_string(),
    }
}

// ---------------------------------------------------------------------------
// Board row ↔ column conversion
// Board.rows[y] (u16): bit x set if cell (x,y) is filled
// WASM rows[y] (u64): bit x set if cell (x,y) is filled (same semantics, wider type)
// ---------------------------------------------------------------------------

fn board_from_row_bitmasks(rows: &[u64]) -> Board {
    let mut board = Board::new();
    for (y, &row) in rows.iter().enumerate() {
        if y >= 40 {
            break;
        }
        board.rows[y] = (row & 0x3FF) as u16;
    }
    // Rebuild cols cache from rows
    board.cols = [0; COL_NB];
    for y in 0..40 {
        let row = board.rows[y];
        if row == 0 {
            continue;
        }
        let mut bits = row as u64;
        while bits != 0 {
            let x = bits.trailing_zeros() as usize;
            board.cols[x] |= 1u64 << y;
            bits &= bits - 1;
        }
    }
    board
}

fn board_to_row_bitmasks(board: &Board) -> Vec<u64> {
    let mut rows = Vec::with_capacity(40);
    for y in 0..40 {
        rows.push(board.rows[y] as u64);
    }
    rows
}

// ---------------------------------------------------------------------------
// JsBoard
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct JsBoard {
    inner: Board,
}

#[wasm_bindgen]
impl JsBoard {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Board::new(),
        }
    }

    #[wasm_bindgen(js_name = "from_rows")]
    pub fn from_rows(rows: &[u64]) -> Self {
        Self {
            inner: board_from_row_bitmasks(rows),
        }
    }

    pub fn get(&self, x: i8, y: i8) -> bool {
        if x < 0 || x >= 10 || y < 0 || y >= 40 {
            return false;
        }
        self.inner.occupied(x as i32, y as i32)
    }

    pub fn set(&mut self, x: i8, y: i8, val: bool) {
        if x < 0 || x >= 10 || y < 0 || y >= 40 {
            return;
        }
        let xu = x as usize;
        let yu = y as usize;
        if val {
            self.inner.rows[yu] |= 1u16 << x;
            self.inner.cols[xu] |= 1u64 << y;
        } else {
            self.inner.rows[yu] &= !(1u16 << x);
            self.inner.cols[xu] &= !(1u64 << y);
        }
    }

    #[wasm_bindgen(js_name = "clear_lines")]
    pub fn clear_lines(&mut self) -> u8 {
        let clears = self.inner.line_clears();
        if clears == 0 {
            return 0;
        }
        let count = clears.count_ones() as u8;
        self.inner.clear_lines(clears);
        count
    }

    #[wasm_bindgen(js_name = "to_rows")]
    pub fn to_rows(&self) -> Vec<u64> {
        board_to_row_bitmasks(&self.inner)
    }

    #[wasm_bindgen(js_name = "apply_move")]
    pub fn apply_move(&mut self, m: &JsMove) -> u8 {
        let internal_move = m.to_internal();
        let lines = self.inner.do_move(&internal_move);
        lines as u8
    }

    #[wasm_bindgen(js_name = "clone_board")]
    pub fn clone_board(&self) -> JsBoard {
        JsBoard {
            inner: self.inner.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// JsMove
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct JsMove {
    piece_val: u8,
    rotation_val: u8,
    x_val: i8,
    y_val: i8,
    hold_used_val: bool,
    spin_val: u8,
}

#[wasm_bindgen]
impl JsMove {
    #[wasm_bindgen(constructor)]
    pub fn new(piece: u8, rotation: u8, x: i8, y: i8) -> Self {
        Self {
            piece_val: piece,
            rotation_val: rotation,
            x_val: x,
            y_val: y,
            hold_used_val: false,
            spin_val: 0,
        }
    }

    pub fn piece(&self) -> u8 {
        self.piece_val
    }

    pub fn rotation(&self) -> u8 {
        self.rotation_val
    }

    pub fn x(&self) -> i8 {
        self.x_val
    }

    pub fn y(&self) -> i8 {
        self.y_val
    }

    #[wasm_bindgen(js_name = "hold_used")]
    pub fn hold_used(&self) -> bool {
        self.hold_used_val
    }

    #[wasm_bindgen(js_name = "set_hold_used")]
    pub fn set_hold_used(&mut self, val: bool) {
        self.hold_used_val = val;
    }

    pub fn spin(&self) -> u8 {
        self.spin_val
    }

    #[wasm_bindgen(js_name = "set_spin")]
    pub fn set_spin(&mut self, val: u8) {
        self.spin_val = val;
    }
}

impl JsMove {
    fn to_internal(&self) -> Move {
        let piece = piece_from_external(self.piece_val).unwrap_or(Piece::I);
        let rotation = rotation_from_u8(self.rotation_val).unwrap_or(Rotation::North);
        let fullspin = self.spin_val == 2;
        if piece == Piece::T && self.spin_val > 0 {
            Move::new_tspin(rotation, self.x_val as i32, self.y_val as i32, fullspin)
        } else {
            Move::new(
                piece,
                rotation,
                self.x_val as i32,
                self.y_val as i32,
                fullspin,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// JsAttackConfig
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct JsAttackConfig {
    inner: AttackConfig,
}

#[wasm_bindgen]
impl JsAttackConfig {
    #[wasm_bindgen(js_name = "tetraLeague")]
    pub fn tetra_league() -> Self {
        Self {
            inner: AttackConfig::tetra_league(),
        }
    }

    #[wasm_bindgen(js_name = "quickPlay")]
    pub fn quick_play() -> Self {
        Self {
            inner: AttackConfig::quick_play(),
        }
    }

    #[wasm_bindgen(constructor)]
    pub fn new(
        pc_garbage: u8,
        pc_b2b: u8,
        b2b_chaining: bool,
        b2b_charging_base: u8,
        combo_table: u8,
        garbage_multiplier: f32,
    ) -> Self {
        let _ = b2b_charging_base; // reserved for future use
        let ct = match combo_table {
            0 => ComboTable::Multiplier,
            1 => ComboTable::Classic,
            2 => ComboTable::Modern,
            _ => ComboTable::None,
        };
        Self {
            inner: AttackConfig {
                pc_garbage,
                pc_b2b,
                b2b_chaining,
                combo_table: ct,
                garbage_multiplier,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = "calculateAttack")]
pub fn calculate_attack_wasm(
    lines: u8,
    spin: u8,
    b2b: u8,
    combo: u8,
    config: &JsAttackConfig,
    is_pc: bool,
) -> f32 {
    let spin_type = spin_from_u8(spin);
    attack::calculate_attack(lines, spin_type, b2b, combo, &config.inner, is_pc)
}

#[wasm_bindgen(js_name = "find_best_move")]
pub fn find_best_move_wasm(board: &JsBoard, piece: u8) -> JsValue {
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return JsValue::NULL,
    };

    let state = game_state_from_external_context(board.inner.clone(), p, None, None);
    let config = SearchConfig {
        time_budget_ms: Some(50),
        ..SearchConfig::default()
    };
    let weights = EvalWeights::default();

    match search::find_best_move(&state, &config, &weights) {
        Some(result) => {
            let m = &result.best_move;
            to_js(&MoveResultJson {
                piece: piece_to_external(m.piece()),
                rotation: m.rotation() as u8,
                x: m.x() as i8,
                y: m.y() as i8,
                score: result.score,
                spin: m.spin() as u8,
                hold_used: result.hold_used,
            })
        }
        None => JsValue::NULL,
    }
}

#[wasm_bindgen(js_name = "get_all_moves")]
pub fn get_all_moves_wasm(board: &JsBoard, piece: u8) -> JsValue {
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return JsValue::NULL,
    };

    let mut moves = MoveBuffer::new();
    generate(&board.inner, &mut moves, p, true);

    let weights = EvalWeights::default();

    let results: Vec<MoveResultJson> = moves
        .as_slice()
        .iter()
        .map(|m| {
            let mut result_board = board.inner.clone();
            result_board.do_move(m);
            let score = eval::evaluate(&result_board, &weights);
            MoveResultJson {
                piece: piece_to_external(m.piece()),
                rotation: m.rotation() as u8,
                x: m.x() as i8,
                y: m.y() as i8,
                score,
                spin: m.spin() as u8,
                hold_used: false,
            }
        })
        .collect();

    to_js(&results)
}

#[wasm_bindgen(js_name = "evaluate_board")]
pub fn evaluate_board_wasm(board: &JsBoard) -> f32 {
    let weights = EvalWeights::default();
    eval::evaluate(&board.inner, &weights)
}

#[wasm_bindgen(js_name = "evaluate_with_weights")]
pub fn evaluate_with_weights_wasm(
    board: &JsBoard,
    height: f32,
    holes: f32,
    bumpiness: f32,
    wells: f32,
) -> f32 {
    let mut weights = EvalWeights::default();
    weights.height = height;
    weights.holes = holes;
    weights.bumpiness = bumpiness;
    weights.well_depth = wells;
    eval::evaluate(&board.inner, &weights)
}

#[wasm_bindgen(js_name = "evaluate_move")]
pub fn evaluate_move_wasm(
    board: &JsBoard,
    piece: u8,
    player_move: &JsMove,
    frame: JsValue,
) -> JsValue {
    let board_clone = board.inner.clone();
    let move_clone = player_move.to_internal();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let p = match piece_from_external(piece) {
            Some(p) => p,
            None => return None,
        };

        let frame_context = from_js::<ReplayFrameContextJson>(frame);
        let state = game_state_from_external_context(
            board_clone.clone(),
            p,
            frame_context.as_ref().and_then(|ctx| ctx.queue.as_deref()),
            frame_context.as_ref().and_then(|ctx| ctx.hold),
        );
        let weights = EvalWeights::default();
        let config = SearchConfig {
            time_budget_ms: Some(50),
            ..SearchConfig::default()
        };

        let mut result_board = board_clone;
        let lines = result_board.do_move(&move_clone);

        let result = analysis::evaluate_move(&state, &move_clone, lines as u8, &weights, &config);

        Some(MoveEvalResultJson {
            eval_before: result.eval_before,
            eval_after: result.eval_after,
            best_eval: result.best_eval,
            best_move: MoveResultJson {
                piece: piece_to_external(result.best_move.piece()),
                rotation: result.best_move.rotation() as u8,
                x: result.best_move.x() as i8,
                y: result.best_move.y() as i8,
                score: result.best_eval,
                spin: result.best_move.spin() as u8,
                hold_used: result.best_hold_used,
            },
            eval_loss: result.eval_loss,
            severity: match result.severity {
                analysis::Severity::None => "none",
                analysis::Severity::Inaccuracy => "inaccuracy",
                analysis::Severity::Mistake => "mistake",
                analysis::Severity::Blunder => "blunder",
            }
            .to_string(),
            meter_value: result.meter_value,
            coaching_before: coaching_to_contract(result.coaching_before),
            coaching_after: coaching_to_contract(result.coaching_after),
            best_coaching_state: coaching_to_contract(result.best_coaching_state),
        })
    }));

    match result {
        Ok(Some(json)) => to_js(&json),
        _ => JsValue::NULL,
    }
}

#[wasm_bindgen(js_name = "analyze_replay")]
pub fn analyze_replay_wasm(frames: JsValue) -> JsValue {
    let frames_vec = match from_js::<Vec<ReplayFrameJson>>(frames) {
        Some(f) => f,
        None => return JsValue::NULL,
    };

    let weights = EvalWeights::default();
    let config = SearchConfig {
        time_budget_ms: Some(50),
        ..SearchConfig::default()
    };

    let mut results: Vec<ReplayAnalysisJson> = Vec::with_capacity(frames_vec.len());

    for frame in &frames_vec {
        let board = board_from_row_bitmasks(&frame.board);
        let p = match piece_from_external(frame.piece) {
            Some(p) => p,
            None => continue,
        };

        let state =
            game_state_from_external_context(board.clone(), p, frame.queue.as_deref(), frame.hold);

        if let Some(ref mv) = frame.player_move {
            let board_for_analysis = board.clone();
            let state_for_analysis = state;
            let weights_ref = &weights;
            let config_ref = &config;

            let frame_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let piece = piece_from_external(mv.piece).unwrap_or(Piece::I);
                let rotation = rotation_from_u8(mv.rotation).unwrap_or(Rotation::North);
                let spin_type = spin_from_u8(mv.spin);
                let fullspin = spin_type == SpinType::Full;

                let internal_move = if piece == Piece::T && mv.spin > 0 {
                    Move::new_tspin(rotation, mv.x as i32, mv.y as i32, fullspin)
                } else {
                    Move::new(piece, rotation, mv.x as i32, mv.y as i32, fullspin)
                };

                let mut result_board = board_for_analysis;
                let lines = result_board.do_move(&internal_move);

                let analysis = analysis::evaluate_move(
                    &state_for_analysis,
                    &internal_move,
                    lines as u8,
                    weights_ref,
                    config_ref,
                );

                ReplayAnalysisJson {
                    eval_before: analysis.eval_before,
                    eval_after: analysis.eval_after,
                    best_eval: analysis.best_eval,
                    eval_loss: analysis.eval_loss,
                    severity: match analysis.severity {
                        analysis::Severity::None => "none",
                        analysis::Severity::Inaccuracy => "inaccuracy",
                        analysis::Severity::Mistake => "mistake",
                        analysis::Severity::Blunder => "blunder",
                    }
                    .to_string(),
                    meter_value: analysis.meter_value,
                    coaching_before: coaching_to_contract(analysis.coaching_before),
                    coaching_after: coaching_to_contract(analysis.coaching_after),
                    best_coaching_state: coaching_to_contract(analysis.best_coaching_state),
                }
            }));

            if let Ok(json) = frame_result {
                results.push(json);
            }
        }
    }

    to_js(&results)
}

// ---------------------------------------------------------------------------
// Serde JSON types for WASM serialization
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct MoveResultJson {
    piece: u8,
    rotation: u8,
    x: i8,
    y: i8,
    score: f32,
    spin: u8,
    hold_used: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MachineDiagnosticsJson {
    fatality: String,
    obligation: String,
    surge: String,
    phase: String,
    plonk: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MoveEvalResultJson {
    eval_before: f32,
    eval_after: f32,
    best_eval: f32,
    best_move: MoveResultJson,
    eval_loss: f32,
    severity: String,
    meter_value: f32,
    coaching_before: MachineDiagnosticsJson,
    coaching_after: MachineDiagnosticsJson,
    best_coaching_state: MachineDiagnosticsJson,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReplayFrameJson {
    board: Vec<u64>,
    piece: u8,
    queue: Option<Vec<u8>>,
    hold: Option<u8>,
    #[serde(rename = "move")]
    player_move: Option<ReplayMoveJson>,
}

#[derive(serde::Deserialize)]
struct ReplayFrameContextJson {
    queue: Option<Vec<u8>>,
    hold: Option<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReplayMoveJson {
    piece: u8,
    rotation: u8,
    x: i8,
    y: i8,
    spin: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReplayAnalysisJson {
    eval_before: f32,
    eval_after: f32,
    best_eval: f32,
    eval_loss: f32,
    severity: String,
    meter_value: f32,
    coaching_before: MachineDiagnosticsJson,
    coaching_after: MachineDiagnosticsJson,
    best_coaching_state: MachineDiagnosticsJson,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_eval_contract_includes_machine_diagnostics_fields() {
        let payload = MoveEvalResultJson {
            eval_before: 1.0,
            eval_after: 0.5,
            best_eval: 1.2,
            best_move: MoveResultJson {
                piece: 2,
                rotation: 0,
                x: 4,
                y: 20,
                score: 1.2,
                spin: 0,
                hold_used: false,
            },
            eval_loss: 0.7,
            severity: "mistake".to_string(),
            meter_value: 3.0,
            coaching_before: MachineDiagnosticsJson {
                fatality: "safe".to_string(),
                obligation: "none".to_string(),
                surge: "dormant".to_string(),
                phase: "opener".to_string(),
                plonk: "stable".to_string(),
            },
            coaching_after: MachineDiagnosticsJson {
                fatality: "critical".to_string(),
                obligation: "must_downstack".to_string(),
                surge: "building".to_string(),
                phase: "midgame".to_string(),
                plonk: "drifting".to_string(),
            },
            best_coaching_state: MachineDiagnosticsJson {
                fatality: "safe".to_string(),
                obligation: "none".to_string(),
                surge: "active".to_string(),
                phase: "midgame".to_string(),
                plonk: "stable".to_string(),
            },
        };

        let json = serde_json::to_value(&payload).expect("serialize move eval payload");
        let obj = json.as_object().expect("move eval payload object");

        assert_eq!(
            obj.get("severity").and_then(|v| v.as_str()),
            Some("mistake")
        );
        assert!(
            obj.get("coaching_before").is_some(),
            "missing coaching_before diagnostics field"
        );
        assert!(
            obj.get("coaching_after").is_some(),
            "missing coaching_after diagnostics field"
        );
        assert!(
            obj.get("best_coaching_state").is_some(),
            "missing best_coaching_state diagnostics field"
        );
        let coaching_before = obj
            .get("coaching_before")
            .and_then(|v| v.as_object())
            .expect("coaching_before object");
        assert_eq!(
            coaching_before.get("fatality").and_then(|v| v.as_str()),
            Some("safe")
        );
        assert_eq!(
            coaching_before.get("obligation").and_then(|v| v.as_str()),
            Some("none")
        );
        assert_eq!(
            coaching_before.get("surge").and_then(|v| v.as_str()),
            Some("dormant")
        );
        assert_eq!(
            coaching_before.get("phase").and_then(|v| v.as_str()),
            Some("opener")
        );
        assert_eq!(
            coaching_before.get("plonk").and_then(|v| v.as_str()),
            Some("stable")
        );
    }

    #[test]
    fn test_replay_analysis_contract_includes_machine_diagnostics_fields() {
        let payload = ReplayAnalysisJson {
            eval_before: 1.0,
            eval_after: 0.6,
            best_eval: 1.4,
            eval_loss: 0.8,
            severity: "blunder".to_string(),
            meter_value: 2.0,
            coaching_before: MachineDiagnosticsJson {
                fatality: "safe".to_string(),
                obligation: "none".to_string(),
                surge: "dormant".to_string(),
                phase: "opener".to_string(),
                plonk: "stable".to_string(),
            },
            coaching_after: MachineDiagnosticsJson {
                fatality: "fatal".to_string(),
                obligation: "must_cancel".to_string(),
                surge: "active".to_string(),
                phase: "endgame".to_string(),
                plonk: "spiral".to_string(),
            },
            best_coaching_state: MachineDiagnosticsJson {
                fatality: "safe".to_string(),
                obligation: "none".to_string(),
                surge: "building".to_string(),
                phase: "midgame".to_string(),
                plonk: "stable".to_string(),
            },
        };

        let json = serde_json::to_value(&payload).expect("serialize replay payload");
        let obj = json.as_object().expect("replay payload object");

        assert_eq!(
            obj.get("severity").and_then(|v| v.as_str()),
            Some("blunder")
        );
        assert!(
            obj.get("coaching_before").is_some(),
            "missing coaching_before diagnostics field"
        );
        assert!(
            obj.get("coaching_after").is_some(),
            "missing coaching_after diagnostics field"
        );
        assert!(
            obj.get("best_coaching_state").is_some(),
            "missing best_coaching_state diagnostics field"
        );
        let coaching_after = obj
            .get("coaching_after")
            .and_then(|v| v.as_object())
            .expect("coaching_after object");
        assert_eq!(
            coaching_after.get("fatality").and_then(|v| v.as_str()),
            Some("fatal")
        );
        assert_eq!(
            coaching_after.get("obligation").and_then(|v| v.as_str()),
            Some("must_cancel")
        );
        assert_eq!(
            coaching_after.get("surge").and_then(|v| v.as_str()),
            Some("active")
        );
        assert_eq!(
            coaching_after.get("phase").and_then(|v| v.as_str()),
            Some("endgame")
        );
        assert_eq!(
            coaching_after.get("plonk").and_then(|v| v.as_str()),
            Some("spiral")
        );
    }
}
