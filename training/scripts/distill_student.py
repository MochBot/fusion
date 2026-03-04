"""Student distillation from trained teacher model.

Uses privileged-information distillation (AlphaStar fog-of-war pattern):
- Teacher sees structured inputs (dual boards, pieces, scalars separately)
- Student sees flat 854-dim feature vector only
- Student learns to match teacher's output distribution
"""

from __future__ import annotations

import logging
import sys
from pathlib import Path
from typing import Literal, cast

import lightning as L
import torch
import torch.nn as nn
import torch.nn.functional as F
from lightning.pytorch.callbacks import EarlyStopping, ModelCheckpoint
from lightning.pytorch.utilities.types import OptimizerLRScheduler
from torch.utils.data import DataLoader

try:
    from ..data.dataset import FusionBinaryDataset
    from ..models.lit_module import FusionDataModule, TeacherLitModule
    from ..models.student import StudentNet
    from ..utils.config import NUM_REGRESSION_HEADS, TOTAL_FEATURES
except ImportError:
    from data.dataset import FusionBinaryDataset
    from models.lit_module import FusionDataModule, TeacherLitModule
    from models.student import StudentNet
    from utils.config import NUM_REGRESSION_HEADS, TOTAL_FEATURES

torch.set_float32_matmul_precision("medium")
logging.getLogger("lightning.pytorch.utilities.rank_zero").setLevel(logging.WARNING)


class StudentDistillModule(L.LightningModule):
    """Lightning module for student distillation from frozen teacher.

    Loss = α * MSE(student_reg, teacher_reg) + β * KL(student_phase, teacher_phase)

    The teacher is frozen and provides soft targets. The student learns to
    approximate the teacher's outputs using only the flat feature vector.
    """

    teacher: TeacherLitModule
    student: StudentNet

    def __init__(
        self,
        teacher_checkpoint: str,
        lr: float = 5e-4,
        weight_decay: float = 1e-4,
        alpha: float = 1.0,
        beta: float = 0.5,
        temperature: float = 3.0,
    ) -> None:
        super().__init__()
        self.save_hyperparameters()

        # Load frozen teacher
        self.teacher = TeacherLitModule.load_from_checkpoint(
            teacher_checkpoint,
            strict=False,
        )
        self.teacher.freeze()
        self.teacher.eval()

        # Student to train
        self.student = StudentNet()

        self.alpha = alpha
        self.beta = beta
        self.temperature = temperature

    def configure_model(self) -> None:
        """Compile student network for kernel fusion on B200 (no CUDA Graphs — batch sizes vary)."""
        self.student = cast(StudentNet, torch.compile(self.student, mode="default", dynamic=False))

    def forward(self, features: torch.Tensor) -> torch.Tensor:
        return self.student(features)

    def _shared_step(
        self, batch: dict[str, torch.Tensor], stage: str
    ) -> torch.Tensor:
        features = batch["features"]

        # Teacher forward (no grad)
        with torch.no_grad():
            teacher_out = self.teacher(features)
            teacher_reg = teacher_out["regression"]  # (B, 6)
            teacher_phase = teacher_out["phase_logits"]  # (B, 3)

        # Student forward
        student_model = cast(StudentNet, self.student)
        student_raw = student_model(features)  # (B, 9)
        student_reg, student_phase = student_model.split_output(student_raw)

        # Regression loss: MSE between student and teacher predictions
        reg_loss = F.mse_loss(student_reg, teacher_reg)

        # Phase distillation: KL divergence with temperature scaling
        teacher_soft = F.log_softmax(teacher_phase / self.temperature, dim=1)
        student_log_soft = F.log_softmax(student_phase / self.temperature, dim=1)
        # KL(teacher || student) = sum(teacher * (log_teacher - log_student))
        kl_loss = F.kl_div(
            student_log_soft,
            teacher_soft,
            log_target=True,
            reduction="batchmean",
        ) * (self.temperature ** 2)

        total_loss = self.alpha * reg_loss + self.beta * kl_loss

        self.log(f"{stage}/total_loss", total_loss, prog_bar=(stage == "val"))
        # Underscore alias for ModelCheckpoint filename interpolation
        self.log(f"{stage}_total_loss", total_loss)
        self.log(f"{stage}/reg_loss", reg_loss)
        self.log(f"{stage}/kl_loss", kl_loss)

        return total_loss

    def training_step(self, batch: dict[str, torch.Tensor], batch_idx: int) -> torch.Tensor:
        return self._shared_step(batch, "train")

    def validation_step(self, batch: dict[str, torch.Tensor], batch_idx: int) -> torch.Tensor:
        return self._shared_step(batch, "val")

    def configure_optimizers(self) -> OptimizerLRScheduler:
        lr = float(self.hparams["lr"])
        weight_decay = float(self.hparams["weight_decay"])
        student_model = cast(StudentNet, self.student)
        optimizer = torch.optim.AdamW(
            student_model.parameters(),
            lr=lr,
            weight_decay=weight_decay,
        )
        scheduler = torch.optim.lr_scheduler.CosineAnnealingLR(
            optimizer, T_max=self.trainer.max_epochs or 100, eta_min=1e-6
        )
        return {
            "optimizer": optimizer,
            "lr_scheduler": {
                "scheduler": scheduler,
                "interval": "epoch",
            },
        }


def distill_student(
    teacher_checkpoint: str,
    data_path: str,
    output_dir: str = "checkpoints/student",
    max_epochs: int = 100,
    batch_size: int = 32768,
    lr: float = 5e-4,
    accelerator: str = "auto",
    num_workers: int = 12,
    prefetch_factor: int = 4,
    precision: Literal["bf16-mixed", "16-mixed"] = "bf16-mixed",
) -> str:
    """Run student distillation and return path to best checkpoint.

    Args:
        teacher_checkpoint: Path to trained teacher .ckpt
        data_path: Path to .bin training data
        output_dir: Directory for student checkpoints
        max_epochs: Maximum training epochs
        batch_size: Batch size (student is small, can use large batches)
        lr: Learning rate
        accelerator: Lightning accelerator

    Returns:
        Path to best student checkpoint.
    """
    out_dir = Path(output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    model = StudentDistillModule(
        teacher_checkpoint=teacher_checkpoint,
        lr=lr,
    )

    dm = FusionDataModule(
        data_path=data_path,
        batch_size=batch_size,
        num_workers=num_workers,
        prefetch_factor=prefetch_factor,
        mirror_augment=True,
    )

    callbacks: list[L.Callback] = [
        ModelCheckpoint(
            dirpath=str(out_dir),
            filename="student-best-{epoch}-{val_total_loss:.4f}",
            monitor="val/total_loss",
            mode="min",
            save_top_k=1,
        ),
        EarlyStopping(
            monitor="val/total_loss",
            patience=15,
            mode="min",
        ),
    ]

    trainer = L.Trainer(
        max_epochs=max_epochs,
        accelerator=accelerator,
        devices=1,
        callbacks=callbacks,
        deterministic=False,
        precision=precision,
    )

    trainer.fit(model, datamodule=dm)

    # Return best checkpoint path
    best = callbacks[0]
    assert isinstance(best, ModelCheckpoint)
    assert best.best_model_path is not None
    return best.best_model_path


def main() -> None:
    """CLI: python -m training.scripts.distill_student <teacher.ckpt> <data.bin> [output_dir]"""
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <teacher.ckpt> <data.bin> [output_dir]")
        sys.exit(1)

    teacher_ckpt = sys.argv[1]
    data_path = sys.argv[2]
    output_dir = sys.argv[3] if len(sys.argv) > 3 else "checkpoints/student"

    best_path = distill_student(teacher_ckpt, data_path, output_dir)
    print(f"Best student checkpoint: {best_path}")


if __name__ == "__main__":
    main()
