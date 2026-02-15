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

    #[wasm_bindgen(js_name = "fromRows")]
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

    #[wasm_bindgen(js_name = "clearLines")]
    pub fn clear_lines(&mut self) -> u8 {
        let clears = self.inner.line_clears();
        if clears == 0 {
            return 0;
        }
        let count = clears.count_ones() as u8;
        self.inner.clear_lines(clears);
        count
    }

    #[wasm_bindgen(js_name = "toRows")]
    pub fn to_rows(&self) -> Vec<u64> {
        board_to_row_bitmasks(&self.inner)
    }

    #[wasm_bindgen(js_name = "applyMove")]
    pub fn apply_move(&mut self, m: &JsMove) -> u8 {
        let internal_move = m.to_internal();
        let lines = self.inner.do_move(&internal_move);
        lines as u8
    }

    #[wasm_bindgen(js_name = "cloneBoard")]
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

    #[wasm_bindgen(js_name = "holdUsed")]
    pub fn hold_used(&self) -> bool {
        self.hold_used_val
    }

    #[wasm_bindgen(js_name = "setHoldUsed")]
    pub fn set_hold_used(&mut self, val: bool) {
        self.hold_used_val = val;
    }

    pub fn spin(&self) -> u8 {
        self.spin_val
    }

    #[wasm_bindgen(js_name = "setSpin")]
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

#[wasm_bindgen(js_name = "findBestMove")]
pub fn find_best_move_wasm(board: &JsBoard, piece: u8) -> JsValue {
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return JsValue::NULL,
    };

    let state = GameState::new(board.inner.clone(), p, vec![]);
    let config = SearchConfig::default();
    let weights = EvalWeights::default();

    match search::find_best_move(&state, &config, &weights) {
        Some(result) => {
            let m = &result.best_move;
            let obj = serde_wasm_bindgen::to_value(&MoveResultJson {
                piece: piece_to_external(m.piece()),
                rotation: m.rotation() as u8,
                x: m.x() as i8,
                y: m.y() as i8,
                score: result.score,
                spin: m.spin() as u8,
                hold_used: result.hold_used,
            });
            obj.unwrap_or(JsValue::NULL)
        }
        None => JsValue::NULL,
    }
}

#[wasm_bindgen(js_name = "getAllMoves")]
pub fn get_all_moves_wasm(board: &JsBoard, piece: u8) -> JsValue {
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return JsValue::NULL,
    };

    let mut moves = MoveBuffer::new();
    generate(&board.inner, &mut moves, p, false);

    let weights = EvalWeights::default();
    let config = AttackConfig::tetra_league();

    let results: Vec<MoveResultJson> = moves
        .as_slice()
        .iter()
        .map(|m| {
            let mut result_board = board.inner.clone();
            let lines = result_board.do_move(m);
            let spin = m.spin();
            let is_pc = lines > 0 && result_board.empty();
            let score = eval::evaluate_move(
                &result_board,
                m,
                lines as u8,
                spin,
                0,
                0,
                is_pc,
                &config,
                &weights,
            );
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

    serde_wasm_bindgen::to_value(&results).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen(js_name = "evaluateBoard")]
pub fn evaluate_board_wasm(board: &JsBoard) -> f32 {
    let weights = EvalWeights::default();
    eval::evaluate(&board.inner, &weights) as f32
}

#[wasm_bindgen(js_name = "evaluateWithWeights")]
pub fn evaluate_with_weights_wasm(
    board: &JsBoard,
    height: f32,
    holes: f32,
    bumpiness: f32,
    wells: f32,
) -> f32 {
    let mut weights = EvalWeights::default();
    weights.height = height as i32;
    weights.hole_cells = holes as i32;
    weights.bumpiness = bumpiness as i32;
    weights.well_depth = wells as i32;
    eval::evaluate(&board.inner, &weights) as f32
}

#[wasm_bindgen(js_name = "detectMisdrop")]
pub fn detect_misdrop_wasm(
    board: &JsBoard,
    piece: u8,
    player_move: &JsMove,
    frame: u32,
) -> JsValue {
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return JsValue::NULL,
    };

    let _ = frame;

    let state = GameState::new(board.inner.clone(), p, vec![]);
    let internal_move = player_move.to_internal();
    let weights = EvalWeights::default();
    let config = SearchConfig::default();

    let mut result_board = board.inner.clone();
    let lines = result_board.do_move(&internal_move);

    let result = analysis::detect_misdrop(&state, &internal_move, lines as u8, &weights, &config);

    let json = MisdropResultJson {
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
            analysis::MisdropSeverity::None => "none",
            analysis::MisdropSeverity::Inaccuracy => "inaccuracy",
            analysis::MisdropSeverity::Mistake => "mistake",
            analysis::MisdropSeverity::Blunder => "blunder",
        }
        .to_string(),
        meter_value: result.meter_value,
    };

    serde_wasm_bindgen::to_value(&json).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen(js_name = "analyzeReplay")]
pub fn analyze_replay_wasm(frames: JsValue) -> JsValue {
    // Parse frames from JS — each frame: {board: u64[], piece: u8, move: {piece,rotation,x,y,spin}}
    let parsed: Result<Vec<ReplayFrameJson>, _> = serde_wasm_bindgen::from_value(frames);
    let frames_vec = match parsed {
        Ok(f) => f,
        Err(_) => return JsValue::NULL,
    };

    let weights = EvalWeights::default();
    let config = SearchConfig::default();

    let mut results: Vec<ReplayAnalysisJson> = Vec::with_capacity(frames_vec.len());

    for frame in &frames_vec {
        let board = board_from_row_bitmasks(&frame.board);
        let p = match piece_from_external(frame.piece) {
            Some(p) => p,
            None => continue,
        };

        let state = GameState::new(board.clone(), p, vec![]);

        if let Some(ref mv) = frame.player_move {
            let piece = piece_from_external(mv.piece).unwrap_or(Piece::I);
            let rotation = rotation_from_u8(mv.rotation).unwrap_or(Rotation::North);
            let spin_type = spin_from_u8(mv.spin);
            let fullspin = spin_type == SpinType::Full;

            let internal_move = if piece == Piece::T && mv.spin > 0 {
                Move::new_tspin(rotation, mv.x as i32, mv.y as i32, fullspin)
            } else {
                Move::new(piece, rotation, mv.x as i32, mv.y as i32, fullspin)
            };

            let mut result_board = board.clone();
            let lines = result_board.do_move(&internal_move);

            let analysis =
                analysis::detect_misdrop(&state, &internal_move, lines as u8, &weights, &config);

            results.push(ReplayAnalysisJson {
                eval_before: analysis.eval_before,
                eval_after: analysis.eval_after,
                best_eval: analysis.best_eval,
                eval_loss: analysis.eval_loss,
                severity: match analysis.severity {
                    analysis::MisdropSeverity::None => "none",
                    analysis::MisdropSeverity::Inaccuracy => "inaccuracy",
                    analysis::MisdropSeverity::Mistake => "mistake",
                    analysis::MisdropSeverity::Blunder => "blunder",
                }
                .to_string(),
                meter_value: analysis.meter_value,
            });
        }
    }

    serde_wasm_bindgen::to_value(&results).unwrap_or(JsValue::NULL)
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
    score: i32,
    spin: u8,
    hold_used: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MisdropResultJson {
    eval_before: i32,
    eval_after: i32,
    best_eval: i32,
    best_move: MoveResultJson,
    eval_loss: i32,
    severity: String,
    meter_value: i32,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReplayFrameJson {
    board: Vec<u64>,
    piece: u8,
    #[serde(rename = "move")]
    player_move: Option<ReplayMoveJson>,
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
    eval_before: i32,
    eval_after: i32,
    best_eval: i32,
    eval_loss: i32,
    severity: String,
    meter_value: i32,
}
