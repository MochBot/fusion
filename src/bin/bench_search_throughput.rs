use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process;
use std::time::Instant;

use direct_cobra_copy::board::{Board, BOARD_HEIGHT, FULL_ROW};
use direct_cobra_copy::eval::EvalWeights;
use direct_cobra_copy::header::{Piece, COL_NB};
use direct_cobra_copy::policy_value_runtime::{PolicyValueRuntime, PolicyValueRuntimeContext};
use direct_cobra_copy::search::find_best_move_with_scores_runtime;
use direct_cobra_copy::search::{find_best_move_with_scores, SearchConfig, SearchResultFull};
use direct_cobra_copy::search_expand::{
    reset_search_expansion_stats, search_expansion_stats, SearchExpansionStats,
};
use direct_cobra_copy::state::GameState;
use serde::Deserialize;

const REPLAY_PROFILER_FIXTURE: &str = include_str!("../../fixtures/search_profiler_cases.json");

#[derive(Debug, Eq, PartialEq)]
struct BenchOptions {
    model_metadata: Option<PathBuf>,
}

impl BenchOptions {
    fn parse_from<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();
        let mut model_metadata = None;

        while let Some(arg) = args.next() {
            if arg == "--model" {
                let value = args.next().unwrap_or_else(|| {
                    panic!("usage: bench_search_throughput [--model <onnx-metadata.json>]")
                });
                model_metadata = Some(PathBuf::from(value));
            } else {
                panic!(
                    "unknown argument {:?}; usage: bench_search_throughput [--model <onnx-metadata.json>]",
                    arg
                );
            }
        }

        Self { model_metadata }
    }
}

struct BenchCase {
    name: String,
    case_id: String,
    replay_id: String,
    round: u32,
    player: String,
    state: GameState,
    opponent_board: Board,
    expected_best_raw: Option<u16>,
}

#[derive(Deserialize)]
struct ReplayProfilerCaseFixture {
    case_id: String,
    replay_id: String,
    round: u32,
    player: String,
    name: String,
    current: String,
    hold: Option<String>,
    queue: Vec<String>,
    board_rows: Vec<[u16; 2]>,
    opponent_rows: Vec<[u16; 2]>,
    expected: ReplayProfilerExpectedFixture,
    #[serde(default)]
    b2b: u8,
    #[serde(default)]
    combo: u32,
    #[serde(default)]
    pending_garbage: u8,
    #[serde(default)]
    lines_total: u32,
    #[serde(default)]
    bag_number: u32,
    #[serde(default)]
    pieces_into_bag: u8,
}

#[derive(Deserialize)]
struct ReplayProfilerExpectedFixture {
    best_move_raw: u16,
}

fn sync_cols(board: &mut Board) {
    for x in 0..COL_NB {
        let mask = 1u16 << x;
        let mut col = 0u64;
        for y in 0..BOARD_HEIGHT {
            if board.rows[y] & mask != 0 {
                col |= 1u64 << y;
            }
        }
        board.cols[x] = col;
    }
}

fn board_from_rows(rows: &[(usize, u16)]) -> Board {
    let mut board = Board::new();
    for &(y, row) in rows {
        board.rows[y] = row;
    }
    sync_cols(&mut board);
    board
}

fn rows_from_fixture(rows: &[[u16; 2]], label: &str) -> Vec<(usize, u16)> {
    rows.iter()
        .map(|[y, row]| {
            let y = usize::from(*y);
            assert!(y < BOARD_HEIGHT, "{label} row y out of bounds: {y}");
            assert!(
                row & !FULL_ROW == 0,
                "{label} row has cells outside board: {row}"
            );
            (y, *row)
        })
        .collect()
}

fn well_board(height: usize, open_col: usize) -> Board {
    let mut rows = Vec::with_capacity(height);
    for y in 0..height {
        rows.push((y, FULL_ROW & !(1u16 << open_col)));
    }
    board_from_rows(&rows)
}

fn ragged_board() -> Board {
    board_from_rows(&[
        (0, 0b11_1111_0111),
        (1, 0b11_0111_1111),
        (2, 0b11_1110_1111),
        (3, 0b10_1111_1111),
        (4, 0b11_1101_1111),
        (5, 0b01_1111_1111),
        (6, 0b11_1111_1011),
        (7, 0b11_1011_1111),
        (8, 0b10_1111_0111),
        (9, 0b01_1110_1111),
    ])
}

fn piece_from_mino(mino: &str) -> Piece {
    match mino {
        "I" => Piece::I,
        "J" => Piece::J,
        "L" => Piece::L,
        "O" => Piece::O,
        "S" => Piece::S,
        "T" => Piece::T,
        "Z" => Piece::Z,
        _ => panic!("unknown piece mino {mino}"),
    }
}

fn replay_profiler_cases() -> Vec<BenchCase> {
    serde_json::from_str::<Vec<ReplayProfilerCaseFixture>>(REPLAY_PROFILER_FIXTURE)
        .unwrap_or_else(|err| panic!("parse replay profiler fixture: {err}"))
        .into_iter()
        .map(|fixture| {
            let rows = rows_from_fixture(&fixture.board_rows, &fixture.case_id);
            let opponent_rows = rows_from_fixture(&fixture.opponent_rows, &fixture.case_id);
            let mut state = GameState::new(
                board_from_rows(&rows),
                piece_from_mino(&fixture.current),
                fixture
                    .queue
                    .iter()
                    .map(|piece| piece_from_mino(piece))
                    .collect(),
            );
            state.hold = fixture.hold.as_deref().map(piece_from_mino);
            state.b2b = fixture.b2b;
            state.combo = fixture.combo;
            state.pending_garbage = fixture.pending_garbage;
            state.lines_total = fixture.lines_total;
            state.bag_number = fixture.bag_number;
            state.pieces_into_bag = fixture.pieces_into_bag;
            BenchCase {
                name: fixture.name,
                case_id: fixture.case_id,
                replay_id: fixture.replay_id,
                round: fixture.round,
                player: fixture.player,
                state,
                opponent_board: board_from_rows(&opponent_rows),
                expected_best_raw: Some(fixture.expected.best_move_raw),
            }
        })
        .collect()
}

fn add_stats(total: &mut SearchExpansionStats, stats: SearchExpansionStats) {
    total.expanded_nodes += stats.expanded_nodes;
    total.movegen_calls += stats.movegen_calls;
    total.action_builder_states += stats.action_builder_states;
    total.duplicate_action_generations += stats.duplicate_action_generations;
    total.action_generation_nanos += stats.action_generation_nanos;
    total.legal_filter_nanos += stats.legal_filter_nanos;
    total.runtime_inference_nanos += stats.runtime_inference_nanos;
    total.child_eval_nanos += stats.child_eval_nanos;
    total.do_move_nanos += stats.do_move_nanos;
    total.eval_fallback_nanos += stats.eval_fallback_nanos;
    total.tt_hash_nanos += stats.tt_hash_nanos;
    total.tt_probe_nanos += stats.tt_probe_nanos;
    total.tt_store_nanos += stats.tt_store_nanos;
    total.sort_prune_truncate_nanos += stats.sort_prune_truncate_nanos;
    total.candidate_copy_nanos += stats.candidate_copy_nanos;
    total.root_score_aggregation_nanos += stats.root_score_aggregation_nanos;
    total.unique_action_keys += stats.unique_action_keys;
    total.repeated_action_builds += stats.repeated_action_builds;
    total.cache_hits += stats.cache_hits;
    total.cache_misses += stats.cache_misses;
}

fn format_stats_fragment(stats: &SearchExpansionStats) -> String {
    format!(
        "expanded_nodes={} movegen_calls={} action_builder_states={} duplicate_action_generations={} unique_action_keys={} repeated_action_builds={} cache_hits={} cache_misses={} action_generation_ns={} legal_filter_ns={} candidate_copy_ns={} runtime_inference_ns={} child_eval_ns={} do_move_ns={} eval_fallback_ns={} tt_hash_ns={} tt_probe_ns={} tt_store_ns={} sort_prune_truncate_ns={} root_score_aggregation_ns={}",
        stats.expanded_nodes,
        stats.movegen_calls,
        stats.action_builder_states,
        stats.duplicate_action_generations,
        stats.unique_action_keys,
        stats.repeated_action_builds,
        stats.cache_hits,
        stats.cache_misses,
        stats.action_generation_nanos,
        stats.legal_filter_nanos,
        stats.candidate_copy_nanos,
        stats.runtime_inference_nanos,
        stats.child_eval_nanos,
        stats.do_move_nanos,
        stats.eval_fallback_nanos,
        stats.tt_hash_nanos,
        stats.tt_probe_nanos,
        stats.tt_store_nanos,
        stats.sort_prune_truncate_nanos,
        stats.root_score_aggregation_nanos,
    )
}

fn piece_label(piece: Piece) -> &'static str {
    match piece {
        Piece::I => "I",
        Piece::J => "J",
        Piece::L => "L",
        Piece::O => "O",
        Piece::S => "S",
        Piece::T => "T",
        Piece::Z => "Z",
    }
}

fn optional_piece_label(piece: Option<Piece>) -> &'static str {
    piece.map(piece_label).unwrap_or("none")
}

fn queue_fragment(queue: &[Piece]) -> String {
    queue
        .iter()
        .map(|piece| piece_label(*piece))
        .collect::<Vec<_>>()
        .join("")
}

fn board_fingerprint(board: &Board) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for row in board.rows {
        hash ^= u64::from(row);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn cases() -> Vec<BenchCase> {
    let mut hold_state = GameState::new(
        well_board(8, 4),
        Piece::I,
        vec![Piece::T, Piece::L, Piece::O, Piece::S, Piece::Z, Piece::J],
    );
    hold_state.hold = Some(Piece::T);
    hold_state.pending_garbage = 4;

    let mut cases = vec![
        BenchCase {
            name: "empty_t".to_string(),
            case_id: "synthetic-empty-t".to_string(),
            replay_id: "synthetic".to_string(),
            round: 0,
            player: "synthetic".to_string(),
            state: GameState::new(
                Board::new(),
                Piece::T,
                vec![Piece::I, Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z],
            ),
            opponent_board: Board::new(),
            expected_best_raw: None,
        },
        BenchCase {
            name: "well_i_hold".to_string(),
            case_id: "synthetic-well-i-hold".to_string(),
            replay_id: "synthetic".to_string(),
            round: 0,
            player: "synthetic".to_string(),
            state: hold_state,
            opponent_board: Board::new(),
            expected_best_raw: None,
        },
        BenchCase {
            name: "ragged_s".to_string(),
            case_id: "synthetic-ragged-s".to_string(),
            replay_id: "synthetic".to_string(),
            round: 0,
            player: "synthetic".to_string(),
            state: GameState::new(
                ragged_board(),
                Piece::S,
                vec![Piece::Z, Piece::T, Piece::I, Piece::O, Piece::L, Piece::J],
            ),
            opponent_board: Board::new(),
            expected_best_raw: None,
        },
    ];
    cases.extend(replay_profiler_cases());
    cases
}

fn bench_config(runtime_enabled: bool) -> SearchConfig {
    if runtime_enabled {
        return SearchConfig {
            beam_width: 8,
            depth: 1,
            use_tt: true,
            extend_queue_7bag: false,
            quiescence_max_extensions: 0,
            ..SearchConfig::default()
        };
    }

    SearchConfig {
        beam_width: 160,
        depth: 5,
        use_tt: true,
        extend_queue_7bag: false,
        quiescence_max_extensions: 0,
        ..SearchConfig::default()
    }
}

fn runtime_case_status(case_name: &str, result: &SearchResultFull) -> Result<(), String> {
    if result.fallback_used {
        return Err(format!(
            "case={case_name} fallback_used=true in runtime profiler mode"
        ));
    }
    Ok(())
}

fn main() {
    let options = BenchOptions::parse_from(env::args_os());
    let config = bench_config(options.model_metadata.is_some());
    let weights = EvalWeights::default();
    let runtime = options.model_metadata.as_ref().map(|path| {
        PolicyValueRuntime::load(path).unwrap_or_else(|err| {
            eprintln!("load runtime {}: {err}", path.display());
            process::exit(1);
        })
    });

    let mut total_elapsed = 0.0;
    let mut total_stats = SearchExpansionStats::default();

    println!(
        "packed_movegen_feature={}",
        cfg!(feature = "packed_movegen")
    );
    println!(
        "config depth={} beam_width={} use_tt={} extend_queue_7bag={} quiescence_max_extensions={}",
        config.depth,
        config.beam_width,
        config.use_tt,
        config.extend_queue_7bag,
        config.quiescence_max_extensions
    );

    for case in cases() {
        direct_cobra_copy::search_expand::set_search_profiling_enabled(true);
        reset_search_expansion_stats();
        let started = Instant::now();
        let runtime_context = PolicyValueRuntimeContext {
            opponent_board: case.opponent_board.clone(),
        };
        let result = if let Some(runtime) = runtime.as_ref() {
            find_best_move_with_scores_runtime(
                &case.state,
                &config,
                &weights,
                runtime,
                &runtime_context,
            )
        } else {
            find_best_move_with_scores(&case.state, &config, &weights)
        }
        .unwrap_or_else(|| panic!("search produced no result for {}", case.name));
        if runtime.is_some() {
            if let Err(err) = runtime_case_status(&case.name, &result) {
                eprintln!("{err}");
                process::exit(1);
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        let stats = search_expansion_stats();
        let stats_fragment = format_stats_fragment(&stats);
        let queue = queue_fragment(&case.state.queue);
        let board_fingerprint = board_fingerprint(&case.state.board);
        let expanded_per_sec = if elapsed > 0.0 {
            stats.expanded_nodes as f64 / elapsed
        } else {
            0.0
        };
        let pv_raw = result
            .best
            .pv
            .iter()
            .map(|mv| mv.raw().to_string())
            .collect::<Vec<_>>()
            .join(",");
        let root_top = result
            .root_scores
            .iter()
            .take(5)
            .map(|(mv, score)| format!("{}:{score:.3}", mv.raw()))
            .collect::<Vec<_>>()
            .join(",");

        println!(
            "case={} case_id={} replay_id={} round={} player={} piece={} current={} hold={} queue={} board_fingerprint={:016x} elapsed_ms={:.3} {} expanded_nodes_per_sec={:.2} best_raw={} expected_best_raw={} hold_used={} score={:.6} fallback_used={} pv_raw=[{}] root_top=[{}]",
            case.name,
            case.case_id,
            case.replay_id,
            case.round,
            case.player,
            piece_label(case.state.current),
            piece_label(case.state.current),
            optional_piece_label(case.state.hold),
            queue,
            board_fingerprint,
            elapsed * 1000.0,
            stats_fragment,
            expanded_per_sec,
            result.best.best_move.raw(),
            case.expected_best_raw.map_or_else(|| "none".to_string(), |raw| raw.to_string()),
            result.best.hold_used,
            result.best.score,
            result.fallback_used,
            pv_raw,
            root_top,
        );

        total_elapsed += elapsed;
        add_stats(&mut total_stats, stats);
    }
    direct_cobra_copy::search_expand::set_search_profiling_enabled(false);

    let total_expanded_per_sec = if total_elapsed > 0.0 {
        total_stats.expanded_nodes as f64 / total_elapsed
    } else {
        0.0
    };
    let total_stats_fragment = format_stats_fragment(&total_stats);
    println!(
        "summary elapsed_ms={:.3} {} expanded_nodes_per_sec={:.2}",
        total_elapsed * 1000.0,
        total_stats_fragment,
        total_expanded_per_sec,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use direct_cobra_copy::search::{SearchResult, SearchResultFull};
    use direct_cobra_copy::state::CoachingState;
    use std::path::PathBuf;

    #[test]
    fn parses_runtime_model_argument() {
        let options = BenchOptions::parse_from([
            "bench_search_throughput",
            "--model",
            "models/pvc-real-r03.onnx.metadata.json",
        ]);

        assert_eq!(
            options.model_metadata,
            Some(PathBuf::from("models/pvc-real-r03.onnx.metadata.json"))
        );
    }

    #[test]
    fn runtime_model_uses_bounded_search_config() {
        let config = bench_config(true);

        assert_eq!(config.depth, 1);
        assert_eq!(config.beam_width, 8);
        assert!(config.use_tt);
        assert!(!config.extend_queue_7bag);
        assert_eq!(config.quiescence_max_extensions, 0);
    }

    #[test]
    fn loads_replay_profiler_fixture() {
        let cases = replay_profiler_cases();

        assert!(cases.len() >= 4);
        assert_eq!(cases[0].name, "golden_replay_opening_t");
        assert_eq!(cases[0].case_id, "golden-replay-opening-t-1164");
        assert_eq!(cases[0].replay_id, "golden-fixture-sample.ttrm");
        assert_eq!(cases[0].round, 1);
        assert_eq!(cases[0].player, "player1");
        assert_eq!(cases[0].state.current, Piece::T);
        assert_eq!(cases[0].state.hold, Some(Piece::I));
        assert_eq!(
            cases[0].state.queue,
            vec![Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z]
        );
        assert_eq!(cases[0].state.pending_garbage, 3);
        assert_eq!(cases[0].state.board.rows[0], 0b11_1111_0111);
        assert_eq!(cases[0].state.board.rows[1], 0b11_1110_1111);
        assert_eq!(cases[0].opponent_board.rows[0], 0b00_0000_1111);
        assert_eq!(cases[0].expected_best_raw, Some(452));
    }

    #[test]
    fn replay_profiler_fixture_covers_required_piece_and_stack_shapes() {
        let cases = replay_profiler_cases();
        let pieces = cases
            .iter()
            .map(|case| case.state.current)
            .collect::<Vec<_>>();

        assert!(pieces.contains(&Piece::L));
        assert!(pieces.contains(&Piece::J));
        assert!(pieces.contains(&Piece::O));
        assert!(cases.iter().any(|case| case.state.board.height() >= 20));
    }

    fn search_result(fallback_used: bool) -> SearchResultFull {
        SearchResultFull {
            best: SearchResult {
                best_move: direct_cobra_copy::header::Move::none(),
                hold_used: false,
                score: 0.0,
                pv: Vec::new(),
                coaching_state: CoachingState::default(),
                pv_clear_events: Vec::new(),
            },
            root_scores: Vec::new(),
            position_complexity: 0.0,
            board_score: 0.0,
            attack_score: 0.0,
            chain_score: 0.0,
            context_score: 0.0,
            path_attack: 0.0,
            path_chain: 0.0,
            path_context: 0.0,
            policy_score: 0.0,
            value_score: 0.0,
            fallback_used,
        }
    }

    #[test]
    fn runtime_mode_rejects_fallback_results() {
        assert!(runtime_case_status("case-a", &search_result(true)).is_err());
        assert!(runtime_case_status("case-a", &search_result(false)).is_ok());
    }

    #[test]
    fn stats_fragment_includes_profiler_timing_buckets() {
        let stats = direct_cobra_copy::search_expand::SearchExpansionStats {
            action_generation_nanos: 1,
            legal_filter_nanos: 2,
            runtime_inference_nanos: 3,
            child_eval_nanos: 4,
            do_move_nanos: 5,
            eval_fallback_nanos: 6,
            tt_probe_nanos: 7,
            sort_prune_truncate_nanos: 8,
            candidate_copy_nanos: 9,
            root_score_aggregation_nanos: 10,
            unique_action_keys: 11,
            repeated_action_builds: 12,
            cache_hits: 13,
            cache_misses: 14,
            tt_hash_nanos: 15,
            tt_store_nanos: 16,
            ..Default::default()
        };

        let fragment = format_stats_fragment(&stats);

        assert!(fragment.contains("action_generation_ns=1"));
        assert!(fragment.contains("legal_filter_ns=2"));
        assert!(fragment.contains("runtime_inference_ns=3"));
        assert!(fragment.contains("child_eval_ns=4"));
        assert!(fragment.contains("do_move_ns=5"));
        assert!(fragment.contains("eval_fallback_ns=6"));
        assert!(fragment.contains("tt_probe_ns=7"));
        assert!(fragment.contains("sort_prune_truncate_ns=8"));
        assert!(fragment.contains("candidate_copy_ns=9"));
        assert!(fragment.contains("root_score_aggregation_ns=10"));
        assert!(fragment.contains("unique_action_keys=11"));
        assert!(fragment.contains("repeated_action_builds=12"));
        assert!(fragment.contains("cache_hits=13"));
        assert!(fragment.contains("cache_misses=14"));
        assert!(fragment.contains("tt_hash_ns=15"));
        assert!(fragment.contains("tt_store_ns=16"));
    }
}
