from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Any

import torch

try:
    from ..models.policy_value import PolicyValueNet
    from ..utils.config import MOVE_FEATURE_DIM, TOTAL_FEATURES
except ImportError:
    TRAINING_ROOT = Path(__file__).resolve().parents[1]
    if str(TRAINING_ROOT) not in sys.path:
        sys.path.insert(0, str(TRAINING_ROOT))
    from models.policy_value import PolicyValueNet
    from utils.config import MOVE_FEATURE_DIM, TOTAL_FEATURES


PHASE2_RUNTIME_SCHEMA_VERSION = "phase2-runtime-v2"
POLICY_VALUE_SHARED_INPUT_CONTRACT = "policy-value-shared-core-v2"
POLICY_VALUE_ONNX_SUFFIX = ".policy_value.onnx"
POLICY_VALUE_ONNX_METADATA_SUFFIX = ".policy_value.onnx.metadata.json"
PHASE2_CANDIDATE_CAPACITY = 64


class _PolicyValueOnnxWrapper(torch.nn.Module):
    def __init__(self, inner: PolicyValueNet) -> None:
        super().__init__()
        self.inner = inner

    def forward(
        self,
        features: torch.Tensor,
        candidate_move_features: torch.Tensor,
        candidate_mask: torch.Tensor,
    ) -> tuple[torch.Tensor, torch.Tensor]:
        return self.inner.forward_shared_core(features, candidate_move_features, candidate_mask)


class _PolicyValuePlayerOnnxWrapper(torch.nn.Module):
    """Wrapper that exports the player policy head (human-aligned) + value."""

    def __init__(self, inner: PolicyValueNet) -> None:
        super().__init__()
        self.inner = inner

    def forward(
        self,
        features: torch.Tensor,
        candidate_move_features: torch.Tensor,
        candidate_mask: torch.Tensor,
        player_aux_context_features: torch.Tensor,
    ) -> tuple[torch.Tensor, torch.Tensor]:
        out = self.inner(
            features, candidate_move_features, candidate_mask, player_aux_context_features
        )
        return out["player_policy_logits"], out["value"]


def policy_value_onnx_path(checkpoint_path: str | Path) -> Path:
    path = Path(checkpoint_path)
    return path.with_suffix(f"{path.suffix}{POLICY_VALUE_ONNX_SUFFIX}")


def policy_value_onnx_metadata_path(checkpoint_path: str | Path) -> Path:
    path = Path(checkpoint_path)
    return path.with_suffix(f"{path.suffix}{POLICY_VALUE_ONNX_METADATA_SUFFIX}")


def build_policy_value_onnx_metadata(*, model_path: str | Path) -> dict[str, Any]:
    return {
        "schema_version": PHASE2_RUNTIME_SCHEMA_VERSION,
        "format": "onnx",
        "model_path": str(Path(model_path).name),
        "state_feature_dim": TOTAL_FEATURES,
        "move_feature_dim": MOVE_FEATURE_DIM,
        "policy_output": "policy_logits",
        "value_output": "value",
        "policy_head_type": "candidate_ranking",
        "move_id_contract": "Move.raw",
        "candidate_capacity": PHASE2_CANDIDATE_CAPACITY,
        "shared_input_contract": POLICY_VALUE_SHARED_INPUT_CONTRACT,
    }


def export_policy_value_onnx(
    checkpoint_path: str | Path,
    *,
    output_path: str | Path | None = None,
    metadata_path: str | Path | None = None,
    opset_version: int = 18,
) -> tuple[Path, Path]:
    checkpoint_path = Path(checkpoint_path)
    output_path = (
        Path(output_path) if output_path is not None else policy_value_onnx_path(checkpoint_path)
    )
    metadata_path = (
        Path(metadata_path)
        if metadata_path is not None
        else policy_value_onnx_metadata_path(checkpoint_path)
    )

    module = PolicyValueNet()
    ckpt = torch.load(checkpoint_path, map_location="cpu")
    state_dict = ckpt.get("state_dict", ckpt)
    normalized: dict[str, torch.Tensor] = {}
    for key, value in state_dict.items():
        clean = key
        for prefix in ("model.", "student.", "policy_value."):
            if clean.startswith(prefix):
                clean = clean[len(prefix) :]
                break
        clean = clean.removeprefix("_orig_mod.")
        normalized[clean] = value
    module.load_state_dict(normalized, strict=False)
    module.eval()

    wrapper = _PolicyValueOnnxWrapper(module)
    wrapper.eval()
    dummy_features = torch.zeros((1, TOTAL_FEATURES), dtype=torch.float32)
    dummy_moves = torch.zeros((1, PHASE2_CANDIDATE_CAPACITY, MOVE_FEATURE_DIM), dtype=torch.float32)
    dummy_mask = torch.ones((1, PHASE2_CANDIDATE_CAPACITY), dtype=torch.bool)

    torch.onnx.export(
        wrapper,
        (dummy_features, dummy_moves, dummy_mask),
        output_path,
        input_names=["features", "candidate_move_features", "candidate_mask"],
        output_names=["policy_logits", "value"],
        dynamic_axes={
            "features": {0: "batch"},
            "candidate_move_features": {0: "batch"},
            "candidate_mask": {0: "batch"},
            "policy_logits": {0: "batch"},
            "value": {0: "batch"},
        },
        opset_version=opset_version,
        dynamo=False,
    )

    metadata = build_policy_value_onnx_metadata(model_path=output_path)
    with metadata_path.open("w") as f:
        json.dump(metadata, f, indent=2, sort_keys=True)
        f.write("\n")
    return output_path, metadata_path


def export_player_head_onnx(
    checkpoint_path: str | Path,
    *,
    output_path: str | Path | None = None,
    metadata_path: str | Path | None = None,
    opset_version: int = 18,
) -> tuple[Path, Path]:
    from models.policy_value import PLAYER_AUX_CONTEXT_FEATURE_DIM

    checkpoint_path = Path(checkpoint_path)
    output_path = (
        Path(output_path)
        if output_path is not None
        else checkpoint_path.with_suffix(".player.onnx")
    )
    metadata_path = (
        Path(metadata_path)
        if metadata_path is not None
        else output_path.with_suffix(".onnx.metadata.json")
    )

    module = PolicyValueNet()
    ckpt = torch.load(checkpoint_path, map_location="cpu")
    state_dict = ckpt.get("state_dict", ckpt)
    normalized: dict[str, torch.Tensor] = {}
    for key, value in state_dict.items():
        clean = key
        for prefix in ("model.", "student.", "policy_value."):
            if clean.startswith(prefix):
                clean = clean[len(prefix) :]
                break
        clean = clean.removeprefix("_orig_mod.")
        normalized[clean] = value
    module.load_state_dict(normalized, strict=False)
    module.eval()

    wrapper = _PolicyValuePlayerOnnxWrapper(module)
    wrapper.eval()
    dummy_features = torch.zeros((1, TOTAL_FEATURES), dtype=torch.float32)
    dummy_moves = torch.zeros((1, PHASE2_CANDIDATE_CAPACITY, MOVE_FEATURE_DIM), dtype=torch.float32)
    dummy_mask = torch.ones((1, PHASE2_CANDIDATE_CAPACITY), dtype=torch.bool)
    dummy_context = torch.zeros((1, PLAYER_AUX_CONTEXT_FEATURE_DIM), dtype=torch.float32)

    torch.onnx.export(
        wrapper,
        (dummy_features, dummy_moves, dummy_mask, dummy_context),
        output_path,
        input_names=["features", "candidate_move_features", "candidate_mask", "player_aux_context"],
        output_names=["player_policy_logits", "value"],
        dynamic_axes={
            "features": {0: "batch"},
            "candidate_move_features": {0: "batch"},
            "candidate_mask": {0: "batch"},
            "player_aux_context": {0: "batch"},
            "player_policy_logits": {0: "batch"},
            "value": {0: "batch"},
        },
        opset_version=opset_version,
        dynamo=False,
    )

    metadata = build_policy_value_onnx_metadata(model_path=output_path)
    metadata["policy_head_type"] = "player_context_ranking"
    metadata["player_aux_context_dim"] = PLAYER_AUX_CONTEXT_FEATURE_DIM
    with metadata_path.open("w") as f:
        json.dump(metadata, f, indent=2, sort_keys=True)
        f.write("\n")
    return output_path, metadata_path


def main() -> None:
    parser = argparse.ArgumentParser(description="Export policy/value checkpoint to ONNX")
    parser.add_argument("checkpoint")
    parser.add_argument("--output")
    parser.add_argument("--metadata")
    parser.add_argument(
        "--player-head", action="store_true", help="Export player policy head instead of search"
    )
    args = parser.parse_args()
    if args.player_head:
        model_path, metadata_path = export_player_head_onnx(
            args.checkpoint,
            output_path=args.output,
            metadata_path=args.metadata,
        )
    else:
        model_path, metadata_path = export_policy_value_onnx(
            args.checkpoint,
            output_path=args.output,
            metadata_path=args.metadata,
        )
    print(model_path)
    print(metadata_path)


if __name__ == "__main__":
    main()
