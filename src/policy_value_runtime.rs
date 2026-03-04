// ---------------------------------------------------------------------------
// Shared feature encoding — used by both native (tract) and WASM targets.
// These return flat Vecs to avoid any dependency on tract_ndarray.
// ---------------------------------------------------------------------------

use crate::board::Board;
use crate::header::{Move, Piece, SpinType};
use crate::state::GameState;

pub const TOTAL_FEATURES: usize = 854;
pub const MOVE_FEATURE_DIM: usize = 14;
pub const CANDIDATE_CAPACITY: usize = 64;
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

// ---------------------------------------------------------------------------
// Native target — full tract-based runtime
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::Path;

    use tract_onnx::prelude::tract_data::internal::bail;
    use tract_onnx::prelude::*;

    use crate::board::Board;
    use crate::header::{Move, Piece};
    use crate::state::GameState;

    pub use super::{CANDIDATE_CAPACITY, MOVE_FEATURE_DIM, TOTAL_FEATURES};

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
        use super::*;

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
    }
}

#[cfg(target_arch = "wasm32")]
mod native {
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
    }

    pub use PolicyValueInference as Inference;
    pub use PolicyValueRuntime as Runtime;
    pub use PolicyValueRuntimeContext as RuntimeContext;
}

pub use native::Inference as PolicyValueInference;
pub use native::Runtime as PolicyValueRuntime;
pub use native::RuntimeContext as PolicyValueRuntimeContext;
