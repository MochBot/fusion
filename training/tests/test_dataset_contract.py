from __future__ import annotations

import tempfile
import types
import sys
import unittest
from pathlib import Path
from typing import cast

import numpy as np
import torch
from torch.utils.data import Subset

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from data.dataset import FusionBinaryDataset, MirrorAugmentedDataset
from utils.example_schema import (
    CanonicalExample,
    ExampleIdentity,
    flatten_example,
    group_ids_path,
    metadata_path,
    stable_group_hash,
    write_dataset_metadata,
)

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
    def save_hyperparameters(self, *args: object, **kwargs: object) -> None:
        return None


setattr(lightning_stub, "LightningModule", _LightningModule)
setattr(lightning_stub, "LightningDataModule", _LightningDataModule)
setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
sys.modules.setdefault("lightning", lightning_stub)
sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

from models.lit_module import FusionDataModule


def _example(group: str, frame_id: int) -> CanonicalExample:
    return CanonicalExample(
        identity=ExampleIdentity(
            schema_version="phase0-v1",
            replay_id=group,
            round_id=0,
            player_id=0,
            frame_id=frame_id,
            group_id=group,
        ),
        player_board=np.zeros((40, 10), dtype=np.float32),
        opponent_board=np.zeros((40, 10), dtype=np.float32),
        current_piece="i",
        hold_piece=None,
        queue=("o",),
        combo=0,
        b2b=0,
        lines_cleared_total=frame_id,
        pending_garbage=0,
        bag_number=0,
        game_outcome=1.0,
        lines_sent=0.0,
        b2b_after=0.0,
        position_normalized=0.0,
        time_to_topout=1.0,
    )


def _write_dataset(path: Path, examples: list[CanonicalExample]) -> None:
    with path.open("wb") as f:
        for example in examples:
            f.write(flatten_example(example).astype(np.float32).tobytes())
    np.asarray([stable_group_hash(example.identity.group_id) for example in examples], dtype=np.uint64).tofile(
        group_ids_path(path)
    )
    write_dataset_metadata(path, len(examples))


class DatasetContractTests(unittest.TestCase):
    def test_dataset_requires_metadata_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "training_data.bin"
            path.write_bytes(b"\x00" * (859 * 4))
            with self.assertRaises(FileNotFoundError):
                FusionBinaryDataset(path, mirror_augment=False)

    def test_dataset_loads_group_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "training_data.bin"
            _write_dataset(path, [_example("g1", 1), _example("g2", 2)])
            ds = FusionBinaryDataset(path, mirror_augment=False)
            self.assertEqual(ds.num_raw_samples, 2)
            self.assertEqual(len(ds.group_ids), 2)

    def test_mirror_augmented_dataset_doubles_length(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "training_data.bin"
            ex = _example("g1", 1)
            ex.player_board[39][0] = 1.0
            _write_dataset(path, [ex])
            base = FusionBinaryDataset(path, mirror_augment=False)
            mirrored = MirrorAugmentedDataset(base)
            self.assertEqual(len(mirrored), 2)
            first = mirrored[0]["features"].numpy()
            second = mirrored[1]["features"].numpy()
            self.assertFalse(np.array_equal(first[:400], second[:400]))

    def test_datamodule_splits_by_group(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "training_data.bin"
            examples = [
                _example("g1", 1),
                _example("g1", 2),
                _example("g2", 3),
                _example("g2", 4),
            ]
            _write_dataset(path, examples)
            data = FusionDataModule(str(path), batch_size=2, num_workers=0, val_split=0.5, mirror_augment=False)
            data.setup()
            assert data.train_ds is not None and data.val_ds is not None
            train_subset = cast(Subset[dict[str, torch.Tensor]], data.train_ds)
            val_subset = cast(Subset[dict[str, torch.Tensor]], data.val_ds)
            train_dataset = cast(FusionBinaryDataset, cast(object, train_subset.dataset))
            val_dataset = cast(FusionBinaryDataset, cast(object, val_subset.dataset))
            train_groups = {int(train_dataset.group_ids[idx]) for idx in train_subset.indices}
            val_groups = {int(val_dataset.group_ids[idx]) for idx in val_subset.indices}
            self.assertTrue(train_groups)
            self.assertTrue(val_groups)
            self.assertTrue(train_groups.isdisjoint(val_groups))


if __name__ == "__main__":
    unittest.main()
