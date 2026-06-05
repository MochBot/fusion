// wasm.rs -- WASM bridge for Mosaic SvelteKit frontend
// Feature-gated behind `wasm` feature. This IS the Fusion V2 engine (Cobra port,
// TL Season-2 attack). The boundary only re-numbers piece IDs to the external /
// Triangle order (I0 O1 T2 S3 Z4 J5 L6) that the JS layer uses; "v1" historically
// named that ID convention, not the engine version.

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

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
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

    #[wasm_bindgen(getter, js_name = "pcGarbage")]
    pub fn pc_garbage(&self) -> u8 {
        self.inner.pc_garbage
    }

    #[wasm_bindgen(getter, js_name = "garbageMultiplier")]
    pub fn garbage_multiplier(&self) -> f32 {
        self.inner.garbage_multiplier
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
    let weights = EvalWeights {
        height,
        holes,
        bumpiness,
        well_depth: wells,
        ..Default::default()
    };
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
            time_budget_ms: None, // Presim coaching — no time limit, full beam search
            ..SearchConfig::default()
        };
        // PC skip: Perfect Clears are unrealistic coaching advice — zero out PC bonuses
        // so the engine doesn't inflate eval scores or recommend PC paths
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

        // Identify actual move BEFORE search so we can force it into the beam
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

        // Run search with forced root move — ensures player's actual move stays in beam
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

                // Look up actual move score in root_scores
                let actual_search_score = actual_move_raw.and_then(|raw| {
                    full.root_scores
                        .iter()
                        .find(|(m, _)| m.raw() == raw)
                        .map(|(_, s)| *s)
                });

                let (loss, sev) = if let Some(actual_score) = actual_search_score {
                    let raw_loss = (best_search_score - actual_score).max(0.0);

                    // Apply coaching state multiplier to amplify ΔP
                    let dp_mul = coaching_dp_multiplier(&coaching_after);
                    let amplified_actual = best_search_score - raw_loss * dp_mul;

                    // Build skill-adaptive sigmoid params from player stats
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
                    // Actual move not in root_scores — can't classify quality
                    (0.0, analysis::Severity::None)
                };

                // Convert principal variation to recommended path for coaching
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

        // Run MVP insight detectors
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

// ---------------------------------------------------------------------------
// Batched expansion for offline search/labeling. One call returns, for every
// legal placement of `piece` given the current signed Triangle chain state
// (b2b, combo, pending_garbage), a fixed 46-float record =
//   [attack, lines, b2b_after, combo_after, pending_after, spin, rows[0..40]].
// Collapses ~34 per-candidate WASM calls into one copy.
// ---------------------------------------------------------------------------

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

#[wasm_bindgen(js_name = "expand_all_g")]
pub fn expand_all_g_wasm(
    board: &JsBoard,
    piece: u8,
    b2b: i32,
    combo: i32,
    pending_garbage: u32,
    garbage_rows: &[u64],
) -> Vec<f64> {
    expand_all_with_garbage_rows(
        board,
        piece,
        b2b,
        combo,
        pending_garbage,
        Some(garbage_rows),
        1.0,
    )
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
    for m in moves.as_slice() {
        let mut nb = board.inner.clone();
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
    out
}

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

// Batched frontier expansion for beam search: expands `n` boards (each 40 u64
// rows + 40 u64 garbage rows + [b2b, combo, pending] i32 state) for one `piece`
// in a single call. Output = n move-counts (f64) followed by concatenated
// 46-float records in board order. Per-move math is identical to expand_all_g.
#[wasm_bindgen(js_name = "expand_beam_g")]
pub fn expand_beam_g_wasm(
    boards: &[u64],
    garbage_rows: &[u64],
    states: &[i32],
    piece: u8,
    n: u32,
) -> Vec<f64> {
    expand_beam_gm_wasm(boards, garbage_rows, states, piece, n, 1.0)
}

#[wasm_bindgen(js_name = "expand_beam_gm")]
pub fn expand_beam_gm_wasm(
    boards: &[u64],
    garbage_rows: &[u64],
    states: &[i32],
    piece: u8,
    n: u32,
    garbage_multiplier: f64,
) -> Vec<f64> {
    let n = n as usize;
    if boards.len() < n.saturating_mul(40)
        || garbage_rows.len() < n.saturating_mul(40)
        || states.len() < n.saturating_mul(3)
    {
        return Vec::new();
    }
    let p = match piece_from_external(piece) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let mut counts: Vec<f64> = Vec::with_capacity(n);
    let mut recs: Vec<f64> = Vec::new();
    for i in 0..n {
        let off = i * 40;
        let inner = crate::wasm_board::board_from_row_bitmasks(&boards[off..off + 40]);
        let gslice = &garbage_rows[off..off + 40];
        let b2b = states[i * 3];
        let combo = states[i * 3 + 1];
        let pending = states[i * 3 + 2].max(0) as u32;
        let mut moves = MoveBuffer::new();
        generate_playable(&inner, &mut moves, p, false);
        let slice = moves.as_slice();
        for m in slice {
            let mut nb = inner.clone();
            nb.place(m);
            let cleared = nb.line_clears();
            let lines = cleared.count_ones() as u8;
            if cleared != 0 {
                nb.clear_lines(cleared);
            }
            let spin = m.spin();
            let next_pending = pending.saturating_sub(lines as u32);
            let garbage_cleared = count_cleared_garbage_rows(cleared, gslice);
            let attack = calculate_attack_s2_tl_with_multiplier(
                lines,
                spin,
                b2b,
                combo,
                nb.is_empty(),
                garbage_cleared,
                garbage_multiplier,
            );
            recs.push(attack.attack as f64);
            recs.push(lines as f64);
            recs.push(attack.b2b_after as f64);
            recs.push(attack.combo_after as f64);
            recs.push(next_pending as f64);
            recs.push(spin as u8 as f64);
            for y in 0..40 {
                recs.push(nb.rows[y] as f64);
            }
        }
        counts.push(slice.len() as f64);
    }
    let mut out: Vec<f64> = Vec::with_capacity(n + recs.len());
    out.extend(counts);
    out.extend(recs);
    out
}

// Full K-deep beam search in Rust, returning only the best achievable attack.
// Replicates the offline TS beam exactly so labels are unchanged: stable
// descending sort by integer accumulated attack, dedup by resulting board, keep
// top `beam_width` survivors, with optional force-keep of the player's actual
// line (`keep_line` = k*40 rows, empty to disable) so the value label is never
// under-estimated. Boards are u32 row bitmasks (<=10 bits) to avoid BigInt on
// the JS side; only one f64 crosses back per call.
#[wasm_bindgen(js_name = "beam_best_g")]
pub fn beam_best_g_wasm(
    start_board: &[u32],
    start_gmask: &[u32],
    pieces: &[u8],
    b2b: i32,
    combo: i32,
    pending: i32,
    keep_line: &[u32],
    beam_width: u32,
) -> f64 {
    beam_best_gm_wasm(
        start_board,
        start_gmask,
        pieces,
        b2b,
        combo,
        pending,
        keep_line,
        beam_width,
        1.0,
    )
}

// Beam-kernel perf helpers. The per-node garbage state is a u64 row-bitmask
// (bit y = row y still holds >=1 garbage cell) instead of a [u64;40] cell mask:
// a garbage row only loses cells via a full-row clear (which deletes the whole
// row), so the per-row predicate is exactly preserved. This shrinks each beam
// child from ~480B to ~168B, the dominant cost of beam expansion.
#[derive(Default)]
struct FxHasher64 {
    h: u64,
}
impl std::hash::Hasher for FxHasher64 {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        while bytes.len() >= 8 {
            let v = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            self.h = (self.h.rotate_left(5) ^ v).wrapping_mul(K);
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut b = [0u8; 8];
            b[..bytes.len()].copy_from_slice(bytes);
            self.h = (self.h.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(K);
        }
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.h
    }
}
type FxRowSet = std::collections::HashSet<[u16; 40], std::hash::BuildHasherDefault<FxHasher64>>;

// Drop the bits of `gm` at cleared row positions and shift higher bits down,
// matching how `clear_lines` compacts the board (software pext on a single u64).
#[inline]
fn compact_gm_bits(gm: u64, cleared: u64) -> u64 {
    if cleared == 0 {
        return gm;
    }
    let mut out = 0u64;
    let mut w = 0u32;
    let mut k = !cleared;
    while k != 0 {
        let y = k.trailing_zeros();
        if gm & (1u64 << y) != 0 {
            out |= 1u64 << w;
        }
        w += 1;
        k &= k - 1;
    }
    out
}

// Bitmask of rows that still contain at least one cell (gm bits for emptied rows
// must be dropped, mirroring the per-cell `gm[y] &= rows[y]` step).
#[inline]
fn nonempty_row_mask(board: &crate::board::Board) -> u64 {
    let mut ne = 0u64;
    for y in 0..40 {
        if board.rows[y] != 0 {
            ne |= 1u64 << y;
        }
    }
    ne
}

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

/// Surge-potential-shaped variant of `beam_best_gm`: each placement's value is
/// its realized attack plus the change in banked surge potential
/// (`surge_potential(b2b_after) - surge_potential(b2b_before)`). This makes
/// building B2B count toward the coaching gap and cashing surge out neutral.
/// Dedups by (rows, b2b, combo) — shaping makes b2b/combo affect value — to
/// match the JS `s2BestLine` beam it parity-anchors.
#[wasm_bindgen(js_name = "beam_best_gm_surge")]
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
    if start_board.len() < 40 || start_gmask.len() < 40 {
        return 0.0;
    }
    let bw = beam_width as usize;
    let k = pieces.len();
    let use_keep = keep_line.len() >= k * 40;

    let mut rows0 = [0u64; 40];
    for y in 0..40 {
        rows0[y] = start_board[y] as u64;
    }
    let mut beam: Vec<BNode> = vec![BNode {
        board: crate::wasm_board::board_from_row_bitmasks(&rows0),
        gm: gm_bits_from_mask(start_gmask),
        acc: 0,
        b2b,
        combo,
        pending,
    }];

    for t in 0..k {
        let p = match piece_from_external(pieces[t]) {
            Some(p) => p,
            None => break,
        };
        let mut children: Vec<Child> = Vec::with_capacity(beam.len().saturating_mul(40));
        for node in &beam {
            let mut moves = MoveBuffer::new();
            generate_playable(&node.board, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut nb = node.board.clone();
                nb.place(m);
                let cleared = nb.line_clears();
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    nb.clear_lines(cleared);
                }
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
                let mut child_gm = compact_gm_bits(node.gm, cleared);
                if child_gm != 0 {
                    child_gm &= nonempty_row_mask(&nb);
                }
                let shaped_delta = if surge_shaping {
                    crate::attack::surge_potential(attack.b2b_after as i32, garbage_multiplier)
                        - crate::attack::surge_potential(node.b2b, garbage_multiplier)
                } else {
                    0
                };
                children.push(Child {
                    board: nb,
                    acc: node.acc + attack.attack as i64 + shaped_delta,
                    b2b: attack.b2b_after as i32,
                    combo: attack.combo_after as i32,
                    pending: (node.pending - lines as i32).max(0),
                    gm: child_gm,
                });
            }
        }
        if children.is_empty() {
            break;
        }
        let mut idx: Vec<usize> = (0..children.len()).collect();
        idx.sort_by(|&a, &b| children[b].acc.cmp(&children[a].acc));
        let keepb: Option<[u16; 40]> = if use_keep {
            let mut kb = [0u16; 40];
            for y in 0..40 {
                kb[y] = keep_line[t * 40 + y] as u16;
            }
            Some(kb)
        } else {
            None
        };
        let mut seen: FxRowSet =
            FxRowSet::with_capacity_and_hasher(children.len(), Default::default());
        let mut seen_full: std::collections::HashSet<([u16; 40], i32, i32)> =
            std::collections::HashSet::with_capacity(if surge_shaping {
                children.len()
            } else {
                0
            });
        let mut pruned: Vec<BNode> = Vec::with_capacity(bw);
        let mut kept = false;
        for &ci in &idx {
            let c = &children[ci];
            let rows = c.board.rows;
            let is_dup = if surge_shaping {
                !seen_full.insert((rows, c.b2b, c.combo))
            } else {
                !seen.insert(rows)
            };
            if is_dup {
                continue;
            }
            if Some(rows) == keepb {
                kept = true;
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
        if let Some(kb) = keepb {
            if !kept {
                for &ci in &idx {
                    let c = &children[ci];
                    let rows = c.board.rows;
                    if rows == kb {
                        pruned.push(BNode {
                            board: c.board.clone(),
                            gm: c.gm,
                            acc: c.acc,
                            b2b: c.b2b,
                            combo: c.combo,
                            pending: c.pending,
                        });
                        break;
                    }
                }
            }
        }
        beam = pruned;
    }
    let mut mx: i64 = 0;
    for node in &beam {
        if node.acc > mx {
            mx = node.acc;
        }
    }
    mx as f64
}

/// Insert `garbage` rows at the bottom of a row/gmask pair, shifting the
/// existing stack up. `garbage[0]` is the bottom-most inserted row. Cells
/// pushed above row 39 are dropped. Returns the new (rows, gmask). All cells
/// of an inserted garbage row are flagged as garbage in the returned mask.
fn apply_garbage_insert(
    rows: &[u64; 40],
    gm: &[u64; 40],
    garbage: &[u64],
) -> ([u64; 40], [u64; 40]) {
    let n = garbage.len().min(40);
    let mut nr = [0u64; 40];
    let mut ng = [0u64; 40];
    for y in n..40 {
        nr[y] = rows[y - n];
        ng[y] = gm[y - n];
    }
    for (y, &g) in garbage.iter().take(n).enumerate() {
        let r = g & (crate::board::FULL_ROW as u64);
        nr[y] = r;
        ng[y] = r;
    }
    (nr, ng)
}

/// Garbage-injecting variant of `beam_best_gm`. After placing+clearing the
/// piece at step t, inserts `garbage_counts[t]` garbage rows (taken from
/// `garbage_rows[t*max_gi ..]`, bottom-most first) into every child board
/// BEFORE the keep comparison, so the player's real (garbage-laden) line in
/// `keep_line` stays reachable and `best >= playerAtk` holds on garbage
/// windows. With all-zero `garbage_counts` it is identical to `beam_best_gm`.
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
    if start_board.len() < 40 || start_gmask.len() < 40 {
        return 0.0;
    }
    let bw = beam_width as usize;
    let k = pieces.len();
    let use_keep = keep_line.len() >= k * 40;
    let stride = max_gi as usize;

    let mut rows0 = [0u64; 40];
    for y in 0..40 {
        rows0[y] = start_board[y] as u64;
    }
    let mut beam: Vec<BNode> = vec![BNode {
        board: crate::wasm_board::board_from_row_bitmasks(&rows0),
        gm: gm_bits_from_mask(start_gmask),
        acc: 0,
        b2b,
        combo,
        pending,
    }];

    for t in 0..k {
        let p = match piece_from_external(pieces[t]) {
            Some(p) => p,
            None => break,
        };
        let mult = multipliers.get(t).copied().unwrap_or(1.0);
        let gc = garbage_counts.get(t).copied().unwrap_or(0) as usize;
        let gc = gc.min(stride);
        let mut garbage = [0u64; 40];
        for i in 0..gc {
            garbage[i] = garbage_rows.get(t * stride + i).copied().unwrap_or(0) as u64;
        }
        // Bit i set iff inserted garbage row i actually has cells (marks it garbage).
        let inserted_bits: u64 = {
            let mut b = 0u64;
            for i in 0..gc {
                if garbage[i] & 0x3FF != 0 {
                    b |= 1u64 << i;
                }
            }
            b
        };
        let mut children: Vec<Child> = Vec::with_capacity(beam.len().saturating_mul(40));
        for node in &beam {
            let mut moves = MoveBuffer::new();
            generate_playable(&node.board, &mut moves, p, false);
            for m in moves.as_slice() {
                let mut nb = node.board.clone();
                nb.place(m);
                let cleared = nb.line_clears();
                let lines = cleared.count_ones() as u8;
                if cleared != 0 {
                    nb.clear_lines(cleared);
                }
                let spin = m.spin();
                let garbage_cleared = (cleared & node.gm).count_ones() as u8;
                let attack = calculate_attack_s2_tl_with_multiplier(
                    lines,
                    spin,
                    node.b2b,
                    node.combo,
                    nb.is_empty(),
                    garbage_cleared,
                    mult,
                );
                let mut child_gm = compact_gm_bits(node.gm, cleared);
                if child_gm != 0 {
                    child_gm &= nonempty_row_mask(&nb);
                }
                let (cboard, cgm) = if gc > 0 {
                    let mut nr = [0u64; 40];
                    for y in gc..40 {
                        nr[y] = nb.rows[y - gc] as u64;
                    }
                    for i in 0..gc {
                        nr[i] = garbage[i] & 0x3FF;
                    }
                    let ngm = ((child_gm << gc) | inserted_bits) & ((1u64 << 40) - 1);
                    (crate::wasm_board::board_from_row_bitmasks(&nr), ngm)
                } else {
                    (nb, child_gm)
                };
                children.push(Child {
                    board: cboard,
                    acc: node.acc + attack.attack as i64,
                    b2b: attack.b2b_after as i32,
                    combo: attack.combo_after as i32,
                    pending: (node.pending - lines as i32).max(0),
                    gm: cgm,
                });
            }
        }
        if children.is_empty() {
            break;
        }
        let mut idx: Vec<usize> = (0..children.len()).collect();
        idx.sort_by(|&a, &b| children[b].acc.cmp(&children[a].acc));
        let keepb: Option<[u16; 40]> = if use_keep {
            let mut kb = [0u16; 40];
            for y in 0..40 {
                kb[y] = keep_line[t * 40 + y] as u16;
            }
            Some(kb)
        } else {
            None
        };
        let mut seen: FxRowSet =
            FxRowSet::with_capacity_and_hasher(children.len(), Default::default());
        let mut pruned: Vec<BNode> = Vec::with_capacity(bw);
        let mut kept = false;
        for &ci in &idx {
            let c = &children[ci];
            let rows = c.board.rows;
            if !seen.insert(rows) {
                continue;
            }
            if Some(rows) == keepb {
                kept = true;
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
        if let Some(kb) = keepb {
            if !kept {
                for &ci in &idx {
                    let c = &children[ci];
                    let rows = c.board.rows;
                    if rows == kb {
                        pruned.push(BNode {
                            board: c.board.clone(),
                            gm: c.gm,
                            acc: c.acc,
                            b2b: c.b2b,
                            combo: c.combo,
                            pending: c.pending,
                        });
                        break;
                    }
                }
            }
        }
        beam = pruned;
    }
    let mut mx: i64 = 0;
    for node in &beam {
        if node.acc > mx {
            mx = node.acc;
        }
    }
    mx as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_all_g_counts_cleared_garbage_rows() {
        let mut rows = [0u64; 40];
        let mut garbage_rows = [0u64; 40];
        for y in 0..4 {
            rows[y] = 0x03FF & !(1u64 << 9);
            garbage_rows[y] = rows[y];
        }

        let board = JsBoard::from_rows(&rows);
        let flat = expand_all_g_wasm(&board, 0, -1, -1, 0, &garbage_rows);

        assert!(
            flat.chunks_exact(EXPAND_REC)
                .any(|rec| rec[0] == 5.0 && rec[1] == 4.0 && rec[4] == 0.0),
            "expected a four-line I clear with exact garbage special bonus"
        );
    }

    #[test]
    fn test_expand_all_gm_applies_dynamic_multiplier() {
        let mut rows = [0u64; 40];
        let garbage_rows = [0u64; 40];
        rows[0] = 0x03FF & !0b1111u64;

        let board = JsBoard::from_rows(&rows);
        let flat = expand_all_gm_wasm(&board, 0, -1, 4, 0, &garbage_rows, 1.027);

        assert!(
            flat.chunks_exact(EXPAND_REC)
                .any(|rec| rec[0] == 2.0 && rec[1] == 1.0),
            "expected dynamic multiplier to lift combo single from 1 to 2 attack"
        );
    }

    #[test]
    fn test_beam_surge_credits_b2b_build() {
        // Tetris well (col 9 open, rows 0..3 = cols 0-8), b2b=6 charged. The only
        // clear is the I quad in col 9 -> b2b 6->7 (build): shaped value adds the
        // marginal surge potential over the realized attack.
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
        // Single-clear well (row 0 = cols 0-8, col 9 open), b2b=6. The best raw
        // line breaks the surge for ~6 realized; shaped, cashing it is neutral
        // because banked potential drops by the same amount.
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
    fn test_expand_beam_gm_rejects_short_inputs() {
        assert!(expand_beam_gm_wasm(&[], &[], &[], 0, 1, 1.0).is_empty());
    }

    #[test]
    fn test_beam_best_gm_rejects_short_inputs() {
        assert_eq!(
            beam_best_gm_wasm(&[], &[], &[0], -1, -1, 0, &[], 1, 1.0),
            0.0
        );
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

// ---------------------------------------------------------------------------
// Coaching sequence simulation
// ---------------------------------------------------------------------------

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

            // Detect clearing rows BEFORE do_move mutates the board
            let mut clearing_rows: Vec<u8> = Vec::new();
            for row_idx in 0..40u8 {
                let row = current_board.rows[row_idx as usize];
                if row == 0x03FF {
                    clearing_rows.push(row_idx);
                }
            }

            let lines_cleared = current_board.do_move(&m) as u8;

            // Compute per-step attack tracking
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

        // Trim fodder moves: keep min 5 steps, cut off after last attack gain
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
            // Keep up to 1 step after the last productive clear (setup move),
            // but always keep at least MIN_COACHING_STEPS
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

// ---------------------------------------------------------------------------
// Feature extraction for browser-side neural inference (onnxruntime-web)
// ---------------------------------------------------------------------------

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
