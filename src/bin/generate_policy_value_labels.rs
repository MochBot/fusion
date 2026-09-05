use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use fusion_engine::board::{Board, BOARD_HEIGHT};
use fusion_engine::eval::EvalWeights;
use fusion_engine::header::{Piece, COL_NB};
use fusion_engine::search::{search, SearchRequest};
use fusion_engine::search_config::SearchConfig;
use fusion_engine::state::GameState;
use serde::{Deserialize, Serialize};

const PHASE1_SCHEMA_VERSION: &str = "phase1-v1";
const GENERATION_MODE: &str = "search_oracle";
const ORACLE_PROFILE: &str = "stronger_offline_oracle";
const POLICY_TEMPERATURE: f32 = 1.0;
const ORACLE_BEAM_WIDTH: usize = 2000;
const ORACLE_DEPTH: usize = 18;

#[derive(Debug, Deserialize)]
struct PolicyValueOracleRequest {
    schema_version: String,
    replay_id: String,
    round_id: u32,
    player_id: u8,
    frame_id: u32,
    group_id: String,
    player_board_rows: Vec<u16>,
    opponent_board_rows: Vec<u16>,
    current_piece: String,
    hold_piece: Option<String>,
    queue: Vec<String>,
    combo: u32,
    b2b: u8,
    lines: u32,
    pending_garbage: u8,
    bag_number: u32,
}

#[derive(Debug, Serialize)]
struct PolicyValueTarget {
    schema_version: String,
    replay_id: String,
    round_id: u32,
    player_id: u8,
    frame_id: u32,
    group_id: String,
    best_move_raw: u16,
    best_value: f32,
    position_complexity: f32,
    root_scores: Vec<(u16, f32)>,
    policy_probs: Vec<f32>,
}

#[derive(Debug, Serialize)]
struct PolicyValueSkipped {
    skipped: bool,
    replay_id: String,
    round_id: u32,
    player_id: u8,
    frame_id: u32,
    group_id: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct PolicyValueMetadata {
    schema_version: String,
    generation_mode: String,
    policy_temperature: f32,
    sample_count: usize,
    skipped_count: usize,
    skip_failures_enabled: bool,
    move_id_contract: String,
    oracle_profile: String,
    oracle_beam_width: usize,
    oracle_depth: usize,
}

fn stronger_offline_oracle_config(time_budget_ms: Option<u64>) -> SearchConfig {
    SearchConfig {
        beam_width: ORACLE_BEAM_WIDTH,
        depth: ORACLE_DEPTH,
        quiescence_max_extensions: 5,
        quiescence_beam_fraction: 0.20,
        time_budget_ms,
        ..SearchConfig::default()
    }
}

fn parse_training_piece(name: &str) -> Result<Piece, String> {
    match name {
        "i" => Ok(Piece::I),
        "j" => Ok(Piece::J),
        "l" => Ok(Piece::L),
        "o" => Ok(Piece::O),
        "s" => Ok(Piece::S),
        "t" => Ok(Piece::T),
        "z" => Ok(Piece::Z),
        _ => Err(format!("unknown training piece: {name}")),
    }
}

fn board_from_rows(rows: &[u16]) -> Result<Board, String> {
    if rows.len() > BOARD_HEIGHT {
        return Err(format!("too many board rows: {}", rows.len()));
    }
    let mut board = Board::new();
    for (y, row) in rows.iter().enumerate() {
        board.rows[y] = *row;
    }
    for x in 0..COL_NB {
        let mut col = 0u64;
        for (y, row) in board.rows.iter().enumerate() {
            if ((row >> x) & 1) != 0 {
                col |= 1u64 << y;
            }
        }
        board.cols[x] = col;
    }
    Ok(board)
}

fn game_state_from_request(request: &PolicyValueOracleRequest) -> Result<GameState, String> {
    if request.schema_version != PHASE1_SCHEMA_VERSION {
        return Err(format!(
            "unexpected schema version: {}",
            request.schema_version
        ));
    }
    let board = board_from_rows(&request.player_board_rows)?;
    let current = parse_training_piece(&request.current_piece)?;
    let queue = request
        .queue
        .iter()
        .map(|piece| parse_training_piece(piece))
        .collect::<Result<Vec<_>, _>>()?;
    let mut state = GameState::new(board, current, queue);
    state.hold = match &request.hold_piece {
        Some(piece) => Some(parse_training_piece(piece)?),
        None => None,
    };
    state.combo = request.combo;
    state.b2b = request.b2b;
    state.pending_garbage = request.pending_garbage;
    Ok(state)
}

fn softmax(scores: &[f32], temperature: f32) -> Result<Vec<f32>, String> {
    if scores.is_empty() {
        return Err("cannot softmax empty score list".to_string());
    }
    if temperature <= 0.0 {
        return Err(format!("temperature must be positive, got {temperature}"));
    }
    let scaled: Vec<f32> = scores.iter().map(|score| score / temperature).collect();
    let max_score = scaled.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = scaled
        .iter()
        .map(|score| (*score - max_score).exp())
        .collect();
    let total: f32 = exps.iter().sum();
    if total <= 0.0 {
        return Err("softmax total must be positive".to_string());
    }
    Ok(exps.into_iter().map(|value| value / total).collect())
}

fn build_target(
    request: PolicyValueOracleRequest,
    time_budget_ms: Option<u64>,
) -> Result<PolicyValueTarget, String> {
    let _ = request.lines;
    let _ = request.bag_number;
    let _ = request.opponent_board_rows.len();
    let state = game_state_from_request(&request)?;
    let config = stronger_offline_oracle_config(time_budget_ms);
    let started = Instant::now();
    let weights = EvalWeights::default();
    let search_result = search(
        &state,
        &SearchRequest {
            config: &config,
            weights: &weights,
            runtime: None,
            forced_root_move: None,
        },
    );
    let elapsed_ms = started.elapsed().as_millis() as u64;
    if let Some(budget) = time_budget_ms {
        if elapsed_ms >= budget {
            return Err(format!(
                "oracle search hit time budget {budget}ms (elapsed {elapsed_ms}ms) for {}:{}; treating as unlabelable",
                request.replay_id, request.frame_id
            ));
        }
    }
    let result = search_result.ok_or_else(|| {
        format!(
            "search produced no result for {}:{}",
            request.replay_id, request.frame_id
        )
    })?;

    let root_scores: Vec<(u16, f32)> = result
        .root_scores
        .iter()
        .map(|(mv, score)| (mv.raw(), *score))
        .collect();
    if root_scores.is_empty() {
        return Err(format!(
            "search returned empty root_scores for {}:{}",
            request.replay_id, request.frame_id
        ));
    }
    let policy_probs = softmax(
        &root_scores
            .iter()
            .map(|(_, score)| *score)
            .collect::<Vec<_>>(),
        POLICY_TEMPERATURE,
    )?;
    Ok(PolicyValueTarget {
        schema_version: PHASE1_SCHEMA_VERSION.to_string(),
        replay_id: request.replay_id,
        round_id: request.round_id,
        player_id: request.player_id,
        frame_id: request.frame_id,
        group_id: request.group_id,
        best_move_raw: result.best.best_move.raw(),
        best_value: result.best.score,
        position_complexity: result.position_complexity,
        root_scores,
        policy_probs,
    })
}

fn metadata(
    sample_count: usize,
    skipped_count: usize,
    skip_failures_enabled: bool,
) -> PolicyValueMetadata {
    PolicyValueMetadata {
        schema_version: PHASE1_SCHEMA_VERSION.to_string(),
        generation_mode: GENERATION_MODE.to_string(),
        policy_temperature: POLICY_TEMPERATURE,
        sample_count,
        skipped_count,
        skip_failures_enabled,
        move_id_contract: "Move.raw".to_string(),
        oracle_profile: ORACLE_PROFILE.to_string(),
        oracle_beam_width: ORACLE_BEAM_WIDTH,
        oracle_depth: ORACLE_DEPTH,
    }
}

fn parse_time_budget(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|err| format!("invalid --time-budget-ms value '{value}': {err}"))
}

fn default_output_path(input_path: &Path) -> PathBuf {
    let input = input_path.to_string_lossy();
    if let Some(prefix) = input.strip_suffix(".policy_value.requests.jsonl") {
        PathBuf::from(format!("{prefix}.policy_value.jsonl"))
    } else {
        PathBuf::from(format!("{}.policy_value.jsonl", input))
    }
}

fn metadata_output_path(output_path: &Path) -> PathBuf {
    let output = output_path.to_string_lossy();
    if let Some(prefix) = output.strip_suffix(".policy_value.jsonl") {
        PathBuf::from(format!("{prefix}.policy_value.metadata.json"))
    } else {
        PathBuf::from(format!("{}.policy_value.metadata.json", output))
    }
}

fn run() -> Result<(), String> {
    let raw_args: Vec<String> = env::args().collect();
    let mut skip_failures = false;
    let mut time_budget_ms: Option<u64> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut idx = 1usize;
    while idx < raw_args.len() {
        let arg = raw_args[idx].as_str();
        match arg {
            "--skip-failures" => skip_failures = true,
            "--time-budget-ms" => {
                idx += 1;
                let value = raw_args.get(idx).ok_or_else(|| {
                    "--time-budget-ms requires a value (milliseconds)".to_string()
                })?;
                time_budget_ms = Some(parse_time_budget(value)?);
            }
            "--help" | "-h" => {
                println!("usage: generate_policy_value_labels [--skip-failures] [--time-budget-ms <N>] <requests.jsonl> [output.jsonl]");
                println!("  --skip-failures      Write skip-marker JSON lines for samples the search cannot label,");
                println!("                       preserving 1:1 input/output line correspondence.  Without this flag,");
                println!("                       the binary exits with code 1 on the first unlabelable sample.");
                println!("  --time-budget-ms <N> Per-position wall-clock budget. Positions that hit the budget are");
                println!("                       treated as unlabelable (skip-marker under --skip-failures) rather than");
                println!("                       emitting a degraded/truncated oracle label.");
                return Ok(());
            }
            other if other.starts_with("--time-budget-ms=") => {
                let value = &other["--time-budget-ms=".len()..];
                time_budget_ms = Some(parse_time_budget(value)?);
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag: {other}"));
            }
            other => positional.push(other.to_string()),
        }
        idx += 1;
    }
    if positional.is_empty() {
        return Err(
            "usage: generate_policy_value_labels [--skip-failures] <requests.jsonl> [output.jsonl]"
                .to_string(),
        );
    }

    let input_path = PathBuf::from(&positional[0]);
    let output_path = if positional.len() >= 2 {
        PathBuf::from(&positional[1])
    } else {
        default_output_path(&input_path)
    };
    let metadata_path = metadata_output_path(&output_path);

    let input =
        File::open(&input_path).map_err(|err| format!("open {}: {err}", input_path.display()))?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("mkdir {}: {err}", parent.display()))?;
    }
    let mut writer = BufWriter::new(
        File::create(&output_path)
            .map_err(|err| format!("create {}: {err}", output_path.display()))?,
    );

    let mut count = 0usize;
    let mut skipped = 0usize;
    for (line_idx, line) in BufReader::new(input).lines().enumerate() {
        let line = line.map_err(|err| format!("read line: {err}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let request: PolicyValueOracleRequest = serde_json::from_str(&line)
            .map_err(|err| format!("parse request json at line {line_idx}: {err}"))?;
        let replay_id = request.replay_id.clone();
        let round_id = request.round_id;
        let player_id = request.player_id;
        let frame_id = request.frame_id;
        let group_id = request.group_id.clone();
        match build_target(request, time_budget_ms) {
            Ok(target) => {
                serde_json::to_writer(&mut writer, &target)
                    .map_err(|err| format!("write target json: {err}"))?;
                writer
                    .write_all(b"\n")
                    .map_err(|err| format!("write newline: {err}"))?;
                count += 1;
            }
            Err(err) if skip_failures => {
                let marker = PolicyValueSkipped {
                    skipped: true,
                    replay_id: replay_id.clone(),
                    round_id,
                    player_id,
                    frame_id,
                    group_id,
                    error: err.clone(),
                };
                serde_json::to_writer(&mut writer, &marker)
                    .map_err(|werr| format!("write skip marker: {werr}"))?;
                writer
                    .write_all(b"\n")
                    .map_err(|werr| format!("write newline: {werr}"))?;
                eprintln!("skip {replay_id}:{frame_id} - {err}");
                skipped += 1;
            }
            Err(err) => return Err(err),
        }
    }
    writer
        .flush()
        .map_err(|err| format!("flush {}: {err}", output_path.display()))?;

    let metadata_file = File::create(&metadata_path)
        .map_err(|err| format!("create {}: {err}", metadata_path.display()))?;
    serde_json::to_writer_pretty(metadata_file, &metadata(count, skipped, skip_failures))
        .map_err(|err| format!("write metadata {}: {err}", metadata_path.display()))?;

    println!("generated_labels={count}");
    println!("skipped_labels={skipped}");
    println!("output_path={}", output_path.display());
    println!("metadata_path={}", metadata_path.display());
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softmax_sums_to_one() {
        let probs = softmax(&[1.0, 2.0, 3.0], 1.0).expect("softmax should work");
        let total: f32 = probs.iter().sum();
        assert!((total - 1.0).abs() < 1e-6);
        assert!(probs[2] > probs[1]);
        assert!(probs[1] > probs[0]);
    }

    #[test]
    fn parse_training_piece_uses_phase1_piece_names() {
        assert_eq!(parse_training_piece("i").expect("piece"), Piece::I);
        assert_eq!(parse_training_piece("j").expect("piece"), Piece::J);
        assert!(parse_training_piece("q").is_err());
    }

    fn sample_request() -> PolicyValueOracleRequest {
        PolicyValueOracleRequest {
            schema_version: PHASE1_SCHEMA_VERSION.to_string(),
            replay_id: "test-replay".to_string(),
            round_id: 0,
            player_id: 0,
            frame_id: 0,
            group_id: "test-group".to_string(),
            player_board_rows: Vec::new(),
            opponent_board_rows: Vec::new(),
            current_piece: "i".to_string(),
            hold_piece: None,
            queue: vec![
                "o".to_string(),
                "t".to_string(),
                "l".to_string(),
                "j".to_string(),
                "s".to_string(),
                "z".to_string(),
            ],
            combo: 0,
            b2b: 0,
            lines: 0,
            pending_garbage: 0,
            bag_number: 0,
        }
    }

    #[test]
    fn build_target_without_budget_returns_full_label() {
        let target = build_target(sample_request(), None).expect("full search should label");
        assert!(!target.policy_probs.is_empty());
        assert!(!target.root_scores.is_empty());
    }

    #[test]
    fn build_target_cut_short_by_budget_is_unlabelable() {
        // Cut-short searches must error (become skip-markers), never emit degraded labels.
        let err = build_target(sample_request(), Some(0))
            .expect_err("zero budget must be treated as unlabelable, not a truncated label");
        assert!(
            err.to_lowercase().contains("budget"),
            "error should mention the time budget, got: {err}"
        );
    }
}
