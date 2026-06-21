use std::env;
use std::fs;
use std::path::PathBuf;

use direct_cobra_copy::board::{Board, BOARD_HEIGHT, FULL_ROW};
use direct_cobra_copy::header::{Move, Piece, COL_NB};
use direct_cobra_copy::move_buffer::MoveBuffer;
use direct_cobra_copy::movegen::generate;
use direct_cobra_copy::policy_value_runtime::{
    encode_candidate_features_flat, encode_state_features_flat, PolicyValueRuntime,
    PolicyValueRuntimeContext, CANDIDATE_CAPACITY, MOVE_FEATURE_DIM, TOTAL_FEATURES,
};
use direct_cobra_copy::state::GameState;

const LOGIT_TOLERANCE: f32 = 1.0e-4;
const RANK_MARGIN_THRESHOLD: f32 = 1.0e-3;

#[derive(serde::Serialize)]
struct Fixture {
    schema_version: &'static str,
    scalar_scope: &'static str,
    metadata_path: String,
    model_path: String,
    state_feature_dim: usize,
    move_feature_dim: usize,
    candidate_capacity: usize,
    logit_tolerance: f32,
    rank_margin_threshold: f32,
    positions: Vec<FixturePosition>,
}

#[derive(serde::Serialize)]
struct FixturePosition {
    id: String,
    description: String,
    source: SourcePosition,
    state_features: Vec<f32>,
    candidate_features: Vec<f32>,
    candidate_mask: Vec<bool>,
    move_count: usize,
    moves: Vec<MoveDescriptor>,
    native: NativeOutputs,
    rank: RankMetadata,
}

#[derive(serde::Serialize)]
struct SourcePosition {
    board_rows: Vec<u16>,
    opponent_board_rows: Vec<u16>,
    current_piece: u8,
    queue: Vec<u8>,
    hold: Option<u8>,
    combo: u32,
    b2b: u8,
    lines_total: u32,
    pending_garbage: u8,
    bag_number: u32,
}

#[derive(serde::Serialize)]
struct MoveDescriptor {
    index: usize,
    raw: u16,
    piece: u8,
    rotation: u8,
    x: i32,
    y: i32,
    spin: u8,
}

#[derive(serde::Serialize)]
struct NativeOutputs {
    policy_logits: Vec<f32>,
    value: f32,
    best_index: usize,
    best_raw: u16,
}

#[derive(serde::Serialize)]
struct RankMetadata {
    top1_margin: f32,
    top3_adjacent_min_margin: f32,
    rank_checks_enabled: bool,
}

struct PositionSpec {
    id: &'static str,
    description: &'static str,
    board: Board,
    opponent_board: Board,
    current: Piece,
    queue: Vec<Piece>,
    hold: Option<Piece>,
}

fn main() {
    let metadata_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: policy_value_runtime_parity_fixture <metadata.json> <out.json>");
    let output_path = env::args_os()
        .nth(2)
        .map(PathBuf::from)
        .expect("usage: policy_value_runtime_parity_fixture <metadata.json> <out.json>");

    let runtime = PolicyValueRuntime::load(&metadata_path).expect("load policy/value runtime");
    let model_path = metadata_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(&runtime.manifest.model_path);

    let mut positions = Vec::new();
    for spec in position_specs() {
        positions.push(build_fixture_position(&runtime, spec));
    }

    let fixture = Fixture {
        schema_version: "runtime-parity-v1",
        scalar_scope: "zero-only",
        metadata_path: metadata_path.display().to_string(),
        model_path: model_path.display().to_string(),
        state_feature_dim: TOTAL_FEATURES,
        move_feature_dim: MOVE_FEATURE_DIM,
        candidate_capacity: CANDIDATE_CAPACITY,
        logit_tolerance: LOGIT_TOLERANCE,
        rank_margin_threshold: RANK_MARGIN_THRESHOLD,
        positions,
    };

    let json = serde_json::to_vec_pretty(&fixture).expect("serialize parity fixture");
    fs::write(output_path, json).expect("write parity fixture");
}

fn build_fixture_position(runtime: &PolicyValueRuntime, spec: PositionSpec) -> FixturePosition {
    let mut state = GameState::new(spec.board.clone(), spec.current, spec.queue.clone());
    state.hold = spec.hold;

    let mut buffer = MoveBuffer::new();
    generate(&state.board, &mut buffer, state.current, false);
    let candidates = buffer.as_slice().to_vec();
    assert!(
        !candidates.is_empty(),
        "parity position {} produced no candidates",
        spec.id
    );
    assert!(
        candidates.len() <= CANDIDATE_CAPACITY,
        "parity position {} produced {} candidates, above capacity {}",
        spec.id,
        candidates.len(),
        CANDIDATE_CAPACITY
    );

    let runtime_context = PolicyValueRuntimeContext {
        opponent_board: spec.opponent_board.clone(),
    };
    let inference = runtime
        .infer(&state, &runtime_context, &candidates)
        .expect("native policy/value inference");

    let state_features = encode_state_features_flat(&state, &spec.opponent_board);
    let (candidate_features, candidate_mask) = encode_candidate_features_flat(&candidates);
    let moves: Vec<MoveDescriptor> = candidates.iter().enumerate().map(move_descriptor).collect();
    let best_index = best_index(&inference.policy_logits);
    let rank = rank_metadata(&inference.policy_logits);

    FixturePosition {
        id: spec.id.to_string(),
        description: spec.description.to_string(),
        source: SourcePosition {
            board_rows: spec.board.rows.to_vec(),
            opponent_board_rows: spec.opponent_board.rows.to_vec(),
            current_piece: piece_to_external(spec.current),
            queue: spec.queue.iter().copied().map(piece_to_external).collect(),
            hold: spec.hold.map(piece_to_external),
            combo: 0,
            b2b: 0,
            lines_total: 0,
            pending_garbage: 0,
            bag_number: 0,
        },
        state_features,
        candidate_features,
        candidate_mask,
        move_count: candidates.len(),
        moves,
        native: NativeOutputs {
            policy_logits: inference.policy_logits,
            value: inference.value,
            best_index,
            best_raw: candidates[best_index].raw(),
        },
        rank,
    }
}

fn position_specs() -> Vec<PositionSpec> {
    vec![
        PositionSpec {
            id: "empty-board-t",
            description: "Empty board with T current and short queue",
            board: Board::new(),
            opponent_board: Board::new(),
            current: Piece::T,
            queue: vec![Piece::I, Piece::O, Piece::S, Piece::Z, Piece::J],
            hold: None,
        },
        PositionSpec {
            id: "constrained-stack-i",
            description: "Four-row stack with a column-4 well and I current",
            board: constrained_board(),
            opponent_board: opponent_board(),
            current: Piece::I,
            queue: vec![Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z],
            hold: Some(Piece::T),
        },
        PositionSpec {
            id: "midgame-l",
            description: "Deterministic midgame stack with L current",
            board: midgame_board(),
            opponent_board: Board::new(),
            current: Piece::L,
            queue: vec![Piece::T, Piece::I, Piece::O, Piece::S, Piece::Z],
            hold: Some(Piece::J),
        },
    ]
}

fn constrained_board() -> Board {
    let mut rows = [0u16; BOARD_HEIGHT];
    for row in rows.iter_mut().take(4) {
        *row = FULL_ROW & !(1u16 << 4);
    }
    board_from_rows(rows)
}

fn midgame_board() -> Board {
    let mut rows = [0u16; BOARD_HEIGHT];
    rows[0] = 0b11_1011_0111;
    rows[1] = 0b11_0011_1111;
    rows[2] = 0b10_1111_1011;
    rows[3] = 0b11_1101_0011;
    rows[4] = 0b00_1111_0111;
    rows[5] = 0b00_1011_1111;
    rows[6] = 0b00_0011_1011;
    board_from_rows(rows)
}

fn opponent_board() -> Board {
    let mut rows = [0u16; BOARD_HEIGHT];
    rows[0] = 0b00_0000_1111;
    rows[1] = 0b00_0001_1111;
    rows[2] = 0b00_0011_0111;
    board_from_rows(rows)
}

fn board_from_rows(rows: [u16; BOARD_HEIGHT]) -> Board {
    let mut board = Board::new();
    board.rows = rows;
    for x in 0..COL_NB {
        board.cols[x] = board.col(x);
    }
    board
}

fn move_descriptor((index, mv): (usize, &Move)) -> MoveDescriptor {
    MoveDescriptor {
        index,
        raw: mv.raw(),
        piece: piece_to_external(mv.piece()),
        rotation: mv.rotation() as u8,
        x: mv.x(),
        y: mv.y(),
        spin: mv.spin() as u8,
    }
}

fn best_index(logits: &[f32]) -> usize {
    let mut best = 0usize;
    for index in 1..logits.len() {
        if logits[index] > logits[best] {
            best = index;
        }
    }
    best
}

fn rank_metadata(logits: &[f32]) -> RankMetadata {
    let mut ranked: Vec<f32> = logits.to_vec();
    ranked.sort_by(|left, right| right.total_cmp(left));

    let top1_margin = if ranked.len() >= 2 {
        ranked[0] - ranked[1]
    } else {
        f32::INFINITY
    };
    let top3_adjacent_min_margin = if ranked.len() >= 3 {
        (ranked[0] - ranked[1]).min(ranked[1] - ranked[2])
    } else {
        f32::INFINITY
    };
    let rank_checks_enabled =
        top1_margin >= RANK_MARGIN_THRESHOLD && top3_adjacent_min_margin >= RANK_MARGIN_THRESHOLD;

    RankMetadata {
        top1_margin,
        top3_adjacent_min_margin,
        rank_checks_enabled,
    }
}

fn piece_to_external(piece: Piece) -> u8 {
    match piece {
        Piece::I => 0,
        Piece::O => 1,
        Piece::T => 2,
        Piece::S => 3,
        Piece::Z => 4,
        Piece::J => 5,
        Piece::L => 6,
    }
}
