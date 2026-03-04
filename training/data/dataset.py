"""Schema-aware memory-mapped dataset with group sidecars and mirror augmentation."""

from __future__ import annotations

from pathlib import Path
from typing import Protocol, cast

import numpy as np
import torch
from torch.utils.data import Dataset

try:
    from ..utils.config import (
        BOARD_CELLS,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        BYTES_PER_SAMPLE,
        FLOATS_PER_SAMPLE,
        OPPONENT_BOARD_FEATURES,
        PLAYER_BOARD_FEATURES,
        TOTAL_FEATURES,
    )
    from ..utils.example_schema import (
        group_ids_path,
        load_dataset_metadata,
        validate_binary_artifacts,
    )
except ImportError:
    from utils.config import (
        BOARD_CELLS,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        BYTES_PER_SAMPLE,
        FLOATS_PER_SAMPLE,
        OPPONENT_BOARD_FEATURES,
        PLAYER_BOARD_FEATURES,
        TOTAL_FEATURES,
    )
    from utils.example_schema import (
        group_ids_path,
        load_dataset_metadata,
        validate_binary_artifacts,
    )


class FusionBinaryDataset(Dataset[dict[str, torch.Tensor]]):
    """Reads contiguous f32 binary produced by preprocess_replays.py.

    Each sample = 859 floats (854 features + 5 labels).
    Layout: [player_board(400) | opponent_board(400) | pieces(49) | scalars(5) | labels(5)]

    Horizontal mirror augmentation flips both boards left-right (column reflection)
    while keeping piece one-hots and scalars unchanged. This doubles effective dataset
    size without storing extra data.
    """

    def __init__(
        self,
        path: str | Path,
        mirror_augment: bool = True,
    ) -> None:
        self.path = Path(path)
        if not self.path.exists():
            msg = f"Binary data file not found: {self.path}"
            raise FileNotFoundError(msg)

        self.metadata = load_dataset_metadata(self.path)

        file_size = self.path.stat().st_size
        if file_size % BYTES_PER_SAMPLE != 0:
            msg = (
                f"File size {file_size} is not a multiple of "
                f"sample size {BYTES_PER_SAMPLE}"
            )
            raise ValueError(msg)

        self.num_raw_samples = file_size // BYTES_PER_SAMPLE
        self.mirror_augment = mirror_augment
        validate_binary_artifacts(self.path, self.num_raw_samples)
        self.data = np.memmap(
            self.path, dtype=np.float32, mode="r",
            shape=(self.num_raw_samples, FLOATS_PER_SAMPLE),
        )
        self.group_ids = np.memmap(
            group_ids_path(self.path), dtype=np.uint64, mode="r", shape=(self.num_raw_samples,)
        )

    def __len__(self) -> int:
        return self.num_raw_samples * 2 if self.mirror_augment else self.num_raw_samples

    def __getitem__(self, idx: int) -> dict[str, torch.Tensor]:
        if self.mirror_augment:
            is_mirror = idx >= self.num_raw_samples
            raw_idx = idx - self.num_raw_samples if is_mirror else idx
        else:
            is_mirror = False
            raw_idx = idx

        row = self.data[raw_idx].copy()  # copy to avoid modifying memmap
        features = row[:TOTAL_FEATURES]
        labels = row[TOTAL_FEATURES:]

        if is_mirror:
            features = _mirror_features(features)

        return {
            "features": torch.from_numpy(features),
            "labels": torch.from_numpy(labels),
        }


class _SizedDataset(Protocol):
    def __len__(self) -> int: ...


class MirrorAugmentedDataset(Dataset[dict[str, torch.Tensor]]):
    """Wrap an unaugmented dataset and add mirrored feature views for training only."""

    def __init__(self, base: Dataset[dict[str, torch.Tensor]]) -> None:
        self.base = base

    def __len__(self) -> int:
        return len(cast(_SizedDataset, cast(object, self.base))) * 2

    def __getitem__(self, idx: int) -> dict[str, torch.Tensor]:
        base_len = len(cast(_SizedDataset, cast(object, self.base)))
        is_mirror = idx >= base_len
        raw_idx = idx - base_len if is_mirror else idx
        sample = self.base[raw_idx]
        features = sample["features"].clone()
        labels = sample["labels"].clone()
        if is_mirror:
            features = torch.from_numpy(_mirror_features(features.numpy()))
        return {"features": features, "labels": labels}


def _mirror_board(board_flat: np.ndarray) -> np.ndarray:
    """Horizontally mirror a column-major board (400,) → (400,).

    Board is stored column-major: columns 0..9, each 40 cells high.
    Mirror = reverse column order: col[i] ↔ col[9-i].
    """
    board = board_flat.reshape(BOARD_WIDTH, BOARD_HEIGHT)  # (10, 40) col-major
    return board[::-1].reshape(-1).copy()


def _mirror_features(features: np.ndarray) -> np.ndarray:
    """Mirror both boards in feature vector, keep pieces/scalars unchanged."""
    result = features.copy()
    # player board: [0..400)
    result[:PLAYER_BOARD_FEATURES] = _mirror_board(
        features[:PLAYER_BOARD_FEATURES]
    )
    # opponent board: [400..800)
    opp_start = PLAYER_BOARD_FEATURES
    opp_end = opp_start + OPPONENT_BOARD_FEATURES
    result[opp_start:opp_end] = _mirror_board(features[opp_start:opp_end])
    # pieces [800..849) and scalars [849..854) stay unchanged
    return result
