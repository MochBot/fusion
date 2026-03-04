from __future__ import annotations

from pathlib import Path
import argparse
import sys
from typing import Literal, cast

import lightning as L
from lightning.pytorch.callbacks import EarlyStopping, ModelCheckpoint

try:
    from ..data.policy_value_dataset import PolicyValueDataModule
    from ..models.policy_value_lit_module import PolicyValueLitModule
except ImportError:
    TRAINING_ROOT = Path(__file__).resolve().parents[1]
    if str(TRAINING_ROOT) not in sys.path:
        sys.path.insert(0, str(TRAINING_ROOT))
    from data.policy_value_dataset import PolicyValueDataModule
    from models.policy_value_lit_module import PolicyValueLitModule


def train_policy_value(
    data_path: str,
    output_dir: str,
    *,
    supervision_mode: str = "search_control",
    batch_size: int = 256,
    num_workers: int = 4,
    max_epochs: int = 50,
    lr: float = 3e-4,
    weight_decay: float = 1e-5,
    accelerator: str = "auto",
    precision: Literal["bf16-mixed", "16-mixed", "32-true"] = "bf16-mixed",
    dropout: float = 0.0,
    search_value_weight: float | None = None,
    search_policy_weight: float | None = None,
    player_policy_weight: float | None = None,
) -> str:
    output_dir_path = Path(output_dir)
    output_dir_path.mkdir(parents=True, exist_ok=True)
    resolved_svw = (
        search_value_weight
        if search_value_weight is not None
        else (0.25 if supervision_mode == "player_context_primary" else 1.0)
    )
    resolved_spw = (
        search_policy_weight
        if search_policy_weight is not None
        else (0.25 if supervision_mode == "player_context_primary" else 1.0)
    )
    resolved_ppw = player_policy_weight if player_policy_weight is not None else 1.0
    model = PolicyValueLitModule(
        lr=lr,
        weight_decay=weight_decay,
        supervision_mode=supervision_mode,
        player_policy_weight=resolved_ppw,
        search_policy_weight=resolved_spw,
        search_value_weight=resolved_svw,
        dropout=dropout,
    )
    data = PolicyValueDataModule(
        data_path,
        batch_size=batch_size,
        num_workers=num_workers,
        supervision_mode=supervision_mode,
    )
    checkpoint = ModelCheckpoint(
        dirpath=output_dir,
        filename="policy-value-{epoch}-{val_total_loss:.4f}",
        monitor="val/total_loss",
        mode="min",
        save_top_k=1,
    )
    early_stop = EarlyStopping(monitor="val/total_loss", mode="min", patience=10)
    trainer = L.Trainer(
        max_epochs=max_epochs,
        accelerator=accelerator,
        devices=1,
        callbacks=[checkpoint, early_stop],
        precision=precision,
        logger=False,
        enable_progress_bar=False,
        enable_model_summary=False,
    )
    data.setup()
    trainer.fit(
        model, train_dataloaders=data.train_dataloader(), val_dataloaders=data.val_dataloader()
    )
    best_model_path = getattr(checkpoint, "best_model_path", None)
    if not isinstance(best_model_path, str):
        raise TypeError(f"expected best_model_path to be str, got {type(best_model_path)!r}")
    return best_model_path


def main() -> None:
    parser = argparse.ArgumentParser(description="Train Phase 1 policy/value model")
    _ = parser.add_argument("data_path")
    _ = parser.add_argument("output_dir")
    _ = parser.add_argument(
        "--supervision-mode",
        choices=["search_control", "player_context_primary"],
        default="search_control",
    )
    _ = parser.add_argument("--batch-size", type=int, default=256)
    _ = parser.add_argument("--num-workers", type=int, default=4)
    _ = parser.add_argument("--max-epochs", type=int, default=50)
    _ = parser.add_argument("--dropout", type=float, default=0.0)
    _ = parser.add_argument("--search-value-weight", type=float, default=None)
    _ = parser.add_argument("--search-policy-weight", type=float, default=None)
    _ = parser.add_argument("--player-policy-weight", type=float, default=None)
    args = parser.parse_args()
    best_path = train_policy_value(
        cast(str, args.data_path),
        cast(str, args.output_dir),
        supervision_mode=cast(str, args.supervision_mode),
        batch_size=cast(int, args.batch_size),
        num_workers=cast(int, args.num_workers),
        max_epochs=cast(int, args.max_epochs),
        dropout=cast(float, args.dropout),
        search_value_weight=args.search_value_weight,
        search_policy_weight=args.search_policy_weight,
        player_policy_weight=args.player_policy_weight,
    )
    print(best_path)
