use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use crate::search_config::{NnBatchMode, NnScoringMode};

use super::{
    EngineMode, GameEndReason, GameResult, LockProfile, MoveRecord, PlayerCfg, ProfileEnd,
};

// allow: SIZE_OK — P0 plan allowlists one report submodule file for manual fixed-order JSON schemas and tests.

#[derive(Clone, Copy)]
pub enum ReportMode {
    Deterministic,
    Clocked,
}

impl ReportMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Clocked => "clocked",
        }
    }

    fn serializes_timing(self) -> bool {
        match self {
            Self::Deterministic => false,
            Self::Clocked => true,
        }
    }
}

pub struct RecordedGame<'a> {
    pub game: u32,
    pub seed: u64,
    pub slot0_label: &'a str,
    pub slot1_label: &'a str,
    pub a_slot: u8,
    pub result: &'a GameResult,
}

pub enum ManifestModel<'a> {
    Heuristic,
    Loaded { path: &'a str, bytes: u64 },
}

pub struct ManifestInput<'a> {
    pub engine_rev: &'a str,
    pub side_a: &'a PlayerCfg,
    pub side_b: &'a PlayerCfg,
    pub model: ManifestModel<'a>,
    pub ruleset: &'a str,
    pub rng: &'a str,
    pub garbage_model: &'a str,
    pub piece_cap: u32,
    pub seed_base: u64,
    pub seed_count: u32,
    pub cli_args: &'a [String],
}

pub struct ManifestExperiment<'a> {
    pub model_sha256: Option<&'a str>,
    pub model_bytes_len: Option<u64>,
    pub metadata_sha256: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryStats {
    pub schema_version: u32,
    pub engine_rev: String,
    pub mode: String,
    pub games: u32,
    pub wins_a: u32,
    pub losses_a: u32,
    pub draws: u32,
    pub decisive_games: u32,
    pub winrate_a_decisive_descriptive: Option<f64>,
    pub pairs: u32,
    pub pair_score_a: Option<f64>,
    pub pair_wins_a: u32,
    pub pair_losses_a: u32,
    pub pair_ties: u32,
    pub decisive_pairs: u32,
    pub pair_winrate_a: Option<f64>,
    pub pair_wilson95_low: Option<f64>,
    pub pair_wilson95_high: Option<f64>,
    pub ci_status: &'static str,
    pub pair_imbalance: u32,
    pub attack_per_piece_a: Option<f64>,
    pub attack_per_piece_b: Option<f64>,
    pub move_ms_p50_a: Option<f64>,
    pub move_ms_p99_a: Option<f64>,
    pub move_ms_p50_b: Option<f64>,
    pub move_ms_p99_b: Option<f64>,
    pub budget_overruns_a: u32,
    pub budget_overruns_b: u32,
    pub move_timing_count_a: u32,
    pub move_timing_count_b: u32,
    pub move_ms_max_a: Option<f64>,
    pub move_ms_max_b: Option<f64>,
    pub overrun_count_a: u32,
    pub overrun_count_b: u32,
}

#[derive(Default)]
struct SideTotals {
    attack: u64,
    pieces: u32,
    timings: Vec<f64>,
    budget_overruns: u32,
}

pub(crate) fn escape_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c <= '\u{1f}' => {
                let _ = write!(escaped, "\\u{:04x}", c as u32);
            }
            c => escaped.push(c),
        }
    }
    escaped
}

pub fn write_replays_jsonl(
    path: &Path,
    games: &[RecordedGame<'_>],
    mode: ReportMode,
) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(replays_jsonl(games, mode).as_bytes())
}

pub fn replays_jsonl(games: &[RecordedGame<'_>], mode: ReportMode) -> String {
    let mut out = String::new();
    for game in games {
        push_game_header(&mut out, game);
        for record in &game.result.per_move {
            push_move_row(&mut out, record, mode);
        }
        push_game_end(&mut out, game);
    }
    out
}

pub fn write_profile_jsonl(
    path: &Path,
    profiles: &[LockProfile],
    ends: &[ProfileEnd],
) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(profile_jsonl(profiles, ends).as_bytes())
}

pub fn profile_jsonl(profiles: &[LockProfile], ends: &[ProfileEnd]) -> String {
    let mut out = String::new();
    for end in ends {
        for profile in profiles.iter().filter(|profile| profile.game == end.game) {
            push_lock_profile(&mut out, profile);
        }
        push_profile_end(&mut out, end);
    }
    out
}

fn push_json_option<T: std::fmt::Display>(out: &mut String, value: Option<T>) {
    match value {
        Some(value) => {
            let _ = write!(out, "{value}");
        }
        None => out.push_str("null"),
    }
}

fn push_lock_profile(out: &mut String, profile: &LockProfile) {
    out.push_str("{\"t\":\"lock\",\"game\":");
    let _ = write!(out, "{}", profile.game);
    out.push_str(",\"round\":");
    let _ = write!(out, "{}", profile.round);
    out.push_str(",\"slot\":");
    let _ = write!(out, "{}", profile.slot);
    out.push_str(",\"wall_ns\":");
    let _ = write!(out, "{}", profile.wall_ns);
    out.push_str(",\"returned_move\":");
    out.push_str(if profile.returned_move {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"piece\":");
    match profile.piece {
        Some(piece) => {
            out.push('"');
            out.push(piece);
            out.push('"');
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"rot\":");
    push_json_option(out, profile.rot);
    out.push_str(",\"x\":");
    push_json_option(out, profile.x);
    out.push_str(",\"y\":");
    push_json_option(out, profile.y);
    let _ = write!(
        out,
        ",\"runtime_attempt_rows\":{},\"runtime_unavailable_nodes\":{},\"noninferable_nodes\":{},\"abandoned_nodes\":{},\"runtime_calls\":{},\"batch_calls\":{},\"inferred_rows\":{},\"fallback_levels\":{},\"abandoned_levels\":{},\"expanded_nodes\":{},\"movegen_calls\":{},\"infer_nanos\":{},\"completed_depth\":{},\"completed_width\":{},\"deadline_hits\":{}",
        profile.runtime_attempt_rows,
        profile.runtime_unavailable_nodes,
        profile.noninferable_nodes,
        profile.abandoned_nodes,
        profile.runtime_calls,
        profile.batch_calls,
        profile.inferred_rows,
        profile.fallback_levels,
        profile.abandoned_levels,
        profile.expanded_nodes,
        profile.movegen_calls,
        profile.infer_nanos,
        profile.completed_depth,
        profile.completed_width,
        profile.deadline_hits,
    );
    out.push_str("}\n");
}

fn push_profile_end(out: &mut String, end: &ProfileEnd) {
    out.push_str("{\"t\":\"end\",\"game\":");
    let _ = write!(out, "{}", end.game);
    out.push_str(",\"terminal_slot0\":\"");
    out.push_str(end.terminal_slot0.as_str());
    out.push_str("\",\"terminal_slot1\":\"");
    out.push_str(end.terminal_slot1.as_str());
    out.push_str("\"}\n");
}

pub fn write_summary_json(
    path: &Path,
    engine_rev: &str,
    mode: ReportMode,
    games: &[RecordedGame<'_>],
    budget_ms: Option<u64>,
) -> io::Result<SummaryStats> {
    let summary = build_summary(engine_rev, mode, games, budget_ms);
    let mut file = File::create(path)?;
    file.write_all(summary_json(&summary).as_bytes())?;
    file.write_all(b"\n")?;
    Ok(summary)
}

pub(crate) fn build_summary(
    engine_rev: &str,
    mode: ReportMode,
    games: &[RecordedGame<'_>],
    budget_ms: Option<u64>,
) -> SummaryStats {
    let mut wins_a = 0u32;
    let mut losses_a = 0u32;
    let mut draws = 0u32;
    let mut game_scores = Vec::with_capacity(games.len());
    let mut totals_a = SideTotals::default();
    let mut totals_b = SideTotals::default();
    for game in games {
        match game_score_a(game) {
            1.0 => wins_a += 1,
            0.0 => losses_a += 1,
            _ => draws += 1,
        }
        game_scores.push(game_score_a(game));
        collect_side_totals(game, mode, budget_ms, &mut totals_a, &mut totals_b);
    }
    let decisive_games = wins_a + losses_a;
    let pair_scores = game_scores
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[first, second]| first + second)
        .collect::<Vec<_>>();
    let mut pair_wins_a = 0u32;
    let mut pair_losses_a = 0u32;
    let mut pair_ties = 0u32;
    let mut pair_score_sum = 0.0f64;
    for raw in &pair_scores {
        pair_score_sum += raw;
        match raw.total_cmp(&1.0) {
            std::cmp::Ordering::Greater => pair_wins_a += 1,
            std::cmp::Ordering::Less => pair_losses_a += 1,
            std::cmp::Ordering::Equal => pair_ties += 1,
        }
    }
    let pairs = u32::try_from(pair_scores.len()).unwrap_or(u32::MAX);
    let decisive_pairs = pair_wins_a + pair_losses_a;
    let (pair_wilson95_low, pair_wilson95_high) = wilson95(pair_wins_a, decisive_pairs)
        .map_or((None, None), |(low, high)| (Some(low), Some(high)));
    SummaryStats {
        schema_version: 1,
        engine_rev: engine_rev.to_owned(),
        mode: mode.as_str().to_owned(),
        games: u32::try_from(games.len()).unwrap_or(u32::MAX),
        wins_a,
        losses_a,
        draws,
        decisive_games,
        winrate_a_decisive_descriptive: ratio(wins_a, decisive_games),
        pairs,
        pair_score_a: (pairs > 0).then_some(pair_score_sum / f64::from(pairs)),
        pair_wins_a,
        pair_losses_a,
        pair_ties,
        decisive_pairs,
        pair_winrate_a: ratio(pair_wins_a, decisive_pairs),
        pair_wilson95_low,
        pair_wilson95_high,
        ci_status: if decisive_pairs == 0 {
            "no_decisive_pairs"
        } else {
            "ok"
        },
        pair_imbalance: (i64::from(pair_wins_a) - i64::from(pair_losses_a)).unsigned_abs() as u32,
        attack_per_piece_a: attack_per_piece(&totals_a),
        attack_per_piece_b: attack_per_piece(&totals_b),
        move_ms_p50_a: percentile(mode, &mut totals_a.timings, 0.50),
        move_ms_p99_a: percentile(mode, &mut totals_a.timings, 0.99),
        move_ms_p50_b: percentile(mode, &mut totals_b.timings, 0.50),
        move_ms_p99_b: percentile(mode, &mut totals_b.timings, 0.99),
        budget_overruns_a: if mode.serializes_timing() {
            totals_a.budget_overruns
        } else {
            0
        },
        budget_overruns_b: if mode.serializes_timing() {
            totals_b.budget_overruns
        } else {
            0
        },
        move_timing_count_a: if mode.serializes_timing() {
            u32::try_from(totals_a.timings.len()).unwrap_or(u32::MAX)
        } else {
            0
        },
        move_timing_count_b: if mode.serializes_timing() {
            u32::try_from(totals_b.timings.len()).unwrap_or(u32::MAX)
        } else {
            0
        },
        move_ms_max_a: maximum(mode, &totals_a.timings),
        move_ms_max_b: maximum(mode, &totals_b.timings),
        overrun_count_a: if mode.serializes_timing() {
            totals_a.budget_overruns
        } else {
            0
        },
        overrun_count_b: if mode.serializes_timing() {
            totals_b.budget_overruns
        } else {
            0
        },
    }
}

pub(crate) fn summary_json(summary: &SummaryStats) -> String {
    let mut out = String::new();
    out.push('{');
    push_u32(&mut out, "schema_version", summary.schema_version, true);
    push_str(&mut out, "engine_rev", &summary.engine_rev, false);
    push_str(&mut out, "mode", &summary.mode, false);
    push_u32(&mut out, "games", summary.games, false);
    push_u32(&mut out, "wins_a", summary.wins_a, false);
    push_u32(&mut out, "losses_a", summary.losses_a, false);
    push_u32(&mut out, "draws", summary.draws, false);
    push_u32(&mut out, "decisive_games", summary.decisive_games, false);
    push_float_option(
        &mut out,
        "winrate_a_decisive_descriptive",
        summary.winrate_a_decisive_descriptive,
        false,
    );
    push_u32(&mut out, "pairs", summary.pairs, false);
    push_float_option(&mut out, "pair_score_a", summary.pair_score_a, false);
    push_u32(&mut out, "pair_wins_a", summary.pair_wins_a, false);
    push_u32(&mut out, "pair_losses_a", summary.pair_losses_a, false);
    push_u32(&mut out, "pair_ties", summary.pair_ties, false);
    push_u32(&mut out, "decisive_pairs", summary.decisive_pairs, false);
    push_float_option(&mut out, "pair_winrate_a", summary.pair_winrate_a, false);
    push_float_option(
        &mut out,
        "pair_wilson95_low",
        summary.pair_wilson95_low,
        false,
    );
    push_float_option(
        &mut out,
        "pair_wilson95_high",
        summary.pair_wilson95_high,
        false,
    );
    push_str(&mut out, "ci_status", summary.ci_status, false);
    push_u32(&mut out, "pair_imbalance", summary.pair_imbalance, false);
    push_float_option(
        &mut out,
        "attack_per_piece_a",
        summary.attack_per_piece_a,
        false,
    );
    push_float_option(
        &mut out,
        "attack_per_piece_b",
        summary.attack_per_piece_b,
        false,
    );
    push_float_option(&mut out, "move_ms_p50_a", summary.move_ms_p50_a, false);
    push_float_option(&mut out, "move_ms_p99_a", summary.move_ms_p99_a, false);
    push_float_option(&mut out, "move_ms_p50_b", summary.move_ms_p50_b, false);
    push_float_option(&mut out, "move_ms_p99_b", summary.move_ms_p99_b, false);
    push_u32(
        &mut out,
        "budget_overruns_a",
        summary.budget_overruns_a,
        false,
    );
    push_u32(
        &mut out,
        "budget_overruns_b",
        summary.budget_overruns_b,
        false,
    );
    push_u32(
        &mut out,
        "move_timing_count_a",
        summary.move_timing_count_a,
        false,
    );
    push_u32(
        &mut out,
        "move_timing_count_b",
        summary.move_timing_count_b,
        false,
    );
    push_float_option(&mut out, "move_ms_max_a", summary.move_ms_max_a, false);
    push_float_option(&mut out, "move_ms_max_b", summary.move_ms_max_b, false);
    push_u32(&mut out, "overrun_count_a", summary.overrun_count_a, false);
    push_u32(&mut out, "overrun_count_b", summary.overrun_count_b, false);
    out.push('}');
    out
}

pub fn write_manifest_json(path: &Path, input: &ManifestInput<'_>) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(manifest_json(input).as_bytes())?;
    file.write_all(b"\n")
}

pub fn write_experiment_manifest_json(
    path: &Path,
    input: &ManifestInput<'_>,
    experiment: &ManifestExperiment<'_>,
) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(manifest_json_with_experiment(input, Some(experiment)).as_bytes())?;
    file.write_all(b"\n")
}

pub(crate) fn manifest_json(input: &ManifestInput<'_>) -> String {
    manifest_json_with_experiment(input, None)
}

fn manifest_json_with_experiment(
    input: &ManifestInput<'_>,
    experiment: Option<&ManifestExperiment<'_>>,
) -> String {
    let mut out = String::new();
    out.push('{');
    push_u32(&mut out, "schema_version", 1, true);
    push_str(&mut out, "engine_rev", input.engine_rev, false);
    push_bool(&mut out, "experiment", experiment.is_some(), false);
    push_search_config(&mut out, "side_a", input.side_a, experiment.is_some());
    push_search_config(&mut out, "side_b", input.side_b, experiment.is_some());
    push_str(&mut out, "weights", "EvalWeights::default", false);
    out.push_str(",\"model\":");
    match input.model {
        ManifestModel::Heuristic => out.push_str("\"heuristic\""),
        ManifestModel::Loaded { path, bytes } => {
            out.push('{');
            push_str(&mut out, "path", path, true);
            out.push_str(",\"bytes\":");
            let _ = write!(out, "{bytes}");
            out.push('}');
        }
    }
    push_optional_str(
        &mut out,
        "model_sha256",
        experiment.and_then(|value| value.model_sha256),
        false,
    );
    push_u64_option(
        &mut out,
        "model_bytes_len",
        experiment.and_then(|value| value.model_bytes_len),
        false,
    );
    push_optional_str(
        &mut out,
        "metadata_sha256",
        experiment.and_then(|value| value.metadata_sha256),
        false,
    );
    push_str(&mut out, "ruleset", input.ruleset, false);
    push_str(&mut out, "rng", input.rng, false);
    push_str(&mut out, "garbage_model", input.garbage_model, false);
    push_u32(&mut out, "piece_cap", input.piece_cap, false);
    out.push_str(",\"seeds\":{");
    out.push_str("\"base\":");
    let _ = write!(out, "{}", input.seed_base);
    push_u32(&mut out, "count", input.seed_count, false);
    out.push('}');
    out.push_str(",\"cli_args\":[");
    for (idx, arg) in input.cli_args.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&escape_json_string(arg));
        out.push('"');
    }
    out.push_str("]}");
    out
}

pub(crate) fn wilson95(successes: u32, trials: u32) -> Option<(f64, f64)> {
    if trials == 0 {
        return None;
    }
    let n = f64::from(trials);
    let p = f64::from(successes) / n;
    let z = 1.96f64;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt() / denom;
    Some(((center - margin).max(0.0), (center + margin).min(1.0)))
}

fn push_game_header(out: &mut String, game: &RecordedGame<'_>) {
    out.push_str("{\"t\":\"g\",\"game\":");
    let _ = write!(out, "{}", game.game);
    out.push_str(",\"seed\":");
    let _ = write!(out, "{}", game.seed);
    out.push_str(",\"slot0\":\"");
    out.push_str(&escape_json_string(game.slot0_label));
    out.push_str("\",\"slot1\":\"");
    out.push_str(&escape_json_string(game.slot1_label));
    out.push_str("\"}\n");
}

fn push_move_row(out: &mut String, record: &MoveRecord, mode: ReportMode) {
    out.push_str("{\"t\":\"m\",\"game\":");
    let _ = write!(out, "{}", record.game);
    out.push_str(",\"round\":");
    let _ = write!(out, "{}", record.round);
    out.push_str(",\"slot\":");
    let _ = write!(out, "{}", record.slot);
    out.push_str(",\"piece\":\"");
    out.push(record.piece);
    out.push_str("\",\"rot\":");
    let _ = write!(out, "{}", record.rot);
    out.push_str(",\"x\":");
    let _ = write!(out, "{}", record.x);
    out.push_str(",\"y\":");
    let _ = write!(out, "{}", record.y);
    out.push_str(",\"spin\":\"");
    out.push_str(&escape_json_string(&record.spin));
    out.push_str("\",\"hold\":");
    out.push(if record.hold { '1' } else { '0' });
    out.push_str(",\"lc\":");
    let _ = write!(out, "{}", record.lc);
    out.push_str(",\"gc\":");
    let _ = write!(out, "{}", record.gc);
    out.push_str(",\"atk\":");
    let _ = write!(out, "{}", record.atk);
    out.push_str(",\"canc\":");
    let _ = write!(out, "{}", record.canc);
    out.push_str(",\"inb\":");
    let _ = write!(out, "{}", record.inb);
    out.push_str(",\"b2b\":");
    let _ = write!(out, "{}", record.b2b);
    out.push_str(",\"combo\":");
    let _ = write!(out, "{}", record.combo);
    out.push_str(",\"ms\":");
    if mode.serializes_timing() {
        push_raw_float_option(out, record.ms);
    } else {
        out.push_str("null");
    }
    out.push_str("}\n");
}

fn push_game_end(out: &mut String, game: &RecordedGame<'_>) {
    out.push_str("{\"t\":\"e\",\"game\":");
    let _ = write!(out, "{}", game.game);
    out.push_str(",\"winner\":");
    match game.result.winner {
        Some(winner) => {
            let _ = write!(out, "{winner}");
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"reason\":\"");
    out.push_str(reason_name(game.result.reason));
    out.push_str("\",\"rounds\":");
    let _ = write!(out, "{}", game.result.rounds);
    out.push_str(",\"pieces0\":");
    let _ = write!(out, "{}", game.result.pieces[0]);
    out.push_str(",\"pieces1\":");
    let _ = write!(out, "{}", game.result.pieces[1]);
    out.push_str("}\n");
}

fn reason_name(reason: GameEndReason) -> &'static str {
    match reason {
        GameEndReason::Win => "win",
        GameEndReason::TopOut => "topout",
        GameEndReason::DrawCap => "draw_cap",
        GameEndReason::DrawSimul => "draw_simul",
    }
}

fn game_score_a(game: &RecordedGame<'_>) -> f64 {
    match game.result.winner {
        Some(winner) if winner == game.a_slot => 1.0,
        Some(_) => 0.0,
        None => 0.5,
    }
}

fn collect_side_totals(
    game: &RecordedGame<'_>,
    mode: ReportMode,
    budget_ms: Option<u64>,
    totals_a: &mut SideTotals,
    totals_b: &mut SideTotals,
) {
    for record in &game.result.per_move {
        let target: &mut SideTotals = if record.slot == game.a_slot {
            &mut *totals_a
        } else {
            &mut *totals_b
        };
        target.attack = target.attack.saturating_add(u64::from(record.atk));
        target.pieces = target.pieces.saturating_add(1);
        if mode.serializes_timing() {
            if let Some(ms) = record.ms {
                target.timings.push(ms);
                if budget_ms.is_some_and(|budget| ms > budget as f64) {
                    target.budget_overruns = target.budget_overruns.saturating_add(1);
                }
            }
        }
    }
}

fn ratio(numerator: u32, denominator: u32) -> Option<f64> {
    (denominator > 0).then_some(f64::from(numerator) / f64::from(denominator))
}

fn attack_per_piece(totals: &SideTotals) -> Option<f64> {
    (totals.pieces > 0).then_some(totals.attack as f64 / f64::from(totals.pieces))
}

fn percentile(mode: ReportMode, values: &mut [f64], quantile: f64) -> Option<f64> {
    if !mode.serializes_timing() || values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let max_index = values.len() - 1;
    let index = (max_index as f64 * quantile).ceil() as usize;
    values.get(index).copied()
}

fn maximum(mode: ReportMode, values: &[f64]) -> Option<f64> {
    mode.serializes_timing()
        .then(|| values.iter().copied().max_by(f64::total_cmp))
        .flatten()
}

fn push_search_config(out: &mut String, key: &str, player: &PlayerCfg, experiment: bool) {
    let config = &player.search;
    out.push_str(",\"");
    out.push_str(key);
    out.push_str("\":{");
    out.push_str("\"beam_width\":");
    let _ = write!(out, "{}", config.beam_width);
    out.push_str(",\"depth\":");
    let _ = write!(out, "{}", config.depth);
    out.push_str(",\"time_budget_ms\":");
    match config.time_budget_ms {
        Some(value) => {
            let _ = write!(out, "{value}");
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"extend_queue_7bag\":");
    out.push_str(if config.extend_queue_7bag {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"quiescence_max_extensions\":");
    let _ = write!(out, "{}", config.quiescence_max_extensions);
    out.push_str(",\"quiescence_beam_fraction\":");
    let _ = write!(out, "{:.4}", config.quiescence_beam_fraction);
    out.push_str(",\"policy_guided_expansion_cap\":");
    let _ = write!(out, "{}", config.policy_guided_expansion_cap);
    out.push_str(",\"attack_config\":\"tetra_league\"");
    if experiment {
        push_str(out, "engine", engine_name(player.engine), false);
        push_str(out, "nn_scoring", scoring_name(config.nn_scoring), false);
        push_str(out, "nn_batch", batch_name(config.nn_batch), false);
        out.push_str(",\"proxy_weight\":");
        let _ = write!(out, "{:.4}", config.policy_proxy_weight);
    }
    out.push('}');
}

fn engine_name(mode: EngineMode) -> &'static str {
    match mode {
        EngineMode::Model => "model",
        EngineMode::Heuristic => "heuristic",
    }
}

fn scoring_name(mode: NnScoringMode) -> &'static str {
    match mode {
        NnScoringMode::PerChildValue => "per-child-value",
        NnScoringMode::PolicyProxy => "policy-proxy",
    }
}

fn batch_name(mode: NnBatchMode) -> &'static str {
    match mode {
        NnBatchMode::Scalar => "scalar",
        NnBatchMode::Level => "level",
    }
}

fn push_bool(out: &mut String, key: &str, value: bool, first: bool) {
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    out.push_str(if value { "true" } else { "false" });
}

fn push_optional_str(out: &mut String, key: &str, value: Option<&str>, first: bool) {
    match value {
        Some(value) => push_str(out, key, value, first),
        None => {
            if !first {
                out.push(',');
            }
            out.push('"');
            out.push_str(key);
            out.push_str("\":null");
        }
    }
}

fn push_u64_option(out: &mut String, key: &str, value: Option<u64>, first: bool) {
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    match value {
        Some(value) => {
            let _ = write!(out, "{value}");
        }
        None => out.push_str("null"),
    }
}

fn push_str(out: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":\"");
    out.push_str(&escape_json_string(value));
    out.push('"');
}

fn push_u32(out: &mut String, key: &str, value: u32, first: bool) {
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    let _ = write!(out, "{value}");
}

fn push_float_option(out: &mut String, key: &str, value: Option<f64>, first: bool) {
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
    push_raw_float_option(out, value);
}

fn push_raw_float_option(out: &mut String, value: Option<f64>) {
    match value {
        Some(value) => {
            let _ = write!(out, "{value:.4}");
        }
        None => out.push_str("null"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versus::{
        baseline_fixture_json_for, baseline_player_cfg, baseline_spec, GameEndReason, GameResult,
        MoveRecord,
    };

    fn result(winner: Option<u8>, moves: Vec<MoveRecord>) -> GameResult {
        GameResult {
            winner,
            rounds: 1,
            pieces: [1, 1],
            reason: winner.map_or(GameEndReason::DrawCap, |_| GameEndReason::Win),
            per_move: moves,
        }
    }

    fn recorded(game: u32, result: &GameResult) -> RecordedGame<'_> {
        RecordedGame {
            game,
            seed: u64::from(game) + 100,
            slot0_label: "A",
            slot1_label: "B",
            a_slot: 0,
            result,
        }
    }

    fn move_record(slot: u8, ms: Option<f64>) -> MoveRecord {
        MoveRecord {
            game: 0,
            round: 0,
            slot,
            piece: 'T',
            rot: 1,
            x: -1,
            y: 2,
            spin: "full".to_owned(),
            hold: true,
            lc: 2,
            gc: 1,
            atk: 4,
            canc: 3,
            inb: 2,
            b2b: -1,
            combo: -1,
            ms,
        }
    }

    #[test]
    fn wilson_pair_edge_cases() {
        let tie_results = [result(None, Vec::new()), result(None, Vec::new())];
        let tie_games = [recorded(0, &tie_results[0]), recorded(1, &tie_results[1])];
        let tie_summary = build_summary("rev", ReportMode::Deterministic, &tie_games, None);
        assert_eq!(tie_summary.decisive_pairs, 0);
        assert_eq!(tie_summary.ci_status, "no_decisive_pairs");
        assert_eq!(tie_summary.pair_wilson95_low, None);

        let five_zero = wilson95(5, 5).expect("5 trials has interval");
        assert!(five_zero.0 > 0.55 && five_zero.1 <= 1.0);
        let zero_five = wilson95(0, 5).expect("5 trials has interval");
        assert!(zero_five.0 >= 0.0 && zero_five.1 < 0.45);
        let split = wilson95(25, 50).expect("50 trials has interval");
        assert!(split.0 < 0.5 && split.1 > 0.5);
        assert!(((split.0 + split.1) / 2.0 - 0.5).abs() < 0.01);
    }

    #[test]
    fn pair_scoring_maps_wld_correctly() {
        let results = [
            result(Some(0), Vec::new()),
            result(None, Vec::new()),
            result(Some(1), Vec::new()),
            result(Some(0), Vec::new()),
        ];
        let games = [
            recorded(0, &results[0]),
            recorded(1, &results[1]),
            recorded(2, &results[2]),
            recorded(3, &results[3]),
        ];
        let summary = build_summary("rev", ReportMode::Deterministic, &games, None);
        assert_eq!(summary.pair_score_a, Some(1.25));
        assert_eq!(summary.pair_wins_a, 1);
        assert_eq!(summary.pair_losses_a, 0);
        assert_eq!(summary.pair_ties, 1);
    }

    #[test]
    fn jsonl_rows_have_fixed_field_order() {
        let result = result(Some(0), vec![move_record(0, Some(1.234_56))]);
        let games = [RecordedGame {
            game: 7,
            seed: 99,
            slot0_label: "A",
            slot1_label: "B",
            a_slot: 0,
            result: &result,
        }];
        let first = replays_jsonl(&games, ReportMode::Clocked);
        let second = replays_jsonl(&games, ReportMode::Clocked);
        assert_eq!(first, second);
        assert!(first.contains("{\"t\":\"m\",\"game\":0,\"round\":0,\"slot\":0,\"piece\":\"T\",\"rot\":1,\"x\":-1,\"y\":2,\"spin\":\"full\",\"hold\":1,\"lc\":2,\"gc\":1,\"atk\":4,\"canc\":3,\"inb\":2,\"b2b\":-1,\"combo\":-1,\"ms\":1.2346}"));
    }

    #[test]
    fn escape_helper_handles_quotes_backslash() {
        assert_eq!(escape_json_string("a\"b\\c"), "a\\\"b\\\\c");
    }

    #[test]
    fn deterministic_summary_has_null_timing() {
        let result = result(
            Some(0),
            vec![move_record(0, Some(999.0)), move_record(1, Some(999.0))],
        );
        let games = [recorded(0, &result)];
        let summary = build_summary("rev", ReportMode::Deterministic, &games, Some(1));
        let json = summary_json(&summary);
        assert!(json.contains("\"move_ms_p50_a\":null"));
        assert!(json.contains("\"move_ms_p99_b\":null"));
        assert!(json.contains("\"budget_overruns_a\":0"));
        assert!(json.contains("\"budget_overruns_b\":0"));
    }

    #[test]
    fn summary_timing_fields_null_in_deterministic_clocked_in_clocked() {
        let result = result(
            Some(0),
            vec![move_record(0, Some(2.0)), move_record(1, Some(5.0))],
        );
        let games = [recorded(0, &result)];

        let deterministic = build_summary("rev", ReportMode::Deterministic, &games, Some(1));
        assert_eq!(deterministic.move_timing_count_a, 0);
        assert_eq!(deterministic.move_timing_count_b, 0);
        assert_eq!(deterministic.move_ms_max_a, None);
        assert_eq!(deterministic.move_ms_max_b, None);
        assert_eq!(deterministic.overrun_count_a, 0);
        assert_eq!(deterministic.overrun_count_b, 0);

        let clocked = build_summary("rev", ReportMode::Clocked, &games, Some(3));
        assert_eq!(clocked.move_timing_count_a, 1);
        assert_eq!(clocked.move_timing_count_b, 1);
        assert_eq!(clocked.move_ms_max_a, Some(2.0));
        assert_eq!(clocked.move_ms_max_b, Some(5.0));
        assert_eq!(clocked.overrun_count_a, 0);
        assert_eq!(clocked.overrun_count_b, 1);
    }

    #[test]
    fn experiment_manifest_records_sides_and_hash() {
        let spec = baseline_spec();
        let mut cfg_a = baseline_player_cfg("A", None);
        cfg_a.engine = EngineMode::Model;
        cfg_a.search.nn_scoring = NnScoringMode::PolicyProxy;
        cfg_a.search.nn_batch = NnBatchMode::Level;
        cfg_a.search.policy_proxy_weight = 0.25;
        let mut cfg_b = baseline_player_cfg("B", None);
        cfg_b.engine = EngineMode::Heuristic;
        let cli_args = vec!["--experiment".to_owned()];
        let input = ManifestInput {
            engine_rev: "rev",
            side_a: &cfg_a,
            side_b: &cfg_b,
            model: ManifestModel::Loaded {
                path: spec.model_path,
                bytes: 17,
            },
            ruleset: spec.ruleset,
            rng: spec.rng,
            garbage_model: spec.garbage_model,
            piece_cap: 6,
            seed_base: 77_000_002,
            seed_count: 2,
            cli_args: &cli_args,
        };
        let manifest = manifest_json_with_experiment(
            &input,
            Some(&ManifestExperiment {
                model_sha256: Some("model-hash"),
                model_bytes_len: Some(99),
                metadata_sha256: Some("metadata-hash"),
            }),
        );
        let value: serde_json::Value =
            serde_json::from_str(&manifest).expect("experiment manifest parses");

        assert_eq!(value["experiment"], true);
        assert_eq!(value["side_a"]["engine"], "model");
        assert_eq!(value["side_a"]["nn_scoring"], "policy-proxy");
        assert_eq!(value["side_a"]["nn_batch"], "level");
        assert_eq!(value["side_a"]["proxy_weight"], 0.25);
        assert_eq!(value["side_b"]["engine"], "heuristic");
        assert_eq!(value["model_sha256"], "model-hash");
        assert_eq!(value["model_bytes_len"], 99);
        assert_eq!(value["metadata_sha256"], "metadata-hash");
    }

    #[test]
    fn empty_summary_has_null_pair_score_and_ci() {
        let summary = build_summary("rev", ReportMode::Deterministic, &[], None);
        let json = summary_json(&summary);
        assert!(json.contains("\"games\":0"));
        assert!(json.contains("\"pair_score_a\":null"));
        assert!(json.contains("\"pair_wilson95_low\":null"));
        assert!(json.contains("\"pair_wilson95_high\":null"));
        assert!(json.contains("\"ci_status\":\"no_decisive_pairs\""));
    }

    #[test]
    fn bot_arena_manifest_b_matches_baseline_fixture() {
        let spec = baseline_spec();
        let cfg_a = baseline_player_cfg("A", Some(spec.clocked_budget_ms));
        let cfg_b = baseline_player_cfg("B", Some(spec.clocked_budget_ms));
        let cli_args = vec!["bot_arena".to_owned()];
        let manifest = manifest_json(&ManifestInput {
            engine_rev: "rev-under-test",
            side_a: &cfg_a,
            side_b: &cfg_b,
            model: ManifestModel::Loaded {
                path: spec.model_path,
                bytes: 123,
            },
            ruleset: spec.ruleset,
            rng: spec.rng,
            garbage_model: spec.garbage_model,
            piece_cap: spec.piece_cap,
            seed_base: spec.seed_base,
            seed_count: spec.seed_count,
            cli_args: &cli_args,
        });
        let manifest_value: serde_json::Value =
            serde_json::from_str(&manifest).expect("manifest should parse as json");
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("bot_arena")
            .join("baseline.json");
        let fixture =
            std::fs::read_to_string(fixture_path).expect("baseline fixture should exist on disk");
        let fixture_value: serde_json::Value =
            serde_json::from_str(&fixture).expect("baseline fixture should parse as json");
        let baseline_block = serde_json::json!({
            "label": "config-frozen baseline",
            "search": manifest_value["side_b"].clone(),
            "model": {
                "path": manifest_value["model"]["path"].clone(),
            },
            "piece_cap": manifest_value["piece_cap"].clone(),
            "ruleset": manifest_value["ruleset"].clone(),
            "rng": manifest_value["rng"].clone(),
            "garbage_model": manifest_value["garbage_model"].clone(),
            "seeds": manifest_value["seeds"].clone(),
        });
        let expected_fixture = baseline_fixture_json_for(&cfg_b.search, spec);
        let expected_fixture_value: serde_json::Value =
            serde_json::from_str(&expected_fixture).expect("expected fixture should parse as json");

        assert_eq!(baseline_block, fixture_value);
        assert_eq!(fixture, expected_fixture);
        assert_eq!(fixture_value, expected_fixture_value);
    }

    #[test]
    fn default_model_mode_b_block_still_matches_v1_fixture() {
        bot_arena_manifest_b_matches_baseline_fixture();
    }
}
