// wasm.rs -- WASM bridge for Mosaic SvelteKit frontend.
// Re-numbers piece IDs to the Triangle order (I0 O1 T2 S3 Z4 J5 L6).

use wasm_bindgen::prelude::*;

use crate::analysis::{self, coaching_dp_multiplier};
use crate::attack::{
    self, calculate_attack_full, calculate_attack_s2_tl_with_multiplier,
    count_cleared_garbage_rows, AttackConfig, AttackContext, ComboTable,
};
use crate::eval::{self, evaluate, EvalWeights};
use crate::header::*;
use crate::move_buffer::MoveBuffer;
use crate::movegen::{generate, generate_playable};
use crate::pathfinder;
use crate::search::{find_best_move, find_best_move_with_scores_forced, SearchConfig};
use crate::state::{ClearType, GameState, TransitionObservation};
use crate::wasm_board::JsBoard;
use crate::wasm_types::*;

// init

#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
}

// JsAttackConfig

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

    #[wasm_bindgen(getter, js_name = "pcGarbage")]
    pub fn pc_garbage(&self) -> u8 {
        self.inner.pc_garbage
    }

    #[wasm_bindgen(getter, js_name = "garbageMultiplier")]
    pub fn garbage_multiplier(&self) -> f32 {
        self.inner.garbage_multiplier
    }
}

// Free functions

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

#[wasm_bindgen(js_name = "evaluate_board")]
pub fn evaluate_board_wasm(board: &JsBoard) -> f32 {
    let weights = EvalWeights::default();
    eval::evaluate(&board.inner, &weights)
}

#[wasm_bindgen(js_name = "evaluate_position")]
pub fn evaluate_position_wasm(
    pre_board: &JsBoard,
    post_board: &JsBoard,
    piece: u8,
    frame: JsValue,
) -> JsValue {
    let pre_board_clone = pre_board.inner.clone();
    let pre_board_for_gen = pre_board.inner.clone();
    let post_board_clone = post_board.inner.clone();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let p = piece_from_external(piece)?;
        let frame_context = from_js::<ReplayFrameContextJson>(frame);
        let state = game_state_from_external_context(pre_board_clone, p, frame_context.as_ref());

        let weights = EvalWeights::default();
        let mut config = SearchConfig {
            time_budget_ms: None, // coaching eval: no time limit, full beam search
            ..SearchConfig::default()
        };
        // PC skip: zero out PC bonuses so eval doesn't inflate for
        // unrealistic coaching advice paths.
        config.attack_config.pc_garbage = 0;
        config.attack_config.pc_b2b = 0;

        let eval_before = evaluate(&state.board, &weights);
        let eval_after = evaluate(&post_board_clone, &weights);

        let coaching_before = state.coaching;

        let post_height = post_board_clone.height();
        let post_spawn_blocked = GameState::spawn_envelope_blocked(&post_board_clone);
        let coaching_after = coaching_before.transition(TransitionObservation {
            resulting_height: post_height,
            resulting_b2b: frame_context.as_ref().and_then(|ctx| ctx.b2b).unwrap_or(0) as u8,
            resulting_combo: frame_context
                .as_ref()
                .and_then(|ctx| ctx.combo)
                .unwrap_or(0) as u32,
            lines_cleared: frame_context
                .as_ref()
                .and_then(|ctx| ctx.lines_cleared)
                .unwrap_or(0),
            hold_used: frame_context
                .as_ref()
                .and_then(|ctx| ctx.hold_used)
                .unwrap_or(false),
            pending_garbage: frame_context
                .as_ref()
                .and_then(|ctx| ctx.pending_garbage)
                .unwrap_or(0) as u8,
            imminent_garbage: frame_context
                .as_ref()
                .and_then(|ctx| ctx.imminent_garbage)
                .unwrap_or(0) as u8,
            spawn_envelope_blocked: post_spawn_blocked,
        });

        // Identify actual move before search to force it into the beam
        let actual_move_for_search: Option<Move>;
        let actual_move_raw: Option<u16>;
        {
            let mut moves = MoveBuffer::new();
            generate(&pre_board_for_gen, &mut moves, p, false);
            let mut found_move: Option<Move> = None;
            let mut found_raw: Option<u16> = None;
            for m in moves.as_slice() {
                let mut trial = pre_board_for_gen.clone();
                trial.do_move(m);
                if trial.rows == post_board_clone.rows {
                    found_move = Some(*m);
                    found_raw = Some(m.raw());
                    break;
                }
            }
            actual_move_for_search = found_move;
            actual_move_raw = found_raw;
        }

        // Run search with forced root move to keep the player's actual move in beam
        let full_result =
            find_best_move_with_scores_forced(&state, &config, &weights, actual_move_for_search);

        let (
            best_eval,
            best_move_json,
            best_coaching_state,
            eval_loss,
            severity,
            position_complexity,
            board_score,
            attack_score,
            chain_score,
            context_score,
            actual_search_score_opt,
            path_attack,
            path_chain,
            path_context,
            recommended_path,
            best_path_attack_summary,
        ) = match &full_result {
            Some(full) => {
                let sr = &full.best;
                let best_search_score = sr.score;

                let move_json = if !post_board_clone.obstructed_move(&sr.best_move) {
                    MoveResultJson {
                        piece: piece_to_external(sr.best_move.piece()),
                        rotation: sr.best_move.rotation() as u8,
                        x: sr.best_move.x() as i8,
                        y: sr.best_move.y() as i8,
                        score: best_search_score,
                        spin: sr.best_move.spin() as u8,
                        hold_used: sr.hold_used,
                    }
                } else {
                    MoveResultJson {
                        piece: piece_to_external(sr.best_move.piece()),
                        rotation: 0,
                        x: 0,
                        y: 0,
                        score: best_search_score,
                        spin: 0,
                        hold_used: sr.hold_used,
                    }
                };

                let actual_search_score = actual_move_raw.and_then(|raw| {
                    full.root_scores
                        .iter()
                        .find(|(m, _)| m.raw() == raw)
                        .map(|(_, s)| *s)
                });

                let (loss, sev) = if let Some(actual_score) = actual_search_score {
                    let raw_loss = (best_search_score - actual_score).max(0.0);

                    // Apply coaching state multiplier to amplify delta-P
                    let dp_mul = coaching_dp_multiplier(&coaching_after);
                    let amplified_actual = best_search_score - raw_loss * dp_mul;

                    let skill = analysis::PlayerSkill {
                        pps: frame_context
                            .as_ref()
                            .and_then(|ctx| ctx.player_pps)
                            .unwrap_or(1.57),
                        app: frame_context
                            .as_ref()
                            .and_then(|ctx| ctx.player_app)
                            .unwrap_or(0.48),
                        dsp: frame_context
                            .as_ref()
                            .and_then(|ctx| ctx.player_dsp)
                            .unwrap_or(0.20),
                    };
                    let sigmoid_c = analysis::compute_sigmoid_c(&skill);
                    let sev = analysis::classify_win_prob_drop(
                        best_search_score,
                        amplified_actual,
                        analysis::SIGMOID_K,
                        sigmoid_c,
                    );

                    (raw_loss, sev)
                } else {
                    // Actual move not in root_scores; can't classify quality
                    (0.0, analysis::Severity::None)
                };

                let recommended_path: Vec<MoveResultJson> = sr
                    .pv
                    .iter()
                    .map(|m| MoveResultJson {
                        piece: piece_to_external(m.piece()),
                        rotation: m.rotation() as u8,
                        x: m.x() as i8,
                        y: m.y() as i8,
                        score: 0.0,
                        spin: m.spin() as u8,
                        hold_used: false,
                    })
                    .collect();

                (
                    best_search_score,
                    move_json,
                    sr.coaching_state,
                    loss,
                    sev,
                    full.position_complexity,
                    full.board_score,
                    full.attack_score,
                    full.chain_score,
                    full.context_score,
                    actual_search_score,
                    full.path_attack,
                    full.path_chain,
                    full.path_context,
                    recommended_path,
                    build_path_attack_summary(&full.best.pv_clear_events),
                )
            }
            None => {
                return None;
            }
        };

        let meter_value = analysis::normalize_meter(eval_after);

        let combo_after = frame_context
            .as_ref()
            .and_then(|ctx| ctx.combo)
            .unwrap_or(0) as u32;
        let combo_before = frame_context
            .as_ref()
            .and_then(|ctx| ctx.combo_before)
            .unwrap_or(0) as u32;
        let lines_cleared_val = frame_context
            .as_ref()
            .and_then(|ctx| ctx.lines_cleared)
            .unwrap_or(0);
        let insight_input = analysis::InsightDetectorInput {
            best_attack_score: path_attack,
            best_chain_score: path_chain,
            best_board_score: board_score,
            actual_score: actual_search_score_opt,
            best_score: best_eval,
            actual_combo_after: combo_after,
            actual_combo_before: combo_before,
            actual_lines_cleared: lines_cleared_val,
            board_eval_delta: eval_after - eval_before,
        };
        let insight_tags: Vec<String> = analysis::detect_insights(&insight_input)
            .iter()
            .map(|r| r.tag.to_str().to_string())
            .collect();

        Some(MoveEvalResultJson {
            eval_before,
            eval_after,
            best_eval,
            best_move: best_move_json,
            eval_loss,
            severity: match severity {
                analysis::Severity::None => "none",
                analysis::Severity::Inaccuracy => "inaccuracy",
                analysis::Severity::Mistake => "mistake",
                analysis::Severity::Blunder => "blunder",
            }
            .to_string(),
            meter_value,
            coaching_before: coaching_to_contract(coaching_before),
            coaching_after: coaching_to_contract(coaching_after),
            best_coaching_state: coaching_to_contract(best_coaching_state),
            position_complexity,
            board_score,
            attack_score,
            chain_score,
            context_score,
            path_attack,
            path_chain,
            path_context,
            insight_tags,
            recommended_path,
            best_path_attack_summary,
            actual_move: actual_move_for_search.map(|m| MoveResultJson {
                piece: piece_to_external(m.piece()),
                rotation: m.rotation() as u8,
                x: m.x() as i8,
                y: m.y() as i8,
                score: actual_search_score_opt.unwrap_or(0.0),
                spin: m.spin() as u8,
                hold_used: false,
            }),
        })
    }));

    match result {
        Ok(Some(json)) => to_js(&json),
        _ => JsValue::NULL,
    }
}

#[wasm_bindgen(js_name = "find_best_move")]
pub fn find_best_move_wasm(board: &JsBoard, piece: u8, frame: JsValue) -> JsValue {
    let board_clone = board.inner.clone();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let p = piece_from_external(piece)?;
        let frame_context = from_js::<ReplayFrameContextJson>(frame);
        let state = game_state_from_external_context(board_clone, p, frame_context.as_ref());

        let weights = EvalWeights::default();
        let mut config = SearchConfig {
            time_budget_ms: Some(50),
            ..SearchConfig::default()
        };
        config.attack_config.pc_garbage = 0;
        config.attack_config.pc_b2b = 0;

        let search_result = find_best_move(&state, &config, &weights)?;
        Some(MoveResultJson {
            piece: piece_to_external(search_result.best_move.piece()),
            rotation: search_result.best_move.rotation() as u8,
            x: search_result.best_move.x() as i8,
            y: search_result.best_move.y() as i8,
            score: search_result.score,
            spin: search_result.best_move.spin() as u8,
            hold_used: search_result.hold_used,
        })
    }));

    match result {
        Ok(Some(json)) => to_js(&json),
        _ => JsValue::NULL,
    }
}

#[wasm_bindgen(js_name = "get_all_moves")]
pub fn get_all_moves_wasm(board: &JsBoard, piece: u8) -> JsValue {
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return JsValue::NULL,
    };

    let mut moves = crate::move_buffer::MoveBuffer::new();
    generate_playable(&board.inner, &mut moves, p, false);

    let all_moves: Vec<MoveResultJson> = moves
        .as_slice()
        .iter()
        .map(|m| MoveResultJson {
            piece: piece_to_external(m.piece()),
            rotation: m.rotation() as u8,
            x: m.x() as i8,
            y: m.y() as i8,
            score: 0.0,
            spin: m.spin() as u8,
            hold_used: false,
        })
        .collect();

    to_js(&all_moves)
}

// Batched expansion for offline search/labeling. Returns a fixed 46-float
// record per legal placement: [attack, lines, b2b_after, combo_after,
// pending_after, spin, rows[0..40]].

pub(crate) const EXPAND_REC: usize = 46;

#[wasm_bindgen(js_name = "expand_all")]
pub fn expand_all_wasm(
    board: &JsBoard,
    piece: u8,
    b2b: i32,
    combo: i32,
    pending_garbage: u32,
) -> Vec<f64> {
    expand_all_with_garbage_rows(board, piece, b2b, combo, pending_garbage, None, 1.0)
}

#[wasm_bindgen(js_name = "expand_all_gm")]
pub fn expand_all_gm_wasm(
    board: &JsBoard,
    piece: u8,
    b2b: i32,
    combo: i32,
    pending_garbage: u32,
    garbage_rows: &[u64],
    garbage_multiplier: f64,
) -> Vec<f64> {
    expand_all_with_garbage_rows(
        board,
        piece,
        b2b,
        combo,
        pending_garbage,
        Some(garbage_rows),
        garbage_multiplier,
    )
}

fn expand_all_with_garbage_rows(
    board: &JsBoard,
    piece: u8,
    b2b: i32,
    combo: i32,
    pending_garbage: u32,
    garbage_rows: Option<&[u64]>,
    garbage_multiplier: f64,
) -> Vec<f64> {
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let mut moves = MoveBuffer::new();
    generate_playable(&board.inner, &mut moves, p, false);

    let mut out: Vec<f64> = Vec::with_capacity(moves.as_slice().len() * EXPAND_REC);
    append_expand_all_records(
        &mut out,
        &board.inner,
        moves.as_slice(),
        b2b,
        combo,
        pending_garbage,
        garbage_rows,
        garbage_multiplier,
    );
    out
}

#[allow(clippy::too_many_arguments)]
fn push_expand_record(
    out: &mut Vec<f64>,
    board: &crate::board::Board,
    m: &crate::header::Move,
    b2b: i32,
    combo: i32,
    pending_garbage: u32,
    garbage_rows: Option<&[u64]>,
    garbage_multiplier: f64,
) {
    let mut nb = board.clone();
    nb.place(m);
    let cleared = nb.line_clears();
    let lines = cleared.count_ones() as u8;
    if cleared != 0 {
        nb.clear_lines(cleared);
    }
    let spin = m.spin();
    let next_pending = pending_garbage.saturating_sub(lines as u32);
    let garbage_cleared = match garbage_rows {
        Some(rows) => count_cleared_garbage_rows(cleared, rows),
        None => {
            if pending_garbage > 0 && lines > 0 {
                1
            } else {
                0
            }
        }
    };
    let attack = calculate_attack_s2_tl_with_multiplier(
        lines,
        spin,
        b2b,
        combo,
        nb.is_empty(),
        garbage_cleared,
        garbage_multiplier,
    );

    out.push(attack.attack as f64);
    out.push(lines as f64);
    out.push(attack.b2b_after as f64);
    out.push(attack.combo_after as f64);
    out.push(next_pending as f64);
    out.push(spin as u8 as f64);
    for y in 0..40 {
        out.push(nb.rows[y] as f64);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_expand_all_records(
    out: &mut Vec<f64>,
    board: &crate::board::Board,
    moves: &[crate::header::Move],
    b2b: i32,
    combo: i32,
    pending_garbage: u32,
    garbage_rows: Option<&[u64]>,
    garbage_multiplier: f64,
) {
    for m in moves {
        push_expand_record(
            out,
            board,
            m,
            b2b,
            combo,
            pending_garbage,
            garbage_rows,
            garbage_multiplier,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn append_expand_ply_node(
    out: &mut Vec<f64>,
    board: &crate::board::Board,
    moves: &[crate::header::Move],
    b2b: i32,
    combo: i32,
    pending_garbage: u32,
    garbage_rows: &[u64],
    garbage_multiplier: f64,
) {
    out.push(moves.len() as f64);
    for m in moves {
        out.push(piece_to_external(m.piece()) as f64);
        out.push(m.rotation() as u8 as f64);
        out.push(m.x() as i8 as f64);
        out.push(m.y() as i8 as f64);
        out.push(m.spin() as u8 as f64);
        push_expand_record(
            out,
            board,
            m,
            b2b,
            combo,
            pending_garbage,
            Some(garbage_rows),
            garbage_multiplier,
        );
    }
}

#[wasm_bindgen(js_name = "expand_ply_batch")]
#[allow(clippy::too_many_arguments)]
pub fn expand_ply_batch_wasm(
    boards: &[u64],
    gmasks: &[u32],
    b2bs: &[i32],
    combos: &[i32],
    node_count: u32,
    piece: u8,
    pending: i32,
    multiplier: f64,
) -> Vec<f64> {
    let count = node_count as usize;
    if boards.len() != count * 40
        || gmasks.len() != count * 40
        || b2bs.len() != count
        || combos.len() != count
        || pending < 0
    {
        return Vec::new();
    }

    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut out = Vec::new();
    let mut moves = MoveBuffer::new();
    for n in 0..count {
        let board_start = n * 40;
        let board =
            crate::wasm_board::board_from_row_bitmasks(&boards[board_start..board_start + 40]);
        let mut garbage_rows = [0u64; 40];
        for y in 0..40 {
            if gmasks[board_start + y] != 0 {
                garbage_rows[y] = 1;
            }
        }

        moves.clear();
        generate_playable(&board, &mut moves, p, false);
        append_expand_ply_node(
            &mut out,
            &board,
            moves.as_slice(),
            b2bs[n],
            combos[n],
            pending as u32,
            &garbage_rows,
            multiplier,
        );
    }
    out
}

#[allow(dead_code)]
fn compact_garbage_rows(garbage_rows: &[u64; 40], cleared: u64) -> [u64; 40] {
    if cleared == 0 {
        return *garbage_rows;
    }

    let mut compacted = [0u64; 40];
    let mut write = 0usize;
    for (read, &row) in garbage_rows.iter().enumerate() {
        if cleared & (1u64 << read) == 0 {
            compacted[write] = row;
            write += 1;
        }
    }
    compacted
}

// Beam kernels in crate::coach_beam; this module has JS marshaling only.

#[inline]
fn gm_bits_from_mask(start_gmask: &[u32]) -> u64 {
    let mut bits = 0u64;
    for y in 0..40 {
        if start_gmask.get(y).copied().unwrap_or(0) != 0 {
            bits |= 1u64 << y;
        }
    }
    bits
}

#[wasm_bindgen(js_name = "beam_best_gm")]
#[allow(clippy::too_many_arguments)]
pub fn beam_best_gm_wasm(
    start_board: &[u32],
    start_gmask: &[u32],
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep_line: &[u32],
    beam_width: u32,
    garbage_multiplier: f64,
) -> f64 {
    beam_best_gm_impl(
        start_board,
        start_gmask,
        pieces,
        b2b,
        combo,
        pending,
        keep_line,
        beam_width,
        garbage_multiplier,
        false,
    )
}

/// Surge-shaped variant of `beam_best_gm`: each placement's value is
/// realized attack plus the change in banked surge potential.
/// Dedups by (rows, b2b, combo).
#[wasm_bindgen(js_name = "beam_best_gm_surge")]
#[allow(clippy::too_many_arguments)]
pub fn beam_best_gm_surge_wasm(
    start_board: &[u32],
    start_gmask: &[u32],
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep_line: &[u32],
    beam_width: u32,
    garbage_multiplier: f64,
) -> f64 {
    beam_best_gm_impl(
        start_board,
        start_gmask,
        pieces,
        b2b,
        combo,
        pending,
        keep_line,
        beam_width,
        garbage_multiplier,
        true,
    )
}

fn rows_u16_from_board_words(start_board: &[u32]) -> [u16; 40] {
    let mut rows0 = [0u16; 40];
    for y in 0..40 {
        rows0[y] = (start_board[y] & 0x3FF) as u16;
    }
    rows0
}

// keep is active only when the flat buffer covers every ply; rows
// are truncated to u16 (not masked).
fn keep_boards_from_flat(keep_line: &[u32], k: usize) -> Option<Vec<[u16; 40]>> {
    if keep_line.len() < k * 40 {
        return None;
    }
    Some(
        (0..k)
            .map(|t| {
                let mut kb = [0u16; 40];
                for y in 0..40 {
                    kb[y] = keep_line[t * 40 + y] as u16;
                }
                kb
            })
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
fn beam_best_gm_impl(
    start_board: &[u32],
    start_gmask: &[u32],
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep_line: &[u32],
    beam_width: u32,
    garbage_multiplier: f64,
    surge_shaping: bool,
) -> f64 {
    if start_board.len() < 40 || start_gmask.len() < 40 {
        return 0.0;
    }
    let rows0 = rows_u16_from_board_words(start_board);
    let keep_boards = keep_boards_from_flat(keep_line, pieces.len());
    crate::coach_beam::beam_best_gm(
        &rows0,
        gm_bits_from_mask(start_gmask),
        pieces,
        b2b,
        combo,
        pending,
        keep_boards.as_deref(),
        beam_width,
        garbage_multiplier,
        surge_shaping,
    )
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LineStepJson {
    rows: Vec<u16>,
    attack: f64,
    lines: u8,
    b2b: i32,
    combo: i32,
    spin: u8,
    b2b_before: i32,
    combo_before: i32,
    is_surge_release: bool,
    surge_potential_delta: f64,
    #[serde(rename = "move")]
    mv: Option<MoveResultJson>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LineResultJson {
    attack: f64,
    selection_score: f64,
    health_penalty: f64,
    steps: Vec<LineStepJson>,
    final_rows: Vec<u16>,
}

fn empty_line_result() -> LineResultJson {
    LineResultJson {
        attack: 0.0,
        selection_score: 0.0,
        health_penalty: 0.0,
        steps: Vec::new(),
        final_rows: Vec::new(),
    }
}

// Step+wellness beam; kernel in crate::coach_beam::beam_best_gm_line.
#[wasm_bindgen(js_name = "beam_best_gm_line")]
#[allow(clippy::too_many_arguments)]
pub fn beam_best_gm_line_wasm(
    start_board: &[u32],
    start_gmask: &[u32],
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    beam_width: u32,
    garbage_multiplier: f64,
    hole_w: f64,
    height_w: f64,
    height_grace: f64,
) -> JsValue {
    if start_board.len() < 40 || start_gmask.len() < 40 {
        return to_js(&empty_line_result());
    }
    let rows0 = rows_u16_from_board_words(start_board);
    match crate::coach_beam::beam_best_gm_line(
        &rows0,
        gm_bits_from_mask(start_gmask),
        pieces,
        b2b,
        combo,
        pending,
        beam_width,
        garbage_multiplier,
        hole_w,
        height_w,
        height_grace,
    ) {
        Some(best) => to_js(&LineResultJson {
            attack: best.attack,
            selection_score: best.selection_score,
            health_penalty: best.attack - best.selection_score,
            steps: best.steps.into_iter().map(line_step_json).collect(),
            final_rows: best.final_rows.to_vec(),
        }),
        None => to_js(&empty_line_result()),
    }
}

fn line_step_json(s: crate::coach_beam::CoachLineStep) -> LineStepJson {
    LineStepJson {
        rows: s.rows.to_vec(),
        attack: s.attack,
        lines: s.lines,
        b2b: s.b2b,
        combo: s.combo,
        spin: s.spin,
        b2b_before: s.b2b_before,
        combo_before: s.combo_before,
        is_surge_release: s.is_surge_release,
        surge_potential_delta: s.surge_potential_delta,
        mv: Some(MoveResultJson {
            piece: s.mv.piece,
            rotation: s.mv.rotation,
            x: s.mv.x,
            y: s.mv.y,
            score: 0.0,
            spin: s.mv.spin,
            hold_used: false,
        }),
    }
}

/// Insert garbage rows at the bottom of a row/gmask pair, shifting the
/// stack up. Cells pushed above row 39 are dropped. Returns the new
/// (rows, gmask). Inserted rows are fully flagged as garbage.
#[allow(dead_code)]
fn apply_garbage_insert(
    rows: &[u64; 40],
    gm: &[u64; 40],
    garbage: &[u64],
) -> ([u64; 40], [u64; 40]) {
    let n = garbage.len().min(40);
    let mut nr = [0u64; 40];
    let mut ng = [0u64; 40];
    nr[n..40].copy_from_slice(&rows[..(40 - n)]);
    ng[n..40].copy_from_slice(&gm[..(40 - n)]);
    for (y, &g) in garbage.iter().take(n).enumerate() {
        let r = g & (crate::board::FULL_ROW as u64);
        nr[y] = r;
        ng[y] = r;
    }
    (nr, ng)
}

/// Garbage-injecting variant of `beam_best_gm`. Inserts garbage rows
/// into every child board BEFORE the keep comparison so the player's
/// real (garbage-laden) line stays reachable.
#[wasm_bindgen(js_name = "beam_best_gm_gi")]
#[allow(clippy::too_many_arguments)]
pub fn beam_best_gm_gi_wasm(
    start_board: &[u32],
    start_gmask: &[u32],
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep_line: &[u32],
    beam_width: u32,
    multipliers: &[f64],
    garbage_rows: &[u32],
    garbage_counts: &[u32],
    max_gi: u32,
) -> f64 {
    if start_board.len() < 40 || start_gmask.len() < 40 {
        return 0.0;
    }
    let rows0 = rows_u16_from_board_words(start_board);
    let keep_boards = keep_boards_from_flat(keep_line, pieces.len());
    let garbage_rows_u16: Vec<u16> = garbage_rows.iter().map(|&v| (v & 0x3FF) as u16).collect();
    crate::coach_beam::beam_best_gm_gi(
        &rows0,
        gm_bits_from_mask(start_gmask),
        pieces,
        b2b,
        combo,
        pending,
        keep_boards.as_deref(),
        beam_width,
        multipliers,
        &garbage_rows_u16,
        garbage_counts,
        max_gi,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coach_beam::{compact_gm_bits, nonempty_row_mask, FxFullSet, FxRowSet};

    #[test]
    fn test_expand_all_gm_counts_cleared_garbage_rows() {
        let mut rows = [0u64; 40];
        let mut garbage_rows = [0u64; 40];
        for y in 0..4 {
            rows[y] = 0x03FF & !(1u64 << 9);
            garbage_rows[y] = rows[y];
        }

        let board = JsBoard::from_rows(&rows);
        let flat = expand_all_gm_wasm(&board, 0, -1, -1, 0, &garbage_rows, 1.0);
        let zero_garbage = [0u64; 40];
        let flat_plain = expand_all_gm_wasm(&board, 0, -1, -1, 0, &zero_garbage, 1.0);

        // Quad PC = 9 under S2/TL; marking rows as garbage adds +1 bonus
        assert!(
            flat_plain
                .chunks_exact(EXPAND_REC)
                .any(|rec| rec[0] == 9.0 && rec[1] == 4.0 && rec[4] == 0.0),
            "expected the plain four-line I perfect clear at 9 attack"
        );
        assert!(
            flat.chunks_exact(EXPAND_REC)
                .any(|rec| rec[0] == 10.0 && rec[1] == 4.0 && rec[4] == 0.0),
            "expected the garbage special bonus to lift the quad from 9 to 10 attack"
        );
    }

    #[test]
    fn test_expand_all_gm_applies_dynamic_multiplier() {
        let mut rows = [0u64; 40];
        let garbage_rows = [0u64; 40];
        rows[0] = 0x03FF & !0b1111u64;

        let board = JsBoard::from_rows(&rows);
        let flat = expand_all_gm_wasm(&board, 0, -1, 4, 0, &garbage_rows, 1.027);
        let flat_m1 = expand_all_gm_wasm(&board, 0, -1, 4, 0, &garbage_rows, 1.0);

        // Horizontal-I PC at combo 4->5 = 6; dynamic multiplier lifts to 7 at 1.027x
        assert!(
            flat_m1
                .chunks_exact(EXPAND_REC)
                .any(|rec| rec[0] == 6.0 && rec[1] == 1.0 && rec[3] == 5.0),
            "expected the combo single at 6 attack with multiplier 1.0"
        );
        assert!(
            flat.chunks_exact(EXPAND_REC)
                .any(|rec| rec[0] == 7.0 && rec[1] == 1.0 && rec[3] == 5.0),
            "expected dynamic multiplier to lift combo single from 6 to 7 attack"
        );
    }

    fn xorshift64(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn seeded_board_rows(state: &mut u64) -> [u64; 40] {
        let mut rows = [0u64; 40];
        let h = 1 + (xorshift64(state) % 10) as usize;
        for row in rows.iter_mut().take(h) {
            let mut bits = xorshift64(state) & 0x03FF;
            bits &= !(1u64 << (xorshift64(state) % 10));
            *row = bits;
        }
        rows
    }

    fn seeded_gmask_rows(state: &mut u64, rows: &[u64; 40]) -> [u32; 40] {
        let mut gm = [0u32; 40];
        for (y, slot) in gm.iter_mut().enumerate() {
            if rows[y] != 0 && (xorshift64(state) & 3) == 0 {
                *slot = 1;
            }
        }
        gm
    }

    fn append_expected_ply_node(
        expected: &mut Vec<f64>,
        rows: &[u64; 40],
        gmask: &[u32; 40],
        piece: u8,
        b2b: i32,
        combo: i32,
        pending: i32,
        multiplier: f64,
    ) {
        let board = crate::wasm_board::board_from_row_bitmasks(rows);
        let p = piece_from_external(piece).expect("test piece id is valid");
        let mut moves = MoveBuffer::new();
        generate_playable(&board, &mut moves, p, false);
        let js_board = JsBoard::from_rows(rows);
        let garbage_rows: Vec<u64> = gmask.iter().map(|&v| u64::from(v != 0)).collect();
        let records = expand_all_gm_wasm(
            &js_board,
            piece,
            b2b,
            combo,
            pending as u32,
            &garbage_rows,
            multiplier,
        );
        assert_eq!(records.len(), moves.as_slice().len() * EXPAND_REC);

        expected.push(moves.as_slice().len() as f64);
        for (i, m) in moves.as_slice().iter().enumerate() {
            expected.push(piece_to_external(m.piece()) as f64);
            expected.push(m.rotation() as u8 as f64);
            expected.push(m.x() as i8 as f64);
            expected.push(m.y() as i8 as f64);
            expected.push(m.spin() as u8 as f64);
            expected.extend_from_slice(&records[i * EXPAND_REC..(i + 1) * EXPAND_REC]);
        }
    }

    fn assert_f64_vec_bits_eq(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "framed output length");
        for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(a.to_bits(), e.to_bits(), "f64 mismatch at output[{i}]");
        }
    }

    #[test]
    fn test_expand_ply_batch_matches_per_node_exports_on_seeded_boards() {
        let mut state = 0xB47C_4ED0_2026_0704u64;
        for &node_count in &[1usize, 2, 7, 64] {
            for piece in 0u8..7 {
                let mut boards = Vec::with_capacity(node_count * 40);
                let mut gmasks = Vec::with_capacity(node_count * 40);
                let mut b2bs = Vec::with_capacity(node_count);
                let mut combos = Vec::with_capacity(node_count);
                let mut expected = Vec::new();

                for n in 0..node_count {
                    let rows = seeded_board_rows(&mut state);
                    let gmask = seeded_gmask_rows(&mut state, &rows);
                    let b2b = (xorshift64(&mut state) % 8) as i32 - 1;
                    let combo = (xorshift64(&mut state) % 6) as i32 - 1;
                    boards.extend_from_slice(&rows);
                    gmasks.extend_from_slice(&gmask);
                    b2bs.push(b2b);
                    combos.push(combo);
                    append_expected_ply_node(
                        &mut expected,
                        &rows,
                        &gmask,
                        piece,
                        b2b,
                        combo,
                        3,
                        1.027,
                    );
                    assert_eq!(boards.len(), (n + 1) * 40);
                }

                let actual = expand_ply_batch_wasm(
                    &boards,
                    &gmasks,
                    &b2bs,
                    &combos,
                    node_count as u32,
                    piece,
                    3,
                    1.027,
                );
                assert_f64_vec_bits_eq(&actual, &expected);
            }
        }
    }

    #[test]
    fn test_expand_ply_batch_rejects_empty_and_mismatched_inputs() {
        assert!(expand_ply_batch_wasm(&[], &[], &[], &[], 0, 0, 0, 1.0).is_empty());
        let rows = [0u64; 40];
        let gmask = [0u32; 40];
        assert!(expand_ply_batch_wasm(&rows[..39], &gmask, &[0], &[0], 1, 0, 0, 1.0).is_empty());
        assert!(expand_ply_batch_wasm(&rows, &gmask[..39], &[0], &[0], 1, 0, 0, 1.0).is_empty());
        assert!(expand_ply_batch_wasm(&rows, &gmask, &[], &[0], 1, 0, 0, 1.0).is_empty());
        assert!(expand_ply_batch_wasm(&rows, &gmask, &[0], &[], 1, 0, 0, 1.0).is_empty());
        assert!(expand_ply_batch_wasm(&rows, &gmask, &[0], &[0], 1, 7, 0, 1.0).is_empty());
        assert!(expand_ply_batch_wasm(&rows, &gmask, &[0], &[0], 1, 0, -1, 1.0).is_empty());
    }

    #[test]
    fn test_beam_surge_credits_b2b_build() {
        // Tetris well (col 9 open), b2b=6. I quad -> b2b 7 (build).
        let mut board = [0u32; 40];
        for y in 0..4 {
            board[y] = 0x03FFu32 & !(1u32 << 9);
        }
        let gmask = [0u32; 40];
        let raw = beam_best_gm_wasm(&board, &gmask, &[0], 6, 0, 0, &[], 64, 1.0);
        let surge = beam_best_gm_surge_wasm(&board, &gmask, &[0], 6, 0, 0, &[], 64, 1.0);
        let dp = (crate::attack::surge_potential(7, 1.0) - crate::attack::surge_potential(6, 1.0))
            as f64;
        assert_eq!(dp, 1.0);
        assert_eq!(
            surge,
            raw + dp,
            "building b2b 6->7 must add +{dp} surge potential (raw={raw} surge={surge})"
        );
    }

    #[test]
    fn test_beam_surge_cashout_is_neutral() {
        // Single-clear well (col 9 open), b2b=6. Cash-out nets neutral.
        let mut board = [0u32; 40];
        board[0] = 0x03FFu32 & !(1u32 << 9);
        let gmask = [0u32; 40];
        let raw = beam_best_gm_wasm(&board, &gmask, &[0], 6, 0, 0, &[], 64, 1.0);
        let surge = beam_best_gm_surge_wasm(&board, &gmask, &[0], 6, 0, 0, &[], 64, 1.0);
        assert!(raw >= 6.0, "raw should cash the surge (got {raw})");
        assert_eq!(
            surge, 0.0,
            "cashing surge nets to neutral under shaping (raw={raw} surge={surge})"
        );
    }

    #[test]
    fn test_beam_best_gm_rejects_short_inputs() {
        assert_eq!(
            beam_best_gm_wasm(&[], &[], &[0], -1, -1, 0, &[], 1, 1.0),
            0.0
        );
    }

    fn assert_f64_bits(label: &str, actual: f64, expected_bits: u64) {
        assert_eq!(
            actual.to_bits(),
            expected_bits,
            "{label}: actual={actual} bits={:#018x}",
            actual.to_bits()
        );
    }

    fn first_child_keep_line(board: &[u32; 40], piece: u8, pieces_len: usize) -> Vec<u32> {
        let mut rows0 = [0u64; 40];
        for y in 0..40 {
            rows0[y] = board[y] as u64;
        }
        let b = crate::wasm_board::board_from_row_bitmasks(&rows0);
        let p = piece_from_external(piece).expect("external piece id is valid");
        let mut moves = MoveBuffer::new();
        generate_playable(&b, &mut moves, p, false);
        let m = moves.as_slice().first().expect("fixture has legal moves");
        let mut child = b.clone();
        child.place(m);
        let cleared = child.line_clears();
        if cleared != 0 {
            child.clear_lines(cleared);
        }

        let mut keep_line = vec![0u32; pieces_len * 40];
        for y in 0..40 {
            keep_line[y] = child.rows[y] as u32;
        }
        keep_line
    }

    #[test]
    fn test_beam_best_gm_impl_characterization() {
        let mut tetris_well = [0u32; 40];
        let mut garbage_mask = [0u32; 40];
        for y in 0..4 {
            tetris_well[y] = 0x03FFu32 & !(1u32 << 9);
            garbage_mask[y] = tetris_well[y];
        }
        assert_f64_bits(
            "raw_multistep_garbage_mask",
            beam_best_gm_impl(
                &tetris_well,
                &garbage_mask,
                &[0, 2, 1],
                -1,
                -1,
                0,
                &[],
                16,
                1.0,
                false,
            ),
            0x4024000000000000,
        );

        assert_f64_bits(
            "surge_shaped_dynamic_multiplier",
            beam_best_gm_impl(
                &tetris_well,
                &garbage_mask,
                &[0, 0],
                6,
                0,
                0,
                &[],
                16,
                1.027,
                true,
            ),
            0x402a000000000000,
        );

        let empty_board = [0u32; 40];
        let empty_gmask = [0u32; 40];
        let keep_line = first_child_keep_line(&empty_board, 1, 2);
        assert_f64_bits(
            "keep_line_branch_empty_board",
            beam_best_gm_impl(
                &empty_board,
                &empty_gmask,
                &[1, 0],
                -1,
                -1,
                0,
                &keep_line,
                4,
                1.0,
                false,
            ),
            0,
        );
    }

    // Manual: cargo test --release --features wasm beam_timing_probe -- --ignored --nocapture
    #[test]
    #[ignore]
    fn beam_timing_probe() {
        let mut board = [0u32; 40];
        let mut gmask = [0u32; 40];
        for y in 0..6 {
            board[y] = 0x03FFu32 & !(1u32 << 4);
            gmask[y] = board[y];
        }
        board[6] = 0b0000110111;
        board[7] = 0b0000100101;
        let pieces = [0u8, 2, 1, 3, 5];

        let mut sink = 0.0f64;
        for _ in 0..3 {
            sink += beam_best_gm_impl(&board, &gmask, &pieces, 1, 0, 0, &[], 300, 1.0, true);
        }
        let iters = 30u32;
        let start = std::time::Instant::now();
        for _ in 0..iters {
            sink += beam_best_gm_impl(&board, &gmask, &pieces, 1, 0, 0, &[], 300, 1.0, true);
        }
        let per_call = start.elapsed().as_nanos() / iters as u128;
        println!("beam_timing_probe ns_per_call={per_call} sink={sink}");
    }

    #[derive(Default)]
    struct BeamPhaseStats {
        generate_ns: u128,
        clone_ns: u128,
        place_clear_ns: u128,
        attack_ns: u128,
        child_misc_ns: u128,
        sort_prune_ns: u128,
        nodes: u64,
        children: u64,
        pathfinder_calls: u64,
    }

    #[test]
    #[ignore]
    fn beam_phase_timing_probe() {
        use std::time::Instant;

        struct BNode {
            board: crate::board::Board,
            gm: u64,
            acc: i64,
            b2b: i32,
            combo: i32,
            pending: i32,
        }
        struct Child {
            board: crate::board::Board,
            acc: i64,
            b2b: i32,
            combo: i32,
            pending: i32,
            gm: u64,
        }

        let mut board = [0u32; 40];
        let mut gmask = [0u32; 40];
        for y in 0..6 {
            board[y] = 0x03FFu32 & !(1u32 << 4);
            gmask[y] = board[y];
        }
        board[6] = 0b0000110111;
        board[7] = 0b0000100101;
        let pieces = [0u8, 2, 1, 3, 5];
        let bw = 300usize;
        let garbage_multiplier = 1.0;

        let mut rows0 = [0u64; 40];
        for y in 0..40 {
            rows0[y] = board[y] as u64;
        }
        let mut beam: Vec<BNode> = vec![BNode {
            board: crate::wasm_board::board_from_row_bitmasks(&rows0),
            gm: gm_bits_from_mask(&gmask),
            acc: 0,
            b2b: 1,
            combo: 0,
            pending: 0,
        }];
        let mut children: Vec<Child> = Vec::new();
        let mut idx: Vec<usize> = Vec::new();
        let mut seen: FxRowSet = FxRowSet::default();
        let mut seen_full: FxFullSet = FxFullSet::default();
        let mut pruned: Vec<BNode> = Vec::with_capacity(bw);
        let mut moves = MoveBuffer::new();
        let mut stats = BeamPhaseStats::default();

        for &piece in &pieces {
            let p = piece_from_external(piece).expect("probe piece id is valid");
            children.clear();
            children.reserve(beam.len().saturating_mul(40));
            for node in &beam {
                stats.nodes += 1;
                moves.clear();

                let start = Instant::now();
                generate(&node.board, &mut moves, p, false);
                stats.generate_ns += start.elapsed().as_nanos();

                for m in moves.as_slice() {
                    let start = Instant::now();
                    let mut nb = node.board.clone();
                    stats.clone_ns += start.elapsed().as_nanos();

                    let start = Instant::now();
                    nb.place(m);
                    let cleared = nb.line_clears();
                    let lines = cleared.count_ones() as u8;
                    if cleared != 0 {
                        nb.clear_lines(cleared);
                    }
                    stats.place_clear_ns += start.elapsed().as_nanos();

                    let start = Instant::now();
                    let spin = m.spin();
                    let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                    let attack = calculate_attack_s2_tl_with_multiplier(
                        lines,
                        spin,
                        node.b2b,
                        node.combo,
                        nb.is_empty(),
                        garbage_cleared,
                        garbage_multiplier,
                    );
                    let shaped_delta =
                        crate::attack::surge_potential(attack.b2b_after as i32, garbage_multiplier)
                            - crate::attack::surge_potential(node.b2b, garbage_multiplier);
                    stats.attack_ns += start.elapsed().as_nanos();

                    let start = Instant::now();
                    let mut child_gm = compact_gm_bits(node.gm, cleared);
                    if child_gm != 0 {
                        child_gm &= nonempty_row_mask(&nb);
                    }
                    children.push(Child {
                        board: nb,
                        acc: node.acc + attack.attack as i64 + shaped_delta,
                        b2b: attack.b2b_after as i32,
                        combo: attack.combo_after as i32,
                        pending: (node.pending - lines as i32).max(0),
                        gm: child_gm,
                    });
                    stats.child_misc_ns += start.elapsed().as_nanos();
                    stats.children += 1;
                }
            }
            if children.is_empty() {
                break;
            }

            let start = Instant::now();
            idx.clear();
            idx.extend(0..children.len());
            idx.sort_by(|&a, &b| children[b].acc.cmp(&children[a].acc));
            seen.clear();
            seen.reserve(children.len());
            seen_full.clear();
            seen_full.reserve(children.len());
            pruned.clear();
            pruned.reserve(bw);
            for &ci in &idx {
                let c = &children[ci];
                let rows = c.board.rows;
                if !seen_full.insert((rows, c.b2b, c.combo)) {
                    continue;
                }
                pruned.push(BNode {
                    board: c.board.clone(),
                    gm: c.gm,
                    acc: c.acc,
                    b2b: c.b2b,
                    combo: c.combo,
                    pending: c.pending,
                });
                if pruned.len() >= bw {
                    break;
                }
            }
            stats.sort_prune_ns += start.elapsed().as_nanos();
            std::mem::swap(&mut beam, &mut pruned);
        }

        let result = beam.iter().map(|node| node.acc).max().unwrap_or(0) as f64;
        let total = stats.generate_ns
            + stats.clone_ns
            + stats.place_clear_ns
            + stats.attack_ns
            + stats.child_misc_ns
            + stats.sort_prune_ns;
        println!(
            "beam_phase_timing_probe result={result} nodes={} children={} pathfinder_calls={}",
            stats.nodes, stats.children, stats.pathfinder_calls
        );
        for (label, ns) in [
            ("generate", stats.generate_ns),
            ("clone", stats.clone_ns),
            ("place_clear", stats.place_clear_ns),
            ("attack", stats.attack_ns),
            ("child_misc", stats.child_misc_ns),
            ("sort_prune", stats.sort_prune_ns),
        ] {
            let pct = if total == 0 {
                0.0
            } else {
                ns as f64 * 100.0 / total as f64
            };
            println!("phase {label:>15}: {ns:>12} ns {pct:>6.2}%");
        }
    }

    #[test]
    fn test_apply_garbage_insert_shifts_up() {
        let mut rows = [0u64; 40];
        rows[0] = 0x0FF;
        let gm = [0u64; 40];
        let garbage = [0x3FBu64];
        let (nr, ng) = apply_garbage_insert(&rows, &gm, &garbage);
        assert_eq!(nr[0], 0x3FB, "inserted garbage row sits at the bottom");
        assert_eq!(nr[1], 0x0FF, "original bottom row shifted up by one");
        assert_eq!(ng[0], 0x3FB, "inserted row flagged as garbage");
        assert_eq!(ng[1], 0, "shifted original row is not garbage");
    }

    #[test]
    fn test_beam_best_gm_gi_parity_no_garbage() {
        let mut rows = [0u64; 40];
        for y in 0..4 {
            rows[y] = 0x03FF & !(1u64 << 9);
        }
        let board: Vec<u32> = rows.iter().map(|&r| r as u32).collect();
        let gmask = vec![0u32; 40];
        let pieces = [0u8, 0u8];
        let old = beam_best_gm_wasm(&board, &gmask, &pieces, -1, -1, 0, &[], 8, 1.0);
        let counts = [0u32, 0u32];
        let gi = beam_best_gm_gi_wasm(
            &board,
            &gmask,
            &pieces,
            -1,
            -1,
            0,
            &[],
            8,
            &[1.0, 1.0],
            &[],
            &counts,
            0,
        );
        assert!(
            old > 0.0,
            "precondition: old beam returns positive, got {old}"
        );
        assert_eq!(
            old, gi,
            "gi-beam with zero garbage must equal old beam ({old} vs {gi})"
        );
    }

    #[test]
    fn test_beam_best_gm_gi_garbage_clear_attack() {
        let board = vec![0u32; 40];
        let gmask = vec![0u32; 40];
        let pieces = [0u8, 0u8];
        let mut garbage_rows = vec![0u32; 2 * 4];
        for cell in garbage_rows.iter_mut().take(4) {
            *cell = 0x3FE;
        }
        let counts = [4u32, 0u32];
        let acc = beam_best_gm_gi_wasm(
            &board,
            &gmask,
            &pieces,
            -1,
            -1,
            0,
            &[],
            16,
            &[1.0, 1.0],
            &garbage_rows,
            &counts,
            4,
        );
        assert!(
            acc > 0.0,
            "injecting 4 garbage rows then clearing them should yield positive attack, got {acc}"
        );
    }

    #[test]
    fn test_compact_garbage_rows_matches_line_clear_compaction() {
        let mut gm = [0u64; 40];
        gm[0] = 0x03FF;
        gm[1] = 0x0200;
        gm[2] = 0x0100;

        let compacted = compact_garbage_rows(&gm, 1u64 << 0);

        assert_eq!(compacted[0], 0x0200);
        assert_eq!(compacted[1], 0x0100);
        assert_eq!(compacted[2], 0);
    }
}

// Coaching sequence simulation

#[wasm_bindgen(js_name = "simulate_coaching_sequence")]
pub fn simulate_coaching_sequence_wasm(board: &JsBoard, path: JsValue) -> JsValue {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let moves: Vec<MoveResultJson> = from_js(path)?;
        let mut current_board = board.inner.clone();
        let mut steps: Vec<CoachingStepJson> = Vec::new();
        let mut sim_b2b: u8 = 0;
        let mut sim_combo: u32 = 0;
        let attack_config = AttackConfig::tetra_league();

        for move_json in &moves {
            let piece = piece_from_external(move_json.piece)?;
            let rotation = Rotation::from_u8(move_json.rotation);
            let m = match move_json.spin {
                2 => Move::new(
                    piece,
                    rotation,
                    move_json.x as i32,
                    move_json.y as i32,
                    true,
                ),
                1 if piece == Piece::T => {
                    Move::new_tspin(rotation, move_json.x as i32, move_json.y as i32, false)
                }
                1 => {
                    Move::new_allspin_mini(piece, rotation, move_json.x as i32, move_json.y as i32)
                }
                _ => Move::new(
                    piece,
                    rotation,
                    move_json.x as i32,
                    move_json.y as i32,
                    false,
                ),
            };

            if current_board.obstructed_move(&m) {
                break;
            }

            let inputs = pathfinder::get_input(&current_board, &m, false, false);
            let input_data: Vec<u8> = inputs.data.iter().map(|i| *i as u8).collect();

            let mut clearing_rows: Vec<u8> = Vec::new();
            for row_idx in 0..40u8 {
                let row = current_board.rows[row_idx as usize];
                if row == 0x03FF {
                    clearing_rows.push(row_idx);
                }
            }

            let lines_cleared = current_board.do_move(&m) as u8;

            let clear_event = if lines_cleared > 0 {
                let spin_type = m.spin();
                let b2b_eligible = spin_type != SpinType::NoSpin || lines_cleared >= 4;
                let next_b2b = if b2b_eligible {
                    sim_b2b.saturating_add(1)
                } else {
                    0
                };
                let next_combo = sim_combo + 1;
                let b2b_broken_from = if sim_b2b >= 4 && next_b2b == 0 {
                    Some(sim_b2b)
                } else {
                    None
                };

                let attack_val = calculate_attack_full(&AttackContext {
                    lines: lines_cleared,
                    spin: spin_type,
                    b2b: sim_b2b,
                    combo: sim_combo.min(255) as u8,
                    config: &attack_config,
                    is_perfect_clear: false,
                    b2b_broken_from,
                    clears_garbage: false,
                });

                let event = ClearEventJson {
                    clear_type: ClearType::from_lines(lines_cleared).to_str().to_string(),
                    spin_type: spin_type_to_str(spin_type).to_string(),
                    lines_cleared,
                    attack_sent: attack_val,
                    b2b_before: sim_b2b,
                    b2b_after: next_b2b,
                    combo_before: sim_combo,
                    combo_after: next_combo,
                    is_surge_release: b2b_broken_from.is_some(),
                    is_garbage_clear: false,
                    is_perfect_clear: current_board.is_empty(),
                    piece: move_json.piece,
                };

                sim_b2b = next_b2b;
                sim_combo = next_combo;
                Some(event)
            } else {
                sim_combo = 0;
                None
            };

            steps.push(CoachingStepJson {
                piece: move_json.piece,
                rotation: move_json.rotation,
                x: move_json.x,
                y: move_json.y,
                inputs: input_data,
                board_after: current_board.rows.to_vec(),
                clearing_rows,
                clear_event,
            });
        }

        // Trim: keep min 5 steps, cut after last attack gain
        const MIN_COACHING_STEPS: usize = 5;
        if steps.len() > MIN_COACHING_STEPS {
            let mut last_attack_idx = 0usize;
            for (i, step) in steps.iter().enumerate() {
                if let Some(ref ce) = step.clear_event {
                    if ce.attack_sent > 0.0 {
                        last_attack_idx = i;
                    }
                }
            }
            let trim_to = (last_attack_idx + 2)
                .max(MIN_COACHING_STEPS)
                .min(steps.len());
            steps.truncate(trim_to);
        }

        Some(steps)
    }));

    match result {
        Ok(Some(steps)) => to_js(&steps),
        _ => JsValue::NULL,
    }
}

// Feature extraction for browser-side neural inference (onnxruntime-web)

#[derive(serde::Serialize)]
struct FeatureExtractionResultJson {
    features: Vec<f32>,
    candidate_features: Vec<f32>,
    candidate_mask: Vec<bool>,
    move_count: usize,
    moves: Vec<MoveResultJson>,
}

#[wasm_bindgen(js_name = "extract_features_for_position")]
pub fn extract_features_for_position_wasm(
    board: &JsBoard,
    piece: u8,
    frame: JsValue,
    opponent_board_js: Option<JsBoard>,
) -> JsValue {
    let board_clone = board.inner.clone();
    let board_for_gen = board.inner.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let p = piece_from_external(piece)?;
        let frame_context = from_js::<ReplayFrameContextJson>(frame);
        let opp_board = match &opponent_board_js {
            Some(opp) => opp.inner.clone(),
            None => board_from_external_rows(
                frame_context
                    .as_ref()
                    .and_then(|ctx| ctx.opponent_board.as_deref()),
            ),
        };
        let state = game_state_from_external_context(board_clone, p, frame_context.as_ref());

        let features = crate::policy_value_runtime::encode_state_features_flat(&state, &opp_board);

        let mut moves = MoveBuffer::new();
        generate(&board_for_gen, &mut moves, p, false);
        let candidates: Vec<crate::header::Move> = moves.as_slice().to_vec();
        let (candidate_features, candidate_mask) =
            crate::policy_value_runtime::encode_candidate_features_flat(&candidates);

        let move_descs: Vec<MoveResultJson> = candidates
            .iter()
            .map(|m| MoveResultJson {
                piece: piece_to_external(m.piece()),
                rotation: m.rotation() as u8,
                x: m.x() as i8,
                y: m.y() as i8,
                score: 0.0,
                spin: m.spin() as u8,
                hold_used: false,
            })
            .collect();

        Some(FeatureExtractionResultJson {
            features,
            candidate_features,
            candidate_mask,
            move_count: candidates.len(),
            moves: move_descs,
        })
    }));

    match result {
        Ok(Some(json)) => to_js(&json),
        _ => JsValue::NULL,
    }
}
