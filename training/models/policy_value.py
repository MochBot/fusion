from __future__ import annotations

from collections import OrderedDict
from typing import cast, final

import torch
import torch.nn as nn
from typing_extensions import override

try:
    from ..utils.config import POLICY_VALUE_MOVE_DIMS, POLICY_VALUE_STATE_DIMS
except ImportError:
    from utils.config import POLICY_VALUE_MOVE_DIMS, POLICY_VALUE_STATE_DIMS


PLAYER_AUX_CONTEXT_FEATURE_DIM = 56


@final
class PolicyValueNet(nn.Module):
    def __init__(self, *, dropout: float = 0.0) -> None:
        super().__init__()
        state_layers: OrderedDict[str, nn.Module] = OrderedDict()
        for idx, (in_dim, out_dim) in enumerate(
            zip(POLICY_VALUE_STATE_DIMS[:-1], POLICY_VALUE_STATE_DIMS[1:], strict=False)
        ):
            state_layers[f"linear{idx}"] = nn.Linear(in_dim, out_dim)
            state_layers[f"layernorm{idx}"] = nn.LayerNorm(out_dim)
            state_layers[f"relu{idx}"] = nn.ReLU()
            if dropout > 0:
                state_layers[f"dropout{idx}"] = nn.Dropout(dropout)
        self.state_encoder: nn.Sequential = nn.Sequential(state_layers)

        move_layers: OrderedDict[str, nn.Module] = OrderedDict()
        for idx, (in_dim, out_dim) in enumerate(
            zip(POLICY_VALUE_MOVE_DIMS[:-1], POLICY_VALUE_MOVE_DIMS[1:], strict=False)
        ):
            move_layers[f"linear{idx}"] = nn.Linear(in_dim, out_dim)
            move_layers[f"layernorm{idx}"] = nn.LayerNorm(out_dim)
            move_layers[f"relu{idx}"] = nn.ReLU()
            if dropout > 0:
                move_layers[f"dropout{idx}"] = nn.Dropout(dropout)
        self.move_encoder: nn.Sequential = nn.Sequential(move_layers)

        aux_layers: OrderedDict[str, nn.Module] = OrderedDict()
        aux_layers["linear0"] = nn.Linear(PLAYER_AUX_CONTEXT_FEATURE_DIM, 64)
        aux_layers["layernorm0"] = nn.LayerNorm(64)
        aux_layers["relu0"] = nn.ReLU()
        if dropout > 0:
            aux_layers["dropout0"] = nn.Dropout(dropout)
        self.player_aux_context_encoder: nn.Sequential = nn.Sequential(aux_layers)

        state_dim = POLICY_VALUE_STATE_DIMS[-1]
        move_dim = POLICY_VALUE_MOVE_DIMS[-1]
        aux_context_dim = 64
        shared_policy_input_dim = state_dim + move_dim
        player_policy_input_dim = state_dim + move_dim + aux_context_dim
        value_input_dim = state_dim

        player_head_layers: OrderedDict[str, nn.Module] = OrderedDict()
        player_head_layers["linear0"] = nn.Linear(player_policy_input_dim, 128)
        player_head_layers["relu0"] = nn.ReLU()
        if dropout > 0:
            player_head_layers["dropout0"] = nn.Dropout(dropout)
        player_head_layers["linear1"] = nn.Linear(128, 1)
        self.player_policy_head: nn.Sequential = nn.Sequential(player_head_layers)

        search_head_layers: OrderedDict[str, nn.Module] = OrderedDict()
        search_head_layers["linear0"] = nn.Linear(shared_policy_input_dim, 128)
        search_head_layers["relu0"] = nn.ReLU()
        if dropout > 0:
            search_head_layers["dropout0"] = nn.Dropout(dropout)
        search_head_layers["linear1"] = nn.Linear(128, 1)
        self.search_policy_head: nn.Sequential = nn.Sequential(search_head_layers)

        value_head_layers: OrderedDict[str, nn.Module] = OrderedDict()
        value_head_layers["linear0"] = nn.Linear(value_input_dim, 64)
        value_head_layers["relu0"] = nn.ReLU()
        if dropout > 0:
            value_head_layers["dropout0"] = nn.Dropout(dropout)
        value_head_layers["linear1"] = nn.Linear(64, 1)
        self.value_head: nn.Sequential = nn.Sequential(value_head_layers)

    def _shared_embeddings(
        self,
        features: torch.Tensor,
        candidate_move_features: torch.Tensor,
    ) -> tuple[torch.Tensor, torch.Tensor]:
        state_embedding = cast(torch.Tensor, self.state_encoder(features))
        move_embedding = cast(torch.Tensor, self.move_encoder(candidate_move_features))
        return state_embedding, move_embedding

    def _masked_policy_logits(
        self,
        policy_scores: torch.Tensor,
        candidate_mask: torch.Tensor,
    ) -> torch.Tensor:
        return policy_scores.squeeze(-1).masked_fill(~candidate_mask, float("-inf"))

    def _forward_shared_core_from_embeddings(
        self,
        state_embedding: torch.Tensor,
        move_embedding: torch.Tensor,
        candidate_mask: torch.Tensor,
    ) -> tuple[torch.Tensor, torch.Tensor]:
        expanded_state = state_embedding.unsqueeze(1).expand(-1, move_embedding.shape[1], -1)
        search_policy_input = torch.cat([expanded_state, move_embedding], dim=-1)
        search_policy_scores = cast(torch.Tensor, self.search_policy_head(search_policy_input))
        value_scores = cast(torch.Tensor, self.value_head(state_embedding))
        return self._masked_policy_logits(
            search_policy_scores, candidate_mask
        ), value_scores.squeeze(-1)

    def forward_shared_core(
        self,
        features: torch.Tensor,
        candidate_move_features: torch.Tensor,
        candidate_mask: torch.Tensor,
    ) -> tuple[torch.Tensor, torch.Tensor]:
        state_embedding, move_embedding = self._shared_embeddings(features, candidate_move_features)
        return self._forward_shared_core_from_embeddings(
            state_embedding, move_embedding, candidate_mask
        )

    @override
    def forward(
        self,
        features: torch.Tensor,
        candidate_move_features: torch.Tensor,
        candidate_mask: torch.Tensor,
        player_aux_context_features: torch.Tensor,
    ) -> dict[str, torch.Tensor]:
        state_embedding, move_embedding = self._shared_embeddings(features, candidate_move_features)
        search_policy_logits, value = self._forward_shared_core_from_embeddings(
            state_embedding,
            move_embedding,
            candidate_mask,
        )
        context_embedding = cast(
            torch.Tensor, self.player_aux_context_encoder(player_aux_context_features)
        )
        expanded_state = state_embedding.unsqueeze(1).expand(-1, move_embedding.shape[1], -1)
        expanded_context = context_embedding.unsqueeze(1).expand(-1, move_embedding.shape[1], -1)
        player_policy_input = torch.cat([expanded_state, expanded_context, move_embedding], dim=-1)
        player_policy_scores = cast(torch.Tensor, self.player_policy_head(player_policy_input))
        player_policy_logits = self._masked_policy_logits(player_policy_scores, candidate_mask)
        return {
            "player_policy_logits": player_policy_logits,
            "search_policy_logits": search_policy_logits,
            "value": value,
        }
