"""Uncertainty-weighted multi-task loss (Kendall et al., 2018)."""

from __future__ import annotations

try:
    from typing import override
except ImportError:
    from typing_extensions import override

import torch
import torch.nn.functional as F
from torch import Tensor, nn


class KendallMultiTaskLoss(nn.Module):
    """Multi-task loss that learns task-specific uncertainty weights.

    From "Multi-Task Learning Using Uncertainty to Weigh Losses for Scene
    Geometry and Semantics" (Kendall, Gal, Cipolla, 2018). Each task gets a
    learnable log-variance parameter that balances its contribution to the
    total loss automatically during training.

    Regression:    L_i = (1 / (2 * sigma_i^2)) * huber_i + 0.5 * log(sigma_i^2)
    Classification: L_c = (1 / sigma_c^2) * ce_c + log(sigma_c^2)
    """

    num_regression_tasks: int
    num_classification_tasks: int
    regression_log_vars: nn.Parameter
    classification_log_vars: nn.Parameter

    def __init__(
        self,
        num_regression_tasks: int = 6,
        num_classification_tasks: int = 1,
    ) -> None:
        super().__init__()
        self.num_regression_tasks = num_regression_tasks
        self.num_classification_tasks = num_classification_tasks

        self.regression_log_vars = nn.Parameter(
            torch.zeros(num_regression_tasks)
        )
        self.classification_log_vars = nn.Parameter(
            torch.zeros(num_classification_tasks)
        )

    @override
    def _load_from_state_dict(
        self,
        state_dict: dict[str, object],
        prefix: str,
        local_metadata: dict[str, object],
        strict: bool,
        missing_keys: list[str],
        unexpected_keys: list[str],
        error_msgs: list[str],
    ) -> None:
        legacy_regression_key = f"{prefix}log_sigma_sq_reg"
        legacy_classification_key = f"{prefix}log_sigma_sq_cls"
        regression_key = f"{prefix}regression_log_vars"
        classification_key = f"{prefix}classification_log_vars"

        if regression_key not in state_dict and legacy_regression_key in state_dict:
            state_dict[regression_key] = state_dict[legacy_regression_key]
        if (
            classification_key not in state_dict
            and legacy_classification_key in state_dict
        ):
            state_dict[classification_key] = state_dict[legacy_classification_key]

        state_dict.pop(legacy_regression_key, None)
        state_dict.pop(legacy_classification_key, None)

        super()._load_from_state_dict(
            state_dict,
            prefix,
            local_metadata,
            strict,
            missing_keys,
            unexpected_keys,
            error_msgs,
        )

    @override
    def forward(
        self,
        regression_preds: Tensor,
        regression_targets: Tensor,
        phase_logits: Tensor,
        phase_targets: Tensor,
    ) -> dict[str, object]:
        """Compute uncertainty-weighted multi-task loss.

        Args:
            regression_preds: Predictions for regression heads (B, 6).
            regression_targets: Ground-truth regression targets (B, 6).
            phase_logits: Raw logits for phase classification (B, 3).
            phase_targets: Class indices for phase (B,), values in {0, 1, 2}.

        Returns:
            Dict with 'total_loss', 'regression_losses', 'classification_loss',
            and 'log_variances' for monitoring.
        """
        total_loss = torch.zeros(1, device=regression_preds.device)
        regression_losses: list[Tensor] = []

        for i in range(self.num_regression_tasks):
            log_var: Tensor = self.regression_log_vars[i]
            raw_loss = F.huber_loss(
                regression_preds[:, i],
                regression_targets[:, i],
                reduction="none",
            ).mean()

            precision = torch.exp(-log_var)
            weighted = 0.5 * precision * raw_loss + 0.5 * log_var
            total_loss = total_loss + weighted
            regression_losses.append(raw_loss.detach())

        ce_loss = F.cross_entropy(phase_logits, phase_targets)
        cls_log_var: Tensor = self.classification_log_vars[0]
        precision = torch.exp(-cls_log_var)
        weighted_ce = precision * ce_loss + cls_log_var
        total_loss = total_loss + weighted_ce

        log_variances: list[float] = [
            float(self.regression_log_vars[i].item())
            for i in range(self.num_regression_tasks)
        ] + [
            float(self.classification_log_vars[i].item())
            for i in range(self.num_classification_tasks)
        ]

        return {
            "total_loss": total_loss.squeeze(),
            "regression_losses": [loss.item() for loss in regression_losses],
            "classification_loss": ce_loss.detach().item(),
            "log_variances": log_variances,
        }
