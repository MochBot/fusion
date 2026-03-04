"""TeacherNet: dual-board CNN with privileged opponent information.

Uses the AlphaStar fog-of-war pattern -- the teacher sees both player and
opponent boards in full detail during training.  Two parallel CNN encoders
(shared architecture, separate weights) each process a 10x40 binary board,
then the representations are fused with piece one-hots and scalar features
before feeding into regression and classification heads.

Total trainable parameters: ~1.17M
"""

import torch
import torch.nn as nn

try:
    from ..utils.config import (
        BOARD_CELLS,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        NUM_PHASE_CLASSES,
        NUM_REGRESSION_HEADS,
        NUM_SCALARS,
        PIECE_ONE_HOTS,
        TEACHER_CHANNELS,
        TEACHER_FC_DIM,
    )
except ImportError:
    from utils.config import (
        BOARD_CELLS,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        NUM_PHASE_CLASSES,
        NUM_REGRESSION_HEADS,
        NUM_SCALARS,
        PIECE_ONE_HOTS,
        TEACHER_CHANNELS,
        TEACHER_FC_DIM,
    )


class TeacherNet(nn.Module):
    """Dual-board CNN teacher with privileged opponent information.

    Input:  (B, 854) — player board(400) | opponent board(400) | pieces(49) | scalars(5)
    Output: dict with 'regression' (B, 6) and 'phase_logits' (B, 3)
    """

    def __init__(self, dropout_fc1: float = 0.3, dropout_fc2: float = 0.2) -> None:
        super().__init__()

        cnn_out_dim = TEACHER_CHANNELS[-1]

        self.player_cnn = self._make_cnn_encoder()
        self.opponent_cnn = self._make_cnn_encoder()

        fusion_input_dim = cnn_out_dim * 2 + PIECE_ONE_HOTS + NUM_SCALARS

        self.fusion = nn.Sequential(
            nn.Linear(fusion_input_dim, TEACHER_FC_DIM),
            nn.ReLU(),
            nn.Dropout(dropout_fc1),
            nn.Linear(TEACHER_FC_DIM, cnn_out_dim),
            nn.ReLU(),
            nn.Dropout(dropout_fc2),
        )

        self.regression_heads = nn.ModuleList(
            [nn.Linear(cnn_out_dim, 1) for _ in range(NUM_REGRESSION_HEADS)]
        )

        self.phase_head = nn.Linear(cnn_out_dim, NUM_PHASE_CLASSES)

    def _make_cnn_encoder(self) -> nn.Sequential:
        """Build a 3-layer CNN encoder for a single 10x40 board.

        Architecture:
            Conv2d(1→64, 3x3)  + BN + ReLU
            Conv2d(64→128, 3x3) + BN + ReLU + MaxPool(2)
            Conv2d(128→256, 3x3) + BN + ReLU + MaxPool(2)
            AdaptiveAvgPool → (B, 256, 1, 1)
        """
        c1, c2, c3 = TEACHER_CHANNELS

        return nn.Sequential(
            nn.Conv2d(1, c1, kernel_size=3, padding=1),
            nn.BatchNorm2d(c1),
            nn.ReLU(),
            nn.Conv2d(c1, c2, kernel_size=3, padding=1),
            nn.BatchNorm2d(c2),
            nn.ReLU(),
            nn.MaxPool2d(2),
            nn.Conv2d(c2, c3, kernel_size=3, padding=1),
            nn.BatchNorm2d(c3),
            nn.ReLU(),
            nn.MaxPool2d(2),
            nn.AdaptiveAvgPool2d((1, 1)),
        )

    def forward(
        self,
        x: torch.Tensor,
        opponent_board: torch.Tensor | None = None,
        pieces: torch.Tensor | None = None,
        scalars: torch.Tensor | None = None,
    ) -> dict[str, torch.Tensor]:
        """Run a forward pass through the dual-board teacher.

        Args:
            x: Input tensor of shape (B, 854).

        Returns:
            Dictionary with keys:
                'regression':   (B, 6) concatenated regression outputs
                'phase_logits': (B, 3) raw logits for phase classification
        """
        if opponent_board is None or pieces is None or scalars is None:
            player_flat = x[:, :BOARD_CELLS]
            opponent_flat = x[:, BOARD_CELLS : BOARD_CELLS * 2]
            piece_features = x[:, BOARD_CELLS * 2 : BOARD_CELLS * 2 + PIECE_ONE_HOTS]
            scalar_features = x[:, BOARD_CELLS * 2 + PIECE_ONE_HOTS :]
        else:
            player_flat = x
            opponent_flat = opponent_board
            piece_features = pieces
            scalar_features = scalars

        player_board = player_flat.view(-1, 1, BOARD_HEIGHT, BOARD_WIDTH)
        opponent_board = opponent_flat.view(-1, 1, BOARD_HEIGHT, BOARD_WIDTH)

        player_enc = self.player_cnn(player_board).flatten(1)
        opponent_enc = self.opponent_cnn(opponent_board).flatten(1)

        fused = torch.cat([player_enc, opponent_enc, piece_features, scalar_features], dim=1)
        shared = self.fusion(fused)
        regression = torch.cat([head(shared) for head in self.regression_heads], dim=1)
        phase_logits = self.phase_head(shared)

        return {
            "regression": regression,
            "phase_logits": phase_logits,
        }

    @property
    def num_params(self) -> int:
        """Total number of trainable parameters."""
        return sum(p.numel() for p in self.parameters())
