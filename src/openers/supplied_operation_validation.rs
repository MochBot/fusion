use std::fmt;

use serde::{Deserialize, Serialize};

use crate::board::Board;
use crate::header::{Move, Piece};
use crate::move_buffer::MoveBuffer;
use crate::movegen::{generate_playable, move_reachable};
use crate::openers::board::rows_to_masks_floor_up;

#[derive(Debug)]
pub enum SuppliedOperationValidationError {
    MalformedBatch(serde_json::Error),
    UnsupportedSchemaVersion(u32),
    InvalidBatchIdentity,
}

impl fmt::Display for SuppliedOperationValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedBatch(error) => {
                write!(formatter, "malformed supplied-operation batch: {error}")
            }
            Self::UnsupportedSchemaVersion(found) => {
                write!(
                    formatter,
                    "unsupported supplied-operation schema version: {found}"
                )
            }
            Self::InvalidBatchIdentity => {
                write!(formatter, "runId and inputAssetSha256 are invalid")
            }
        }
    }
}

impl std::error::Error for SuppliedOperationValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MalformedBatch(error) => Some(error),
            Self::UnsupportedSchemaVersion(_) | Self::InvalidBatchIdentity => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BatchV1 {
    schema_version: u32,
    run_id: String,
    input_asset_sha256: String,
    candidates: Vec<CandidateInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CandidateInput {
    candidate_id: String,
    candidate_digest: String,
    record_id: String,
    search_shape_index: u32,
    start_rows: Vec<String>,
    steps: Vec<StepInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StepInput {
    piece: String,
    cells: [[u8; 2]; 4],
    expected_rows_after: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuppliedOperationResultsV1 {
    schema_version: u32,
    run_id: String,
    input_asset_sha256: String,
    fusion_revision: &'static str,
    results: Vec<CandidateResult>,
}

#[derive(Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum CandidateResult {
    WitnessFound {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        search_shape_index: u32,
        witness: Witness,
    },
    InvalidInput {
        candidate_id: String,
        candidate_digest: String,
        record_id: String,
        search_shape_index: u32,
        reason: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Witness {
    queue_scope: &'static str,
    start_rows: Vec<String>,
    steps: Vec<StepWitness>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StepWitness {
    piece: &'static str,
    rotation: &'static str,
    x: i32,
    y: i32,
    spin: &'static str,
    cells: [[u8; 2]; 4],
    rows_before_clear: Vec<String>,
    rows_after: Vec<String>,
}

pub fn validate_supplied_operation_batch(
    batch_json: &[u8],
) -> Result<SuppliedOperationResultsV1, SuppliedOperationValidationError> {
    let batch: BatchV1 = serde_json::from_slice(batch_json)
        .map_err(SuppliedOperationValidationError::MalformedBatch)?;
    if batch.schema_version != 1 {
        return Err(SuppliedOperationValidationError::UnsupportedSchemaVersion(
            batch.schema_version,
        ));
    }
    if batch.run_id.is_empty() || !is_sha256(&batch.input_asset_sha256) {
        return Err(SuppliedOperationValidationError::InvalidBatchIdentity);
    }
    let results = batch.candidates.iter().map(validate_candidate).collect();
    Ok(SuppliedOperationResultsV1 {
        schema_version: 1,
        run_id: batch.run_id,
        input_asset_sha256: batch.input_asset_sha256,
        fusion_revision: option_env!("FUSION_REVISION").unwrap_or(env!("CARGO_PKG_VERSION")),
        results,
    })
}

fn validate_candidate(candidate: &CandidateInput) -> CandidateResult {
    match replay(candidate) {
        Ok(witness) => CandidateResult::WitnessFound {
            candidate_id: candidate.candidate_id.clone(),
            candidate_digest: candidate.candidate_digest.clone(),
            record_id: candidate.record_id.clone(),
            search_shape_index: candidate.search_shape_index,
            witness,
        },
        Err(reason) => CandidateResult::InvalidInput {
            candidate_id: candidate.candidate_id.clone(),
            candidate_digest: candidate.candidate_digest.clone(),
            record_id: candidate.record_id.clone(),
            search_shape_index: candidate.search_shape_index,
            reason,
        },
    }
}

fn replay(candidate: &CandidateInput) -> Result<Witness, String> {
    if candidate.candidate_id.is_empty()
        || candidate.record_id.is_empty()
        || !is_sha256(&candidate.candidate_digest)
        || candidate.steps.is_empty()
        || !rows_are_valid(&candidate.start_rows)
    {
        return Err("candidate fields are invalid".to_owned());
    }
    let mut board = board_from_rows(&candidate.start_rows);
    let mut witnesses = Vec::with_capacity(candidate.steps.len());
    for step in &candidate.steps {
        if !rows_are_valid(&step.expected_rows_after) {
            return Err("expectedRowsAfter is invalid".to_owned());
        }
        let piece = piece_from_name(&step.piece).ok_or_else(|| "piece is invalid".to_owned())?;
        let target = exact_reachable_move(&board, piece, step.cells)
            .ok_or_else(|| "supplied operation is not a reachable legal Fusion lock".to_owned())?;
        let mut before_clear = board.clone();
        before_clear.place(&target);
        board.lock(&target);
        let rows_after = board_rows(board.rows);
        if rows_after != step.expected_rows_after {
            return Err("supplied operation does not reproduce expectedRowsAfter".to_owned());
        }
        witnesses.push(StepWitness {
            piece: piece_name(piece),
            rotation: rotation_name(target),
            x: target.x(),
            y: target.y(),
            spin: spin_name(target),
            cells: sorted_cells(target).ok_or_else(|| "move cells are invalid".to_owned())?,
            rows_before_clear: board_rows(before_clear.rows),
            rows_after,
        });
    }
    Ok(Witness {
        queue_scope: "notClaimedMidConstruction",
        start_rows: candidate.start_rows.clone(),
        steps: witnesses,
    })
}

fn exact_reachable_move(board: &Board, piece: Piece, cells: [[u8; 2]; 4]) -> Option<Move> {
    let mut expected = cells;
    expected.sort_unstable();
    let mut moves = MoveBuffer::new();
    generate_playable(board, &mut moves, piece, false);
    moves.iter().copied().find(|target| {
        sorted_cells(*target) == Some(expected)
            && board.legal_lock_placement(target)
            && move_reachable(board, target, false)
    })
}

fn board_from_rows(rows: &[String]) -> Board {
    let mut board = Board::new();
    for (y, row) in rows_to_masks_floor_up(rows).into_iter().enumerate() {
        board.rows[y] = row;
        for x in 0..10 {
            if row & (1 << x) != 0 {
                board.cols[x] |= 1u64 << y;
            }
        }
    }
    board
}

fn sorted_cells(target: Move) -> Option<[[u8; 2]; 4]> {
    let offsets = target.cells();
    let points = [
        (target.x(), target.y()),
        (
            target.x() + i32::from(offsets[0].x),
            target.y() + i32::from(offsets[0].y),
        ),
        (
            target.x() + i32::from(offsets[1].x),
            target.y() + i32::from(offsets[1].y),
        ),
        (
            target.x() + i32::from(offsets[2].x),
            target.y() + i32::from(offsets[2].y),
        ),
    ];
    let mut cells = [[0; 2]; 4];
    for (index, (x, y)) in points.into_iter().enumerate() {
        cells[index] = [u8::try_from(x).ok()?, u8::try_from(y).ok()?];
    }
    cells.sort_unstable();
    Some(cells)
}

fn board_rows(rows: [u16; 40]) -> Vec<String> {
    let Some(last) = rows.iter().rposition(|row| *row != 0) else {
        return Vec::new();
    };
    rows[..=last]
        .iter()
        .rev()
        .map(|row| {
            (0..10)
                .map(|x| if row & (1 << x) == 0 { '_' } else { 'X' })
                .collect()
        })
        .collect()
}

fn rows_are_valid(rows: &[String]) -> bool {
    rows.len() <= 40
        && rows
            .iter()
            .all(|row| row.len() == 10 && row.bytes().all(|cell| cell == b'_' || cell == b'X'))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn piece_from_name(name: &str) -> Option<Piece> {
    match name {
        "I" => Some(Piece::I),
        "O" => Some(Piece::O),
        "T" => Some(Piece::T),
        "L" => Some(Piece::L),
        "J" => Some(Piece::J),
        "S" => Some(Piece::S),
        "Z" => Some(Piece::Z),
        _ => None,
    }
}

fn piece_name(piece: Piece) -> &'static str {
    match piece {
        Piece::I => "I",
        Piece::O => "O",
        Piece::T => "T",
        Piece::L => "L",
        Piece::J => "J",
        Piece::S => "S",
        Piece::Z => "Z",
    }
}

fn rotation_name(target: Move) -> &'static str {
    match target.rotation() {
        crate::header::Rotation::North => "north",
        crate::header::Rotation::East => "east",
        crate::header::Rotation::South => "south",
        crate::header::Rotation::West => "west",
    }
}

fn spin_name(target: Move) -> &'static str {
    match target.spin() {
        crate::header::SpinType::NoSpin => "none",
        crate::header::SpinType::Mini => "mini",
        crate::header::SpinType::Full => "full",
    }
}
