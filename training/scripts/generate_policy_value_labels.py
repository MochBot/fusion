from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import cast

try:
    from ..utils.policy_value_schema import (
        PHASE1_SCHEMA_VERSION,
        build_policy_value_metadata,
        load_policy_value_metadata,
        policy_value_labels_path,
        policy_value_requests_path,
        PolicyValueOracleRequest,
        PolicyValueTarget,
        deserialize_policy_value_oracle_request,
        deserialize_policy_value_target,
        serialize_policy_value_oracle_request,
        serialize_policy_value_target,
        softmax_root_scores,
        validate_policy_value_target,
    )
except ImportError:
    TRAINING_ROOT = Path(__file__).resolve().parents[1]
    if str(TRAINING_ROOT) not in sys.path:
        sys.path.insert(0, str(TRAINING_ROOT))
    from utils.policy_value_schema import (
        PHASE1_SCHEMA_VERSION,
        build_policy_value_metadata,
        load_policy_value_metadata,
        policy_value_labels_path,
        policy_value_requests_path,
        PolicyValueOracleRequest,
        PolicyValueTarget,
        deserialize_policy_value_oracle_request,
        deserialize_policy_value_target,
        serialize_policy_value_oracle_request,
        serialize_policy_value_target,
        softmax_root_scores,
        validate_policy_value_target,
    )


def build_policy_value_target(
    *,
    replay_id: str,
    round_id: int,
    player_id: int,
    frame_id: int,
    group_id: str,
    root_scores: Iterable[tuple[int, float]],
    best_value: float,
    position_complexity: float,
    temperature: float,
) -> PolicyValueTarget:
    root_scores_list = [(int(move_raw), float(score)) for move_raw, score in root_scores]
    if not root_scores_list:
        raise ValueError("root_scores must not be empty")
    best_move_raw, _ = max(root_scores_list, key=lambda entry: entry[1])
    policy_probs = softmax_root_scores([score for _, score in root_scores_list], temperature=temperature)
    target = PolicyValueTarget(
        schema_version=PHASE1_SCHEMA_VERSION,
        replay_id=replay_id,
        round_id=round_id,
        player_id=player_id,
        frame_id=frame_id,
        group_id=group_id,
        best_move_raw=best_move_raw,
        best_value=float(best_value),
        position_complexity=float(position_complexity),
        root_scores=root_scores_list,
        policy_probs=policy_probs,
    )
    validate_policy_value_target(target)
    return target


def default_requests_input_path(data_path: str | Path) -> Path:
    data_path = Path(data_path)
    if data_path.name.endswith(".policy_value.requests.jsonl"):
        return data_path
    return policy_value_requests_path(data_path)


def default_labels_output_path(data_path: str | Path) -> Path:
    data_path = Path(data_path)
    if data_path.name.endswith(".policy_value.requests.jsonl"):
        stem = str(data_path)
        return Path(stem.removesuffix(".policy_value.requests.jsonl") + ".policy_value.jsonl")
    return policy_value_labels_path(data_path)


RELEASE_BINARY_RELATIVE_PATH = Path("target/release/generate_policy_value_labels")
MODAL_RELEASE_BINARY_TARGET_DIR = Path("target/modal-release")
MODAL_RELEASE_BINARY_RELATIVE_PATH = MODAL_RELEASE_BINARY_TARGET_DIR / "release/generate_policy_value_labels"
MODAL_RELEASE_TARGET_CPU = "x86-64"
RELEASE_BINARY_ENV_VAR = "FUSION_POLICY_VALUE_LABEL_BINARY"
DEFAULT_LABEL_GENERATION_WORKERS = 8
CHUNK_MIN_REQUESTS = 1000
CHUNKED_ORACLE_PROFILE = "stronger_offline_oracle"
CHUNKED_ORACLE_BEAM_WIDTH = 2000
CHUNKED_ORACLE_DEPTH = 18
CHUNKED_ORACLE_USE_TT = True


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _resolve_cargo_bin() -> str:
    cargo_bin = shutil.which("cargo")
    if cargo_bin is not None:
        return cargo_bin

    cargo_fallback = Path.home() / ".cargo/bin/cargo"
    if cargo_fallback.exists():
        return str(cargo_fallback)

    raise FileNotFoundError(f"cargo not found on PATH and fallback '{cargo_fallback}' does not exist")


def _cargo_env(cargo_bin: str) -> dict[str, str]:
    env = os.environ.copy()
    cargo_bin_dir = str(Path(cargo_bin).parent)
    env["PATH"] = f"{cargo_bin_dir}:{env.get('PATH', '')}" if env.get("PATH") else cargo_bin_dir
    return env


def shard_requests_path(output_dir: str | Path, shard_index: int) -> Path:
    return Path(output_dir) / f"requests-{shard_index:04d}.policy_value.requests.jsonl"


def shard_labels_path(output_dir: str | Path, shard_index: int) -> Path:
    return Path(output_dir) / f"labels-{shard_index:04d}.policy_value.jsonl"


def _release_binary_override() -> Path | None:
    override = os.environ.get(RELEASE_BINARY_ENV_VAR)
    if not override:
        return None

    binary_path = Path(override)
    if not binary_path.exists():
        raise FileNotFoundError(
            f"{RELEASE_BINARY_ENV_VAR} points to missing label generator binary: {binary_path}"
        )
    return binary_path


def _ensure_release_binary() -> Path:
    override = _release_binary_override()
    if override is not None:
        return override

    repo_root = _repo_root()
    release_binary = repo_root / RELEASE_BINARY_RELATIVE_PATH
    if release_binary.exists():
        return release_binary

    cargo_bin = _resolve_cargo_bin()
    _ = subprocess.run(
        [cargo_bin, "build", "--release", "--bin", "generate_policy_value_labels"],
        check=True,
        cwd=repo_root,
        env=_cargo_env(cargo_bin),
    )
    if not release_binary.exists():
        raise FileNotFoundError(f"release label generator missing after build: {release_binary}")
    return release_binary


def ensure_release_binary() -> Path:
    return _ensure_release_binary()


def ensure_modal_compatible_release_binary() -> Path:
    repo_root = _repo_root()
    modal_binary = repo_root / MODAL_RELEASE_BINARY_RELATIVE_PATH
    if modal_binary.exists():
        return modal_binary

    cargo_bin = _resolve_cargo_bin()
    env = _cargo_env(cargo_bin)
    env["CARGO_BUILD_RUSTFLAGS"] = f"-C target-cpu={MODAL_RELEASE_TARGET_CPU}"
    _ = subprocess.run(
        [
            cargo_bin,
            "build",
            "--release",
            "--bin",
            "generate_policy_value_labels",
            "--target-dir",
            str(repo_root / MODAL_RELEASE_BINARY_TARGET_DIR),
        ],
        cwd=repo_root,
        env=env,
        check=True,
    )
    if not modal_binary.exists():
        raise FileNotFoundError(f"Modal-compatible label generator missing after build: {modal_binary}")
    return modal_binary


def _count_nonblank_lines(path: Path) -> int:
    with path.open() as handle:
        return sum(1 for line in handle if line.strip())


def split_requests_file(
    requests_path: str | Path,
    output_dir: str | Path,
    *,
    shard_count: int,
) -> list[Path]:
    requests_path = Path(requests_path)
    output_dir = Path(output_dir)
    request_count = _count_nonblank_lines(requests_path)
    if request_count <= 0:
        raise ValueError(f"Policy/value request sidecar is empty: {requests_path}")
    if shard_count <= 0:
        raise ValueError(f"shard_count must be positive, got {shard_count}")

    resolved_shard_count = min(shard_count, request_count)
    target_counts = [request_count // resolved_shard_count] * resolved_shard_count
    for index in range(request_count % resolved_shard_count):
        target_counts[index] += 1

    output_dir.mkdir(parents=True, exist_ok=True)
    shard_paths = [shard_requests_path(output_dir, index) for index in range(resolved_shard_count)]

    shard_index = 0
    lines_in_shard = 0
    shard_handle = shard_paths[shard_index].open("w")
    try:
        with requests_path.open() as source:
            for line in source:
                if not line.strip():
                    continue
                if lines_in_shard >= target_counts[shard_index]:
                    shard_handle.close()
                    shard_index += 1
                    lines_in_shard = 0
                    shard_handle = shard_paths[shard_index].open("w")
                _ = shard_handle.write(line)
                lines_in_shard += 1
    finally:
        shard_handle.close()

    return shard_paths


def _generate_chunk(binary_path: Path, requests_path: Path, labels_path: Path) -> tuple[str, str]:
    proc = subprocess.run(
        [str(binary_path), str(requests_path), str(labels_path)],
        check=True,
        cwd=_repo_root(),
        capture_output=True,
        text=True,
    )
    return proc.stdout, proc.stderr


def _metadata_output_path_from_labels_path(labels_path: Path) -> Path:
    labels_str = str(labels_path)
    if labels_str.endswith(".policy_value.jsonl"):
        return Path(labels_str.removesuffix(".policy_value.jsonl") + ".policy_value.metadata.json")
    return Path(f"{labels_str}.policy_value.metadata.json")


def _write_final_metadata(labels_path: Path, sample_count: int) -> None:
    metadata_path = _metadata_output_path_from_labels_path(labels_path)
    metadata = build_policy_value_metadata(
        sample_count=sample_count,
        generation_mode="search_oracle",
        policy_temperature=1.0,
        oracle_profile=CHUNKED_ORACLE_PROFILE,
        oracle_beam_width=CHUNKED_ORACLE_BEAM_WIDTH,
        oracle_depth=CHUNKED_ORACLE_DEPTH,
        oracle_use_tt=CHUNKED_ORACLE_USE_TT,
    )
    _ = metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")


def merge_label_shards(
    shard_label_paths: Iterable[str | Path],
    labels_path: str | Path,
    *,
    expected_count: int | None = None,
) -> int:
    labels_path = Path(labels_path)
    labels_path.parent.mkdir(parents=True, exist_ok=True)
    merged_count = 0
    with labels_path.open("w") as merged:
        for shard_label_path in shard_label_paths:
            shard_path = Path(shard_label_path)
            with shard_path.open() as handle:
                for line in handle:
                    if line.strip():
                        _ = merged.write(line)
                        merged_count += 1

    if expected_count is not None and merged_count != expected_count:
        raise ValueError(
            f"Merged policy/value shard count mismatch: expected {expected_count}, got {merged_count}"
        )

    _write_final_metadata(labels_path, merged_count)
    return merged_count


def _generate_labels_single_process(binary_path: Path, requests_path: Path, labels_path: Path) -> None:
    proc = subprocess.run(
        [str(binary_path), str(requests_path), str(labels_path)],
        check=True,
        cwd=_repo_root(),
        capture_output=True,
        text=True,
    )
    if proc.stderr.strip():
        print(proc.stderr.strip(), file=sys.stderr)


def _generate_labels_chunked(
    binary_path: Path,
    requests_path: Path,
    labels_path: Path,
    *,
    request_count: int,
    worker_count: int,
) -> None:
    with tempfile.TemporaryDirectory(prefix="policy-value-labels-") as tmpdir:
        tmpdir_path = Path(tmpdir)
        chunk_requests = split_requests_file(requests_path, tmpdir_path, shard_count=worker_count)
        chunk_labels = [shard_labels_path(tmpdir_path, chunk_index) for chunk_index in range(len(chunk_requests))]

        max_workers = min(worker_count, len(chunk_requests))
        with ThreadPoolExecutor(max_workers=max_workers) as executor:
            futures = [
                executor.submit(_generate_chunk, binary_path, chunk_request_path, chunk_label_path)
                for chunk_request_path, chunk_label_path in zip(chunk_requests, chunk_labels, strict=True)
            ]
            for chunk_idx, future in enumerate(futures):
                try:
                    _ = future.result()
                except subprocess.CalledProcessError as exc:
                    stderr_text = str(exc)
                    stderr_tail = stderr_text[-2000:] if stderr_text else ""
                    raise RuntimeError(
                        f"policy/value label generation failed for chunk {chunk_idx}: {stderr_tail}"
                    ) from exc

        _ = merge_label_shards(chunk_labels, labels_path, expected_count=request_count)


def generate_policy_value_labels_for_dataset(
    data_path: str | Path,
    *,
    output_path: str | Path | None = None,
    num_workers: int | None = None,
) -> Path:
    requests_path = default_requests_input_path(data_path)
    labels_path = Path(output_path) if output_path is not None else default_labels_output_path(data_path)
    request_count = _count_nonblank_lines(requests_path)
    if request_count <= 0:
        raise ValueError(f"Policy/value request sidecar is empty: {requests_path}")

    binary_path = _ensure_release_binary()
    resolved_workers = max(1, num_workers or min(DEFAULT_LABEL_GENERATION_WORKERS, os.cpu_count() or 1))
    _ = labels_path.parent.mkdir(parents=True, exist_ok=True)
    if request_count >= CHUNK_MIN_REQUESTS and resolved_workers > 1:
        _generate_labels_chunked(
            binary_path,
            requests_path,
            labels_path,
            request_count=request_count,
            worker_count=resolved_workers,
        )
    else:
        _generate_labels_single_process(binary_path, requests_path, labels_path)

    if output_path is None:
        metadata = load_policy_value_metadata(data_path)
    else:
        metadata = cast(dict[str, object], json.loads(_metadata_output_path_from_labels_path(labels_path).read_text()))
    sample_count = metadata.get("sample_count")
    if sample_count != request_count:
        raise ValueError(
            f"Generated policy/value metadata sample_count mismatch: requests={request_count} labels={sample_count}"
        )
    return labels_path


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate Phase 1 policy/value labels from request sidecars")
    _ = parser.add_argument("data_path")
    _ = parser.add_argument("output_path", nargs="?")
    args = parser.parse_args()
    data_path = cast(str, args.data_path)
    output_path = cast(str | None, args.output_path)
    print(generate_policy_value_labels_for_dataset(data_path, output_path=output_path))


__all__ = [
    "PolicyValueOracleRequest",
    "PolicyValueTarget",
    "RELEASE_BINARY_ENV_VAR",
    "build_policy_value_target",
    "default_requests_input_path",
    "default_labels_output_path",
    "ensure_release_binary",
    "ensure_modal_compatible_release_binary",
    "generate_policy_value_labels_for_dataset",
    "merge_label_shards",
    "shard_labels_path",
    "shard_requests_path",
    "split_requests_file",
    "serialize_policy_value_oracle_request",
    "deserialize_policy_value_oracle_request",
    "serialize_policy_value_target",
    "deserialize_policy_value_target",
]


if __name__ == "__main__":
    main()
