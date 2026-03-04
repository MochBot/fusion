from __future__ import annotations

from pathlib import Path
from typing import cast, final

import numpy as np
import numpy.typing as npt
import torch
from torch.utils.data import DataLoader, Dataset, Subset
from typing_extensions import override

try:
    from ..data.dataset import FusionBinaryDataset
    from ..utils.config import MOVE_FEATURE_DIM, TOTAL_FEATURES
    from ..utils.policy_value_schema import (
        PolicyValuePlayerContext,
        PolicyValueTarget,
        load_player_context_metadata,
        load_player_contexts,
        load_policy_value_targets,
    )
except ImportError:
    from data.dataset import FusionBinaryDataset
    from utils.config import MOVE_FEATURE_DIM, TOTAL_FEATURES
    from utils.policy_value_schema import (
        PolicyValuePlayerContext,
        PolicyValueTarget,
        load_player_context_metadata,
        load_player_contexts,
        load_policy_value_targets,
    )


PLAYER_CONTEXT_RECENT_HORIZON = 7
PLAYER_CONTEXT_FUTURE_HORIZON = 14
PLAYER_CONTEXT_STEP_FEATURE_DIM = 8
PLAYER_AUX_CONTEXT_FEATURE_DIM = PLAYER_CONTEXT_RECENT_HORIZON * PLAYER_CONTEXT_STEP_FEATURE_DIM
PIECE_ORDER = "iotljsz"
UNKNOWN_PIECE_ID = len(PIECE_ORDER)


def piece_id(piece: str | None) -> int:
    if piece is None:
        return UNKNOWN_PIECE_ID
    piece_normalized = piece.lower()
    return PIECE_ORDER.index(piece_normalized) if piece_normalized in PIECE_ORDER else UNKNOWN_PIECE_ID


def encode_recent_context_features(
    context: PolicyValuePlayerContext | None,
    *,
    horizon: int = PLAYER_CONTEXT_RECENT_HORIZON,
) -> np.ndarray:
    features = np.zeros(horizon * PLAYER_CONTEXT_STEP_FEATURE_DIM, dtype=np.float32)
    if context is None:
        return features
    recent_pieces = context.recent_piece_sequence[-horizon:]
    recent_holds = context.recent_hold_usage[-horizon:]
    offset = horizon - len(recent_pieces)
    for idx, piece in enumerate(recent_pieces):
        base = (offset + idx) * PLAYER_CONTEXT_STEP_FEATURE_DIM
        pid = piece_id(piece)
        if pid < len(PIECE_ORDER):
            features[base + pid] = 1.0
        hold_used = recent_holds[idx] if idx < len(recent_holds) else False
        features[base + 7] = float(hold_used)
    return features


def encode_future_piece_ids(
    context: PolicyValuePlayerContext | None,
    *,
    horizon: int = PLAYER_CONTEXT_FUTURE_HORIZON,
) -> np.ndarray:
    encoded = np.full(horizon, UNKNOWN_PIECE_ID, dtype=np.int64)
    if context is None:
        return encoded
    for idx, piece in enumerate(context.future_piece_sequence[:horizon]):
        encoded[idx] = piece_id(piece)
    return encoded


def encode_future_hold_usage(
    context: PolicyValuePlayerContext | None,
    *,
    horizon: int = PLAYER_CONTEXT_FUTURE_HORIZON,
) -> np.ndarray:
    encoded = np.zeros(horizon, dtype=np.float32)
    if context is None:
        return encoded
    for idx, used in enumerate(context.future_hold_usage[:horizon]):
        encoded[idx] = float(used)
    return encoded


def find_player_policy_target(
    target: PolicyValueTarget,
    context: PolicyValuePlayerContext | None,
) -> tuple[int, bool]:
    if context is None:
        return -1, False
    for idx, (move_raw, _score) in enumerate(target.root_scores):
        if move_raw == context.actual_move_raw:
            return idx, True
    return -1, False


def decode_move_raw(move_raw: int) -> dict[str, int]:
    return {
        "y": move_raw & 0x3F,
        "x": (move_raw >> 6) & 0x0F,
        "piece": (move_raw >> 10) & 0x07,
        "rotation": (move_raw >> 13) & 0x03,
        "spin": (move_raw >> 15) & 0x01,
    }


def featurize_move_raw(move_raw: int) -> np.ndarray:
    decoded = decode_move_raw(move_raw)
    features = np.zeros(MOVE_FEATURE_DIM, dtype=np.float32)
    piece = min(decoded["piece"], 6)
    rotation = min(decoded["rotation"], 3)
    features[piece] = 1.0
    features[7 + rotation] = 1.0
    features[11] = decoded["x"] / 9.0
    features[12] = decoded["y"] / 39.0
    features[13] = float(decoded["spin"])
    return features


@final
class PolicyValueTrainingDataset(Dataset[dict[str, torch.Tensor]]):
    def __init__(self, data_path: str | Path, supervision_mode: str = "search_control") -> None:
        self.data_path: Path = Path(data_path)
        self.supervision_mode: str = supervision_mode
        self.base: FusionBinaryDataset = FusionBinaryDataset(self.data_path, mirror_augment=False)
        self.targets: list[PolicyValueTarget] = load_policy_value_targets(self.data_path)
        if len(self.targets) != self.base.num_raw_samples:
            raise ValueError(
                f"policy/value target count mismatch: {len(self.targets)} vs {self.base.num_raw_samples}"
            )
        self.player_contexts: list[PolicyValuePlayerContext] | None = None
        if self.supervision_mode == "player_context_primary":
            self.player_contexts = load_player_contexts(self.data_path)
            if len(self.player_contexts) != self.base.num_raw_samples:
                raise ValueError(
                    f"player-context target count mismatch: {len(self.player_contexts)} vs {self.base.num_raw_samples}"
                )
            metadata = load_player_context_metadata(self.data_path)
            self.recent_horizon: int = int(cast(int, metadata.get("recent_horizon", PLAYER_CONTEXT_RECENT_HORIZON)))
            self.future_horizon: int = int(cast(int, metadata.get("future_horizon", PLAYER_CONTEXT_FUTURE_HORIZON)))
        else:
            self.recent_horizon = PLAYER_CONTEXT_RECENT_HORIZON
            self.future_horizon = PLAYER_CONTEXT_FUTURE_HORIZON
        self.group_ids: npt.NDArray[np.uint64] = np.asarray(self.base.group_ids, dtype=np.uint64)

    def __len__(self) -> int:
        return self.base.num_raw_samples

    @override
    def __getitem__(self, idx: int) -> dict[str, torch.Tensor]:
        row = np.asarray(cast(np.ndarray, self.base.data[idx]), dtype=np.float32).copy()
        target = self.targets[idx]
        context = self.player_contexts[idx] if self.player_contexts is not None else None
        candidate_move_features = np.stack(
            [featurize_move_raw(move_raw) for move_raw, _ in target.root_scores],
            axis=0,
        )
        candidate_move_raw = np.asarray([move_raw for move_raw, _ in target.root_scores], dtype=np.int64)
        player_policy_index, player_policy_available = find_player_policy_target(target, context)
        return {
            "features": torch.tensor(row[:TOTAL_FEATURES], dtype=torch.float32),
            "candidate_move_features": torch.tensor(candidate_move_features, dtype=torch.float32),
            "candidate_move_raw": torch.tensor(candidate_move_raw, dtype=torch.int64),
            "policy_probs": torch.tensor(target.policy_probs, dtype=torch.float32),
            "search_policy_probs": torch.tensor(target.policy_probs, dtype=torch.float32),
            "best_value": torch.tensor(target.best_value, dtype=torch.float32),
            "search_best_value": torch.tensor(target.best_value, dtype=torch.float32),
            "search_best_index": torch.tensor(int(np.argmax(np.asarray(target.policy_probs))), dtype=torch.int64),
            "player_policy_index": torch.tensor(player_policy_index, dtype=torch.int64),
            "player_policy_available": torch.tensor(player_policy_available, dtype=torch.bool),
            "player_aux_context_features": torch.tensor(
                encode_recent_context_features(context, horizon=self.recent_horizon),
                dtype=torch.float32,
            ),
            "player_aux_future_piece_ids": torch.tensor(
                encode_future_piece_ids(context, horizon=self.future_horizon),
                dtype=torch.int64,
            ),
            "player_aux_future_hold_usage": torch.tensor(
                encode_future_hold_usage(context, horizon=self.future_horizon),
                dtype=torch.float32,
            ),
        }


def collate_policy_value_batch(batch: list[dict[str, torch.Tensor]]) -> dict[str, torch.Tensor]:
    batch_size = len(batch)
    max_candidates = max(
        int(sample["search_policy_probs"].shape[0])
        for sample in batch
    )
    features = torch.stack([sample["features"] for sample in batch], dim=0)
    move_features = torch.zeros((batch_size, max_candidates, MOVE_FEATURE_DIM), dtype=torch.float32)
    move_raw = torch.full((batch_size, max_candidates), -1, dtype=torch.int64)
    search_policy_probs = torch.zeros((batch_size, max_candidates), dtype=torch.float32)
    candidate_mask = torch.zeros((batch_size, max_candidates), dtype=torch.bool)
    search_best_value = torch.stack([sample["search_best_value"] for sample in batch], dim=0)
    search_best_index = torch.stack([sample["search_best_index"] for sample in batch], dim=0)
    player_policy_index = torch.stack([sample["player_policy_index"] for sample in batch], dim=0)
    player_policy_available = torch.stack([sample["player_policy_available"] for sample in batch], dim=0)
    player_aux_context_features = torch.stack([sample["player_aux_context_features"] for sample in batch], dim=0)
    player_aux_future_piece_ids = torch.stack([sample["player_aux_future_piece_ids"] for sample in batch], dim=0)
    player_aux_future_hold_usage = torch.stack([sample["player_aux_future_hold_usage"] for sample in batch], dim=0)

    for batch_idx, sample in enumerate(batch):
        search_probs = sample["search_policy_probs"]
        count = int(search_probs.shape[0])
        move_features[batch_idx, :count] = sample["candidate_move_features"]
        move_raw[batch_idx, :count] = sample["candidate_move_raw"]
        search_policy_probs[batch_idx, :count] = search_probs
        candidate_mask[batch_idx, :count] = True

    return {
        "features": features,
        "candidate_move_features": move_features,
        "candidate_move_raw": move_raw,
        "search_policy_probs": search_policy_probs,
        "candidate_mask": candidate_mask,
        "search_best_value": search_best_value,
        "search_best_index": search_best_index,
        "player_policy_index": player_policy_index,
        "player_policy_available": player_policy_available,
        "player_aux_context_features": player_aux_context_features,
        "player_aux_future_piece_ids": player_aux_future_piece_ids,
        "player_aux_future_hold_usage": player_aux_future_hold_usage,
    }


@final
class PolicyValueDataModule:
    def __init__(
        self,
        data_path: str,
        batch_size: int = 256,
        num_workers: int = 4,
        val_split: float = 0.1,
        supervision_mode: str = "search_control",
    ) -> None:
        self.data_path: str = data_path
        self.batch_size: int = batch_size
        self.num_workers: int = num_workers
        self.val_split: float = val_split
        self.supervision_mode: str = supervision_mode
        self.train_ds: Dataset[dict[str, torch.Tensor]] | None = None
        self.val_ds: Dataset[dict[str, torch.Tensor]] | None = None

    def setup(self) -> None:
        full_ds = PolicyValueTrainingDataset(self.data_path, supervision_mode=self.supervision_mode)
        unique_groups: npt.NDArray[np.uint64] = np.unique(full_ds.group_ids)
        rng = np.random.default_rng(42)
        rng.shuffle(unique_groups)
        n_val_groups = int(len(unique_groups) * self.val_split)
        if len(unique_groups) > 1:
            n_val_groups = max(1, min(n_val_groups, len(unique_groups) - 1))
        val_groups = set(cast(list[int], unique_groups[:n_val_groups].tolist()))
        group_ids = full_ds.group_ids.astype(np.uint64, copy=False)
        train_indices = [i for i in range(len(group_ids)) if int(cast(np.uint64, group_ids[i])) not in val_groups]
        val_indices = [i for i in range(len(group_ids)) if int(cast(np.uint64, group_ids[i])) in val_groups]
        self.train_ds = Subset(full_ds, train_indices)
        self.val_ds = Subset(full_ds, val_indices)

    def _loader(self, dataset: Dataset[dict[str, torch.Tensor]], shuffle: bool) -> DataLoader[dict[str, torch.Tensor]]:
        return DataLoader(
            dataset,
            batch_size=self.batch_size,
            shuffle=shuffle,
            num_workers=self.num_workers,
            pin_memory=True,
            persistent_workers=self.num_workers > 0,
            collate_fn=collate_policy_value_batch,
        )

    def train_dataloader(self) -> DataLoader[dict[str, torch.Tensor]]:
        assert self.train_ds is not None
        return self._loader(self.train_ds, shuffle=True)

    def val_dataloader(self) -> DataLoader[dict[str, torch.Tensor]]:
        assert self.val_ds is not None
        return self._loader(self.val_ds, shuffle=False)
