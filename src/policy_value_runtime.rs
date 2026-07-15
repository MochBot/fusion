// Shared feature encoding - used by both native (tract) and WASM targets.
// These return flat Vecs to avoid any dependency on tract_ndarray.

use crate::board::Board;
use crate::header::{Move, Piece, SpinType};
use crate::state::GameState;

pub const TOTAL_FEATURES: usize = 854;
pub const MOVE_FEATURE_DIM: usize = 14;
pub const CANDIDATE_CAPACITY: usize = 64;
pub const MAX_INFER_BATCH: usize = 256;
const TRAINING_PIECE_ORDER: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::T,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
];

fn training_piece_index(piece: Piece) -> Option<usize> {
    TRAINING_PIECE_ORDER
        .iter()
        .position(|candidate| *candidate == piece)
}

fn encode_board_flat(board: &Board, out: &mut [f32]) {
    for x in 0..10 {
        for y in 0..40 {
            out[x * 40 + y] = if board.occupied(x as i32, y as i32) {
                1.0
            } else {
                0.0
            };
        }
    }
}

fn encode_piece_slots_flat(state: &GameState, out: &mut [f32]) {
    for value in out.iter_mut() {
        *value = 0.0;
    }
    if let Some(index) = training_piece_index(state.current) {
        out[index] = 1.0;
    }
    if let Some(hold) = state.hold.and_then(training_piece_index) {
        out[7 + hold] = 1.0;
    }
    for (slot, piece) in state.queue.iter().take(5).enumerate() {
        if let Some(index) = training_piece_index(*piece) {
            out[14 + slot * 7 + index] = 1.0;
        }
    }
}

/// Encode 854 state features as a flat Vec<f32>.
pub fn encode_state_features_flat(state: &GameState, opponent_board: &Board) -> Vec<f32> {
    let mut values = vec![0.0f32; TOTAL_FEATURES];
    encode_board_flat(&state.board, &mut values[0..400]);
    encode_board_flat(opponent_board, &mut values[400..800]);
    encode_piece_slots_flat(state, &mut values[800..849]);
    values[849] = (state.combo as f32 / 20.0).min(1.0);
    values[850] = (state.b2b as f32 / 10.0).min(1.0);
    values[851] = (state.lines_total as f32 / 100.0).min(1.0);
    values[852] = (state.pending_garbage as f32 / 12.0).min(1.0);
    values[853] = (state.bag_number as f32 / 20.0).min(1.0);
    values
}

/// Encode candidate move features as flat Vecs.
/// Returns (features: Vec<f32> of len CANDIDATE_CAPACITY * MOVE_FEATURE_DIM,
///          mask: Vec<bool> of len CANDIDATE_CAPACITY).
pub fn encode_candidate_features_flat(candidates: &[Move]) -> (Vec<f32>, Vec<bool>) {
    let mut values = vec![0.0f32; CANDIDATE_CAPACITY * MOVE_FEATURE_DIM];
    let mut mask = vec![false; CANDIDATE_CAPACITY];
    for (index, mv) in candidates.iter().enumerate() {
        mask[index] = true;
        let base = index * MOVE_FEATURE_DIM;
        if let Some(piece_index) = training_piece_index(mv.piece()) {
            values[base + piece_index] = 1.0;
        }
        values[base + 7 + mv.rotation() as usize] = 1.0;
        values[base + 11] = mv.x() as f32 / 9.0;
        values[base + 12] = mv.y() as f32 / 39.0;
        values[base + 13] = if mv.spin() == SpinType::NoSpin {
            0.0
        } else {
            1.0
        };
    }
    (values, mask)
}

// Native target - full tract-based runtime

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::Path;

    use tract_onnx::prelude::tract_data::internal::bail;
    use tract_onnx::prelude::*;

    use crate::board::Board;
    use crate::header::Move;
    use crate::state::GameState;

    #[derive(Clone, Debug, serde::Deserialize)]
    pub struct PolicyValueRuntimeManifest {
        pub schema_version: String,
        pub format: String,
        pub model_path: String,
        pub state_feature_dim: usize,
        pub move_feature_dim: usize,
        pub policy_output: String,
        pub value_output: String,
        pub policy_head_type: String,
        pub move_id_contract: String,
        pub candidate_capacity: usize,
        pub shared_input_contract: String,
    }

    impl PolicyValueRuntimeManifest {
        pub fn validate(&self) -> TractResult<()> {
            if self.schema_version != "phase2-runtime-v2" {
                bail!(
                    "unexpected policy/value runtime schema: {}",
                    self.schema_version
                );
            }
            if self.format != "onnx" {
                bail!("unexpected policy/value format: {}", self.format);
            }
            if self.state_feature_dim != super::TOTAL_FEATURES {
                bail!("unexpected state feature dim: {}", self.state_feature_dim);
            }
            if self.move_feature_dim != super::MOVE_FEATURE_DIM {
                bail!("unexpected move feature dim: {}", self.move_feature_dim);
            }
            if self.policy_head_type != "candidate_ranking" {
                bail!("unexpected policy head type: {}", self.policy_head_type);
            }
            if self.move_id_contract != "Move.raw" {
                bail!("unexpected move id contract: {}", self.move_id_contract);
            }
            if self.candidate_capacity != super::CANDIDATE_CAPACITY {
                bail!("unexpected candidate capacity: {}", self.candidate_capacity);
            }
            if self.shared_input_contract != "policy-value-shared-core-v2" {
                bail!(
                    "unexpected shared input contract: {}",
                    self.shared_input_contract
                );
            }
            Ok(())
        }
    }

    pub struct PolicyValueRuntime {
        pub manifest: PolicyValueRuntimeManifest,
        model: TypedRunnableModel<TypedModel>,
    }

    #[derive(Clone)]
    pub struct PolicyValueRuntimeContext {
        pub opponent_board: Board,
    }

    pub struct PolicyValueInference {
        pub policy_logits: Vec<f32>,
        pub value: f32,
    }

    impl PolicyValueRuntime {
        pub fn load(metadata_path: impl AsRef<Path>) -> TractResult<Self> {
            let metadata_path = metadata_path.as_ref();
            let manifest: PolicyValueRuntimeManifest =
                serde_json::from_str(&std::fs::read_to_string(metadata_path)?)?;
            manifest.validate()?;
            let model_path = metadata_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&manifest.model_path);
            let model = tract_onnx::onnx()
                .model_for_path(model_path)?
                .into_typed()?
                .into_runnable()?;
            Ok(Self { manifest, model })
        }

        pub fn infer(
            &self,
            state: &GameState,
            runtime: &PolicyValueRuntimeContext,
            candidates: &[Move],
        ) -> TractResult<PolicyValueInference> {
            if candidates.is_empty() {
                bail!("cannot run policy/value inference without candidates");
            }
            if candidates.len() > self.manifest.candidate_capacity {
                bail!(
                    "candidate count {} exceeds runtime capacity {}",
                    candidates.len(),
                    self.manifest.candidate_capacity
                );
            }
            let features = encode_state_features(state, &runtime.opponent_board);
            let (candidate_features, candidate_mask) = encode_candidate_features(candidates);
            let inputs = tvec![
                features.into_tensor().into(),
                candidate_features.into_tensor().into(),
                candidate_mask.into_tensor().into(),
            ];
            let outputs = self.model.run(inputs)?;
            let policy = outputs[0].to_array_view::<f32>()?;
            let value = outputs[1].to_array_view::<f32>()?;
            Ok(PolicyValueInference {
                policy_logits: policy.iter().copied().take(candidates.len()).collect(),
                value: value.iter().copied().next().unwrap_or(0.0),
            })
        }

        pub fn infer_batch_chunk(
            &self,
            states: &[&GameState],
            runtime: &PolicyValueRuntimeContext,
            candidates_per: &[&[Move]],
        ) -> TractResult<Vec<(Vec<f32>, f32)>> {
            if states.is_empty() {
                bail!("cannot run policy/value inference on an empty batch");
            }
            if states.len() != candidates_per.len() {
                bail!(
                    "batch state count {} does not match candidate row count {}",
                    states.len(),
                    candidates_per.len()
                );
            }
            if states.len() > super::MAX_INFER_BATCH {
                bail!(
                    "batch size {} exceeds maximum {}",
                    states.len(),
                    super::MAX_INFER_BATCH
                );
            }
            for (row, candidates) in candidates_per.iter().enumerate() {
                if candidates.is_empty() {
                    bail!("candidate row {row} is empty");
                }
                if candidates.len() > super::CANDIDATE_CAPACITY {
                    bail!(
                        "candidate row {row} count {} exceeds runtime capacity {}",
                        candidates.len(),
                        super::CANDIDATE_CAPACITY
                    );
                }
            }

            let batch_size = states.len();
            let Some(expected_state_values) = batch_size.checked_mul(super::TOTAL_FEATURES) else {
                bail!("batch state tensor length overflow");
            };
            let Some(expected_candidate_values) = batch_size
                .checked_mul(super::CANDIDATE_CAPACITY)
                .and_then(|value| value.checked_mul(super::MOVE_FEATURE_DIM))
            else {
                bail!("batch candidate tensor length overflow");
            };
            let Some(expected_mask_values) = batch_size.checked_mul(super::CANDIDATE_CAPACITY)
            else {
                bail!("batch mask tensor length overflow");
            };
            let mut state_values = Vec::with_capacity(expected_state_values);
            let mut candidate_values = Vec::with_capacity(expected_candidate_values);
            let mut mask_values = Vec::with_capacity(expected_mask_values);
            for (state, candidates) in states.iter().zip(candidates_per) {
                state_values.extend(super::encode_state_features_flat(
                    state,
                    &runtime.opponent_board,
                ));
                let (features, mask) = super::encode_candidate_features_flat(candidates);
                candidate_values.extend(features);
                mask_values.extend(mask);
            }
            if state_values.len() != expected_state_values
                || candidate_values.len() != expected_candidate_values
                || mask_values.len() != expected_mask_values
            {
                bail!(
                    "generated batch tensor lengths do not match shapes: state {}/{}, candidate {}/{}, mask {}/{}",
                    state_values.len(),
                    expected_state_values,
                    candidate_values.len(),
                    expected_candidate_values,
                    mask_values.len(),
                    expected_mask_values
                );
            }

            let state_tensor = tract_ndarray::Array2::from_shape_vec(
                (batch_size, super::TOTAL_FEATURES),
                state_values,
            )?;
            let candidate_tensor = tract_ndarray::Array3::from_shape_vec(
                (
                    batch_size,
                    super::CANDIDATE_CAPACITY,
                    super::MOVE_FEATURE_DIM,
                ),
                candidate_values,
            )?;
            let mask_tensor = tract_ndarray::Array2::from_shape_vec(
                (batch_size, super::CANDIDATE_CAPACITY),
                mask_values,
            )?;
            let outputs = self.model.run(tvec![
                state_tensor.into_tensor().into(),
                candidate_tensor.into_tensor().into(),
                mask_tensor.into_tensor().into(),
            ])?;
            let candidate_counts: Vec<usize> = candidates_per.iter().map(|row| row.len()).collect();
            decode_batch_outputs(outputs, &candidate_counts)
        }
    }

    fn decode_batch_outputs(
        outputs: TVec<TValue>,
        candidate_counts: &[usize],
    ) -> TractResult<Vec<(Vec<f32>, f32)>> {
        if outputs.len() != 2 {
            bail!(
                "policy/value runtime returned {} outputs instead of 2",
                outputs.len()
            );
        }
        split_batch_outputs(
            outputs[0].to_array_view::<f32>()?,
            outputs[1].to_array_view::<f32>()?,
            candidate_counts,
        )
    }

    fn split_batch_outputs(
        policy: tract_ndarray::ArrayViewD<'_, f32>,
        value: tract_ndarray::ArrayViewD<'_, f32>,
        candidate_counts: &[usize],
    ) -> TractResult<Vec<(Vec<f32>, f32)>> {
        let expected_policy_shape = [candidate_counts.len(), super::CANDIDATE_CAPACITY];
        let expected_value_shape = [candidate_counts.len()];
        if policy.shape() != expected_policy_shape {
            bail!(
                "unexpected policy output shape {:?}; expected {:?}",
                policy.shape(),
                expected_policy_shape
            );
        }
        if value.shape() != expected_value_shape {
            bail!(
                "unexpected value output shape {:?}; expected {:?}",
                value.shape(),
                expected_value_shape
            );
        }

        let policy_values: Vec<f32> = policy.iter().copied().collect();
        let value_values: Vec<f32> = value.iter().copied().collect();
        Ok(candidate_counts
            .iter()
            .enumerate()
            .map(|(row, candidate_count)| {
                let row_start = row * super::CANDIDATE_CAPACITY;
                (
                    policy_values[row_start..row_start + candidate_count].to_vec(),
                    value_values[row],
                )
            })
            .collect())
    }

    fn encode_state_features(
        state: &GameState,
        opponent_board: &Board,
    ) -> tract_ndarray::Array2<f32> {
        let values = super::encode_state_features_flat(state, opponent_board);
        tract_ndarray::Array2::from_shape_vec((1, super::TOTAL_FEATURES), values)
            .expect("fixed feature shape")
    }

    fn encode_candidate_features(
        candidates: &[Move],
    ) -> (tract_ndarray::Array3<f32>, tract_ndarray::Array2<bool>) {
        let (values, mask) = super::encode_candidate_features_flat(candidates);
        (
            tract_ndarray::Array3::from_shape_vec(
                (1, super::CANDIDATE_CAPACITY, super::MOVE_FEATURE_DIM),
                values,
            )
            .expect("fixed candidate feature shape"),
            tract_ndarray::Array2::from_shape_vec((1, super::CANDIDATE_CAPACITY), mask)
                .expect("fixed candidate mask shape"),
        )
    }

    pub use PolicyValueInference as Inference;
    pub use PolicyValueRuntime as Runtime;
    pub use PolicyValueRuntimeContext as RuntimeContext;

    #[cfg(test)]
    mod tests {
        use std::path::PathBuf;

        use super::super::{
            encode_candidate_features_flat, encode_state_features_flat, CANDIDATE_CAPACITY,
            MOVE_FEATURE_DIM, TOTAL_FEATURES,
        };
        use super::*;
        use crate::header::{Piece, Rotation};

        const MODEL_METADATA: &str =
            "models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json";

        fn load_checked_in_runtime() -> Option<PolicyValueRuntime> {
            let metadata_path = PathBuf::from(MODEL_METADATA);
            if !metadata_path.exists() {
                eprintln!(
                    "skipping policy/value batch test: {} is missing",
                    metadata_path.display()
                );
                return None;
            }
            Some(
                PolicyValueRuntime::load(&metadata_path)
                    .expect("load checked-in policy/value runtime"),
            )
        }

        fn batch_fixture() -> (Vec<GameState>, Vec<Vec<Move>>, PolicyValueRuntimeContext) {
            let states = vec![
                GameState::new(
                    Board::new(),
                    Piece::T,
                    vec![Piece::I, Piece::O, Piece::S, Piece::Z, Piece::J],
                ),
                GameState::new(
                    Board::new(),
                    Piece::I,
                    vec![Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z],
                ),
                GameState::new(
                    Board::new(),
                    Piece::L,
                    vec![Piece::Z, Piece::T, Piece::O, Piece::I, Piece::S],
                ),
            ];
            let candidates = vec![
                vec![
                    Move::new(Piece::T, Rotation::North, 3, 0, false),
                    Move::new(Piece::T, Rotation::East, 4, 1, false),
                    Move::new(Piece::T, Rotation::South, 2, 2, false),
                ],
                vec![Move::new(Piece::I, Rotation::North, 3, 0, false)],
                vec![
                    Move::new(Piece::L, Rotation::West, 5, 0, false),
                    Move::new(Piece::L, Rotation::East, 1, 2, false),
                ],
            ];
            let context = PolicyValueRuntimeContext {
                opponent_board: Board::new(),
            };
            (states, candidates, context)
        }

        fn assert_inference_close(actual: &(Vec<f32>, f32), expected: &PolicyValueInference) {
            assert_eq!(actual.0.len(), expected.policy_logits.len());
            for (actual_logit, expected_logit) in actual.0.iter().zip(&expected.policy_logits) {
                assert!((actual_logit - expected_logit).abs() <= 1.0e-6);
            }
            assert!((actual.1 - expected.value).abs() <= 1.0e-6);
        }

        #[test]
        fn candidate_feature_shape_matches_contract() {
            let (features, mask) = encode_candidate_features(&[Move::none(), Move::none()]);
            assert_eq!(features.shape(), &[1, CANDIDATE_CAPACITY, MOVE_FEATURE_DIM]);
            assert_eq!(mask.shape(), &[1, CANDIDATE_CAPACITY]);
            assert!(mask[[0, 0]]);
            assert!(mask[[0, 1]]);
            assert!(!mask[[0, 2]]);
        }

        #[test]
        fn state_feature_shape_matches_contract() {
            let state = GameState::new(Board::new(), Piece::T, vec![Piece::I, Piece::O]);
            let features = encode_state_features(&state, &Board::new());
            assert_eq!(features.shape(), &[1, TOTAL_FEATURES]);
        }

        #[test]
        fn batch_rows_match_scalar_infer_within_tolerance() {
            let Some(runtime) = load_checked_in_runtime() else {
                return;
            };
            let (states, candidates, context) = batch_fixture();
            let state_refs: Vec<&GameState> = states.iter().collect();
            let candidate_refs: Vec<&[Move]> = candidates.iter().map(Vec::as_slice).collect();

            let batch = runtime
                .infer_batch_chunk(&state_refs, &context, &candidate_refs)
                .expect("batch inference");

            for ((actual, state), row_candidates) in batch.iter().zip(&states).zip(&candidates) {
                let scalar = runtime
                    .infer(state, &context, row_candidates)
                    .expect("scalar inference");
                assert_inference_close(actual, &scalar);
            }
        }

        #[test]
        fn batch_of_one_matches_scalar() {
            let Some(runtime) = load_checked_in_runtime() else {
                return;
            };
            let (states, candidates, context) = batch_fixture();
            let batch = runtime
                .infer_batch_chunk(&[&states[1]], &context, &[&candidates[1]])
                .expect("single-row batch inference");
            let scalar = runtime
                .infer(&states[1], &context, &candidates[1])
                .expect("scalar inference");

            assert_eq!(batch.len(), 1);
            assert_inference_close(&batch[0], &scalar);
        }

        #[test]
        fn batch_mixed_padding_and_masks_correct() {
            let Some(runtime) = load_checked_in_runtime() else {
                return;
            };
            let (states, candidates, context) = batch_fixture();
            let order = [2, 0, 1];
            let state_refs: Vec<&GameState> = order.iter().map(|index| &states[*index]).collect();
            let candidate_refs: Vec<&[Move]> = order
                .iter()
                .map(|index| candidates[*index].as_slice())
                .collect();

            let original_state_refs: Vec<&GameState> = states.iter().collect();
            let original_candidate_refs: Vec<&[Move]> =
                candidates.iter().map(Vec::as_slice).collect();
            let original_batch = runtime
                .infer_batch_chunk(&original_state_refs, &context, &original_candidate_refs)
                .expect("original mixed batch inference");
            let permuted_batch = runtime
                .infer_batch_chunk(&state_refs, &context, &candidate_refs)
                .expect("permuted mixed batch inference");

            for (row, source_index) in order.into_iter().enumerate() {
                let scalar = runtime
                    .infer(&states[source_index], &context, &candidates[source_index])
                    .expect("scalar inference");
                let (_, mask) = encode_candidate_features_flat(&candidates[source_index]);
                assert!(mask[..candidates[source_index].len()]
                    .iter()
                    .all(|value| *value));
                assert!(mask[candidates[source_index].len()..]
                    .iter()
                    .all(|value| !*value));
                assert_eq!(permuted_batch[row].0.len(), candidates[source_index].len());
                assert_inference_close(&permuted_batch[row], &scalar);
                assert_eq!(permuted_batch[row], original_batch[source_index]);
            }

            let mut policy_a = tract_ndarray::Array2::zeros((3, CANDIDATE_CAPACITY));
            let mut policy_b = policy_a.clone();
            for (row, candidate_count) in candidates.iter().map(Vec::len).enumerate() {
                for column in candidate_count..CANDIDATE_CAPACITY {
                    policy_a[[row, column]] = 1_000.0;
                    policy_b[[row, column]] = -1_000.0;
                }
            }
            let values = tract_ndarray::Array1::zeros(3);
            let candidate_counts: Vec<usize> = candidates.iter().map(Vec::len).collect();
            let decoded_a = super::split_batch_outputs(
                policy_a.view().into_dyn(),
                values.view().into_dyn(),
                &candidate_counts,
            )
            .expect("decode first padded output");
            let decoded_b = super::split_batch_outputs(
                policy_b.view().into_dyn(),
                values.view().into_dyn(),
                &candidate_counts,
            )
            .expect("decode second padded output");
            assert_eq!(decoded_a, decoded_b, "padded logits must be inert");
        }

        #[test]
        fn batch_error_cases_explicit() {
            let Some(runtime) = load_checked_in_runtime() else {
                return;
            };
            let (states, candidates, context) = batch_fixture();
            let too_many_candidates = vec![Move::none(); CANDIDATE_CAPACITY + 1];
            let too_many_states: Vec<&GameState> = (0..=super::super::MAX_INFER_BATCH)
                .map(|_| &states[0])
                .collect();
            let too_many_rows: Vec<&[Move]> = (0..=super::super::MAX_INFER_BATCH)
                .map(|_| candidates[0].as_slice())
                .collect();

            let errors = [
                runtime
                    .infer_batch_chunk(&[], &context, &[])
                    .expect_err("empty batch must fail")
                    .to_string(),
                runtime
                    .infer_batch_chunk(&[&states[0]], &context, &[&[]])
                    .expect_err("empty candidate row must fail")
                    .to_string(),
                runtime
                    .infer_batch_chunk(&[&states[0]], &context, &[&too_many_candidates])
                    .expect_err("oversized candidate row must fail")
                    .to_string(),
                runtime
                    .infer_batch_chunk(&[&states[0]], &context, &[])
                    .expect_err("mismatched row counts must fail")
                    .to_string(),
                runtime
                    .infer_batch_chunk(&too_many_states, &context, &too_many_rows)
                    .expect_err("oversized batch must fail")
                    .to_string(),
            ];
            assert!(errors[0].contains("empty batch"));
            assert!(errors[1].contains("row 0 is empty"));
            assert!(errors[2].contains("row 0 count 65"));
            assert!(errors[3].contains("does not match"));
            assert!(errors[4].contains("batch size 257"));
        }

        #[test]
        fn batch_output_shape_validated() {
            let policy = tract_ndarray::Array2::zeros((2, CANDIDATE_CAPACITY));
            let short_policy = tract_ndarray::Array2::zeros((2, CANDIDATE_CAPACITY - 1));
            let value = tract_ndarray::Array1::zeros(2);
            let oversized_value = tract_ndarray::Array1::zeros(3);
            let candidate_counts = [1, 2];

            assert!(super::decode_batch_outputs(
                tvec![policy.clone().into_tensor().into()],
                &candidate_counts,
            )
            .is_err());
            assert!(super::decode_batch_outputs(
                tvec![
                    policy.clone().into_tensor().into(),
                    value.clone().into_tensor().into(),
                    value.clone().into_tensor().into(),
                ],
                &candidate_counts,
            )
            .is_err());
            assert!(super::split_batch_outputs(
                short_policy.view().into_dyn(),
                value.view().into_dyn(),
                &candidate_counts,
            )
            .is_err());
            assert!(super::split_batch_outputs(
                policy.view().into_dyn(),
                oversized_value.view().into_dyn(),
                &candidate_counts,
            )
            .is_err());
            assert!(super::split_batch_outputs(
                value.view().into_dyn(),
                policy.view().into_dyn(),
                &candidate_counts,
            )
            .is_err());
        }

        #[test]
        fn batch_dim_accepts_n_gt_1_on_checked_in_model() {
            let metadata_path = PathBuf::from(MODEL_METADATA);
            if !metadata_path.exists() {
                eprintln!(
                    "skipping batch-dimension spike: {} is missing",
                    metadata_path.display()
                );
                return;
            }

            let runtime = PolicyValueRuntime::load(&metadata_path)
                .expect("load checked-in policy/value runtime");
            let opponent_board = Board::new();
            let states = [
                GameState::new(
                    Board::new(),
                    Piece::T,
                    vec![Piece::I, Piece::O, Piece::S, Piece::Z, Piece::J],
                ),
                GameState::new(
                    Board::new(),
                    Piece::I,
                    vec![Piece::O, Piece::L, Piece::J, Piece::S, Piece::Z],
                ),
            ];
            let candidates = [Move::none(), Move::none()];
            let context = PolicyValueRuntimeContext {
                opponent_board: opponent_board.clone(),
            };
            let scalar = states.each_ref().map(|state| {
                runtime
                    .infer(state, &context, &candidates)
                    .expect("scalar inference")
            });

            for batch_size in [1, 2, 256] {
                let mut state_values = Vec::with_capacity(batch_size * TOTAL_FEATURES);
                let mut candidate_values =
                    Vec::with_capacity(batch_size * CANDIDATE_CAPACITY * MOVE_FEATURE_DIM);
                let mut mask_values = Vec::with_capacity(batch_size * CANDIDATE_CAPACITY);
                for row in 0..batch_size {
                    state_values.extend(encode_state_features_flat(
                        &states[row % states.len()],
                        &opponent_board,
                    ));
                    let (features, mask) = encode_candidate_features_flat(&candidates);
                    candidate_values.extend(features);
                    mask_values.extend(mask);
                }

                let state_tensor = tract_ndarray::Array2::from_shape_vec(
                    (batch_size, TOTAL_FEATURES),
                    state_values,
                )
                .expect("batch state shape");
                let candidate_tensor = tract_ndarray::Array3::from_shape_vec(
                    (batch_size, CANDIDATE_CAPACITY, MOVE_FEATURE_DIM),
                    candidate_values,
                )
                .expect("batch candidate shape");
                let mask_tensor = tract_ndarray::Array2::from_shape_vec(
                    (batch_size, CANDIDATE_CAPACITY),
                    mask_values,
                )
                .expect("batch mask shape");

                let outputs = runtime
                    .model
                    .run(tvec![
                        state_tensor.into_tensor().into(),
                        candidate_tensor.into_tensor().into(),
                        mask_tensor.into_tensor().into(),
                    ])
                    .unwrap_or_else(|error| panic!("BATCH_FAILS: {error}"));
                let policy = outputs[0]
                    .to_array_view::<f32>()
                    .expect("policy output view");
                let value = outputs[1]
                    .to_array_view::<f32>()
                    .expect("value output view");
                assert_eq!(policy.shape(), &[batch_size, CANDIDATE_CAPACITY]);
                assert_eq!(value.shape(), &[batch_size]);
                eprintln!(
                    "batch N={batch_size}: policy_shape={:?} value_shape={:?}",
                    policy.shape(),
                    value.shape()
                );

                let policy_values: Vec<f32> = policy.iter().copied().collect();
                let value_values: Vec<f32> = value.iter().copied().collect();
                for row in 0..batch_size {
                    let expected = &scalar[row % scalar.len()];
                    for (column, expected_logit) in expected.policy_logits.iter().enumerate() {
                        let actual = policy_values[row * CANDIDATE_CAPACITY + column];
                        assert!((actual - expected_logit).abs() <= 1.0e-6);
                    }
                    assert!((value_values[row] - expected.value).abs() <= 1.0e-6);
                }

                if batch_size == 2 {
                    let differing_policy = (0..CANDIDATE_CAPACITY).find(|column| {
                        policy_values[*column] != policy_values[CANDIDATE_CAPACITY + *column]
                    });
                    assert!(
                        differing_policy.is_some() || value_values[0] != value_values[1],
                        "differing states produced identical rows"
                    );
                    eprintln!(
                        "N=2 differing-state proof: policy_diff_column={differing_policy:?} value0={} value1={}",
                        value_values[0], value_values[1]
                    );
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod native {
    use crate::board::Board;
    use crate::header::Move;
    use crate::state::GameState;

    pub struct PolicyValueRuntime;

    #[derive(Clone)]
    pub struct PolicyValueRuntimeContext {
        pub opponent_board: Board,
    }

    pub struct PolicyValueInference {
        pub policy_logits: Vec<f32>,
        pub value: f32,
    }

    impl PolicyValueRuntime {
        pub fn load(_metadata_path: impl AsRef<std::path::Path>) -> Result<Self, String> {
            Err("policy/value runtime is native-only; wasm stays on heuristic fallback".to_string())
        }

        pub fn infer(
            &self,
            _state: &GameState,
            _runtime: &PolicyValueRuntimeContext,
            _candidates: &[Move],
        ) -> Result<PolicyValueInference, String> {
            Err("policy/value runtime is native-only; wasm stays on heuristic fallback".to_string())
        }

        pub fn infer_batch_chunk(
            &self,
            _states: &[&GameState],
            _runtime: &PolicyValueRuntimeContext,
            _candidates_per: &[&[Move]],
        ) -> Result<Vec<(Vec<f32>, f32)>, String> {
            Err("policy/value runtime is native-only; wasm stays on heuristic fallback".to_string())
        }
    }

    pub use PolicyValueInference as Inference;
    pub use PolicyValueRuntime as Runtime;
    pub use PolicyValueRuntimeContext as RuntimeContext;
}

pub use native::Inference as PolicyValueInference;
pub use native::Runtime as PolicyValueRuntime;
pub use native::RuntimeContext as PolicyValueRuntimeContext;
