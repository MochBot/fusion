use fusion_wasm::*;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn test_board_new() {
    let board = JsBoard::new();
    assert!(!board.get(0, 0));
}

#[wasm_bindgen_test]
fn test_attack_config_presets() {
    let tl = JsAttackConfig::tetra_league();
    assert_eq!(tl.pc_garbage(), 5);

    let qp = JsAttackConfig::quick_play();
    assert_eq!(qp.pc_garbage(), 3);
}

#[wasm_bindgen_test]
fn test_calculate_attack_quad() {
    let config = JsAttackConfig::tetra_league();
    let attack = calculate_attack_js(4, 0, 0, 0, &config, false);
    assert_eq!(attack, 4.0); // Quad = 4 lines
}

// ============================================================================
// Analysis Pipeline Tests
// ============================================================================

use serde::{Deserialize, Serialize};

// Helper structs for testing
#[derive(Serialize)]
struct TestReplayFrame {
    pub frame_number: u32,
    pub piece: u8,
    pub player_move: JsMoveData,
    pub board: Vec<u64>,
    pub lines_cleared: u8,
}

#[derive(Deserialize)]
struct TestMisdropResult {
    pub severity: String,
    pub score_diff: f32,
}

#[derive(Deserialize)]
struct TestAnalysisResult {
    pub misdrops: Vec<TestMisdropResult>,
    pub overall_score: f32,
}

#[wasm_bindgen_test]
fn test_detect_misdrop_returns_null_for_optimal_move() {
    let board = JsBoard::new();
    let piece = 0; // I piece

    // Find best move first to ensure we have the optimal one
    let best_move_val = find_best_move(&board, piece);
    assert!(
        !best_move_val.is_null(),
        "Should find a best move for empty board"
    );

    let best_move: JsMoveResult =
        serde_wasm_bindgen::from_value(best_move_val).expect("Failed to deserialize best move");

    let player_move = JsMove::new(piece, best_move.rotation, best_move.x, best_move.y);

    let result = detect_misdrop(&board, piece, &player_move, 1);
    assert!(result.is_null(), "Should return null for optimal move");
}

#[wasm_bindgen_test]
fn test_detect_misdrop_returns_result_for_bad_move() {
    // Setup: 4-high well at x=0 (Tetris ready)
    let mut board = JsBoard::new();
    // Fill rows 0-3 except col 0
    for y in 0..4 {
        for x in 1..10 {
            board.set(x, y, true);
        }
    }

    // I piece. Best move: drop into the well (x=0, rot=1). Clears 4 lines.
    // Bad move: Block the well with horizontal I (x=0, rot=0, y=4).

    let piece = 0; // I
    let bad_move = JsMove::new(piece, 0, 0, 4); // Horizontal at x=0, y=4

    let result = detect_misdrop(&board, piece, &bad_move, 10);
    assert!(!result.is_null(), "Expected misdrop for obvious bad move");

    let misdrop: TestMisdropResult =
        serde_wasm_bindgen::from_value(result).expect("Failed to deserialize misdrop");

    assert!(misdrop.score_diff > 0.0);
    assert!(["Minor", "Moderate", "Major"].contains(&misdrop.severity.as_str()));
}

#[wasm_bindgen_test]
fn test_analyze_replay_empty_frames() {
    let frames: Vec<TestReplayFrame> = vec![];
    let frames_js = serde_wasm_bindgen::to_value(&frames).unwrap();

    let result_js = analyze_replay(frames_js);
    assert!(!result_js.is_null());

    let result: TestAnalysisResult = serde_wasm_bindgen::from_value(result_js).unwrap();
    assert_eq!(result.misdrops.len(), 0);
    assert_eq!(result.overall_score, 100.0);
}

#[wasm_bindgen_test]
fn test_analyze_replay_single_frame() {
    let board = JsBoard::new();
    // I piece placed flat at bottom
    let move_data = JsMoveData {
        piece: 0,
        rotation: 0,
        x: 3,
        y: 0,
    };

    let frame = TestReplayFrame {
        frame_number: 1,
        piece: 0,
        player_move: move_data,
        board: board.to_rows(),
        lines_cleared: 0,
    };

    let frames = vec![frame];
    let frames_js = serde_wasm_bindgen::to_value(&frames).unwrap();

    let result_js = analyze_replay(frames_js);
    assert!(!result_js.is_null());

    let result: TestAnalysisResult = serde_wasm_bindgen::from_value(result_js).unwrap();
    assert!(result.overall_score >= 0.0);
}

#[wasm_bindgen_test]
fn test_js_misdrop_serialization() {
    // Verify we can serialize/deserialize the JsMisdrop structure correctly

    // Re-use the Tetris ready scenario
    let mut board = JsBoard::new();
    for y in 0..4 {
        for x in 1..10 {
            board.set(x, y, true);
        }
    }
    let piece = 0; // I
    let bad_move = JsMove::new(piece, 0, 0, 4);

    let result_js = detect_misdrop(&board, piece, &bad_move, 100);

    if result_js.is_null() {
        return;
    }

    let val: serde_json::Value = serde_wasm_bindgen::from_value(result_js).unwrap();

    assert!(val.get("frame").is_some());
    assert!(val.get("player_move").is_some());
    assert!(val.get("best_move").is_some());
    assert!(val.get("severity").is_some());
    assert!(val.get("creates_hole").is_some());

    let severity = val["severity"]
        .as_str()
        .expect("Severity should be a string");
    assert!(["Minor", "Moderate", "Major"].contains(&severity));
}
