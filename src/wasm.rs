// wasm.rs -- WASM bridge for Mosaic SvelteKit frontend
// Feature-gated behind `wasm` feature. Exposes Fusion v1-compatible API.

use wasm_bindgen::prelude::*;

use crate::analysis::{self, coaching_dp_multiplier};
use crate::attack::{self, calculate_attack_full, AttackConfig, AttackContext, ComboTable};
use crate::eval::{self, evaluate, EvalWeights};
use crate::header::*;
use crate::move_buffer::MoveBuffer;
use crate::movegen::generate;
use crate::search::{find_best_move, find_best_move_with_scores_forced, SearchConfig};
use crate::state::{ClearType, GameState, TransitionObservation};
use crate::wasm_board::JsBoard;
use crate::pathfinder;
use crate::wasm_types::*;

#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
}

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
        let state = game_state_from_external_context(
            pre_board_clone,
            p,
            frame_context.as_ref().and_then(|ctx| ctx.queue.as_deref()),
            frame_context.as_ref().and_then(|ctx| ctx.hold),
        );

        let weights = EvalWeights::default();
        let mut config = SearchConfig {
            time_budget_ms: None, // Presim coaching — no time limit, full beam search
            ..SearchConfig::default()
        };
        // PCs are unrealistic coaching advice, zero the bonus so it doesn't skew eval
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
                let recommended_path: Vec<MoveResultJson> = sr.pv.iter().map(|m| {
                    MoveResultJson {
                        piece: piece_to_external(m.piece()),
                        rotation: m.rotation() as u8,
                        x: m.x() as i8,
                        y: m.y() as i8,
                        score: 0.0,
                        spin: m.spin() as u8,
                        hold_used: false,
                    }
                }).collect();

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
        let state = game_state_from_external_context(
            board_clone,
            p,
            frame_context.as_ref().and_then(|ctx| ctx.queue.as_deref()),
            frame_context.as_ref().and_then(|ctx| ctx.hold),
        );

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
    crate::movegen::generate(&board.inner, &mut moves, p, false);

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
                2 => Move::new(piece, rotation, move_json.x as i32, move_json.y as i32, true),
                1 if piece == Piece::T => Move::new_tspin(rotation, move_json.x as i32, move_json.y as i32, false),
                1 => Move::new_allspin_mini(piece, rotation, move_json.x as i32, move_json.y as i32),
                _ => Move::new(piece, rotation, move_json.x as i32, move_json.y as i32, false),
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
                let next_b2b = if b2b_eligible { sim_b2b.saturating_add(1) } else { 0 };
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
            let mut cumulative_attack = 0.0f32;
            for (i, step) in steps.iter().enumerate() {
                if let Some(ref ce) = step.clear_event {
                    if ce.attack_sent > 0.0 {
                        cumulative_attack += ce.attack_sent;
                        last_attack_idx = i;
                    }
                }
            }
            // Keep up to 1 step after the last productive clear (setup move),
            // but always keep at least MIN_COACHING_STEPS
            let trim_to = (last_attack_idx + 2).max(MIN_COACHING_STEPS).min(steps.len());
            steps.truncate(trim_to);
        }

        Some(steps)
    }));

    match result {
        Ok(Some(steps)) => to_js(&steps),
        _ => JsValue::NULL,
    }
}
