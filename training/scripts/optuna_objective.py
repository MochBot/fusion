"""Optuna objective function for teacher hyperparameter search."""

from __future__ import annotations

import importlib
import logging
from pathlib import Path
from typing import Literal, Sequence

import lightning as L
import optuna
import torch
from lightning.pytorch.callbacks import EarlyStopping, ModelCheckpoint

try:
    from ..models.lit_module import FusionDataModule, TeacherLitModule
except ImportError:
    from models.lit_module import FusionDataModule, TeacherLitModule

torch.set_float32_matmul_precision("medium")
logging.getLogger("lightning.pytorch.utilities.rank_zero").setLevel(logging.WARNING)


def teacher_objective(
    trial: optuna.Trial,
    data_path: str,
    checkpoint_dir: str = "/checkpoints",
    max_epochs: int = 100,
    accelerator: str = "auto",
    batch_size_choices: Sequence[int] | None = None,
    num_workers: int = 12,
    prefetch_factor: int = 4,
    precision: Literal["bf16-mixed", "16-mixed"] = "bf16-mixed",
) -> float:
    """Optuna objective for teacher CNN hyperparameter tuning.

    Searches over:
    - Learning rate (log-uniform 1e-5 to 1e-2)
    - Weight decay (log-uniform 1e-6 to 1e-2)
    - Batch size (8192, 16384, 32768, 65536)
    - Dropout rates (uniform 0.1 to 0.5)

    Returns validation loss for Optuna minimization.
    Uses HyperbandPruner-compatible epoch reporting.
    """
    # Hyperparameter search space
    lr = trial.suggest_float("lr", 1e-5, 1e-2, log=True)
    weight_decay = trial.suggest_float("weight_decay", 1e-6, 1e-2, log=True)
    batch_candidates = (
        list(batch_size_choices)
        if batch_size_choices is not None
        else [8192, 16384, 32768, 65536]
    )
    batch_size = trial.suggest_categorical("batch_size", batch_candidates)
    dropout_fc1 = trial.suggest_float("dropout_fc1", 0.1, 0.5)
    dropout_fc2 = trial.suggest_float("dropout_fc2", 0.05, 0.3)

    # Model and data
    model = TeacherLitModule(
        lr=lr,
        weight_decay=weight_decay,
        dropout_fc1=dropout_fc1,
        dropout_fc2=dropout_fc2,
    )

    dm = FusionDataModule(
        data_path=data_path,
        batch_size=batch_size,
        num_workers=num_workers,
        prefetch_factor=prefetch_factor,
        mirror_augment=True,
    )

    # Callbacks
    trial_dir = Path(checkpoint_dir) / f"trial_{trial.number}"
    trial_dir.mkdir(parents=True, exist_ok=True)

    callbacks: list[L.Callback] = [
        ModelCheckpoint(
            dirpath=str(trial_dir),
            filename="best-{epoch}-{val_total_loss:.4f}",
            monitor="val/total_loss",
            mode="min",
            save_top_k=1,
        ),
        EarlyStopping(
            monitor="val/total_loss",
            patience=10,
            mode="min",
        ),
    ]
    try:
        pruning_module = importlib.import_module("optuna_integration.pytorch_lightning")
    except ModuleNotFoundError:
        pruning_module = importlib.import_module("optuna.integration")
    PyTorchLightningPruningCallback = getattr(
        pruning_module,
        "PyTorchLightningPruningCallback",
    )
    callbacks.append(PyTorchLightningPruningCallback(trial, monitor="val/total_loss"))

    trainer = L.Trainer(
        max_epochs=max_epochs,
        accelerator=accelerator,
        devices=1,
        callbacks=callbacks,
        enable_progress_bar=False,
        enable_model_summary=False,
        logger=False,  # Optuna handles logging
        deterministic=False,
        precision=precision,
    )

    val_loss: torch.Tensor | None = None
    try:
        trainer.fit(model, datamodule=dm)
        val_loss = trainer.callback_metrics.get("val/total_loss")
    except torch.OutOfMemoryError as exc:
        trial.set_user_attr("oom", str(exc))
        raise optuna.TrialPruned("Pruned due to CUDA OOM") from exc
    finally:
        dm.teardown("fit")
        del trainer
        if torch.cuda.is_available():
            torch.cuda.empty_cache()

    if val_loss is None:
        return float("inf")
    return val_loss.item()
