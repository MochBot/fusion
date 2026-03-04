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
lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")


class _LightningModule(torch.nn.Module):
    def __init__(self) -> None:
        super().__init__()

    def save_hyperparameters(self, *args: object, **kwargs: object) -> None:
        return None

    def log(self, *args: object, **kwargs: object) -> None:
        return None


class _LightningDataModule:
    pass


setattr(lightning_stub, "LightningModule", _LightningModule)
setattr(lightning_stub, "LightningDataModule", _LightningDataModule)
setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
sys.modules.setdefault("lightning", lightning_stub)
sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

from models.lit_module import TeacherLitModule


class TeacherTargetContractTests(unittest.TestCase):
    def test_bag_number_reads_named_scalar_slot(self) -> None:
        module = TeacherLitModule()
        labels = torch.tensor([[1.0, 0.1, 0.0, 0.2, 0.9]], dtype=torch.float32)
        # slot 2 intentionally looks like a large lines value; slot 4 is the actual bag slot.
        scalars = torch.tensor([[0.0, 0.0, 0.95, 0.0, 0.0]], dtype=torch.float32)
        _, phase = module._derive_targets(labels, scalars)
        self.assertEqual(int(phase[0]), 0)

    def test_garbage_pending_uses_named_slot_for_survival(self) -> None:
        module = TeacherLitModule()
        labels = torch.tensor([[1.0, 0.1, 0.0, 0.2, 0.9]], dtype=torch.float32)
        scalars = torch.tensor([[0.0, 0.0, 0.0, 0.8, 0.5]], dtype=torch.float32)
        _, phase = module._derive_targets(labels, scalars)
        self.assertEqual(int(phase[0]), 2)


if __name__ == "__main__":
    unittest.main()
