from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

import torch

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from scripts.export_policy_value_onnx import (
    PHASE2_RUNTIME_SCHEMA_VERSION,
    POLICY_VALUE_SHARED_INPUT_CONTRACT,
    _PolicyValueOnnxWrapper,
    build_policy_value_onnx_metadata,
    policy_value_onnx_metadata_path,
    policy_value_onnx_path,
)
from models.policy_value import PolicyValueNet


class ExportPolicyValueOnnxTests(unittest.TestCase):
    def test_path_helpers_use_phase2_suffixes(self) -> None:
        checkpoint = Path("/tmp/model.ckpt")
        self.assertEqual(policy_value_onnx_path(checkpoint).name, "model.ckpt.policy_value.onnx")
        self.assertEqual(
            policy_value_onnx_metadata_path(checkpoint).name,
            "model.ckpt.policy_value.onnx.metadata.json",
        )

    def test_metadata_builder_matches_runtime_contract(self) -> None:
        metadata = build_policy_value_onnx_metadata(model_path="model.onnx")
        self.assertEqual(metadata["schema_version"], PHASE2_RUNTIME_SCHEMA_VERSION)
        self.assertEqual(metadata["format"], "onnx")
        self.assertEqual(metadata["policy_head_type"], "candidate_ranking")
        self.assertEqual(metadata["move_id_contract"], "Move.raw")
        self.assertEqual(metadata["shared_input_contract"], POLICY_VALUE_SHARED_INPUT_CONTRACT)

    def test_wrapper_elides_mask_input_for_inference(self) -> None:
        model = PolicyValueNet()
        wrapper = _PolicyValueOnnxWrapper(model)
        features = torch.zeros((2, 854), dtype=torch.float32)
        candidate_moves = torch.zeros((2, 3, 14), dtype=torch.float32)
        candidate_mask = torch.ones((2, 3), dtype=torch.bool)
        policy_logits, value = wrapper(features, candidate_moves, candidate_mask)
        self.assertEqual(tuple(policy_logits.shape), (2, 3))
        self.assertEqual(tuple(value.shape), (2,))


if __name__ == "__main__":
    unittest.main()
