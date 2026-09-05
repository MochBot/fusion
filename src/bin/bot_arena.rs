use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use fusion_engine::eval::EvalWeights;
use fusion_engine::policy_value_runtime::PolicyValueRuntime;
use fusion_engine::search_config::{NnBatchMode, NnScoringMode};
use fusion_engine::versus::report::{
    write_experiment_manifest_json, write_manifest_json, write_profile_jsonl, write_replays_jsonl,
    write_summary_json, ManifestExperiment, ManifestInput, ManifestModel, RecordedGame, ReportMode,
};
use fusion_engine::versus::{
    baseline_player_cfg, baseline_spec, play_game_profiled, play_game_with_stream_index,
    EngineMode, GameSeeds,
};

// allow: SIZE_OK — P0 pins this binary as a single manual-arg-loop CLI with pairing, reports, and probes.

const USAGE: &str = "bot_arena [--experiment] [--games N=200] [--base-seed S=20260709] [--budget-ms X|none=500] [--heuristic] [--profile] [--model PATH=models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json] [--model-sha256 HEX] [--metadata-sha256 HEX] [--{a,b}-engine model|heuristic] [--{a,b}-nn-scoring per-child-value|policy-proxy] [--{a,b}-batch scalar|level] [--{a,b}-proxy-weight F] [--a-beam N=800] [--a-depth N=14] [--b-beam N=800] [--b-depth N=14] [--piece-cap N=1000] [--engine-rev STR=unknown] [--out DIR=target/bot_arena_out]";

const FROZEN_MODEL_SHA256: &str =
    "e2a6219be4c9646858c7e42d45d4755bb56b82a394095e15a16e4fdc28b74557";
const FROZEN_METADATA_SHA256: &str =
    "86bbeff18f34b1d06244b671737b8bba395c87c0337e1d64c5b52f65682cf77a";

struct Args {
    games: u32,
    base_seed: u64,
    budget_ms: Option<u64>,
    heuristic: bool,
    profile: bool,
    model: String,
    a_beam: usize,
    a_depth: usize,
    b_beam: usize,
    b_depth: usize,
    piece_cap: u32,
    engine_rev: String,
    out: PathBuf,
    normalized_cli_args: Vec<String>,
    experiment: bool,
    side_override: bool,
    a_engine: EngineMode,
    b_engine: EngineMode,
    a_nn_scoring: NnScoringMode,
    b_nn_scoring: NnScoringMode,
    a_batch: NnBatchMode,
    b_batch: NnBatchMode,
    a_proxy_weight: f32,
    b_proxy_weight: f32,
    model_sha256: Option<String>,
    metadata_sha256: Option<String>,
}

fn main() -> ExitCode {
    match run(env::args().collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("usage: {USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(raw_args: Vec<String>) -> Result<(), String> {
    let args = parse_args(&raw_args)?;
    if args.games < 2 || args.games % 2 != 0 {
        return Err("--games must be an even value >= 2".to_owned());
    }

    let spec = baseline_spec();
    if args.side_override && !args.experiment {
        return Err("per-side overrides require --experiment".to_owned());
    }
    if args.experiment && args.heuristic {
        return Err("--heuristic is available only outside experiment mode".to_owned());
    }
    let model_required = if args.experiment {
        matches!(args.a_engine, EngineMode::Model) || matches!(args.b_engine, EngineMode::Model)
    } else {
        !args.heuristic
    };
    if model_required
        && ((!args.experiment && args.model != spec.model_path)
            || (args.experiment && !model_path_allowed(&args.model, spec.model_path)))
    {
        return Err("P0 accepts only the frozen baseline model".to_owned());
    }
    if args.experiment
        && args.budget_ms.is_some()
        && (matches!(args.a_nn_scoring, NnScoringMode::PerChildValue)
            || matches!(args.b_nn_scoring, NnScoringMode::PerChildValue))
    {
        return Err("clocked experiment runs require policy-proxy scoring".to_owned());
    }
    if model_required && args.experiment {
        validate_hash(args.model_sha256.as_deref(), "--model-sha256")?;
        validate_hash(args.metadata_sha256.as_deref(), "--metadata-sha256")?;
        if args.model_sha256.as_deref() != Some(FROZEN_MODEL_SHA256)
            || args.metadata_sha256.as_deref() != Some(FROZEN_METADATA_SHA256)
        {
            return Err("P0 accepts only the frozen baseline model".to_owned());
        }
    }

    let model = if model_required {
        let model_path = PathBuf::from(&args.model);
        if !model_path.exists() {
            return Err(format!(
                "model metadata does not exist: {}",
                model_path.display()
            ));
        }
        Some(
            PolicyValueRuntime::load(&model_path)
                .map_err(|err| format!("load model {}: {err}", model_path.display()))?,
        )
    } else {
        None
    };

    let mut cfg_a = baseline_player_cfg("A", args.budget_ms);
    cfg_a.search.beam_width = args.a_beam;
    cfg_a.search.depth = args.a_depth;
    cfg_a.engine = if args.experiment {
        args.a_engine
    } else if args.heuristic {
        EngineMode::Heuristic
    } else {
        EngineMode::Model
    };
    cfg_a.search.nn_scoring = args.a_nn_scoring;
    cfg_a.search.nn_batch = args.a_batch;
    cfg_a.search.policy_proxy_weight = args.a_proxy_weight;
    let mut cfg_b = baseline_player_cfg("B", args.budget_ms);
    cfg_b.search.beam_width = args.b_beam;
    cfg_b.search.depth = args.b_depth;
    cfg_b.engine = if args.experiment {
        args.b_engine
    } else if args.heuristic {
        EngineMode::Heuristic
    } else {
        EngineMode::Model
    };
    cfg_b.search.nn_scoring = args.b_nn_scoring;
    cfg_b.search.nn_batch = args.b_batch;
    cfg_b.search.policy_proxy_weight = args.b_proxy_weight;

    fs::create_dir_all(&args.out).map_err(|err| format!("mkdir {}: {err}", args.out.display()))?;

    let weights = EvalWeights::default();
    let mut owned_results = Vec::with_capacity(args.games as usize);
    let mut lock_profiles = Vec::new();
    let mut profile_ends = Vec::with_capacity(args.games as usize);
    for pair in 0..(args.games / 2) {
        let game_seed = args.base_seed.wrapping_add(u64::from(pair));
        let game0 = pair.saturating_mul(2);
        let seeds0 = GameSeeds {
            seed: game_seed,
            stream_game_idx: 0,
            report_game_idx: game0,
        };
        let seeds1 = GameSeeds {
            seed: game_seed,
            stream_game_idx: 0,
            report_game_idx: game0.saturating_add(1),
        };
        if args.profile {
            let (result, end) = play_game_profiled(
                seeds0,
                [&cfg_a, &cfg_b],
                model.as_ref(),
                &weights,
                args.piece_cap,
                &mut lock_profiles,
                &mut |_| {},
            );
            owned_results.push(result);
            profile_ends.push(end);
            let (result, end) = play_game_profiled(
                seeds1,
                [&cfg_b, &cfg_a],
                model.as_ref(),
                &weights,
                args.piece_cap,
                &mut lock_profiles,
                &mut |_| {},
            );
            owned_results.push(result);
            profile_ends.push(end);
        } else {
            owned_results.push(play_game_with_stream_index(
                seeds0,
                [&cfg_a, &cfg_b],
                model.as_ref(),
                &weights,
                args.piece_cap,
                &mut |_| {},
            ));
            owned_results.push(play_game_with_stream_index(
                seeds1,
                [&cfg_b, &cfg_a],
                model.as_ref(),
                &weights,
                args.piece_cap,
                &mut |_| {},
            ));
        }
    }

    let mut recorded_games = Vec::with_capacity(owned_results.len());
    for (idx, result) in owned_results.iter().enumerate() {
        let game =
            u32::try_from(idx).map_err(|_| "too many games for u32 report ids".to_owned())?;
        let pair = game / 2;
        let seed = args.base_seed.wrapping_add(u64::from(pair));
        if game % 2 == 0 {
            recorded_games.push(RecordedGame {
                game,
                seed,
                slot0_label: &cfg_a.label,
                slot1_label: &cfg_b.label,
                a_slot: 0,
                result,
            });
        } else {
            recorded_games.push(RecordedGame {
                game,
                seed,
                slot0_label: &cfg_b.label,
                slot1_label: &cfg_a.label,
                a_slot: 1,
                result,
            });
        }
    }

    let mode = if args.budget_ms.is_some() {
        ReportMode::Clocked
    } else {
        ReportMode::Deterministic
    };
    let replay_path = args.out.join("replays.jsonl");
    let summary_path = args.out.join("summary.json");
    let manifest_path = args.out.join("manifest.json");
    write_replays_jsonl(&replay_path, &recorded_games, mode)
        .map_err(|err| format!("write {}: {err}", replay_path.display()))?;
    let summary = write_summary_json(
        &summary_path,
        &args.engine_rev,
        mode,
        &recorded_games,
        args.budget_ms,
    )
    .map_err(|err| format!("write {}: {err}", summary_path.display()))?;
    let model_bytes_len = model
        .as_ref()
        .map(|runtime| {
            PathBuf::from(&args.model)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(&runtime.manifest.model_path)
        })
        .map(|path| {
            fs::metadata(&path)
                .map(|metadata| metadata.len())
                .map_err(|err| format!("stat {}: {err}", path.display()))
        })
        .transpose()?;
    let manifest_model = if !model_required {
        ManifestModel::Heuristic
    } else {
        ManifestModel::Loaded {
            path: &args.model,
            bytes: fs::metadata(&args.model)
                .map_err(|err| format!("stat {}: {err}", args.model))?
                .len(),
        }
    };
    let manifest_input = ManifestInput {
        engine_rev: &args.engine_rev,
        side_a: &cfg_a,
        side_b: &cfg_b,
        model: manifest_model,
        ruleset: spec.ruleset,
        rng: spec.rng,
        garbage_model: spec.garbage_model,
        piece_cap: args.piece_cap,
        seed_base: args.base_seed,
        seed_count: args.games,
        cli_args: &args.normalized_cli_args,
    };
    if args.experiment {
        write_experiment_manifest_json(
            &manifest_path,
            &manifest_input,
            &ManifestExperiment {
                model_sha256: args.model_sha256.as_deref(),
                model_bytes_len,
                metadata_sha256: args.metadata_sha256.as_deref(),
            },
        )
    } else {
        write_manifest_json(&manifest_path, &manifest_input)
    }
    .map_err(|err| format!("write {}: {err}", manifest_path.display()))?;
    if args.profile {
        let profile_path = args.out.join("profile.jsonl");
        write_profile_jsonl(&profile_path, &lock_profiles, &profile_ends)
            .map_err(|err| format!("write {}: {err}", profile_path.display()))?;
    }

    println!(
        "RESULT games={} wins_a={} losses_a={} draws={} pair_score_a={} pair_ci={}",
        summary.games,
        summary.wins_a,
        summary.losses_a,
        summary.draws,
        display_float(summary.pair_score_a),
        display_ci(summary.pair_wilson95_low, summary.pair_wilson95_high),
    );
    Ok(())
}

fn parse_args(raw_args: &[String]) -> Result<Args, String> {
    let spec = baseline_spec();
    let mut args = Args {
        games: spec.seed_count,
        base_seed: spec.seed_base,
        budget_ms: Some(spec.clocked_budget_ms),
        heuristic: false,
        profile: false,
        model: spec.model_path.to_owned(),
        a_beam: spec.beam_width,
        a_depth: spec.depth,
        b_beam: spec.beam_width,
        b_depth: spec.depth,
        piece_cap: spec.piece_cap,
        engine_rev: "unknown".to_owned(),
        out: PathBuf::from("target/bot_arena_out"),
        normalized_cli_args: Vec::new(),
        experiment: false,
        side_override: false,
        a_engine: EngineMode::Model,
        b_engine: EngineMode::Model,
        a_nn_scoring: NnScoringMode::PerChildValue,
        b_nn_scoring: NnScoringMode::PerChildValue,
        a_batch: NnBatchMode::Scalar,
        b_batch: NnBatchMode::Scalar,
        a_proxy_weight: fusion_engine::search_config::POLICY_BONUS_WEIGHT,
        b_proxy_weight: fusion_engine::search_config::POLICY_BONUS_WEIGHT,
        model_sha256: None,
        metadata_sha256: None,
    };

    let mut idx = 1usize;
    while idx < raw_args.len() {
        let arg = raw_args[idx].as_str();
        match arg {
            "--help" | "-h" => {
                println!("usage: {USAGE}");
                std::process::exit(0);
            }
            "--heuristic" => {
                args.heuristic = true;
                args.normalized_cli_args.push(arg.to_owned());
            }
            "--profile" => {
                args.profile = true;
                args.normalized_cli_args.push(arg.to_owned());
            }
            "--experiment" => {
                args.experiment = true;
                args.normalized_cli_args.push(arg.to_owned());
            }
            "--games" => {
                let value = next_value(raw_args, &mut idx, "--games")?;
                args.games = parse_u32(value, "--games")?;
                push_normalized(&mut args.normalized_cli_args, "--games", value);
            }
            "--base-seed" => {
                let value = next_value(raw_args, &mut idx, "--base-seed")?;
                args.base_seed = parse_u64(value, "--base-seed")?;
                push_normalized(&mut args.normalized_cli_args, "--base-seed", value);
            }
            "--budget-ms" => {
                let value = next_value(raw_args, &mut idx, "--budget-ms")?;
                args.budget_ms = parse_budget(value)?;
                push_normalized(&mut args.normalized_cli_args, "--budget-ms", value);
            }
            "--model" => {
                let value = next_value(raw_args, &mut idx, "--model")?;
                args.model = value.to_owned();
                push_normalized(&mut args.normalized_cli_args, "--model", value);
            }
            "--model-sha256" => parse_string_arg(
                raw_args,
                &mut idx,
                &mut args.model_sha256,
                &mut args.normalized_cli_args,
                "--model-sha256",
            )?,
            "--metadata-sha256" => parse_string_arg(
                raw_args,
                &mut idx,
                &mut args.metadata_sha256,
                &mut args.normalized_cli_args,
                "--metadata-sha256",
            )?,
            "--a-engine" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.a_engine = parse_engine(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--b-engine" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.b_engine = parse_engine(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--a-nn-scoring" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.a_nn_scoring = parse_scoring(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--b-nn-scoring" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.b_nn_scoring = parse_scoring(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--a-batch" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.a_batch = parse_batch(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--b-batch" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.b_batch = parse_batch(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--a-proxy-weight" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.a_proxy_weight = parse_f32(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--b-proxy-weight" => {
                let value = next_value(raw_args, &mut idx, arg)?;
                args.b_proxy_weight = parse_f32(value, arg)?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, arg, value);
            }
            "--a-beam" => {
                let value = next_value(raw_args, &mut idx, "--a-beam")?;
                args.a_beam = parse_usize(value, "--a-beam")?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, "--a-beam", value);
            }
            "--a-depth" => {
                let value = next_value(raw_args, &mut idx, "--a-depth")?;
                args.a_depth = parse_usize(value, "--a-depth")?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, "--a-depth", value);
            }
            "--b-beam" => {
                let value = next_value(raw_args, &mut idx, "--b-beam")?;
                args.b_beam = parse_usize(value, "--b-beam")?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, "--b-beam", value);
            }
            "--b-depth" => {
                let value = next_value(raw_args, &mut idx, "--b-depth")?;
                args.b_depth = parse_usize(value, "--b-depth")?;
                args.side_override = true;
                push_normalized(&mut args.normalized_cli_args, "--b-depth", value);
            }
            "--piece-cap" => {
                let value = next_value(raw_args, &mut idx, "--piece-cap")?;
                args.piece_cap = parse_u32(value, "--piece-cap")?;
                push_normalized(&mut args.normalized_cli_args, "--piece-cap", value);
            }
            "--engine-rev" => {
                let value = next_value(raw_args, &mut idx, "--engine-rev")?;
                args.engine_rev = value.to_owned();
                push_normalized(&mut args.normalized_cli_args, "--engine-rev", value);
            }
            "--out" => {
                let value = next_value(raw_args, &mut idx, "--out")?;
                args.out = PathBuf::from(value);
            }
            other if other.starts_with("--") => parse_equals_arg(other, &mut args)?,
            other => return Err(format!("unexpected positional argument: {other}")),
        }
        idx += 1;
    }
    Ok(args)
}

fn parse_equals_arg(raw: &str, args: &mut Args) -> Result<(), String> {
    let Some((flag, value)) = raw.split_once('=') else {
        return Err(format!("unknown flag: {raw}"));
    };
    match flag {
        "--games" => args.games = parse_u32(value, flag)?,
        "--base-seed" => args.base_seed = parse_u64(value, flag)?,
        "--budget-ms" => args.budget_ms = parse_budget(value)?,
        "--model" => args.model = value.to_owned(),
        "--model-sha256" => args.model_sha256 = Some(value.to_owned()),
        "--metadata-sha256" => args.metadata_sha256 = Some(value.to_owned()),
        "--a-engine" => {
            args.a_engine = parse_engine(value, flag)?;
            args.side_override = true;
        }
        "--b-engine" => {
            args.b_engine = parse_engine(value, flag)?;
            args.side_override = true;
        }
        "--a-nn-scoring" => {
            args.a_nn_scoring = parse_scoring(value, flag)?;
            args.side_override = true;
        }
        "--b-nn-scoring" => {
            args.b_nn_scoring = parse_scoring(value, flag)?;
            args.side_override = true;
        }
        "--a-batch" => {
            args.a_batch = parse_batch(value, flag)?;
            args.side_override = true;
        }
        "--b-batch" => {
            args.b_batch = parse_batch(value, flag)?;
            args.side_override = true;
        }
        "--a-proxy-weight" => {
            args.a_proxy_weight = parse_f32(value, flag)?;
            args.side_override = true;
        }
        "--b-proxy-weight" => {
            args.b_proxy_weight = parse_f32(value, flag)?;
            args.side_override = true;
        }
        "--a-beam" => {
            args.a_beam = parse_usize(value, flag)?;
            args.side_override = true;
        }
        "--a-depth" => {
            args.a_depth = parse_usize(value, flag)?;
            args.side_override = true;
        }
        "--b-beam" => {
            args.b_beam = parse_usize(value, flag)?;
            args.side_override = true;
        }
        "--b-depth" => {
            args.b_depth = parse_usize(value, flag)?;
            args.side_override = true;
        }
        "--piece-cap" => args.piece_cap = parse_u32(value, flag)?,
        "--engine-rev" => args.engine_rev = value.to_owned(),
        "--out" => {
            args.out = PathBuf::from(value);
            return Ok(());
        }
        _ => return Err(format!("unknown flag: {flag}")),
    }
    push_normalized(&mut args.normalized_cli_args, flag, value);
    Ok(())
}

fn next_value<'a>(raw_args: &'a [String], idx: &mut usize, flag: &str) -> Result<&'a str, String> {
    *idx = idx.saturating_add(1);
    raw_args
        .get(*idx)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn push_normalized(out: &mut Vec<String>, flag: &str, value: &str) {
    out.push(flag.to_owned());
    out.push(value.to_owned());
}

fn parse_u32(value: &str, flag: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|err| format!("{flag} must be u32: {err}"))
}

fn parse_u64(value: &str, flag: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|err| format!("{flag} must be u64: {err}"))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|err| format!("{flag} must be usize: {err}"))
}

fn parse_budget(value: &str) -> Result<Option<u64>, String> {
    if value == "none" {
        Ok(None)
    } else {
        parse_u64(value, "--budget-ms").map(Some)
    }
}

fn parse_string_arg(
    raw_args: &[String],
    idx: &mut usize,
    target: &mut Option<String>,
    normalized: &mut Vec<String>,
    flag: &str,
) -> Result<(), String> {
    let value = next_value(raw_args, idx, flag)?;
    *target = Some(value.to_owned());
    push_normalized(normalized, flag, value);
    Ok(())
}

fn parse_engine(value: &str, flag: &str) -> Result<EngineMode, String> {
    match value {
        "model" => Ok(EngineMode::Model),
        "heuristic" => Ok(EngineMode::Heuristic),
        _ => Err(format!("{flag} must be model or heuristic")),
    }
}

fn parse_scoring(value: &str, flag: &str) -> Result<NnScoringMode, String> {
    match value {
        "per-child-value" => Ok(NnScoringMode::PerChildValue),
        "policy-proxy" => Ok(NnScoringMode::PolicyProxy),
        _ => Err(format!("{flag} must be per-child-value or policy-proxy")),
    }
}

fn parse_batch(value: &str, flag: &str) -> Result<NnBatchMode, String> {
    match value {
        "scalar" => Ok(NnBatchMode::Scalar),
        "level" => Ok(NnBatchMode::Level),
        _ => Err(format!("{flag} must be scalar or level")),
    }
}

fn parse_f32(value: &str, flag: &str) -> Result<f32, String> {
    value
        .parse::<f32>()
        .map_err(|err| format!("{flag} must be f32: {err}"))
}

fn validate_hash(value: Option<&str>, flag: &str) -> Result<(), String> {
    let Some(value) = value else {
        return Err(format!(
            "{flag} is required when experiment mode loads a model"
        ));
    };
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{flag} must be exactly 64 lowercase hexadecimal characters"
        ));
    }
    Ok(())
}

fn model_path_allowed(path: &str, canonical: &str) -> bool {
    if path == canonical {
        return true;
    }
    let candidate = PathBuf::from(path);
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return false;
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let snapshot_root = manifest_dir.join("target").join("evidence_bin");
    let absolute = if candidate.is_absolute() {
        candidate
    } else {
        manifest_dir.join(candidate)
    };
    absolute
        .strip_prefix(snapshot_root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|component| {
            matches!(component, std::path::Component::Normal(name) if name.to_string_lossy().starts_with("model_"))
        })
}

fn display_float(value: Option<f64>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| format!("{value:.4}"))
}

fn display_ci(low: Option<f64>, high: Option<f64>) -> String {
    match (low, high) {
        (Some(low), Some(high)) => format!("[{low:.4},{high:.4}]"),
        _ => "na".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(arguments: &[&str]) -> Vec<String> {
        std::iter::once("bot_arena")
            .chain(arguments.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn experiment_flag_gates_side_overrides() {
        let error = run(argv(&["--a-nn-scoring", "policy-proxy"]))
            .expect_err("side override without experiment must fail");
        assert_eq!(error, "per-side overrides require --experiment");
    }

    #[test]
    fn unknown_experiment_flag_exits_2() {
        let error = match parse_args(&argv(&["--experiment", "--a-bogus", "x"])) {
            Ok(_) => panic!("unknown experiment flag must fail"),
            Err(error) => error,
        };
        assert_eq!(error, "unknown flag: --a-bogus");
    }

    #[test]
    fn bad_sha256_format_exits_2() {
        for (flag, bad_hash) in [
            ("--model-sha256", "a".repeat(63)),
            ("--metadata-sha256", "A".repeat(64)),
        ] {
            let mut arguments = vec![
                "--experiment".to_owned(),
                "--budget-ms".to_owned(),
                "none".to_owned(),
                "--model-sha256".to_owned(),
                FROZEN_MODEL_SHA256.to_owned(),
                "--metadata-sha256".to_owned(),
                FROZEN_METADATA_SHA256.to_owned(),
            ];
            let value_index = arguments
                .iter()
                .position(|argument| argument == flag)
                .expect("flag exists")
                + 1;
            arguments[value_index] = bad_hash;
            let mut raw = vec!["bot_arena".to_owned()];
            raw.extend(arguments);
            let error = run(raw).expect_err("malformed hash must fail");
            assert!(error.contains("must be exactly 64 lowercase hexadecimal characters"));
        }
    }

    #[test]
    fn clocked_experiment_rejects_per_child_value() {
        for side in ["--a-nn-scoring", "--b-nn-scoring"] {
            let error = run(argv(&[
                "--experiment",
                side,
                "per-child-value",
                "--model-sha256",
                FROZEN_MODEL_SHA256,
                "--metadata-sha256",
                FROZEN_METADATA_SHA256,
            ]))
            .expect_err("clocked per-child experiment must fail");
            assert_eq!(
                error,
                "clocked experiment runs require policy-proxy scoring"
            );
        }
    }
}
