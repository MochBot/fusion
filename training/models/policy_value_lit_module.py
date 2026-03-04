from __future__ import annotations

from collections.abc import Callable
from typing import cast, final

import lightning as L
import torch
import torch.nn.functional as F
from lightning.pytorch.utilities.types import OptimizerLRScheduler
from typing_extensions import override

try:
    from .policy_value import PolicyValueNet
except ImportError:
    from models.policy_value import PolicyValueNet


def _topk_accuracy(logits: torch.Tensor, target_index: torch.Tensor, *, k: int) -> torch.Tensor:
    topk = min(k, logits.shape[-1])
    indices = torch.topk(logits, k=topk, dim=-1).indices
    return (indices == target_index.unsqueeze(-1)).any(dim=-1).float().mean()


def _mean_rank(logits: torch.Tensor, target_index: torch.Tensor) -> torch.Tensor:
    target_scores = logits.gather(1, target_index.unsqueeze(1)).squeeze(1)
    return (1 + (logits > target_scores.unsqueeze(1)).sum(dim=1)).float().mean()


def masked_search_policy_kl_loss(
    search_policy_logits: torch.Tensor,
    search_policy_probs: torch.Tensor,
    candidate_mask: torch.Tensor,
) -> torch.Tensor:
    search_log_probs = F.log_softmax(search_policy_logits, dim=-1)
    valid_log_probs = search_log_probs[candidate_mask]
    valid_targets = search_policy_probs[candidate_mask]
    return F.kl_div(valid_log_probs, valid_targets, reduction="sum") / search_policy_logits.shape[0]


def compile_policy_value_model(model: PolicyValueNet) -> PolicyValueNet:
    compile_model = cast(Callable[..., PolicyValueNet], getattr(torch, "compile"))
    return compile_model(model, mode="default", dynamic=True)


def compute_policy_value_metrics(
    batch: dict[str, torch.Tensor],
    out: dict[str, torch.Tensor],
) -> dict[str, torch.Tensor]:
    metrics: dict[str, torch.Tensor] = {}
    search_target_index = batch["search_best_index"]
    metrics["search_policy_top1_accuracy"] = (
        (out["search_policy_logits"].argmax(dim=-1) == search_target_index).float().mean()
    )
    metrics["search_policy_top3_accuracy"] = _topk_accuracy(
        out["search_policy_logits"], search_target_index, k=3
    )
    metrics["search_policy_mean_rank"] = _mean_rank(
        out["search_policy_logits"], search_target_index
    )
    player_available = batch["player_policy_available"]
    metrics["player_target_availability_rate"] = player_available.float().mean()
    if bool(player_available.any()):
        player_logits = out["player_policy_logits"][player_available]
        player_targets = batch["player_policy_index"][player_available]
        metrics["player_policy_top1_accuracy"] = (
            (player_logits.argmax(dim=-1) == player_targets).float().mean()
        )
        metrics["player_policy_top3_accuracy"] = _topk_accuracy(player_logits, player_targets, k=3)
        metrics["player_policy_mean_rank"] = _mean_rank(player_logits, player_targets)
        metrics["player_search_agreement_rate"] = (
            (search_target_index[player_available] == player_targets).float().mean()
        )
    else:
        zero = out["value"].new_zeros(())
        metrics["player_policy_top1_accuracy"] = zero
        metrics["player_policy_top3_accuracy"] = zero
        metrics["player_policy_mean_rank"] = zero
        metrics["player_search_agreement_rate"] = zero
    return metrics


@final
class PolicyValueLitModule(L.LightningModule):
    def __init__(
        self,
        lr: float = 3e-4,
        weight_decay: float = 1e-5,
        supervision_mode: str = "search_control",
        player_policy_weight: float = 1.0,
        search_policy_weight: float = 1.0,
        search_value_weight: float = 1.0,
        dropout: float = 0.0,
    ) -> None:
        super().__init__()
        self.save_hyperparameters()
        self.model: PolicyValueNet = PolicyValueNet(dropout=dropout)
        self.lr: float = lr
        self.weight_decay: float = weight_decay
        self.supervision_mode: str = supervision_mode
        self.player_policy_weight: float = player_policy_weight
        self.search_policy_weight: float = search_policy_weight
        self.search_value_weight: float = search_value_weight

    @override
    def configure_model(self) -> None:
        self.model = compile_policy_value_model(self.model)

    @override
    def forward(
        self,
        features: torch.Tensor,
        candidate_move_features: torch.Tensor,
        candidate_mask: torch.Tensor,
        player_aux_context_features: torch.Tensor,
    ) -> dict[str, torch.Tensor]:
        return cast(
            dict[str, torch.Tensor],
            self.model(
                features, candidate_move_features, candidate_mask, player_aux_context_features
            ),
        )

    def _shared_step(self, batch: dict[str, torch.Tensor], stage: str) -> torch.Tensor:
        out = cast(
            dict[str, torch.Tensor],
            self.model(
                batch["features"],
                batch["candidate_move_features"],
                batch["candidate_mask"],
                batch["player_aux_context_features"],
            ),
        )
        search_policy_loss = masked_search_policy_kl_loss(
            out["search_policy_logits"],
            batch["search_policy_probs"],
            batch["candidate_mask"],
        )
        search_value_loss = F.mse_loss(out["value"], batch["search_best_value"])

        player_available = batch["player_policy_available"]
        if bool(player_available.any()):
            player_policy_loss = F.cross_entropy(
                out["player_policy_logits"][player_available],
                batch["player_policy_index"][player_available],
            )
        else:
            player_policy_loss = out["player_policy_logits"].new_zeros(())

        if self.supervision_mode == "player_context_primary":
            total_loss = (
                self.player_policy_weight * player_policy_loss
                + self.search_policy_weight * search_policy_loss
                + self.search_value_weight * search_value_loss
            )
        else:
            total_loss = (
                self.search_policy_weight * search_policy_loss
                + self.search_value_weight * search_value_loss
            )
        self.log(f"{stage}/total_loss", total_loss, prog_bar=(stage == "val"))
        self.log(f"{stage}_total_loss", total_loss)
        self.log(f"{stage}/player_policy_loss", player_policy_loss)
        self.log(f"{stage}/search_policy_loss", search_policy_loss)
        self.log(f"{stage}/search_value_loss", search_value_loss)
        if stage == "val":
            for metric_name, metric_value in compute_policy_value_metrics(batch, out).items():
                self.log(f"{stage}/{metric_name}", metric_value)
        return total_loss

    @override
    def training_step(self, batch: dict[str, torch.Tensor], batch_idx: int) -> torch.Tensor:
        return self._shared_step(batch, "train")

    @override
    def validation_step(self, batch: dict[str, torch.Tensor], batch_idx: int) -> torch.Tensor:
        return self._shared_step(batch, "val")

    @override
    def configure_optimizers(self) -> OptimizerLRScheduler:
        optimizer = torch.optim.AdamW(
            self.parameters(),
            lr=self.lr,
            weight_decay=self.weight_decay,
        )
        scheduler = torch.optim.lr_scheduler.CosineAnnealingLR(optimizer, T_max=50, eta_min=1e-6)
        return {
            "optimizer": optimizer,
            "lr_scheduler": {"scheduler": scheduler, "interval": "epoch"},
        }
