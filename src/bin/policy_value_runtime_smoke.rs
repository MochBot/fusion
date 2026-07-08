use std::env;
use std::path::PathBuf;

use fusion_engine::board::{Board, BOARD_HEIGHT, FULL_ROW};
use fusion_engine::eval::EvalWeights;
use fusion_engine::header::Piece;
use fusion_engine::header::COL_NB;
use fusion_engine::policy_value_runtime::{PolicyValueRuntime, PolicyValueRuntimeContext};
use fusion_engine::search::{find_best_move_with_scores_runtime, SearchConfig};
use fusion_engine::state::GameState;

fn constrained_board() -> Board {
    let mut board = Board::new();
    for y in 0..4 {
        board.rows[y] = FULL_ROW & !(1u16 << 4);
    }
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
    board
}

fn main() {
    let metadata_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: policy_value_runtime_smoke <onnx-metadata.json>");

    println!("loading_runtime={}", metadata_path.display());
    let runtime = PolicyValueRuntime::load(&metadata_path).expect("load policy/value runtime");
    println!("runtime_loaded=true");
    let runtime_context = PolicyValueRuntimeContext {
        opponent_board: Board::new(),
    };
    let state = GameState::new(constrained_board(), Piece::I, vec![Piece::O, Piece::L]);
    let result = find_best_move_with_scores_runtime(
        &state,
        &SearchConfig {
            depth: 1,
            beam_width: 8,
            extend_queue_7bag: false,
            ..SearchConfig::default()
        },
        &EvalWeights::default(),
        &runtime,
        &runtime_context,
    )
    .expect("native runtime search result");
    println!("search_completed=true");

    println!("best_raw={}", result.best.best_move.raw());
    println!("best_score={:.6}", result.best.score);
    println!("policy_score={:.6}", result.policy_score);
    println!("value_score={:.6}", result.value_score);
    println!("fallback_used={}", result.fallback_used);
}
