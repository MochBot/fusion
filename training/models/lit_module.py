"""Lightning training module for teacher and student models."""

from __future__ import annotations

from typing import cast

import lightning as L
import torch
from lightning.pytorch.utilities.types import OptimizerLRScheduler
import numpy as np
from torch.utils.data import DataLoader, Dataset, Subset

try:
    from ..data.dataset import FusionBinaryDataset, MirrorAugmentedDataset
    from ..models.teacher import TeacherNet
    from ..utils.config import (
        BOARD_CELLS,
        NUM_PHASE_CLASSES,
        NUM_REGRESSION_HEADS,
        PIECE_ONE_HOTS as PIECE_ONE_HOT_FEATURES,
        PLAYER_BOARD_FEATURES,
        NUM_SCALARS as SCALAR_FEATURES,
        TOTAL_FEATURES,
    )
    from ..utils.losses import KendallMultiTaskLoss
    from ..utils.example_schema import scalar_slot
except ImportError:
    from data.dataset import FusionBinaryDataset, MirrorAugmentedDataset
    from models.teacher import TeacherNet
    from utils.config import (
        BOARD_CELLS,
        NUM_PHASE_CLASSES,
        NUM_REGRESSION_HEADS,
        PIECE_ONE_HOTS as PIECE_ONE_HOT_FEATURES,
        PLAYER_BOARD_FEATURES,
        NUM_SCALARS as SCALAR_FEATURES,
        TOTAL_FEATURES,
    )
    from utils.losses import KendallMultiTaskLoss
    from utils.example_schema import scalar_slot


class TeacherLitModule(L.LightningModule):
    """Lightning wrapper for TeacherNet training with Kendall multi-task loss.

    Handles:
    - Feature vector unpacking into board/pieces/scalars
    - Label derivation (phase from bag number, regression from raw labels)
    - Kendall uncertainty-weighted multi-task loss
    - Optuna trial pruning via validation loss reporting
    - Learning rate scheduling with ReduceLROnPlateau
    """

    def __init__(
        self,
        lr: float = 1e-3,
        weight_decay: float = 1e-4,
        dropout_fc1: float = 0.3,
        dropout_fc2: float = 0.2,
    ) -> None:
        super().__init__()
        self.save_hyperparameters()

        self.model = TeacherNet(
            dropout_fc1=dropout_fc1,
            dropout_fc2=dropout_fc2,
        )
        self.loss_fn = KendallMultiTaskLoss()

    def configure_model(self) -> None:
        """Compile inner model with CUDA Graphs for kernel-launch-bound workloads.

        Fires before device placement and DDP wrapping — compiles the raw
        nn.Module so TorchInductor sees the full graph without communication hooks.
        mode='default' applies operator fusion and kernel optimization without CUDA Graphs,
        which avoids shape-mismatch crashes when Optuna varies batch_size across trials.
        which is the dominant bottleneck for small models on B200 (73% GPU util).
        """
        self.model = cast(TeacherNet, torch.compile(self.model, mode="default", dynamic=False))

    def _unpack_features(
        self, features: torch.Tensor
    ) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor, torch.Tensor]:
        """Unpack flat 854-dim feature vector into teacher inputs.

        Returns:
            player_board (B, 400), opponent_board (B, 400),
            pieces (B, 49), scalars (B, 5)
        """
        idx = 0
        player_board = features[:, idx : idx + PLAYER_BOARD_FEATURES]
        idx += PLAYER_BOARD_FEATURES
        opponent_board = features[:, idx : idx + BOARD_CELLS]
        idx += BOARD_CELLS
        pieces = features[:, idx : idx + PIECE_ONE_HOT_FEATURES]
        idx += PIECE_ONE_HOT_FEATURES
        scalars = features[:, idx : idx + SCALAR_FEATURES]
        return player_board, opponent_board, pieces, scalars

    def _derive_targets(
        self, labels: torch.Tensor, scalars: torch.Tensor
    ) -> tuple[torch.Tensor, torch.Tensor]:
        """Derive regression targets and phase class from raw labels.

        Raw labels: [game_outcome, lines_sent, b2b_after, position_normalized, time_to_topout]

        Regression targets (6): The teacher learns to predict value and 5 strategic
        metrics. For self-supervised training, we use the raw labels as proxy targets:
            0: value = game_outcome
            1: attack_potential = lines_sent
            2: defensive_solidity = time_to_topout
            3: efficiency = lines_sent * (1 - position_normalized)
            4: flexibility = 1 - position_normalized (more options early)
            5: tempo = lines_sent / (position_normalized + 1e-6)

        Phase class: derived from bag_number scalar (named access through schema)
        denormalization). opener=0 (bags 0..3), midgame=1 (bags 4+, healthy),
        survival=2 (high garbage or late game with low time_to_topout).
        """
        game_outcome = labels[:, 0]
        lines_sent = labels[:, 1]
        b2b_after = labels[:, 2]
        position_norm = labels[:, 3]
        time_to_topout = labels[:, 4]

        bag_norm = scalars[:, scalar_slot("bag_number")]

        # Regression targets
        value = game_outcome
        attack_potential = lines_sent
        defensive_solidity = time_to_topout
        efficiency = lines_sent * (1.0 - position_norm).clamp(min=0.01)
        flexibility = 1.0 - position_norm
        tempo = lines_sent / (position_norm + 1e-6)
        # Clamp tempo to reasonable range
        tempo = tempo.clamp(max=10.0) / 10.0  # normalize to ~[0, 1]

        reg_targets = torch.stack(
            [value, attack_potential, defensive_solidity, efficiency, flexibility, tempo],
            dim=1,
        )  # (B, 6)

        # Phase classification
        # opener: early game (low bag number)
        # survival: high garbage or very low time_to_topout
        # midgame: everything else
        garbage_norm = scalars[:, scalar_slot("garbage_pending")]
        phase = torch.ones(labels.shape[0], dtype=torch.long, device=labels.device)  # default midgame
        phase[bag_norm < 0.15] = 0  # opener (approx bags 0-3 out of ~25+ bags)
        phase[(garbage_norm > 0.5) | (time_to_topout < 0.2)] = 2  # survival

        return reg_targets, phase

    def forward(self, features: torch.Tensor) -> dict[str, torch.Tensor]:
        player_board, opponent_board, pieces, scalars = self._unpack_features(features)
        return self.model(player_board, opponent_board, pieces, scalars)

    def _shared_step(
        self, batch: dict[str, torch.Tensor], stage: str
    ) -> torch.Tensor:
        features = batch["features"]
        labels = batch["labels"]

        player_board, opponent_board, pieces, scalars = self._unpack_features(features)
        out = self.model(player_board, opponent_board, pieces, scalars)

        reg_targets, phase_targets = self._derive_targets(labels, scalars)

        loss_dict = self.loss_fn(
            regression_preds=out["regression"],
            regression_targets=reg_targets,
            phase_logits=out["phase_logits"],
            phase_targets=phase_targets,
        )

        # Log all losses
        self.log(f"{stage}/total_loss", loss_dict["total_loss"], prog_bar=(stage == "val"))
        # Underscore alias for ModelCheckpoint filename interpolation —
        # Lightning uses template vars literally, so {val_total_loss} needs
        # a matching key without the slash.
        self.log(f"{stage}_total_loss", loss_dict["total_loss"])
        self.log(f"{stage}/cls_loss", loss_dict["classification_loss"])
        for i, name in enumerate(
            ["value", "attack", "defense", "efficiency", "flexibility", "tempo"]
        ):
            self.log(f"{stage}/reg_{name}", loss_dict["regression_losses"][i])

        return loss_dict["total_loss"]

    def training_step(self, batch: dict[str, torch.Tensor], batch_idx: int) -> torch.Tensor:
        return self._shared_step(batch, "train")

    def validation_step(self, batch: dict[str, torch.Tensor], batch_idx: int) -> torch.Tensor:
        return self._shared_step(batch, "val")

    def configure_optimizers(self) -> OptimizerLRScheduler:
        lr = float(self.hparams["lr"])
        weight_decay = float(self.hparams["weight_decay"])
        optimizer = torch.optim.AdamW(
            self.parameters(),
            lr=lr,
            weight_decay=weight_decay,
        )
        scheduler = torch.optim.lr_scheduler.ReduceLROnPlateau(
            optimizer, mode="min", factor=0.5, patience=5, min_lr=1e-6
        )
        return {
            "optimizer": optimizer,
            "lr_scheduler": {
                "scheduler": scheduler,
                "monitor": "val/total_loss",
                "interval": "epoch",
            },
        }


class FusionDataModule(L.LightningDataModule):
    """Data module for binary fusion training data.

    Splits a single .bin file into train/val sets (90/10) and creates
    DataLoaders with configurable batch size and workers.
    """

    def __init__(
        self,
        data_path: str,
        batch_size: int = 2048,
        num_workers: int = 4,
        val_split: float = 0.1,
        mirror_augment: bool = True,
        prefetch_factor: int | None = None,
    ) -> None:
        super().__init__()
        self.save_hyperparameters()
        self.data_path = data_path
        self.batch_size = batch_size
        self.num_workers = num_workers
        self.val_split = val_split
        self.mirror_augment = mirror_augment
        self.prefetch_factor = prefetch_factor
        self.train_ds: Dataset[dict[str, torch.Tensor]] | None = None
        self.val_ds: Dataset[dict[str, torch.Tensor]] | None = None

    def setup(self, stage: str | None = None) -> None:
        full_ds = FusionBinaryDataset(self.data_path, mirror_augment=False)
        unique_groups = np.unique(full_ds.group_ids)
        rng = np.random.default_rng(42)
        rng.shuffle(unique_groups)
        n_val_groups = int(len(unique_groups) * self.val_split)
        if len(unique_groups) > 1:
            n_val_groups = max(1, min(n_val_groups, len(unique_groups) - 1))
        val_groups = set(unique_groups[:n_val_groups].tolist())
        train_indices = [i for i, group_id in enumerate(full_ds.group_ids) if group_id not in val_groups]
        val_indices = [i for i, group_id in enumerate(full_ds.group_ids) if group_id in val_groups]

        base_train: Dataset[dict[str, torch.Tensor]] = Subset(full_ds, train_indices)
        self.train_ds = MirrorAugmentedDataset(base_train) if self.mirror_augment else base_train
        self.val_ds = Subset(full_ds, val_indices)

    def train_dataloader(self) -> DataLoader[dict[str, torch.Tensor]]:
        assert self.train_ds is not None
        if self.num_workers > 0:
            return DataLoader(
                self.train_ds,
                batch_size=self.batch_size,
                shuffle=True,
                num_workers=self.num_workers,
                pin_memory=True,
                persistent_workers=True,
                drop_last=True,
                prefetch_factor=self.prefetch_factor,
            )
        return DataLoader(
            self.train_ds,
            batch_size=self.batch_size,
            shuffle=True,
            num_workers=0,
            pin_memory=True,
            persistent_workers=False,
            drop_last=True,
        )

    def val_dataloader(self) -> DataLoader[dict[str, torch.Tensor]]:
        assert self.val_ds is not None
        if self.num_workers > 0:
            return DataLoader(
                self.val_ds,
                batch_size=self.batch_size,
                shuffle=False,
                num_workers=self.num_workers,
                pin_memory=True,
                persistent_workers=True,
                drop_last=True,
                prefetch_factor=self.prefetch_factor,
            )
        return DataLoader(
            self.val_ds,
            batch_size=self.batch_size,
            shuffle=False,
            num_workers=0,
            pin_memory=True,
            persistent_workers=False,
            drop_last=True,
        )
