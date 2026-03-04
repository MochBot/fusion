"""SCReLU (Squared Clipped ReLU) activation for NNUE-style networks."""

from __future__ import annotations

try:
    from typing import override
except ImportError:
    from typing_extensions import override

import torch
from torch import Tensor, nn


class SCReLU(nn.Module):
    """Squared Clipped ReLU: SCReLU(x) = clamp(x, 0, 1)^2.

    Standard activation in efficiently-updatable neural network (NNUE)
    architectures. The clamp provides bounded outputs while the square
    introduces non-linearity beyond a simple threshold.
    """

    @override
    def forward(self, x: Tensor) -> Tensor:
        return torch.clamp(x, 0.0, 1.0).square()
