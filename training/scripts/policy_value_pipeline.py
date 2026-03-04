from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
import json
from itertools import zip_longest
from pathlib import Path
from typing import Literal, cast

try:
    from ..utils.config import BYTES_PER_SAMPLE
    from ..utils.example_schema import (
        load_dataset_metadata,
        validate_binary_artifacts,
    )
    from ..utils.example_schema import group_ids_path, metadata_path
    from ..utils.policy_value_schema import (
        load_player_context_metadata,
        load_policy_value_metadata,
        policy_value_labels_path,
        policy_value_metadata_path,
        policy_value_player_context_metadata_path,
        policy_value_player_context_path,
        policy_value_requests_path,
    )
except ImportError:
    from utils.config import BYTES_PER_SAMPLE
    from utils.example_schema import load_dataset_metadata, validate_binary_artifacts
    from utils.example_schema import group_ids_path, metadata_path
    from utils.policy_value_schema import (
        load_player_context_metadata,
        load_policy_value_metadata,
        policy_value_labels_path,
        policy_value_metadata_path,
        policy_value_player_context_metadata_path,
        policy_value_player_context_path,
        policy_value_requests_path,
    )


PolicyValueSupervisionMode = Literal["search_control", "player_context_primary"]


@dataclass(slots=True)
class ArtifactReadiness:
    ready: bool
    dataset_ready: bool
    labels_ready: bool
    player_context_ready: bool
    missing_paths: list[Path]
    reasons: list[str]
    sample_count: int | None


POLICY_VALUE_BATCH_SIZE_BY_PROFILE = {
    "t4": 256,
    "l4": 512,
    "a10": 1024,
    "b200": 2048,
}

POLICY_VALUE_NUM_WORKERS_BY_PROFILE = {
    "t4": 2,
    "l4": 4,
    "a10": 4,
    "b200": 8,
}


def policy_value_batch_size_for_profile(profile_name: str) -> int:
    return POLICY_VALUE_BATCH_SIZE_BY_PROFILE.get(profile_name, 512)


def policy_value_num_workers_for_profile(profile_name: str) -> int:
    return POLICY_VALUE_NUM_WORKERS_BY_PROFILE.get(profile_name, 4)


def required_policy_value_artifact_paths(
    data_path: str | Path,
    supervision_mode: PolicyValueSupervisionMode = "search_control",
) -> list[Path]:
    data_path = Path(data_path)
    required = [
        data_path,
        metadata_path(data_path),
        group_ids_path(data_path),
        policy_value_requests_path(data_path),
        policy_value_labels_path(data_path),
        policy_value_metadata_path(data_path),
    ]
    if supervision_mode == "player_context_primary":
        required.extend(
            [
                policy_value_player_context_path(data_path),
                policy_value_player_context_metadata_path(data_path),
            ]
        )
    return required


def missing_policy_value_artifact_paths(
    data_path: str | Path,
    supervision_mode: PolicyValueSupervisionMode = "search_control",
) -> list[Path]:
    return [path for path in required_policy_value_artifact_paths(data_path, supervision_mode=supervision_mode) if not path.exists()]


def _count_non_empty_lines(path: Path) -> int:
    with path.open() as handle:
        return sum(1 for line in handle if line.strip())


def _identity_int(value: object, *, field_name: str, path: Path, line_number: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(
            f"{path.name} field {field_name} must be an int at line {line_number}, got {value!r}"
        )
    return value


def _identity_str(value: object, *, field_name: str, path: Path, line_number: int) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(
            f"{path.name} field {field_name} must be a non-empty string at line {line_number}, got {value!r}"
        )
    return value


def _iter_identity_keys(path: Path) -> tuple[tuple[str, int, int, int], ...]:
    identity_keys: list[tuple[str, int, int, int]] = []
    with path.open() as handle:
        for line_number, raw_line in enumerate(handle, start=1):
            if not raw_line.strip():
                continue
            payload_raw = cast(object, json.loads(raw_line))
            if not isinstance(payload_raw, dict):
                raise ValueError(f"{path.name} must contain JSON objects per line, got {type(payload_raw).__name__} at line {line_number}")
            payload = cast(dict[str, object], payload_raw)
            missing_fields = [field for field in ("replay_id", "round_id", "player_id", "frame_id") if field not in payload]
            if missing_fields:
                missing_display = ", ".join(missing_fields)
                raise ValueError(f"{path.name} missing identity fields at line {line_number}: {missing_display}")
            replay_id = payload["replay_id"]
            round_id = payload["round_id"]
            player_id = payload["player_id"]
            frame_id = payload["frame_id"]
            identity_keys.append(
                (
                    _identity_str(replay_id, field_name="replay_id", path=path, line_number=line_number),
                    _identity_int(round_id, field_name="round_id", path=path, line_number=line_number),
                    _identity_int(player_id, field_name="player_id", path=path, line_number=line_number),
                    _identity_int(frame_id, field_name="frame_id", path=path, line_number=line_number),
                )
            )
    return tuple(identity_keys)


def _validate_identity_alignment(reference_path: Path, candidate_path: Path, candidate_label: str) -> None:
    reference_keys = _iter_identity_keys(reference_path)
    candidate_keys = _iter_identity_keys(candidate_path)
    for line_number, (reference_key, candidate_key) in enumerate(
        zip_longest(reference_keys, candidate_keys),
        start=1,
    ):
        if reference_key != candidate_key:
            raise ValueError(
                f"{candidate_label} identity mismatch at line {line_number}: requests={reference_key!r} {candidate_label}={candidate_key!r}"
            )


def validate_label_identity_alignment(data_path: str | Path) -> None:
    data_path = Path(data_path)
    _validate_identity_alignment(
        policy_value_requests_path(data_path),
        policy_value_labels_path(data_path),
        "policy/value label",
    )


def validate_player_context_identity_alignment(data_path: str | Path) -> None:
    data_path = Path(data_path)
    _validate_identity_alignment(
        policy_value_requests_path(data_path),
        policy_value_player_context_path(data_path),
        "player-context",
    )


def artifact_readiness(
    data_path: str | Path,
    supervision_mode: PolicyValueSupervisionMode = "search_control",
) -> ArtifactReadiness:
    data_path = Path(data_path)
    missing = missing_policy_value_artifact_paths(data_path, supervision_mode=supervision_mode)
    reasons: list[str] = []
    sample_count: int | None = None
    dataset_paths = {
        data_path,
        metadata_path(data_path),
        group_ids_path(data_path),
        policy_value_requests_path(data_path),
    }
    label_paths = {
        policy_value_labels_path(data_path),
        policy_value_metadata_path(data_path),
    }
    player_context_paths = {
        policy_value_player_context_path(data_path),
        policy_value_player_context_metadata_path(data_path),
    }
    dataset_ready = not any(path in dataset_paths for path in missing)
    labels_ready = not any(path in label_paths for path in missing)
    player_context_required = supervision_mode == "player_context_primary"
    player_context_ready = (not player_context_required) or not any(path in player_context_paths for path in missing)

    if missing:
        reasons.extend(f"missing:{path.name}" for path in missing)

    if dataset_ready:
        try:
            dataset_metadata = load_dataset_metadata(data_path)
            raw_sample_count = dataset_metadata.get("sample_count")
            if not isinstance(raw_sample_count, int) or raw_sample_count <= 0:
                reasons.append(f"dataset sample_count must be positive, got {raw_sample_count!r}")
                dataset_ready = False
            else:
                sample_count = raw_sample_count
                expected_bytes = sample_count * BYTES_PER_SAMPLE
                actual_bytes = data_path.stat().st_size
                if actual_bytes != expected_bytes:
                    reasons.append(
                        f"dataset byte size mismatch: expected {expected_bytes}, got {actual_bytes}"
                    )
                    dataset_ready = False
                validate_binary_artifacts(data_path, sample_count)
                requests_count = _count_non_empty_lines(policy_value_requests_path(data_path))
                if requests_count != sample_count:
                    reasons.append(
                        f"policy/value oracle request count mismatch: expected {sample_count}, got {requests_count}"
                    )
                    dataset_ready = False
        except Exception as exc:
            reasons.append(str(exc))
            dataset_ready = False

    if labels_ready:
        try:
            policy_metadata = load_policy_value_metadata(data_path)
            policy_sample_count = policy_metadata.get("sample_count")
            if sample_count is None and isinstance(policy_sample_count, int):
                sample_count = policy_sample_count
            if not isinstance(policy_sample_count, int) or policy_sample_count <= 0:
                reasons.append(f"policy/value metadata sample_count must be positive, got {policy_sample_count!r}")
                labels_ready = False
            elif sample_count is not None and policy_sample_count != sample_count:
                reasons.append(
                    f"policy/value metadata sample_count mismatch: dataset={sample_count} policy_value={policy_sample_count}"
                )
                labels_ready = False
            labels_count = _count_non_empty_lines(policy_value_labels_path(data_path))
            if sample_count is not None and labels_count != sample_count:
                reasons.append(f"policy/value label count mismatch: expected {sample_count}, got {labels_count}")
                labels_ready = False
            if labels_ready:
                validate_label_identity_alignment(data_path)
        except Exception as exc:
            reasons.append(str(exc))
            labels_ready = False

    if player_context_required and player_context_ready:
        try:
            player_context_metadata = load_player_context_metadata(data_path)
            player_context_sample_count = player_context_metadata.get("sample_count")
            if not isinstance(player_context_sample_count, int) or player_context_sample_count <= 0:
                reasons.append(
                    f"player-context metadata sample_count must be positive, got {player_context_sample_count!r}"
                )
                player_context_ready = False
            elif sample_count is not None and player_context_sample_count != sample_count:
                reasons.append(
                    f"player-context metadata sample_count mismatch: dataset={sample_count} player_context={player_context_sample_count}"
                )
                player_context_ready = False
            context_count = _count_non_empty_lines(policy_value_player_context_path(data_path))
            if sample_count is not None and context_count != sample_count:
                reasons.append(
                    f"player-context count mismatch: expected {sample_count}, got {context_count}"
                )
                player_context_ready = False
            if player_context_ready:
                validate_player_context_identity_alignment(data_path)
        except Exception as exc:
            reasons.append(str(exc))
            player_context_ready = False

    return ArtifactReadiness(
        ready=dataset_ready and labels_ready and player_context_ready and not reasons,
        dataset_ready=dataset_ready,
        labels_ready=labels_ready,
        player_context_ready=player_context_ready,
        missing_paths=missing,
        reasons=reasons,
        sample_count=sample_count,
    )


def validate_policy_value_artifacts(
    data_path: str | Path,
    supervision_mode: PolicyValueSupervisionMode = "search_control",
) -> None:
    readiness = artifact_readiness(data_path, supervision_mode=supervision_mode)
    if readiness.ready:
        return
    if readiness.missing_paths:
        missing_display = ", ".join(str(path) for path in readiness.missing_paths)
        raise FileNotFoundError(f"Missing policy/value artifacts: {missing_display}")
    raise ValueError("; ".join(readiness.reasons) or "unknown policy/value artifact validation failure")


def policy_value_run_id(prefix: str = "policy-value") -> str:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return f"{prefix}-{timestamp}"
