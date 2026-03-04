from __future__ import annotations

import sys
import unittest
from pathlib import Path

import torch

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from models.policy_value import PolicyValueNet


class PolicyValueModelTests(unittest.TestCase):
    def test_forward_shapes(self) -> None:
        model = PolicyValueNet()
        out = model(
            torch.zeros((2, 854), dtype=torch.float32),
            torch.zeros((2, 3, 14), dtype=torch.float32),
            torch.tensor([[True, True, False], [True, False, False]]),
            torch.zeros((2, 56), dtype=torch.float32),
        )
        self.assertEqual(tuple(out["player_policy_logits"].shape), (2, 3))
        self.assertEqual(tuple(out["search_policy_logits"].shape), (2, 3))
        self.assertEqual(tuple(out["value"].shape), (2,))
        self.assertTrue(torch.isinf(out["player_policy_logits"][0, 2]))
        self.assertTrue(torch.isinf(out["search_policy_logits"][0, 2]))

    def test_search_and_value_outputs_ignore_player_aux_context(self) -> None:
        model = PolicyValueNet()
        features = torch.randn((1, 854), dtype=torch.float32)
        candidate_moves = torch.randn((1, 3, 14), dtype=torch.float32)
        candidate_mask = torch.tensor([[True, True, False]])
        zeros = torch.zeros((1, 56), dtype=torch.float32)
        ones = torch.ones((1, 56), dtype=torch.float32)
        out_zero = model(features, candidate_moves, candidate_mask, zeros)
        out_one = model(features, candidate_moves, candidate_mask, ones)
        self.assertTrue(torch.allclose(out_zero["search_policy_logits"], out_one["search_policy_logits"]))
        self.assertTrue(torch.allclose(out_zero["value"], out_one["value"]))
