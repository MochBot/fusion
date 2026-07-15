use std::path::PathBuf;

use fusion_engine::eval::EvalWeights;
use fusion_engine::policy_value_runtime::PolicyValueRuntime;
use fusion_engine::versus::report::{replays_jsonl, RecordedGame, ReportMode};
use fusion_engine::versus::{
    baseline_player_cfg, play_game, play_game_with_stream_index, GameEndReason, GameResult,
    GameSeeds, PlayerCfg,
};

const DET_SEED: u64 = 12_345;
const DET_PIECE_CAP: u32 = 40;
const MODEL_PIECE_CAP: u32 = 4;
const MODEL_METADATA: &str = "models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json";

fn tiny_heuristic_cfg(label: &str) -> PlayerCfg {
    let mut cfg = baseline_player_cfg(label, None);
    cfg.search.beam_width = 16;
    cfg.search.depth = 2;
    cfg
}

fn tiny_clocked_cfg(label: &str) -> PlayerCfg {
    let mut cfg = baseline_player_cfg(label, Some(100));
    cfg.search.beam_width = 16;
    cfg.search.depth = 2;
    cfg
}

fn play_heuristic_pair() -> Vec<GameResult> {
    let cfg_a = tiny_heuristic_cfg("A");
    let cfg_b = tiny_heuristic_cfg("B");
    let weights = EvalWeights::default();
    vec![
        play_game_with_stream_index(
            GameSeeds {
                seed: DET_SEED,
                stream_game_idx: 0,
                report_game_idx: 0,
            },
            [&cfg_a, &cfg_b],
            None,
            &weights,
            DET_PIECE_CAP,
            &mut |_| {},
        ),
        play_game_with_stream_index(
            GameSeeds {
                seed: DET_SEED,
                stream_game_idx: 0,
                report_game_idx: 1,
            },
            [&cfg_b, &cfg_a],
            None,
            &weights,
            DET_PIECE_CAP,
            &mut |_| {},
        ),
    ]
}

fn serialize_heuristic_pair(results: &[GameResult]) -> String {
    let cfg_a = tiny_heuristic_cfg("A");
    let cfg_b = tiny_heuristic_cfg("B");
    let [game0, game1] = results else {
        panic!("heuristic smoke must serialize exactly two games");
    };
    let games = [
        RecordedGame {
            game: 0,
            seed: DET_SEED,
            slot0_label: &cfg_a.label,
            slot1_label: &cfg_b.label,
            a_slot: 0,
            result: game0,
        },
        RecordedGame {
            game: 1,
            seed: DET_SEED,
            slot0_label: &cfg_b.label,
            slot1_label: &cfg_a.label,
            a_slot: 1,
            result: game1,
        },
    ];
    replays_jsonl(&games, ReportMode::Deterministic)
}

fn assert_replay_ms_values_are_null(replay: &str) {
    let key = "\"ms\":";
    let mut count = 0usize;
    let mut remaining = replay;
    while let Some(offset) = remaining.find(key) {
        count += 1;
        let value = &remaining[offset + key.len()..];
        assert!(
            value.starts_with("null"),
            "deterministic replay serialized measured timing near: {value:.32}"
        );
        remaining = &value["null".len()..];
    }
    assert!(count > 0, "deterministic replay smoke recorded no moves");
}

#[test]
fn heuristic_pair_is_deterministic_within_one_process() {
    let first_results = play_heuristic_pair();
    let first_replay = serialize_heuristic_pair(&first_results);
    let second_results = play_heuristic_pair();
    let second_replay = serialize_heuristic_pair(&second_results);

    assert_eq!(first_results, second_results);
    assert_eq!(first_replay.as_bytes(), second_replay.as_bytes());
    assert_replay_ms_values_are_null(&first_replay);
    assert_replay_ms_values_are_null(&second_replay);
}

#[test]
fn model_smoke_returns_coherent_clocked_result_when_metadata_exists() {
    let metadata_path = PathBuf::from(MODEL_METADATA);
    if !metadata_path.exists() {
        eprintln!(
            "skipping model versus smoke test: {} is missing",
            metadata_path.display()
        );
        return;
    }
    eprintln!(
        "running model versus smoke test: {} is present",
        metadata_path.display()
    );

    let model = PolicyValueRuntime::load(&metadata_path).expect("load policy/value runtime");
    let cfg_a = tiny_clocked_cfg("A");
    let cfg_b = tiny_clocked_cfg("B");
    let weights = EvalWeights::default();
    let result = play_game(
        DET_SEED,
        0,
        [&cfg_a, &cfg_b],
        Some(&model),
        &weights,
        MODEL_PIECE_CAP,
        &mut |_| {},
    );

    assert!(result.rounds > 0, "clocked model game recorded no rounds");
    assert!(
        result.pieces[0] + result.pieces[1] > 0,
        "clocked model game locked no pieces"
    );
    assert!(
        result.per_move.iter().any(|record| record.ms.is_some()),
        "clocked model game recorded no measured move timings"
    );
    match result.reason {
        GameEndReason::Win | GameEndReason::TopOut => {
            assert!(result.winner.is_some(), "decisive game has no winner");
        }
        GameEndReason::DrawCap | GameEndReason::DrawSimul => {
            assert!(result.winner.is_none(), "draw game recorded a winner");
        }
    }
}
