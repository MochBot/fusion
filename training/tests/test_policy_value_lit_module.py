from __future__ import annotations

import sys
import types
import unittest
from pathlib import Path

import torch

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

lightning_stub = types.ModuleType("lightning")
setattr(lightning_stub, "LightningModule", object)
lightning_types = types.ModuleType("lightning.pytorch.utilities.types")
setattr(lightning_types, "OptimizerLRScheduler", object)
_ = sys.modules.setdefault("lightning", lightning_stub)
_ = sys.modules.setdefault("lightning.pytorch", types.ModuleType("lightning.pytorch"))
_ = sys.modules.setdefault("lightning.pytorch.utilities", types.ModuleType("lightning.pytorch.utilities"))
_ = sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_types)

from models.policy_value_lit_module import compute_policy_value_metrics, masked_search_policy_kl_loss


class PolicyValueLitModuleMetricTests(unittest.TestCase):
    def test_metrics_report_zero_availability_cleanly(self) -> None:
        batch = {
            "search_best_index": torch.tensor([0, 1], dtype=torch.int64),
            "player_policy_index": torch.tensor([-1, -1], dtype=torch.int64),
            "player_policy_available": torch.tensor([False, False]),
        }
        out = {
            "search_policy_logits": torch.tensor([[2.0, 1.0], [0.0, 1.0]], dtype=torch.float32),
            "player_policy_logits": torch.tensor([[2.0, 1.0], [0.0, 1.0]], dtype=torch.float32),
            "value": torch.tensor([0.1, 0.2], dtype=torch.float32),
        }
        metrics = compute_policy_value_metrics(batch, out)
        self.assertEqual(float(metrics["player_target_availability_rate"]), 0.0)
        self.assertEqual(float(metrics["player_policy_top1_accuracy"]), 0.0)
        self.assertEqual(float(metrics["player_policy_top3_accuracy"]), 0.0)

    def test_metrics_report_available_player_ranking(self) -> None:
        batch = {
            "search_best_index": torch.tensor([0, 2], dtype=torch.int64),
            "player_policy_index": torch.tensor([0, 1], dtype=torch.int64),
            "player_policy_available": torch.tensor([True, True]),
        }
        out = {
            "search_policy_logits": torch.tensor([[3.0, 1.0, 0.0], [0.0, 1.0, 2.0]], dtype=torch.float32),
            "player_policy_logits": torch.tensor([[3.0, 1.0, 0.0], [2.0, 1.0, 0.0]], dtype=torch.float32),
            "value": torch.tensor([0.1, 0.2], dtype=torch.float32),
        }
        metrics = compute_policy_value_metrics(batch, out)
        self.assertEqual(float(metrics["player_target_availability_rate"]), 1.0)
        self.assertAlmostEqual(float(metrics["player_policy_top1_accuracy"]), 0.5)
        self.assertAlmostEqual(float(metrics["search_policy_top1_accuracy"]), 1.0)
        self.assertGreaterEqual(float(metrics["player_policy_mean_rank"]), 1.0)

    def test_masked_search_policy_loss_ignores_padded_candidates(self) -> None:
        search_policy_logits = torch.tensor(
            [[2.0, 0.0, float("-inf")], [1.5, float("-inf"), float("-inf")]],
            dtype=torch.float32,
        )
        search_policy_probs = torch.tensor(
            [[0.75, 0.25, 0.0], [1.0, 0.0, 0.0]],
            dtype=torch.float32,
        )
        candidate_mask = torch.tensor(
            [[True, True, False], [True, False, False]],
            dtype=torch.bool,
        )

        loss = masked_search_policy_kl_loss(search_policy_logits, search_policy_probs, candidate_mask)

        self.assertTrue(torch.isfinite(loss))
        expected_first = 0.75 * (torch.log(torch.tensor(0.75)) - torch.log_softmax(torch.tensor([2.0, 0.0]), dim=0)[0])
        expected_first += 0.25 * (torch.log(torch.tensor(0.25)) - torch.log_softmax(torch.tensor([2.0, 0.0]), dim=0)[1])
        self.assertAlmostEqual(float(loss), float(expected_first / 2.0), places=6)
