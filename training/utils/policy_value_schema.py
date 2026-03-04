from __future__ import annotations

from dataclasses import asdict, dataclass
import json
from math import exp, isclose, isfinite
from pathlib import Path
from typing import Any


PHASE1_SCHEMA_VERSION = "phase1-v1"
PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION = "phase1-player-context-v1"
POLICY_VALUE_CONTRACT_VERSION = "policy-value-v2"
POLICY_VALUE_SHARED_INPUT_CONTRACT = "policy-value-shared-core-v2"
POLICY_VALUE_AUX_INPUT_CONTRACT = "policy-value-player-aux-v2"
PLAYER_TARGET_SEMANTICS = "exact_move_raw_preprocessed_v2"
GENERATION_MODE_SEARCH_ORACLE = "search_oracle"
GENERATION_MODE_PLAYER_CONTEXT = "player_context"
POLICY_VALUE_LABELS_SUFFIX = ".policy_value.jsonl"
POLICY_VALUE_METADATA_SUFFIX = ".policy_value.metadata.json"
POLICY_VALUE_REQUESTS_SUFFIX = ".policy_value.requests.jsonl"
POLICY_VALUE_PLAYER_CONTEXT_SUFFIX = ".policy_value.player_context.jsonl"
POLICY_VALUE_PLAYER_CONTEXT_METADATA_SUFFIX = ".policy_value.player_context.metadata.json"
MOVE_ID_CONTRACT = "Move.raw"
MOVE_RAW_PIECE_ORDER = "iotljsz"
ORACLE_PROFILE_STRONGER_OFFLINE = "stronger_offline_oracle"
POLICY_PROBS_RENORMALIZE_TOLERANCE = 1e-5


@dataclass(slots=True)
class PolicyValueTarget:
    schema_version: str
    replay_id: str
    round_id: int
    player_id: int
    frame_id: int
    group_id: str
    best_move_raw: int
    best_value: float
    position_complexity: float
    root_scores: list[tuple[int, float]]
    policy_probs: list[float]


@dataclass(slots=True)
class PolicyValueOracleRequest:
    schema_version: str
    replay_id: str
    round_id: int
    player_id: int
    frame_id: int
    group_id: str
    player_board_rows: list[int]
    opponent_board_rows: list[int]
    current_piece: str
    hold_piece: str | None
    queue: list[str]
    combo: int
    b2b: int
    lines: int
    pending_garbage: int
    bag_number: int


@dataclass(slots=True)
class PolicyValuePlayerContext:
    schema_version: str
    replay_id: str
    round_id: int
    player_id: int
    frame_id: int
    group_id: str
    spawn_piece: str
    actual_piece: str
    actual_move_raw: int
    actual_x: int
    actual_y: int
    actual_rotation: int
    actual_hold_used: bool
    actual_lines_cleared: int
    input_keys: list[str]
    hold_piece: str | None
    queue: list[str]
    recent_piece_sequence: list[str]
    future_piece_sequence: list[str]
    recent_hold_usage: list[bool]
    future_hold_usage: list[bool]


def policy_value_labels_path(data_path: str | Path) -> Path:
    return Path(f"{Path(data_path)}{POLICY_VALUE_LABELS_SUFFIX}")


def policy_value_metadata_path(data_path: str | Path) -> Path:
    return Path(f"{Path(data_path)}{POLICY_VALUE_METADATA_SUFFIX}")


def policy_value_requests_path(data_path: str | Path) -> Path:
    return Path(f"{Path(data_path)}{POLICY_VALUE_REQUESTS_SUFFIX}")


def policy_value_player_context_path(data_path: str | Path) -> Path:
    return Path(f"{Path(data_path)}{POLICY_VALUE_PLAYER_CONTEXT_SUFFIX}")


def policy_value_player_context_metadata_path(data_path: str | Path) -> Path:
    return Path(f"{Path(data_path)}{POLICY_VALUE_PLAYER_CONTEXT_METADATA_SUFFIX}")


def build_policy_value_metadata(
    *,
    sample_count: int,
    generation_mode: str,
    policy_temperature: float,
    oracle_profile: str | None = None,
    oracle_beam_width: int | None = None,
    oracle_depth: int | None = None,
    oracle_use_tt: bool | None = None,
) -> dict[str, Any]:
    metadata = {
        "schema_version": PHASE1_SCHEMA_VERSION,
        "contract_version": POLICY_VALUE_CONTRACT_VERSION,
        "generation_mode": generation_mode,
        "policy_temperature": policy_temperature,
        "sample_count": sample_count,
        "move_id_contract": MOVE_ID_CONTRACT,
        "shared_input_contract": POLICY_VALUE_SHARED_INPUT_CONTRACT,
        "runtime_compatible_shared_inputs": True,
    }
    if oracle_profile is not None:
        metadata["oracle_profile"] = oracle_profile
    if oracle_beam_width is not None:
        metadata["oracle_beam_width"] = oracle_beam_width
    if oracle_depth is not None:
        metadata["oracle_depth"] = oracle_depth
    if oracle_use_tt is not None:
        metadata["oracle_use_tt"] = oracle_use_tt
    return metadata


def validate_policy_value_metadata(metadata: dict[str, Any]) -> None:
    if metadata.get("schema_version") != PHASE1_SCHEMA_VERSION:
        raise ValueError(f"Invalid schema version: {metadata.get('schema_version')!r}")
    if metadata.get("contract_version") != POLICY_VALUE_CONTRACT_VERSION:
        raise ValueError(f"Invalid contract version: {metadata.get('contract_version')!r}")
    if metadata.get("generation_mode") not in {GENERATION_MODE_SEARCH_ORACLE, GENERATION_MODE_PLAYER_CONTEXT}:
        raise ValueError(f"Invalid generation mode: {metadata.get('generation_mode')!r}")
    temperature = metadata.get("policy_temperature")
    if not isinstance(temperature, (int, float)) or temperature <= 0:
        raise ValueError(f"Invalid policy temperature: {temperature!r}")
    sample_count = metadata.get("sample_count")
    if not isinstance(sample_count, int) or sample_count < 0:
        raise ValueError(f"Invalid sample count: {sample_count!r}")
    if metadata.get("move_id_contract") != MOVE_ID_CONTRACT:
        raise ValueError(f"Invalid move id contract: {metadata.get('move_id_contract')!r}")
    if metadata.get("shared_input_contract") != POLICY_VALUE_SHARED_INPUT_CONTRACT:
        raise ValueError(f"Invalid shared_input_contract: {metadata.get('shared_input_contract')!r}")
    if metadata.get("runtime_compatible_shared_inputs") is not True:
        raise ValueError(
            f"Invalid runtime_compatible_shared_inputs: {metadata.get('runtime_compatible_shared_inputs')!r}"
        )
    oracle_beam_width = metadata.get("oracle_beam_width")
    if oracle_beam_width is not None and (not isinstance(oracle_beam_width, int) or oracle_beam_width <= 0):
        raise ValueError(f"Invalid oracle_beam_width: {oracle_beam_width!r}")
    oracle_depth = metadata.get("oracle_depth")
    if oracle_depth is not None and (not isinstance(oracle_depth, int) or oracle_depth <= 0):
        raise ValueError(f"Invalid oracle_depth: {oracle_depth!r}")


def build_player_context_metadata(
    *,
    sample_count: int,
    recent_horizon: int,
    future_horizon: int,
) -> dict[str, Any]:
    return {
        "schema_version": PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
        "contract_version": POLICY_VALUE_CONTRACT_VERSION,
        "generation_mode": GENERATION_MODE_PLAYER_CONTEXT,
        "sample_count": sample_count,
        "recent_horizon": recent_horizon,
        "future_horizon": future_horizon,
        "aux_input_contract": POLICY_VALUE_AUX_INPUT_CONTRACT,
        "player_target_semantics": PLAYER_TARGET_SEMANTICS,
    }


def validate_player_context_metadata(metadata: dict[str, Any]) -> None:
    if metadata.get("schema_version") != PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION:
        raise ValueError(f"Invalid schema version: {metadata.get('schema_version')!r}")
    if metadata.get("contract_version") != POLICY_VALUE_CONTRACT_VERSION:
        raise ValueError(f"Invalid contract version: {metadata.get('contract_version')!r}")
    if metadata.get("generation_mode") != GENERATION_MODE_PLAYER_CONTEXT:
        raise ValueError(f"Invalid generation mode: {metadata.get('generation_mode')!r}")
    sample_count = metadata.get("sample_count")
    if not isinstance(sample_count, int) or sample_count < 0:
        raise ValueError(f"Invalid sample count: {sample_count!r}")
    recent_horizon = metadata.get("recent_horizon")
    if not isinstance(recent_horizon, int) or recent_horizon < 0:
        raise ValueError(f"Invalid recent_horizon: {recent_horizon!r}")
    future_horizon = metadata.get("future_horizon")
    if not isinstance(future_horizon, int) or future_horizon < 0:
        raise ValueError(f"Invalid future_horizon: {future_horizon!r}")
    if metadata.get("aux_input_contract") != POLICY_VALUE_AUX_INPUT_CONTRACT:
        raise ValueError(f"Invalid aux_input_contract: {metadata.get('aux_input_contract')!r}")
    if metadata.get("player_target_semantics") != PLAYER_TARGET_SEMANTICS:
        raise ValueError(f"Invalid player_target_semantics: {metadata.get('player_target_semantics')!r}")


def move_raw_piece_id(piece: str) -> int:
    piece_normalized = piece.lower()
    if piece_normalized not in MOVE_RAW_PIECE_ORDER:
        raise ValueError(f"Unsupported Move.raw piece: {piece!r}")
    return MOVE_RAW_PIECE_ORDER.index(piece_normalized)


def encode_move_raw(*, piece: str, x: int, y: int, rotation: int, spin: bool = False) -> int:
    if not 0 <= x <= 0x0F:
        raise ValueError(f"Move.raw x out of range: {x}")
    if not 0 <= y <= 0x3F:
        raise ValueError(f"Move.raw y out of range: {y}")
    if not 0 <= rotation <= 0x03:
        raise ValueError(f"Move.raw rotation out of range: {rotation}")
    piece_value = move_raw_piece_id(piece)
    return y | (x << 6) | (piece_value << 10) | (rotation << 13) | (int(spin) << 15)


def softmax_root_scores(scores: list[float], *, temperature: float) -> list[float]:
    if not scores:
        raise ValueError("Cannot normalize empty root score list")
    if temperature <= 0:
        raise ValueError(f"Temperature must be positive, got {temperature}")
    scaled = [score / temperature for score in scores]
    max_score = max(scaled)
    exps = [exp(score - max_score) for score in scaled]
    total = sum(exps)
    if total <= 0:
        raise ValueError("Softmax total must be positive")
    return [value / total for value in exps]


def validate_policy_value_target(target: PolicyValueTarget) -> None:
    if target.schema_version != PHASE1_SCHEMA_VERSION:
        raise ValueError(f"Unexpected schema version: {target.schema_version}")
    if not target.root_scores:
        raise ValueError("PolicyValueTarget.root_scores must not be empty")
    if len(target.root_scores) != len(target.policy_probs):
        raise ValueError("root_scores and policy_probs length mismatch")
    if not isclose(sum(target.policy_probs), 1.0, rel_tol=1e-6, abs_tol=1e-6):
        raise ValueError("policy_probs must sum to 1")
    for move_raw, _ in target.root_scores:
        if move_raw < 0 or move_raw > 0xFFFF:
            raise ValueError(f"Invalid move id: {move_raw}")
    if not any(move_raw == target.best_move_raw for move_raw, _ in target.root_scores):
        raise ValueError("best_move_raw must exist in root_scores")


def maybe_renormalize_policy_probs(policy_probs: list[float]) -> list[float]:
    if not policy_probs:
        return policy_probs
    if any((not isfinite(value)) or value < 0 for value in policy_probs):
        return policy_probs
    total = sum(policy_probs)
    if total <= 0:
        return policy_probs
    if isclose(total, 1.0, rel_tol=POLICY_PROBS_RENORMALIZE_TOLERANCE, abs_tol=POLICY_PROBS_RENORMALIZE_TOLERANCE):
        return [value / total for value in policy_probs]
    return policy_probs


def validate_policy_value_oracle_request(request: PolicyValueOracleRequest) -> None:
    if request.schema_version != PHASE1_SCHEMA_VERSION:
        raise ValueError(f"Unexpected schema version: {request.schema_version}")
    if not request.current_piece:
        raise ValueError("current_piece is required")
    if len(request.queue) > 5:
        raise ValueError("queue length must be at most 5")


def validate_player_context(context: PolicyValuePlayerContext) -> None:
    if context.schema_version != PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION:
        raise ValueError(f"Unexpected schema version: {context.schema_version}")
    if not context.spawn_piece:
        raise ValueError("spawn_piece is required")
    if not context.actual_piece:
        raise ValueError("actual_piece is required")
    if context.actual_move_raw < 0 or context.actual_move_raw > 0xFFFF:
        raise ValueError(f"Invalid actual_move_raw: {context.actual_move_raw}")
    if not context.input_keys:
        raise ValueError("input_keys must not be empty")
    if len(context.queue) > 5:
        raise ValueError("queue length must be at most 5")
    if len(context.recent_piece_sequence) != len(context.recent_hold_usage):
        raise ValueError("recent_piece_sequence and recent_hold_usage length mismatch")
    if len(context.future_piece_sequence) != len(context.future_hold_usage):
        raise ValueError("future_piece_sequence and future_hold_usage length mismatch")


def write_policy_value_metadata(
    data_path: str | Path,
    *,
    sample_count: int,
    generation_mode: str,
    policy_temperature: float,
    oracle_profile: str | None = None,
    oracle_beam_width: int | None = None,
    oracle_depth: int | None = None,
    oracle_use_tt: bool | None = None,
) -> Path:
    path = policy_value_metadata_path(data_path)
    metadata = build_policy_value_metadata(
        sample_count=sample_count,
        generation_mode=generation_mode,
        policy_temperature=policy_temperature,
        oracle_profile=oracle_profile,
        oracle_beam_width=oracle_beam_width,
        oracle_depth=oracle_depth,
        oracle_use_tt=oracle_use_tt,
    )
    validate_policy_value_metadata(metadata)
    with path.open("w") as f:
        json.dump(metadata, f, indent=2, sort_keys=True)
        f.write("\n")
    return path


def load_policy_value_metadata(data_path: str | Path) -> dict[str, Any]:
    path = policy_value_metadata_path(data_path)
    if not path.exists():
        raise FileNotFoundError(f"Policy/value metadata sidecar missing: {path}")
    with path.open() as f:
        metadata = json.load(f)
    validate_policy_value_metadata(metadata)
    return metadata


def write_player_context_metadata(
    data_path: str | Path,
    *,
    sample_count: int,
    recent_horizon: int,
    future_horizon: int,
) -> Path:
    path = policy_value_player_context_metadata_path(data_path)
    metadata = build_player_context_metadata(
        sample_count=sample_count,
        recent_horizon=recent_horizon,
        future_horizon=future_horizon,
    )
    validate_player_context_metadata(metadata)
    with path.open("w") as f:
        json.dump(metadata, f, indent=2, sort_keys=True)
        f.write("\n")
    return path


def load_player_context_metadata(data_path: str | Path) -> dict[str, Any]:
    path = policy_value_player_context_metadata_path(data_path)
    if not path.exists():
        raise FileNotFoundError(f"Policy/value player-context metadata sidecar missing: {path}")
    with path.open() as f:
        metadata = json.load(f)
    validate_player_context_metadata(metadata)
    return metadata


def write_policy_value_targets(data_path: str | Path, targets: list[PolicyValueTarget]) -> Path:
    path = policy_value_labels_path(data_path)
    with path.open("w") as f:
        for target in targets:
            f.write(serialize_policy_value_target(target))
            f.write("\n")
    return path


def load_policy_value_targets(data_path: str | Path) -> list[PolicyValueTarget]:
    path = policy_value_labels_path(data_path)
    if not path.exists():
        raise FileNotFoundError(f"Policy/value labels sidecar missing: {path}")
    targets: list[PolicyValueTarget] = []
    with path.open() as f:
        for line in f:
            stripped = line.strip()
            if not stripped:
                continue
            targets.append(deserialize_policy_value_target(stripped))
    return targets


def serialize_policy_value_oracle_request(request: PolicyValueOracleRequest) -> str:
    validate_policy_value_oracle_request(request)
    return json.dumps(asdict(request), separators=(",", ":"), sort_keys=True)


def deserialize_policy_value_oracle_request(payload: str) -> PolicyValueOracleRequest:
    raw = json.loads(payload)
    request = PolicyValueOracleRequest(
        schema_version=raw["schema_version"],
        replay_id=raw["replay_id"],
        round_id=raw["round_id"],
        player_id=raw["player_id"],
        frame_id=raw["frame_id"],
        group_id=raw["group_id"],
        player_board_rows=list(raw["player_board_rows"]),
        opponent_board_rows=list(raw["opponent_board_rows"]),
        current_piece=raw["current_piece"],
        hold_piece=raw.get("hold_piece"),
        queue=list(raw["queue"]),
        combo=raw["combo"],
        b2b=raw["b2b"],
        lines=raw["lines"],
        pending_garbage=raw["pending_garbage"],
        bag_number=raw["bag_number"],
    )
    validate_policy_value_oracle_request(request)
    return request


def write_policy_value_oracle_requests(data_path: str | Path, requests: list[PolicyValueOracleRequest]) -> Path:
    path = policy_value_requests_path(data_path)
    with path.open("w") as f:
        for request in requests:
            f.write(serialize_policy_value_oracle_request(request))
            f.write("\n")
    return path


def load_policy_value_oracle_requests(data_path: str | Path) -> list[PolicyValueOracleRequest]:
    path = policy_value_requests_path(data_path)
    if not path.exists():
        raise FileNotFoundError(f"Policy/value oracle request sidecar missing: {path}")
    requests: list[PolicyValueOracleRequest] = []
    with path.open() as f:
        for line in f:
            stripped = line.strip()
            if not stripped:
                continue
            requests.append(deserialize_policy_value_oracle_request(stripped))
    return requests


def serialize_player_context(context: PolicyValuePlayerContext) -> str:
    validate_player_context(context)
    return json.dumps(asdict(context), separators=(",", ":"), sort_keys=True)


def deserialize_player_context(payload: str) -> PolicyValuePlayerContext:
    raw = json.loads(payload)
    context = PolicyValuePlayerContext(
        schema_version=raw["schema_version"],
        replay_id=raw["replay_id"],
        round_id=raw["round_id"],
        player_id=raw["player_id"],
        frame_id=raw["frame_id"],
        group_id=raw["group_id"],
        spawn_piece=raw["spawn_piece"],
        actual_piece=raw["actual_piece"],
        actual_move_raw=raw["actual_move_raw"],
        actual_x=raw["actual_x"],
        actual_y=raw["actual_y"],
        actual_rotation=raw["actual_rotation"],
        actual_hold_used=raw["actual_hold_used"],
        actual_lines_cleared=raw["actual_lines_cleared"],
        input_keys=list(raw["input_keys"]),
        hold_piece=raw.get("hold_piece"),
        queue=list(raw["queue"]),
        recent_piece_sequence=list(raw["recent_piece_sequence"]),
        future_piece_sequence=list(raw["future_piece_sequence"]),
        recent_hold_usage=list(raw["recent_hold_usage"]),
        future_hold_usage=list(raw["future_hold_usage"]),
    )
    validate_player_context(context)
    return context


def write_player_contexts(data_path: str | Path, contexts: list[PolicyValuePlayerContext]) -> Path:
    path = policy_value_player_context_path(data_path)
    with path.open("w") as f:
        for context in contexts:
            f.write(serialize_player_context(context))
            f.write("\n")
    return path


def load_player_contexts(data_path: str | Path) -> list[PolicyValuePlayerContext]:
    path = policy_value_player_context_path(data_path)
    if not path.exists():
        raise FileNotFoundError(f"Policy/value player-context sidecar missing: {path}")
    contexts: list[PolicyValuePlayerContext] = []
    with path.open() as f:
        for line in f:
            stripped = line.strip()
            if not stripped:
                continue
            contexts.append(deserialize_player_context(stripped))
    return contexts


def serialize_policy_value_target(target: PolicyValueTarget) -> str:
    validate_policy_value_target(target)
    return json.dumps(asdict(target), separators=(",", ":"), sort_keys=True)


def deserialize_policy_value_target(payload: str) -> PolicyValueTarget:
    raw = json.loads(payload)
    policy_probs = maybe_renormalize_policy_probs(list(raw["policy_probs"]))
    target = PolicyValueTarget(
        schema_version=raw["schema_version"],
        replay_id=raw["replay_id"],
        round_id=raw["round_id"],
        player_id=raw["player_id"],
        frame_id=raw["frame_id"],
        group_id=raw["group_id"],
        best_move_raw=raw["best_move_raw"],
        best_value=raw["best_value"],
        position_complexity=raw["position_complexity"],
        root_scores=[(move_raw, score) for move_raw, score in raw["root_scores"]],
        policy_probs=policy_probs,
    )
    validate_policy_value_target(target)
    return target
