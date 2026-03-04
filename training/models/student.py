"""StudentNet — compact MLP distilled from the teacher for WASM inference."""

from collections import OrderedDict

from torch import Tensor, nn

try:
    from ..utils.activations import SCReLU
    from ..utils.config import (
        NUM_REGRESSION_HEADS,
        STUDENT_DIMS,
        STUDENT_OUTPUT_DIM,
        TOTAL_FEATURES,
    )
except ImportError:
    from utils.activations import SCReLU
    from utils.config import (
        NUM_REGRESSION_HEADS,
        STUDENT_DIMS,
        STUDENT_OUTPUT_DIM,
        TOTAL_FEATURES,
    )


class StudentNet(nn.Module):
    """Compact MLP with SCReLU activation for sub-microsecond WASM inference.

    Architecture: 854 → 192 → 96 → 48 → 9 (pure MLP, no BatchNorm/Dropout).
    Layers are built dynamically from STUDENT_DIMS with SCReLU after each
    hidden linear layer. The output layer has no activation.

    Input:  (B, 854)  — feature vector (player board + opponent board + pieces + scalars)
    Output: (B, 9)    — 6 regression heads + 3 phase logits
        [:, 0:6] = value, attack_potential, defensive_solidity, efficiency, flexibility, tempo
        [:, 6:9] = opener, midgame, survival (phase logits)

    Parameters: ~189K trainable (187,785 exact).

    Weight export format (flat little-endian f32):
        [W1(854×192)] [b1(192)] [W2(192×96)] [b2(96)] [W3(96×48)] [b3(48)] [W4(48×9)] [b4(9)]
        W matrices are stored in row-major order matching nn.Linear's
        (out_features, in_features) layout.
    """

    def __init__(self) -> None:
        super().__init__()

        assert STUDENT_DIMS[0] == TOTAL_FEATURES, (
            f"First dim in STUDENT_DIMS ({STUDENT_DIMS[0]}) must match "
            f"TOTAL_FEATURES ({TOTAL_FEATURES})"
        )

        layers: OrderedDict[str, nn.Module] = OrderedDict()
        for i in range(len(STUDENT_DIMS) - 1):
            layers[f"linear{i}"] = nn.Linear(STUDENT_DIMS[i], STUDENT_DIMS[i + 1])
            layers[f"screlu{i}"] = SCReLU()

        self.hidden = nn.Sequential(layers)
        self.output = nn.Linear(STUDENT_DIMS[-1], STUDENT_OUTPUT_DIM)

    def forward(self, x: Tensor) -> Tensor:
        """Forward pass through all layers.

        Args:
            x: Input tensor of shape (B, 854).

        Returns:
            Output tensor of shape (B, 9).
        """
        return self.output(self.hidden(x))

    def split_output(self, output: Tensor) -> tuple[Tensor, Tensor]:
        """Split raw output into regression values and phase logits.

        Args:
            output: Raw model output of shape (B, 9).

        Returns:
            Tuple of (regression, phase_logits):
                regression:   (B, 6) — value through tempo
                phase_logits: (B, 3) — opener, midgame, survival
        """
        regression = output[:, :NUM_REGRESSION_HEADS]
        phase_logits = output[:, NUM_REGRESSION_HEADS:]
        return regression, phase_logits

    @property
    def num_params(self) -> int:
        """Total number of trainable parameters."""
        return sum(p.numel() for p in self.parameters() if p.requires_grad)
