//! fusion-wasm - WebAssembly entry points and bindings for browser execution.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use fusion_analysis::{
    analyze_replay as rust_analyze_replay, detect_misdrop as detect_misdrop_core, AnalysisResult,
    GameStats, Misdrop, MisdropSeverity, Moment, MomentType, ReplayFrame,
};
use fusion_core::{Board, Move, Piece, Rotation, SpinType};
use fusion_engine::{calculate_attack, AttackConfig, ChargingConfig, ComboTable};
use fusion_eval::{evaluate, EvalWeights};
use fusion_search::BeamSearch;

#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

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

    pub fn from_rows(rows: &[u64]) -> Self {
        let mut board = Board::new();
        let limit = rows.len().min(Board::HEIGHT);
        for y in 0..limit {
            let row = rows[y] & 0x3FF;
            for x in 0..Board::WIDTH {
                if (row >> x) & 1 == 1 {
                    board.set(x, y, true);
                }
            }
        }
        Self { inner: board }
    }

    pub fn get(&self, x: i8, y: i8) -> bool {
        if !in_bounds(x, y) {
            return false;
        }
        self.inner.get(x as usize, y as usize)
    }

    pub fn set(&mut self, x: i8, y: i8, value: bool) {
        if !in_bounds(x, y) {
            return;
        }
        self.inner.set(x as usize, y as usize, value);
    }

    pub fn clear_lines(&mut self) -> u8 {
        self.inner.clear_lines()
    }

    pub fn to_rows(&self) -> Vec<u64> {
        (0..Board::HEIGHT)
            .map(|y| self.inner.row(y) as u64)
            .collect()
    }
}

#[wasm_bindgen]
pub struct JsMove {
    inner: Move,
}

#[wasm_bindgen]
impl JsMove {
    #[wasm_bindgen(constructor)]
    pub fn new(piece: u8, rotation: u8, x: i8, y: i8) -> Self {
        Self {
            inner: Move::new(piece_from_u8(piece), rotation_from_u8(rotation), x, y),
        }
    }

    pub fn piece(&self) -> u8 {
        piece_to_u8(self.inner.piece)
    }

    pub fn rotation(&self) -> u8 {
        rotation_to_u8(self.inner.rotation)
    }

    pub fn x(&self) -> i8 {
        self.inner.x
    }

    pub fn y(&self) -> i8 {
        self.inner.y
    }

    pub fn hold_used(&self) -> bool {
        self.inner.hold_used
    }

    pub fn spin(&self) -> u8 {
        spin_to_u8(self.inner.spin_type)
    }

    pub fn set_hold_used(&mut self, hold_used: bool) {
        self.inner.hold_used = hold_used;
    }

    pub fn set_spin(&mut self, spin: u8) {
        self.inner.spin_type = spin_from_u8(spin);
    }
}

#[derive(Serialize, Deserialize)]
pub struct JsMoveResult {
    pub piece: u8,
    pub rotation: u8,
    pub x: i8,
    pub y: i8,
    pub score: f32,
    pub spin: u8,
    pub hold_used: bool,
}

#[derive(Serialize, Deserialize)]
pub struct JsMoveData {
    pub piece: u8,
    pub rotation: u8,
    pub x: i8,
    pub y: i8,
}

#[derive(Deserialize)]
pub struct JsReplayFrame {
    pub frame_number: u32,
    pub piece: u8,
    pub player_move: JsMoveData,
    pub board: Board,
    pub lines_cleared: u8,
}

impl From<JsReplayFrame> for ReplayFrame {
    fn from(js: JsReplayFrame) -> Self {
        Self {
            frame_number: js.frame_number,
            piece: piece_from_u8(js.piece),
            player_move: Move::new(
                piece_from_u8(js.player_move.piece),
                rotation_from_u8(js.player_move.rotation),
                js.player_move.x,
                js.player_move.y,
            ),
            board_before: js.board,
            lines_cleared: js.lines_cleared,
        }
    }
}

#[derive(Serialize)]
pub struct JsMisdrop {
    pub frame: u32,
    pub player_move: JsMoveData,
    pub best_move: JsMoveData,
    pub player_score: f32,
    pub best_score: f32,
    pub score_diff: f32,
    pub creates_hole: bool,
    pub severity: String,
}

impl From<&Move> for JsMoveData {
    fn from(mv: &Move) -> Self {
        Self {
            piece: piece_to_u8(mv.piece),
            rotation: rotation_to_u8(mv.rotation),
            x: mv.x,
            y: mv.y,
        }
    }
}

impl From<&Misdrop> for JsMisdrop {
    fn from(m: &Misdrop) -> Self {
        Self {
            frame: m.frame,
            player_move: JsMoveData::from(&m.player_move),
            best_move: JsMoveData::from(&m.best_move),
            player_score: m.player_score,
            best_score: m.best_score,
            score_diff: m.score_diff,
            creates_hole: m.creates_hole,
            severity: match m.severity {
                MisdropSeverity::Minor => "Minor".to_string(),
                MisdropSeverity::Moderate => "Moderate".to_string(),
                MisdropSeverity::Major => "Major".to_string(),
            },
        }
    }
}

#[derive(Serialize)]
pub struct JsMoment {
    pub frame: u32,
    pub moment_type: String,
    pub description: String,
    pub suggestion: Option<String>,
    pub impact: f32,
}

impl From<&Moment> for JsMoment {
    fn from(m: &Moment) -> Self {
        Self {
            frame: m.frame,
            moment_type: match &m.moment_type {
                MomentType::Misdrop(s) => match s {
                    MisdropSeverity::Minor => "Misdrop(Minor)".to_string(),
                    MisdropSeverity::Moderate => "Misdrop(Moderate)".to_string(),
                    MisdropSeverity::Major => "Misdrop(Major)".to_string(),
                },
                MomentType::MissedTSpin => "MissedTSpin".to_string(),
                MomentType::InefficientClear => "InefficientClear".to_string(),
                MomentType::GoodPlay => "GoodPlay".to_string(),
                MomentType::ClutchSave => "ClutchSave".to_string(),
            },
            description: m.description.clone(),
            suggestion: m.suggestion.clone(),
            impact: m.impact,
        }
    }
}

#[derive(Serialize)]
pub struct JsGameStats {
    pub total_pieces: u32,
    pub misdrops: u32,
    pub lines_cleared: u32,
    pub attack_sent: u32,
    pub max_combo: u32,
    pub max_b2b: u32,
    pub tspins: u32,
    pub quads: u32,
}

impl From<&GameStats> for JsGameStats {
    fn from(s: &GameStats) -> Self {
        Self {
            total_pieces: s.total_pieces,
            misdrops: s.misdrops,
            lines_cleared: s.lines_cleared,
            attack_sent: s.attack_sent,
            max_combo: s.max_combo,
            max_b2b: s.max_b2b,
            tspins: s.tspins,
            quads: s.quads,
        }
    }
}

#[derive(Serialize)]
pub struct JsAnalysisResult {
    pub moments: Vec<JsMoment>,
    pub stats: JsGameStats,
    pub misdrops: Vec<JsMisdrop>,
    pub overall_score: f32,
}

impl From<&AnalysisResult> for JsAnalysisResult {
    fn from(r: &AnalysisResult) -> Self {
        Self {
            moments: r.moments.iter().map(JsMoment::from).collect(),
            stats: JsGameStats::from(&r.stats),
            misdrops: r.misdrops.iter().map(JsMisdrop::from).collect(),
            overall_score: r.overall_score,
        }
    }
}

#[wasm_bindgen]
pub fn find_best_move(board: &JsBoard, piece: u8) -> JsValue {
    let piece = piece_from_u8(piece);
    let search = BeamSearch::default();
    match search.find_best_move(&board.inner, piece) {
        Some((mv, score)) => {
            let result = JsMoveResult {
                piece: piece_to_u8(mv.piece),
                rotation: rotation_to_u8(mv.rotation),
                x: mv.x,
                y: mv.y,
                score,
                spin: spin_to_u8(mv.spin_type),
                hold_used: mv.hold_used,
            };
            serde_wasm_bindgen::to_value(&result).unwrap_or_else(|_| JsValue::NULL)
        }
        None => JsValue::NULL,
    }
}

#[wasm_bindgen]
pub fn evaluate_board(board: &JsBoard) -> f32 {
    evaluate(&board.inner, &EvalWeights::default())
}

#[wasm_bindgen]
pub fn evaluate_with_weights(
    board: &JsBoard,
    height: f32,
    holes: f32,
    bumpiness: f32,
    wells: f32,
) -> f32 {
    let weights = EvalWeights {
        height,
        holes,
        bumpiness,
        wells,
        ..Default::default()
    };
    evaluate(&board.inner, &weights)
}

#[wasm_bindgen]
pub fn get_all_moves(board: &JsBoard, piece: u8) -> JsValue {
    let piece = piece_from_u8(piece);
    let moves = fusion_engine::generate_moves(&board.inner, piece);
    let results: Vec<JsMoveResult> = moves
        .into_iter()
        .map(|mv| JsMoveResult {
            piece: piece_to_u8(mv.piece),
            rotation: rotation_to_u8(mv.rotation),
            x: mv.x,
            y: mv.y,
            score: 0.0,
            spin: spin_to_u8(mv.spin_type),
            hold_used: mv.hold_used,
        })
        .collect();
    serde_wasm_bindgen::to_value(&results).unwrap_or_else(|_| JsValue::NULL)
}

#[wasm_bindgen(js_name = detect_misdrop)]
pub fn detect_misdrop(board: &JsBoard, piece: u8, player_move: &JsMove, frame: u32) -> JsValue {
    let piece = piece_from_u8(piece);
    match detect_misdrop_core(&board.inner, piece, &player_move.inner, frame) {
        Some(m) => {
            let result = JsMisdrop::from(&m);
            serde_wasm_bindgen::to_value(&result).unwrap_or_else(|_| JsValue::NULL)
        }
        None => JsValue::NULL,
    }
}

#[wasm_bindgen]
pub fn analyze_replay(frames: JsValue) -> JsValue {
    let js_frames: Vec<JsReplayFrame> = match serde_wasm_bindgen::from_value(frames) {
        Ok(frames) => frames,
        Err(_) => return JsValue::NULL,
    };

    let rust_frames: Vec<ReplayFrame> = js_frames.into_iter().map(ReplayFrame::from).collect();

    let result = rust_analyze_replay(&rust_frames);
    let js_result = JsAnalysisResult::from(&result);

    serde_wasm_bindgen::to_value(&js_result).unwrap_or_else(|_| JsValue::NULL)
}

fn in_bounds(x: i8, y: i8) -> bool {
    x >= 0 && y >= 0 && x < Board::WIDTH as i8 && y < Board::HEIGHT as i8
}

fn piece_from_u8(value: u8) -> Piece {
    match value {
        0 => Piece::I,
        1 => Piece::O,
        2 => Piece::T,
        3 => Piece::S,
        4 => Piece::Z,
        5 => Piece::J,
        6 => Piece::L,
        _ => Piece::I,
    }
}

fn piece_to_u8(piece: Piece) -> u8 {
    match piece {
        Piece::I => 0,
        Piece::O => 1,
        Piece::T => 2,
        Piece::S => 3,
        Piece::Z => 4,
        Piece::J => 5,
        Piece::L => 6,
    }
}

fn rotation_from_u8(value: u8) -> Rotation {
    match value {
        0 => Rotation::North,
        1 => Rotation::East,
        2 => Rotation::South,
        3 => Rotation::West,
        _ => Rotation::North,
    }
}

fn rotation_to_u8(rotation: Rotation) -> u8 {
    match rotation {
        Rotation::North => 0,
        Rotation::East => 1,
        Rotation::South => 2,
        Rotation::West => 3,
    }
}

fn spin_from_u8(value: u8) -> SpinType {
    match value {
        1 => SpinType::Mini,
        2 => SpinType::Full,
        _ => SpinType::None,
    }
}

fn spin_to_u8(spin: SpinType) -> u8 {
    match spin {
        SpinType::None => 0,
        SpinType::Mini => 1,
        SpinType::Full => 2,
    }
}

fn combo_table_from_u8(value: u8) -> ComboTable {
    match value {
        0 => ComboTable::Multiplier,
        1 => ComboTable::Classic,
        2 => ComboTable::Modern,
        _ => ComboTable::None,
    }
}

// ============================================================================
// Attack Configuration Bindings
// ============================================================================

/// JavaScript-friendly attack configuration for S2 mechanics
#[wasm_bindgen]
pub struct JsAttackConfig {
    inner: AttackConfig,
}

#[wasm_bindgen]
impl JsAttackConfig {
    /// Create Tetra League preset (pc=5, surge_base=4)
    #[wasm_bindgen(js_name = tetraLeague)]
    pub fn tetra_league() -> Self {
        Self {
            inner: AttackConfig::tetra_league(),
        }
    }

    /// Create Quick Play preset (pc=3, surge_base=1)
    #[wasm_bindgen(js_name = quickPlay)]
    pub fn quick_play() -> Self {
        Self {
            inner: AttackConfig::quick_play(),
        }
    }

    /// Create custom configuration
    #[wasm_bindgen(constructor)]
    pub fn new(
        pc_garbage: u8,
        pc_b2b: u8,
        b2b_chaining: bool,
        b2b_charging_base: u8,
        combo_table: u8,
        garbage_multiplier: f32,
    ) -> Self {
        Self {
            inner: AttackConfig {
                pc_garbage,
                pc_b2b,
                b2b_chaining,
                b2b_charging: Some(ChargingConfig::new(4, b2b_charging_base)),
                combo_table: combo_table_from_u8(combo_table),
                garbage_multiplier,
            },
        }
    }

    /// Get PC garbage value
    #[wasm_bindgen(getter, js_name = pcGarbage)]
    pub fn pc_garbage(&self) -> u8 {
        self.inner.pc_garbage
    }

    /// Get garbage multiplier
    #[wasm_bindgen(getter, js_name = garbageMultiplier)]
    pub fn garbage_multiplier(&self) -> f32 {
        self.inner.garbage_multiplier
    }
}

/// Calculate attack value for a line clear
#[wasm_bindgen(js_name = calculateAttack)]
pub fn calculate_attack_js(
    lines: u8,
    spin: u8,
    b2b: u8,
    combo: u8,
    config: &JsAttackConfig,
    is_pc: bool,
) -> f32 {
    calculate_attack(lines, spin_from_u8(spin), b2b, combo, &config.inner, is_pc)
}

// ============================================================================
// Tilt Detection Bindings
// ============================================================================
