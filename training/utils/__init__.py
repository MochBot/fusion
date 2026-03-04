from __future__ import annotations

from typing import Any

__all__ = ["SCReLU", "KendallMultiTaskLoss"]


def __getattr__(name: str) -> Any:
    if name == "SCReLU":
        from .activations import SCReLU

        return SCReLU
    if name == "KendallMultiTaskLoss":
        from .losses import KendallMultiTaskLoss

        return KendallMultiTaskLoss
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
