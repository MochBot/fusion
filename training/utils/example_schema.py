"""Canonical Phase 0 schema helpers for training artifacts and replay examples."""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import blake2b
import json
from pathlib import Path
from typing import Any

import numpy as np

try:
    from .config import (
        BOARD_CELLS,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        FLOATS_PER_SAMPLE,
        LABELS_PER_SAMPLE,
        NUM_PIECE_TYPES,
        PIECE_ONE_HOTS,
        PLAYER_BOARD_FEATURES,
        TOTAL_FEATURES,
    )
except ImportError:
    from utils.config import (
        BOARD_CELLS,
        BOARD_HEIGHT,
        BOARD_WIDTH,
        FLOATS_PER_SAMPLE,
        LABELS_PER_SAMPLE,
        NUM_PIECE_TYPES,
        PIECE_ONE_HOTS,
        PLAYER_BOARD_FEATURES,
        TOTAL_FEATURES,
    )


SCHEMA_VERSION = "phase0-v1"
BOARD_ENCODING = "column_major_binary"
PIECE_ORDER = ("i", "j", "l", "o", "s", "t", "z")
PIECE_INDEX = {name: idx for idx, name in enumerate(PIECE_ORDER)}
RUNTIME_EXTERNAL_PIECE_ORDER = ("i", "o", "t", "s", "z", "j", "l")
SCALAR_ORDER = ("combo", "b2b", "lines", "garbage_pending", "bag_number")
SCALAR_INDEX = {name: idx for idx, name in enumerate(SCALAR_ORDER)}
LABEL_ORDER = (
    "game_outcome",
    "lines_sent",
    "b2b_after",
    "position_normalized",
    "time_to_topout",
)

METADATA_SUFFIX = ".metadata.json"
GROUP_SIDECAR_SUFFIX = ".groups.u64"

FEATURE_LAYOUT = {
    "player_board": (0, BOARD_CELLS),
    "opponent_board": (BOARD_CELLS, BOARD_CELLS),
    "pieces": (BOARD_CELLS * 2, PIECE_ONE_HOTS),
    "scalars": (TOTAL_FEATURES - len(SCALAR_ORDER), len(SCALAR_ORDER)),
}


@dataclass(slots=True, frozen=True)
class ExampleIdentity:
    schema_version: str
    replay_id: str
    round_id: int
    player_id: int
    frame_id: int
    group_id: str


@dataclass(slots=True)
class CanonicalExample:
    identity: ExampleIdentity
    player_board: np.ndarray
    opponent_board: np.ndarray
    current_piece: str | None
    hold_piece: str | None
    queue: tuple[str, ...]
    combo: int
    b2b: int
    lines_cleared_total: int
    pending_garbage: int
    bag_number: int
    game_outcome: float
    lines_sent: float
    b2b_after: float
    position_normalized: float
    time_to_topout: float


def metadata_path(data_path: str | Path) -> Path:
    return Path(f"{Path(data_path)}{METADATA_SUFFIX}")


def group_ids_path(data_path: str | Path) -> Path:
    return Path(f"{Path(data_path)}{GROUP_SIDECAR_SUFFIX}")


def stable_group_hash(group_id: str) -> int:
    digest = blake2b(group_id.encode("utf-8"), digest_size=8).digest()
    return int.from_bytes(digest, byteorder="little", signed=False)


def build_dataset_metadata(sample_count: int | None = None) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "board_encoding": BOARD_ENCODING,
        "board_width": BOARD_WIDTH,
        "board_height": BOARD_HEIGHT,
        "total_features": TOTAL_FEATURES,
        "floats_per_sample": FLOATS_PER_SAMPLE,
        "label_count": LABELS_PER_SAMPLE,
        "piece_order": list(PIECE_ORDER),
        "scalar_order": list(SCALAR_ORDER),
        "label_order": list(LABEL_ORDER),
        "runtime_external_piece_order": list(RUNTIME_EXTERNAL_PIECE_ORDER),
        "group_sidecar_suffix": GROUP_SIDECAR_SUFFIX,
        "sample_count": sample_count,
    }


def validate_dataset_metadata(metadata: dict[str, Any]) -> None:
    expected = build_dataset_metadata(sample_count=metadata.get("sample_count"))
    for key in (
        "schema_version",
        "board_encoding",
        "board_width",
        "board_height",
        "total_features",
        "floats_per_sample",
        "label_count",
    ):
        if metadata.get(key) != expected[key]:
            raise ValueError(f"Invalid dataset metadata for {key}: {metadata.get(key)!r}")
    if tuple(metadata.get("piece_order", ())) != PIECE_ORDER:
        raise ValueError("Piece order mismatch in dataset metadata")
    if tuple(metadata.get("scalar_order", ())) != SCALAR_ORDER:
        raise ValueError("Scalar order mismatch in dataset metadata")
    if tuple(metadata.get("label_order", ())) != LABEL_ORDER:
        raise ValueError("Label order mismatch in dataset metadata")


def load_dataset_metadata(data_path: str | Path) -> dict[str, Any]:
    path = metadata_path(data_path)
    if not path.exists():
        raise FileNotFoundError(
            f"Dataset metadata sidecar missing: {path}. Re-run preprocess_replays.py to regenerate training artifacts."
        )
    with path.open() as f:
        metadata = json.load(f)
    validate_dataset_metadata(metadata)
    return metadata


def write_dataset_metadata(data_path: str | Path, sample_count: int) -> Path:
    path = metadata_path(data_path)
    with path.open("w") as f:
        json.dump(build_dataset_metadata(sample_count=sample_count), f, indent=2, sort_keys=True)
        f.write("\n")
    return path


def validate_binary_artifacts(data_path: str | Path, sample_count: int) -> None:
    data_path = Path(data_path)
    group_path = group_ids_path(data_path)
    if not data_path.exists():
        raise FileNotFoundError(f"Dataset file not found: {data_path}")
    if not group_path.exists():
        raise FileNotFoundError(f"Group sidecar missing: {group_path}")
    expected_group_bytes = sample_count * np.dtype(np.uint64).itemsize
    if group_path.stat().st_size != expected_group_bytes:
        raise ValueError(
            f"Group sidecar size mismatch: expected {expected_group_bytes}, got {group_path.stat().st_size}"
        )


def scalar_slot(name: str) -> int:
    try:
        return SCALAR_INDEX[name]
    except KeyError as exc:
        raise KeyError(f"Unknown scalar field: {name}") from exc


def piece_slot(name: str) -> int:
    try:
        return PIECE_INDEX[name]
    except KeyError as exc:
        raise KeyError(f"Unknown piece field: {name}") from exc


def validate_example(example: CanonicalExample) -> None:
    if example.identity.schema_version != SCHEMA_VERSION:
        raise ValueError(f"Unexpected schema version: {example.identity.schema_version}")
    if example.player_board.shape != (BOARD_HEIGHT, BOARD_WIDTH):
        raise ValueError(f"Bad player board shape: {example.player_board.shape}")
    if example.opponent_board.shape != (BOARD_HEIGHT, BOARD_WIDTH):
        raise ValueError(f"Bad opponent board shape: {example.opponent_board.shape}")
    if len(example.queue) > 5:
        raise ValueError(f"Queue too long: {len(example.queue)}")
    for piece in (example.current_piece, example.hold_piece, *example.queue):
        if piece is not None and piece not in PIECE_INDEX:
            raise ValueError(f"Unknown piece in canonical example: {piece}")


def build_feature_vector(example: CanonicalExample) -> np.ndarray:
    validate_example(example)
    features = np.zeros(TOTAL_FEATURES, dtype=np.float32)
    features[0:BOARD_CELLS] = example.player_board.T.flatten()
    features[BOARD_CELLS : BOARD_CELLS * 2] = example.opponent_board.T.flatten()

    offset = FEATURE_LAYOUT["pieces"][0]
    if example.current_piece is not None:
        features[offset + piece_slot(example.current_piece)] = 1.0
    offset += NUM_PIECE_TYPES
    if example.hold_piece is not None:
        features[offset + piece_slot(example.hold_piece)] = 1.0
    offset += NUM_PIECE_TYPES
    for queue_index in range(5):
        if queue_index < len(example.queue):
            features[offset + piece_slot(example.queue[queue_index])] = 1.0
        offset += NUM_PIECE_TYPES

    scalar_start = FEATURE_LAYOUT["scalars"][0]
    features[scalar_start + scalar_slot("combo")] = min(example.combo / 20.0, 1.0)
    features[scalar_start + scalar_slot("b2b")] = min(example.b2b / 10.0, 1.0)
    features[scalar_start + scalar_slot("lines")] = min(example.lines_cleared_total / 100.0, 1.0)
    features[scalar_start + scalar_slot("garbage_pending")] = min(example.pending_garbage / 12.0, 1.0)
    features[scalar_start + scalar_slot("bag_number")] = min(example.bag_number / 20.0, 1.0)
    return features


def build_label_vector(example: CanonicalExample) -> np.ndarray:
    labels = np.zeros(LABELS_PER_SAMPLE, dtype=np.float32)
    labels[0] = example.game_outcome
    labels[1] = example.lines_sent
    labels[2] = example.b2b_after
    labels[3] = example.position_normalized
    labels[4] = example.time_to_topout
    return labels


def flatten_example(example: CanonicalExample) -> np.ndarray:
    return np.concatenate([build_feature_vector(example), build_label_vector(example)])
