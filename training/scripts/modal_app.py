from __future__ import annotations

import os
import shutil
import sys
import tempfile
import time
import importlib.util
from itertools import product
from importlib import import_module
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING, Literal, Sequence, cast

import modal

TRAINING_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = TRAINING_ROOT.parent


def _add_sys_path(path: Path) -> None:
    resolved = str(path.resolve())
    if resolved not in sys.path:
        sys.path.insert(0, resolved)


def _candidate_bootstrap_roots() -> list[Path]:
    candidates: list[Path] = []
    seen: set[str] = set()
    for seed in [Path.cwd(), Path(__file__).resolve().parent, TRAINING_ROOT, REPO_ROOT]:
        for candidate in [seed, *seed.parents]:
            resolved = str(candidate.resolve())
            if resolved in seen:
                continue
            seen.add(resolved)
            candidates.append(candidate)
    for entry in sys.path:
        if not entry:
            continue
        candidate = Path(entry)
        resolved = str(candidate.resolve())
        if resolved in seen:
            continue
        seen.add(resolved)
        candidates.append(candidate)
    return candidates


def _ensure_modal_import_paths() -> None:
    for candidate in _candidate_bootstrap_roots():
        training_scripts = candidate / "training" / "scripts" / "gpu_profiles.py"
        if training_scripts.exists():
            _add_sys_path(candidate)
            _add_sys_path(candidate / "training")
            return
        bare_scripts = candidate / "scripts" / "gpu_profiles.py"
        if bare_scripts.exists():
            _add_sys_path(candidate)
            return


def _load_module_from_file(module_name: str, relative_parts: Sequence[str]) -> ModuleType:
    for candidate in _candidate_bootstrap_roots():
        module_path = candidate.joinpath(*relative_parts)
        if module_path.exists():
            spec = importlib.util.spec_from_file_location(
                f"_modal_bootstrap_{module_name}", module_path
            )
            if spec is None or spec.loader is None:
                continue
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            return module
    raise ModuleNotFoundError(
        f"unable to locate module file for {module_name}: {'/'.join(relative_parts)}"
    )


def _import_training_module(
    package_path: str, *, file_relative_parts: Sequence[str], bare_path: str | None = None
) -> ModuleType:
    _ensure_modal_import_paths()
    try:
        return import_module(package_path)
    except ImportError:
        if bare_path is not None:
            try:
                return import_module(bare_path)
            except ImportError:
                pass
    return _load_module_from_file(package_path.replace(".", "_"), file_relative_parts)


def _import_training_script_module(module_name: str) -> ModuleType:
    return _import_training_module(
        f"training.scripts.{module_name}",
        bare_path=f"scripts.{module_name}",
        file_relative_parts=("training", "scripts", f"{module_name}.py"),
    )


def _import_training_utils_module(module_name: str) -> ModuleType:
    return _import_training_module(
        f"training.utils.{module_name}",
        bare_path=f"utils.{module_name}",
        file_relative_parts=("training", "utils", f"{module_name}.py"),
    )


if TYPE_CHECKING:
    from .policy_value_pipeline import ArtifactReadiness, PolicyValueSupervisionMode

try:
    from . import gpu_profiles as _gpu_profiles
except ImportError:
    _gpu_profiles = _import_training_script_module("gpu_profiles")

default_profile_name = _gpu_profiles.DEFAULT_PROFILE_NAME
gpu_profiles = _gpu_profiles.GPU_PROFILES
get_profile = _gpu_profiles.get_profile


def _policy_value_pipeline_module() -> ModuleType:
    return _import_training_script_module("policy_value_pipeline")


def artifact_readiness(
    data_path: str | Path,
    supervision_mode: "PolicyValueSupervisionMode" = "search_control",
) -> "ArtifactReadiness":
    return cast(
        "ArtifactReadiness",
        _policy_value_pipeline_module().artifact_readiness(
            data_path, supervision_mode=supervision_mode
        ),
    )


def missing_policy_value_artifact_paths(
    data_path: str | Path,
    supervision_mode: "PolicyValueSupervisionMode" = "search_control",
) -> list[Path]:
    return cast(
        list[Path],
        _policy_value_pipeline_module().missing_policy_value_artifact_paths(
            data_path, supervision_mode=supervision_mode
        ),
    )


def policy_value_batch_size_for_profile(*args: object, **kwargs: object) -> int:
    return cast(
        int, _policy_value_pipeline_module().policy_value_batch_size_for_profile(*args, **kwargs)
    )


def policy_value_num_workers_for_profile(*args: object, **kwargs: object) -> int:
    return cast(
        int, _policy_value_pipeline_module().policy_value_num_workers_for_profile(*args, **kwargs)
    )


def policy_value_run_id(*args: object, **kwargs: object) -> str:
    return cast(str, _policy_value_pipeline_module().policy_value_run_id(*args, **kwargs))


def required_policy_value_artifact_paths(
    data_path: str | Path,
    supervision_mode: "PolicyValueSupervisionMode" = "search_control",
) -> list[Path]:
    return cast(
        list[Path],
        _policy_value_pipeline_module().required_policy_value_artifact_paths(
            data_path, supervision_mode=supervision_mode
        ),
    )


def validate_policy_value_artifacts(
    data_path: str | Path,
    supervision_mode: "PolicyValueSupervisionMode" = "search_control",
) -> None:
    _policy_value_pipeline_module().validate_policy_value_artifacts(
        data_path, supervision_mode=supervision_mode
    )


# ---------------------------------------------------------------------------
# Modal app & image
# ---------------------------------------------------------------------------
app = modal.App("fusion-training")

ACTIVE_PROFILE_NAME = os.environ.get("FUSION_GPU_PROFILE", default_profile_name)
PROFILE = get_profile(ACTIVE_PROFILE_NAME)
DATA_VOLUME_NAME = os.environ.get("FUSION_TRAINING_DATA_VOLUME", "fusion-training-data")
CHECKPOINT_VOLUME_NAME = os.environ.get(
    "FUSION_TRAINING_CHECKPOINT_VOLUME",
    "fusion-training-checkpoints",
)
COMPILE_CACHE_VOLUME_NAME = os.environ.get(
    "FUSION_TRAINING_COMPILE_CACHE_VOLUME",
    "fusion-compile-cache",
)
MODAL_LABEL_BINARY_RELATIVE_PATH = Path("policy_value_tools/generate_policy_value_labels")
MODAL_LABEL_BINARY_REMOTE_PATH = str(Path("/data") / MODAL_LABEL_BINARY_RELATIVE_PATH)
PIPELINE_TIMEOUT_S = max(
    14400,
    PROFILE.resources.teacher_timeout_s + PROFILE.resources.student_timeout_s + 1800,
)

training_image = (
    modal.Image.debian_slim(python_version="3.11")
    .pip_install(
        "torch>=2.2",
        "lightning==2.6.1",
        "optuna>=3.5",
        "optuna-integration[pytorch_lightning]",
        "numpy",
        "onnx",
        "onnxscript",
    )
    .env(
        {
            # --- Memory allocation ---
            "PYTORCH_CUDA_ALLOC_CONF": "expandable_segments:True",
            "CUBLAS_WORKSPACE_CONFIG": ":4096:8",
            # --- Prevent memory leaks over long training runs ---
            "TORCH_NCCL_AVOID_RECORD_STREAMS": "1",
            # --- Prevent hangs in serverless topologies (single-GPU, no NVSwitch) ---
            "NCCL_NVLS_ENABLE": "0",
            "NCCL_CUMEM_ENABLE": "0",
            "TORCH_CUDNN_V8_API_ENABLED": "1",
            # --- torch.compile kernel cache: shared across containers via Volume ---
            # First worker compiles; subsequent workers load cached .so/PTX binaries.
            "TORCHINDUCTOR_CACHE_DIR": "/compile-cache",
            "FUSION_POLICY_VALUE_LABEL_BINARY": MODAL_LABEL_BINARY_REMOTE_PATH,
        }
    )
    .add_local_python_source(
        "training",
        ignore=[
            "**/tests/**",
            "**/__pycache__",
            "*.pyc",
            "*.pyo",
            "*.md",
            "training_data.bin*",
            "*.policy_value.jsonl",
            "*.policy_value.requests.jsonl",
            "*.onnx",
            ".venv*",
        ],
    )
)

# ---------------------------------------------------------------------------
# Volumes
# ---------------------------------------------------------------------------
data_vol = modal.Volume.from_name(DATA_VOLUME_NAME, create_if_missing=True)
ckpt_vol = modal.Volume.from_name(CHECKPOINT_VOLUME_NAME, create_if_missing=True)
compile_cache_vol = modal.Volume.from_name(COMPILE_CACHE_VOLUME_NAME, create_if_missing=True)

DATA_DIR = "/data"
CKPT_DIR = "/checkpoints"
COMPILE_CACHE_DIR = "/compile-cache"


def checkpoint_relative_path(path: str | Path) -> str:
    checkpoint_path = Path(path).resolve()
    checkpoint_root = Path(CKPT_DIR).resolve()
    return str(checkpoint_path.relative_to(checkpoint_root))


# ---------------------------------------------------------------------------
# Upload data
# ---------------------------------------------------------------------------


@app.local_entrypoint()
def upload_data(local_path: str = "training_data.bin") -> None:
    """Upload binary training data to Modal volume.

    Usage: modal run training/scripts/modal_app.py::upload_data --local-path data.bin
    """
    local = Path(local_path)
    if not local.exists():
        raise FileNotFoundError(f"Local file not found: {local}")

    size_mb = local.stat().st_size / (1024 * 1024)
    print(f"Uploading {local} ({size_mb:.1f} MB) to volume...")

    with data_vol.batch_upload() as batch:
        batch.put_file(str(local), "training_data.bin")

    print(f"Upload complete: {DATA_DIR}/training_data.bin")


def _upload_policy_value_artifacts(local_data_path: str | Path) -> list[str]:
    artifact_paths = required_policy_value_artifact_paths(local_data_path)
    validate_policy_value_artifacts(local_data_path)

    uploaded_names: list[str] = []
    with data_vol.batch_upload() as batch:
        for artifact_path in artifact_paths:
            batch.put_file(str(artifact_path), artifact_path.name)
            uploaded_names.append(artifact_path.name)
    return uploaded_names


def _upload_policy_value_dataset_artifacts(local_data_path: str | Path) -> list[str]:
    data_path = Path(local_data_path)
    readiness = artifact_readiness(data_path)
    if not readiness.dataset_ready:
        reasons = "; ".join(readiness.reasons) or "dataset artifacts are invalid"
        raise RuntimeError(f"Policy/value dataset artifacts are invalid for upload: {reasons}")

    artifact_paths = required_policy_value_artifact_paths(data_path)[:4]
    uploaded_names: list[str] = []
    try:
        with data_vol.batch_upload() as batch:
            for artifact_path in artifact_paths:
                batch.put_file(str(artifact_path), artifact_path.name)
                uploaded_names.append(artifact_path.name)
    except FileExistsError:
        uploaded_names = [artifact_path.name for artifact_path in artifact_paths]
    return uploaded_names


def _upload_modal_label_binary(local_binary_path: str | Path) -> str:
    local_binary = Path(local_binary_path)
    try:
        with data_vol.batch_upload() as batch:
            batch.put_file(str(local_binary), str(MODAL_LABEL_BINARY_RELATIVE_PATH))
    except FileExistsError:
        pass
    return str(MODAL_LABEL_BINARY_RELATIVE_PATH)


def _policy_value_shard_dir(data_filename: str) -> Path:
    return Path("policy_value_shards") / data_filename


def _policy_value_shard_requests_relpath(data_filename: str, shard_index: int) -> Path:
    return (
        _policy_value_shard_dir(data_filename)
        / f"requests-{shard_index:04d}.policy_value.requests.jsonl"
    )


def _policy_value_shard_labels_relpath(data_filename: str, shard_index: int) -> Path:
    return _policy_value_shard_dir(data_filename) / f"labels-{shard_index:04d}.policy_value.jsonl"


def _player_context_run_dir(run_id: str) -> Path:
    return Path("player_context_artifacts") / run_id


def _player_context_shard_output_relpath(run_id: str, data_filename: str, shard_index: int) -> Path:
    return _player_context_run_dir(run_id) / "shards" / f"shard-{shard_index:04d}" / data_filename


def _player_context_staging_output_relpath(run_id: str, data_filename: str) -> Path:
    return _player_context_run_dir(run_id) / "staging" / data_filename


def _player_context_replay_relpath(run_id: str, relative_replay_path: str) -> Path:
    return _player_context_run_dir(run_id) / "replays" / relative_replay_path


def preprocess_directory(
    input_dir: str | Path,
    output_path: str | Path,
    max_files: int | None = None,
    num_workers: int | None = None,
) -> int:
    try:
        from .preprocess_replays import preprocess_directory as impl
    except ImportError:
        impl = _import_training_script_module("preprocess_replays").preprocess_directory
    return impl(input_dir, output_path, max_files=max_files, num_workers=num_workers)


def preprocess_replay_files(
    replay_files: Sequence[str | Path],
    output_path: str | Path,
    *,
    num_workers: int | None = None,
) -> int:
    try:
        from .preprocess_replays import preprocess_replay_files as impl
    except ImportError:
        impl = _import_training_script_module("preprocess_replays").preprocess_replay_files
    return impl(replay_files, output_path, num_workers=num_workers)


def split_replay_files(
    input_dir: str | Path,
    *,
    shard_count: int,
    max_files: int | None = None,
) -> list[list[Path]]:
    try:
        from .preprocess_replays import split_replay_files as impl
    except ImportError:
        impl = _import_training_script_module("preprocess_replays").split_replay_files
    return impl(input_dir, shard_count=shard_count, max_files=max_files)


def merge_preprocessed_shards(
    shard_paths: Sequence[str | Path],
    output_path: str | Path,
    *,
    expected_sample_count: int | None = None,
) -> int:
    try:
        from .preprocess_replays import merge_preprocessed_shards as impl
    except ImportError:
        impl = _import_training_script_module("preprocess_replays").merge_preprocessed_shards
    return impl(shard_paths, output_path, expected_sample_count=expected_sample_count)


def merge_label_shards(
    shard_paths: Sequence[str | Path],
    output_path: str | Path,
    *,
    expected_count: int | None = None,
) -> int:
    try:
        from .generate_policy_value_labels import merge_label_shards as impl
    except ImportError:
        impl = _import_training_script_module("generate_policy_value_labels").merge_label_shards
    return impl(shard_paths, output_path, expected_count=expected_count)


def split_requests_file(
    requests_path: str | Path,
    output_dir: str | Path,
    *,
    shard_count: int,
) -> list[Path]:
    try:
        from .generate_policy_value_labels import split_requests_file as impl
    except ImportError:
        impl = _import_training_script_module("generate_policy_value_labels").split_requests_file
    return impl(requests_path, output_dir, shard_count=shard_count)


def _count_nonblank_lines(path: Path) -> int:
    with path.open() as handle:
        return sum(1 for line in handle if line.strip())


def _validate_player_context_dataset_artifacts(data_path: Path) -> dict[str, int]:
    example_schema = _import_training_utils_module("example_schema")
    policy_schema = _import_training_utils_module("policy_value_schema")
    validate_player_context_identity_alignment = (
        _policy_value_pipeline_module().validate_player_context_identity_alignment
    )
    _group_ids_path = example_schema.group_ids_path
    load_dataset_metadata = example_schema.load_dataset_metadata
    load_player_context_metadata = policy_schema.load_player_context_metadata
    policy_value_player_context_path = policy_schema.policy_value_player_context_path
    policy_value_requests_path = policy_schema.policy_value_requests_path

    dataset_metadata = load_dataset_metadata(data_path)
    sample_count = int(dataset_metadata["sample_count"])
    context_metadata = load_player_context_metadata(data_path)
    context_sample_count = int(context_metadata["sample_count"])
    if sample_count <= 0 or context_sample_count != sample_count:
        raise RuntimeError(
            f"player-context artifact sample_count mismatch: dataset={sample_count} player_context={context_sample_count}"
        )

    groups_size = _group_ids_path(data_path).stat().st_size
    if groups_size != sample_count * 8:
        raise RuntimeError(
            f"group ids size mismatch: expected {sample_count * 8}, got {groups_size}"
        )
    request_count = _count_nonblank_lines(policy_value_requests_path(data_path))
    context_count = _count_nonblank_lines(policy_value_player_context_path(data_path))
    if request_count != sample_count:
        raise RuntimeError(f"request count mismatch: expected {sample_count}, got {request_count}")
    if context_count != sample_count:
        raise RuntimeError(
            f"player-context count mismatch: expected {sample_count}, got {context_count}"
        )
    validate_player_context_identity_alignment(data_path)
    return {
        "sample_count": sample_count,
        "request_count": request_count,
        "context_count": context_count,
    }


def generate_policy_value_labels_for_dataset(
    data_path: str | Path,
    *,
    output_path: str | Path | None = None,
    num_workers: int | None = None,
) -> Path:
    try:
        from .generate_policy_value_labels import generate_policy_value_labels_for_dataset as impl
    except ImportError:
        impl = import_module(
            "training.scripts.generate_policy_value_labels"
        ).generate_policy_value_labels_for_dataset
    return impl(data_path, output_path=output_path, num_workers=num_workers)


@app.local_entrypoint()
def prepare_policy_value_artifacts(
    local_data_path: str = "training/training_data.bin",
    replay_dir: str = "data/replays",
    max_files: int = 0,
    num_workers: int = 4,
    supervision_mode: Literal["search_control", "player_context_primary"] = "search_control",
) -> None:
    data_path = Path(local_data_path)
    readiness = artifact_readiness(data_path, supervision_mode=supervision_mode)
    if not readiness.dataset_ready:
        replay_path = Path(replay_dir)
        if not replay_path.exists():
            raise FileNotFoundError(f"Replay directory not found: {replay_path}")
        print(f"Preparing Phase 0/1 artifacts from {replay_path} -> {data_path}")
        preprocess_directory(
            replay_path,
            data_path,
            max_files=max_files if max_files > 0 else None,
            num_workers=num_workers,
        )
    readiness = artifact_readiness(data_path, supervision_mode=supervision_mode)
    if not readiness.dataset_ready:
        reasons = "; ".join(readiness.reasons) or "dataset artifacts are invalid"
        raise RuntimeError(
            f"Policy/value dataset artifacts are invalid after preprocessing: {reasons}"
        )
    if not readiness.labels_ready:
        print(f"Generating policy/value labels for {data_path}")
        generate_policy_value_labels_for_dataset(data_path)
    readiness = artifact_readiness(data_path, supervision_mode=supervision_mode)
    if not readiness.ready:
        if readiness.missing_paths:
            remaining_display = ", ".join(str(path) for path in readiness.missing_paths)
            raise RuntimeError(f"Policy/value artifact preparation incomplete: {remaining_display}")
        reasons = "; ".join(readiness.reasons) or "unknown policy/value artifact validation failure"
        raise RuntimeError(
            f"Policy/value artifact preparation produced invalid artifacts: {reasons}"
        )
    validate_policy_value_artifacts(data_path, supervision_mode=supervision_mode)
    print("Policy/value artifacts ready.")


@app.local_entrypoint()
def upload_policy_value_artifacts(
    local_data_path: str = "training/training_data.bin",
    supervision_mode: Literal["search_control", "player_context_primary"] = "search_control",
) -> None:
    data_path = Path(local_data_path)
    artifact_paths = required_policy_value_artifact_paths(
        data_path, supervision_mode=supervision_mode
    )
    validate_policy_value_artifacts(data_path, supervision_mode=supervision_mode)
    uploaded: list[str] = []
    with data_vol.batch_upload() as batch:
        for artifact_path in artifact_paths:
            batch.put_file(str(artifact_path), artifact_path.name)
            uploaded.append(artifact_path.name)
    print("Uploaded policy/value artifacts:")
    for name in uploaded:
        print(f"  - {DATA_DIR}/{name}")


@app.function(
    image=training_image,
    cpu=1,
    memory=8192,
    timeout=10800,
    retries=0,
    volumes={DATA_DIR: data_vol},
)
def generate_policy_value_label_shard_remote(
    requests_relpath: str,
    labels_relpath: str,
) -> dict[str, object]:
    requests_path = Path(DATA_DIR) / requests_relpath
    labels_path = Path(DATA_DIR) / labels_relpath
    labels_path.parent.mkdir(parents=True, exist_ok=True)
    generated_path = generate_policy_value_labels_for_dataset(
        requests_path,
        output_path=labels_path,
        num_workers=1,
    )
    sample_count = 0
    with generated_path.open() as handle:
        sample_count = sum(1 for line in handle if line.strip())
    data_vol.commit()
    return {
        "requests_relpath": requests_relpath,
        "labels_relpath": labels_relpath,
        "sample_count": sample_count,
    }


@app.function(
    image=training_image,
    cpu=2,
    memory=8192,
    timeout=10800,
    retries=0,
    volumes={DATA_DIR: data_vol},
)
def preprocess_player_context_shard_remote(
    replay_relpaths: list[str],
    shard_output_relpath: str,
    num_workers: int = 1,
) -> dict[str, object]:
    replay_paths = [Path(DATA_DIR) / relpath for relpath in replay_relpaths]
    output_path = Path(DATA_DIR) / shard_output_relpath
    output_path.parent.mkdir(parents=True, exist_ok=True)
    sample_count = preprocess_replay_files(replay_paths, output_path, num_workers=num_workers)
    if sample_count == 0:
        data_vol.commit()
        return {
            "shard_output_relpath": shard_output_relpath,
            "sample_count": 0,
            "request_count": 0,
            "context_count": 0,
        }

    counts = _validate_player_context_dataset_artifacts(output_path)
    data_vol.commit()
    return {
        "shard_output_relpath": shard_output_relpath,
        "sample_count": sample_count,
        **counts,
    }


@app.function(
    image=training_image,
    cpu=4,
    memory=8192,
    timeout=14400,
    retries=0,
    volumes={DATA_DIR: data_vol},
)
def launch_modal_player_context_artifact_pipeline_remote(
    data_filename: str,
    run_id: str,
    shard_replay_relpaths: list[list[str]],
    shard_count: int,
    shard_num_workers: int = 1,
) -> dict[str, object]:
    shard_calls = [
        preprocess_player_context_shard_remote.spawn(
            replay_relpaths=replay_relpaths,
            shard_output_relpath=str(
                _player_context_shard_output_relpath(run_id, data_filename, shard_index)
            ),
            num_workers=shard_num_workers,
        )
        for shard_index, replay_relpaths in enumerate(shard_replay_relpaths)
    ]
    shard_results = [call.get() for call in shard_calls]
    data_vol.reload()
    nonempty_shard_paths = [
        Path(DATA_DIR) / str(result["shard_output_relpath"])
        for result in shard_results
        if int(cast(int, result["sample_count"])) > 0
    ]
    if not nonempty_shard_paths:
        raise RuntimeError(
            "Modal player-context artifact pipeline produced zero samples across all shards"
        )

    merged_output = Path(DATA_DIR) / _player_context_staging_output_relpath(run_id, data_filename)
    merged_count = merge_preprocessed_shards(
        nonempty_shard_paths,
        merged_output,
        expected_sample_count=sum(
            int(cast(int, result["sample_count"])) for result in shard_results
        ),
    )
    counts = _validate_player_context_dataset_artifacts(merged_output)

    canonical_output = Path(DATA_DIR) / data_filename
    shutil.copy2(merged_output, canonical_output)
    shutil.copy2(
        merged_output.with_name(f"{merged_output.name}.metadata.json"),
        canonical_output.with_name(f"{canonical_output.name}.metadata.json"),
    )
    shutil.copy2(
        merged_output.with_name(f"{merged_output.name}.groups.u64"),
        canonical_output.with_name(f"{canonical_output.name}.groups.u64"),
    )
    shutil.copy2(
        merged_output.with_name(f"{merged_output.name}.policy_value.requests.jsonl"),
        canonical_output.with_name(f"{canonical_output.name}.policy_value.requests.jsonl"),
    )
    shutil.copy2(
        merged_output.with_name(f"{merged_output.name}.policy_value.player_context.jsonl"),
        canonical_output.with_name(f"{canonical_output.name}.policy_value.player_context.jsonl"),
    )
    shutil.copy2(
        merged_output.with_name(f"{merged_output.name}.policy_value.player_context.metadata.json"),
        canonical_output.with_name(
            f"{canonical_output.name}.policy_value.player_context.metadata.json"
        ),
    )
    data_vol.commit()
    return {
        "status": "success",
        "data_volume_name": DATA_VOLUME_NAME,
        "data_filename": data_filename,
        "run_id": run_id,
        "shard_count": shard_count,
        "merged_count": merged_count,
        "shards": shard_results,
        **counts,
    }


@app.function(
    image=training_image,
    cpu=2,
    memory=8192,
    timeout=14400,
    retries=0,
    volumes={DATA_DIR: data_vol},
)
def run_modal_policy_value_label_pipeline(
    data_filename: str = "training_data.bin",
    shard_count: int = 20,
) -> dict[str, object]:
    data_path = Path(DATA_DIR) / data_filename
    readiness = artifact_readiness(data_path)
    if not readiness.dataset_ready or readiness.sample_count is None:
        reasons = "; ".join(readiness.reasons) or "dataset artifacts are invalid"
        raise RuntimeError(f"Policy/value dataset artifacts are invalid on Modal volume: {reasons}")

    shard_calls = [
        generate_policy_value_label_shard_remote.spawn(
            requests_relpath=str(_policy_value_shard_requests_relpath(data_filename, shard_index)),
            labels_relpath=str(_policy_value_shard_labels_relpath(data_filename, shard_index)),
        )
        for shard_index in range(shard_count)
    ]
    shard_results = [call.get() for call in shard_calls]
    merged_count = _resume_modal_policy_value_label_merge(
        data_path, shard_count, readiness.sample_count
    )

    return {
        "status": "success",
        "data_volume_name": DATA_VOLUME_NAME,
        "data_filename": data_filename,
        "shard_count": shard_count,
        "merged_count": merged_count,
        "shards": shard_results,
    }


def _resume_modal_policy_value_label_merge(
    data_path: Path,
    shard_count: int,
    expected_count: int,
) -> int:
    data_vol.reload()
    data_filename = data_path.name

    labels_path = required_policy_value_artifact_paths(data_path)[4]
    merged_count = merge_label_shards(
        [
            Path(DATA_DIR) / _policy_value_shard_labels_relpath(data_path.name, shard_index)
            for shard_index in range(shard_count)
        ],
        labels_path,
        expected_count=expected_count,
    )
    data_vol.commit()

    final_readiness = artifact_readiness(data_path)
    if not final_readiness.ready:
        reasons = (
            "; ".join(final_readiness.reasons) or "unknown policy/value artifact validation failure"
        )
        raise RuntimeError(f"Modal shard labeling produced invalid artifacts: {reasons}")

    return merged_count


@app.function(
    image=training_image,
    cpu=2,
    memory=8192,
    timeout=14400,
    retries=0,
    volumes={DATA_DIR: data_vol},
)
def repair_modal_policy_value_label_shards_remote(
    data_filename: str = "training_data.bin",
    shard_indices: list[int] | None = None,
    shard_count: int = 70,
) -> dict[str, object]:
    data_path = Path(DATA_DIR) / data_filename
    readiness = artifact_readiness(data_path)
    if not readiness.dataset_ready or readiness.sample_count is None:
        reasons = "; ".join(readiness.reasons) or "dataset artifacts are invalid"
        raise RuntimeError(f"Policy/value dataset artifacts are invalid on Modal volume: {reasons}")

    repaired_indices = sorted(set(shard_indices or []))
    if not repaired_indices:
        raise ValueError("at least one shard index is required for selective repair")

    shard_calls = [
        generate_policy_value_label_shard_remote.spawn(
            requests_relpath=str(_policy_value_shard_requests_relpath(data_filename, shard_index)),
            labels_relpath=str(_policy_value_shard_labels_relpath(data_filename, shard_index)),
        )
        for shard_index in repaired_indices
    ]
    shard_results = [call.get() for call in shard_calls]
    merged_count = _resume_modal_policy_value_label_merge(
        data_path, shard_count, readiness.sample_count
    )

    return {
        "status": "success",
        "data_volume_name": DATA_VOLUME_NAME,
        "data_filename": data_filename,
        "repaired_shards": repaired_indices,
        "merged_count": merged_count,
        "shards": shard_results,
    }


@app.function(
    image=training_image,
    cpu=2,
    memory=8192,
    timeout=14400,
    retries=0,
    volumes={DATA_DIR: data_vol},
)
def resume_modal_policy_value_label_merge_remote(
    data_filename: str = "training_data.bin",
    shard_count: int = 20,
) -> dict[str, object]:
    data_path = Path(DATA_DIR) / data_filename
    readiness = artifact_readiness(data_path)
    if not readiness.dataset_ready or readiness.sample_count is None:
        reasons = "; ".join(readiness.reasons) or "dataset artifacts are invalid"
        raise RuntimeError(f"Policy/value dataset artifacts are invalid on Modal volume: {reasons}")

    merged_count = _resume_modal_policy_value_label_merge(
        data_path, shard_count, readiness.sample_count
    )

    return {
        "status": "success",
        "data_volume_name": DATA_VOLUME_NAME,
        "data_filename": data_path.name,
        "shard_count": shard_count,
        "merged_count": merged_count,
    }


@app.local_entrypoint()
def launch_modal_policy_value_label_pipeline(
    local_data_path: str = "training/training_data.bin",
    shard_count: int = 20,
) -> None:
    try:
        from .generate_policy_value_labels import ensure_modal_compatible_release_binary
    except ImportError:
        ensure_modal_compatible_release_binary = _import_training_script_module(
            "generate_policy_value_labels"
        ).ensure_modal_compatible_release_binary

    data_path = Path(local_data_path)
    readiness = artifact_readiness(data_path)
    if not readiness.dataset_ready:
        reasons = "; ".join(readiness.reasons) or "dataset artifacts are invalid"
        raise RuntimeError(f"Policy/value dataset artifacts are invalid locally: {reasons}")

    modal_binary_path = ensure_modal_compatible_release_binary()
    requests_path = data_path.with_name(f"{data_path.name}.policy_value.requests.jsonl")
    result: dict[str, object]
    with tempfile.TemporaryDirectory(prefix="policy-value-modal-shards-") as tmpdir:
        shard_paths = split_requests_file(requests_path, Path(tmpdir), shard_count=shard_count)
        uploaded = _upload_policy_value_dataset_artifacts(data_path)
        uploaded_binary = _upload_modal_label_binary(modal_binary_path)
        try:
            with data_vol.batch_upload() as batch:
                for shard_index, shard_path in enumerate(shard_paths):
                    batch.put_file(
                        str(shard_path),
                        str(_policy_value_shard_requests_relpath(data_path.name, shard_index)),
                    )
        except FileExistsError:
            pass
        result = run_modal_policy_value_label_pipeline.remote(
            data_filename=data_path.name,
            shard_count=len(shard_paths),
        )

    print("Completed Modal policy/value shard labeling pipeline")
    print(f"  data_volume={DATA_VOLUME_NAME}")
    print(f"  data_filename={data_path.name}")
    print(f"  shard_count={shard_count}")
    print(f"  uploaded_dataset_artifacts={len(uploaded)}")
    print(f"  uploaded_modal_binary={uploaded_binary}")
    print(f"  merged_count={result['merged_count']}")


@app.local_entrypoint()
def launch_modal_policy_value_label_merge_resume(
    local_data_path: str = "training/training_data.bin",
    shard_count: int = 70,
) -> None:
    data_path = Path(local_data_path)
    result = resume_modal_policy_value_label_merge_remote.remote(
        data_filename=data_path.name,
        shard_count=shard_count,
    )
    print("Completed Modal policy/value shard merge resume")
    print(f"  data_volume={DATA_VOLUME_NAME}")
    print(f"  data_filename={data_path.name}")
    print(f"  shard_count={shard_count}")
    print(f"  merged_count={result['merged_count']}")


@app.local_entrypoint()
def launch_modal_policy_value_label_shard_repair(
    local_data_path: str = "training/training_data.bin",
    shard_indices: str = "",
    shard_count: int = 70,
) -> None:
    data_path = Path(local_data_path)
    repaired_indices = sorted(
        {int(item.strip()) for item in shard_indices.split(",") if item.strip()}
    )
    if not repaired_indices:
        raise ValueError("shard_indices must contain at least one shard index")

    result = repair_modal_policy_value_label_shards_remote.remote(
        data_filename=data_path.name,
        shard_indices=repaired_indices,
        shard_count=shard_count,
    )
    print("Completed Modal selective policy/value shard repair")
    print(f"  data_volume={DATA_VOLUME_NAME}")
    print(f"  data_filename={data_path.name}")
    print(f"  repaired_shards={result['repaired_shards']}")
    print(f"  merged_count={result['merged_count']}")


@app.local_entrypoint()
def launch_modal_player_context_artifact_pipeline(
    replay_dir: str = "data/replays",
    local_output_path: str = "training/training_data.bin",
    supervision_mode: Literal[
        "search_control", "player_context_primary"
    ] = "player_context_primary",
    shard_count: int = 20,
    shard_num_workers: int = 1,
    max_files: int = 0,
) -> None:
    if supervision_mode != "player_context_primary":
        raise ValueError(
            "Modal player-context artifact generation requires supervision_mode='player_context_primary'"
        )

    replay_path = Path(replay_dir)
    if not replay_path.exists():
        raise FileNotFoundError(f"Replay directory not found: {replay_path}")

    requested_shard_count = min(max(shard_count, 1), 70)
    shards = split_replay_files(
        replay_path,
        shard_count=requested_shard_count,
        max_files=max_files if max_files > 0 else None,
    )
    if not shards:
        raise RuntimeError(f"No replay files found under {replay_path}")

    output_path = Path(local_output_path)
    run_id = policy_value_run_id("player-context-artifacts")
    shard_replay_relpaths: list[list[str]] = []
    with data_vol.batch_upload() as batch:
        for shard in shards:
            shard_relpaths: list[str] = []
            for replay_file in shard:
                relative_replay_path = replay_file.relative_to(replay_path)
                relpath = _player_context_replay_relpath(run_id, str(relative_replay_path))
                batch.put_file(str(replay_file), str(relpath))
                shard_relpaths.append(str(relpath))
            shard_replay_relpaths.append(shard_relpaths)

    result = launch_modal_player_context_artifact_pipeline_remote.remote(
        data_filename=output_path.name,
        run_id=run_id,
        shard_replay_relpaths=shard_replay_relpaths,
        shard_count=len(shards),
        shard_num_workers=shard_num_workers,
    )
    print("Completed Modal player-context artifact pipeline")
    print(f"  data_volume={DATA_VOLUME_NAME}")
    print(f"  data_filename={output_path.name}")
    print(f"  run_id={run_id}")
    print(f"  shard_count={result['shard_count']}")
    print(f"  sample_count={result['sample_count']}")


def _stage_policy_value_dataset(
    data_filename: str,
    supervision_mode: Literal["search_control", "player_context_primary"] = "search_control",
) -> Path:
    source = Path(DATA_DIR) / data_filename
    destination = Path("/tmp") / data_filename
    shutil.copy2(source, destination)
    for artifact_path in required_policy_value_artifact_paths(
        source, supervision_mode=supervision_mode
    )[1:]:
        shutil.copy2(artifact_path, Path("/tmp") / artifact_path.name)
    return destination


@app.function(
    image=training_image,
    gpu=PROFILE.resources.gpu,
    cpu=PROFILE.resources.cpu,
    memory=PROFILE.resources.memory_mib,
    timeout=PROFILE.resources.student_timeout_s,
    retries=PROFILE.resources.retries,
    scaledown_window=PROFILE.resources.scaledown_window_s,
    startup_timeout=PROFILE.resources.startup_timeout_s,
    volumes={DATA_DIR: data_vol, CKPT_DIR: ckpt_vol, COMPILE_CACHE_DIR: compile_cache_vol},
)
def train_policy_value_remote(
    data_filename: str = "training_data.bin",
    run_id: str | None = None,
    supervision_mode: Literal["search_control", "player_context_primary"] = "search_control",
    batch_size: int | None = None,
    num_workers: int | None = None,
    max_epochs: int = 50,
    lr: float = 3e-4,
    weight_decay: float = 1e-5,
    dropout: float = 0.0,
    search_value_weight: float | None = None,
    search_policy_weight: float | None = None,
    player_policy_weight: float | None = None,
) -> str:
    try:
        from .train_policy_value import train_policy_value
    except ImportError:
        train_policy_value = import_module("training.scripts.train_policy_value").train_policy_value

    import torch

    resolved_run_id: str = run_id or policy_value_run_id(f"policy-value-{PROFILE.name}")
    local_data_path = _stage_policy_value_dataset(data_filename, supervision_mode=supervision_mode)
    output_dir = Path(CKPT_DIR) / "policy_value" / resolved_run_id
    output_dir.mkdir(parents=True, exist_ok=True)
    resolved_batch_size = batch_size or policy_value_batch_size_for_profile(PROFILE.name)
    resolved_num_workers = num_workers or policy_value_num_workers_for_profile(PROFILE.name)
    readiness = artifact_readiness(local_data_path, supervision_mode=supervision_mode)
    sample_count = readiness.sample_count or 0

    if torch.cuda.is_available():
        torch.cuda.reset_peak_memory_stats()
        torch.cuda.synchronize()
    started_at = time.perf_counter()
    best_checkpoint = train_policy_value(
        data_path=str(local_data_path),
        output_dir=str(output_dir),
        supervision_mode=supervision_mode,
        batch_size=resolved_batch_size,
        num_workers=resolved_num_workers,
        max_epochs=max_epochs,
        lr=lr,
        weight_decay=weight_decay,
        accelerator="gpu",
        precision=PROFILE.settings.precision,
        dropout=dropout,
        search_value_weight=search_value_weight,
        search_policy_weight=search_policy_weight,
        player_policy_weight=player_policy_weight,
    )
    if torch.cuda.is_available():
        torch.cuda.synchronize()
        peak_allocated_gib = torch.cuda.max_memory_allocated() / (1024**3)
        peak_reserved_gib = torch.cuda.max_memory_reserved() / (1024**3)
        device_name = torch.cuda.get_device_name(0)
    else:
        peak_allocated_gib = 0.0
        peak_reserved_gib = 0.0
        device_name = "cpu"
    elapsed_s = time.perf_counter() - started_at
    print(
        "BENCH_TELEMETRY "
        f"profile={PROFILE.name} "
        f"device={device_name!r} "
        f"sample_count={sample_count} "
        f"batch_size={resolved_batch_size} "
        f"num_workers={resolved_num_workers} "
        f"max_epochs={max_epochs} "
        f"elapsed_s={elapsed_s:.3f} "
        f"peak_allocated_gib={peak_allocated_gib:.3f} "
        f"peak_reserved_gib={peak_reserved_gib:.3f}"
    )
    ckpt_vol.commit()
    compile_cache_vol.commit()
    return checkpoint_relative_path(best_checkpoint)


@app.function(
    image=training_image,
    cpu=4,
    memory=8192,
    timeout=3600,
    volumes={CKPT_DIR: ckpt_vol},
)
def export_policy_value_onnx_remote(
    checkpoint_relpath: str,
    run_id: str | None = None,
) -> dict[str, str]:
    try:
        from .export_policy_value_onnx import export_policy_value_onnx
    except ImportError:
        export_policy_value_onnx = import_module(
            "training.scripts.export_policy_value_onnx"
        ).export_policy_value_onnx

    resolved_run_id: str = run_id or policy_value_run_id("policy-value-export")
    checkpoint_path = Path(CKPT_DIR) / checkpoint_relpath
    export_dir = Path(CKPT_DIR) / "policy_value_export" / resolved_run_id
    export_dir.mkdir(parents=True, exist_ok=True)
    onnx_path, metadata_path = export_policy_value_onnx(
        checkpoint_path,
        output_path=export_dir / f"{checkpoint_path.name}.policy_value.onnx",
        metadata_path=export_dir / f"{checkpoint_path.name}.policy_value.onnx.metadata.json",
    )
    ckpt_vol.commit()
    return {
        "onnx_relpath": checkpoint_relative_path(onnx_path),
        "metadata_relpath": checkpoint_relative_path(metadata_path),
    }


@app.function(timeout=max(14400, PROFILE.resources.student_timeout_s + 3600))
def run_policy_value_pipeline(
    data_filename: str = "training_data.bin",
    run_id: str | None = None,
    supervision_mode: Literal["search_control", "player_context_primary"] = "search_control",
    batch_size: int | None = None,
    num_workers: int | None = None,
    max_epochs: int = 50,
    lr: float = 3e-4,
    weight_decay: float = 1e-5,
) -> dict[str, object]:
    run_id = run_id or policy_value_run_id(f"policy-value-{PROFILE.name}")
    checkpoint_relpath = train_policy_value_remote.remote(
        data_filename=data_filename,
        run_id=run_id,
        supervision_mode=supervision_mode,
        batch_size=batch_size,
        num_workers=num_workers,
        max_epochs=max_epochs,
        lr=lr,
        weight_decay=weight_decay,
    )
    export_paths = export_policy_value_onnx_remote.remote(
        checkpoint_relpath=checkpoint_relpath,
        run_id=run_id,
    )
    return {
        "status": "success",
        "profile": PROFILE.name,
        "data_filename": data_filename,
        "checkpoint_relpath": checkpoint_relpath,
        **export_paths,
    }


def _parse_float_csv(raw: str) -> list[float]:
    values = [item.strip() for item in raw.split(",") if item.strip()]
    if not values:
        raise ValueError("expected at least one float value")
    return [float(value) for value in values]


def _parse_int_csv(raw: str) -> list[int]:
    values = [item.strip() for item in raw.split(",") if item.strip()]
    if not values:
        raise ValueError("expected at least one integer value")
    return [int(value) for value in values]


def _build_policy_value_run_specs(
    *,
    learning_rates: Sequence[float],
    weight_decays: Sequence[float],
    batch_sizes: Sequence[int],
    num_worker_values: Sequence[int],
    max_epochs: int,
    run_prefix: str,
) -> list[dict[str, object]]:
    return [
        {
            "run_id": f"{run_prefix}-r{index:02d}",
            "lr": float(lr),
            "weight_decay": float(weight_decay),
            "batch_size": int(batch_size),
            "num_workers": int(num_workers),
            "max_epochs": int(max_epochs),
        }
        for index, (lr, weight_decay, batch_size, num_workers) in enumerate(
            product(learning_rates, weight_decays, batch_sizes, num_worker_values)
        )
    ]


def _write_policy_value_sweep_smoke_artifacts(data_path: str | Path) -> Path:
    import numpy as np

    data_path = Path(data_path)
    data_path.parent.mkdir(parents=True, exist_ok=True)
    example_schema = _import_training_utils_module("example_schema")
    policy_schema = _import_training_utils_module("policy_value_schema")

    raw = np.zeros((2, 859), dtype=np.float32)
    raw.tofile(data_path)
    np.asarray([1, 2], dtype=np.uint64).tofile(example_schema.group_ids_path(data_path))
    example_schema.write_dataset_metadata(data_path, sample_count=2)

    policy_schema.policy_value_requests_path(data_path).write_text('{"request":1}\n{"request":2}\n')
    policy_schema.policy_value_labels_path(data_path).write_text('{"label":1}\n{"label":2}\n')
    policy_schema.write_policy_value_metadata(
        data_path,
        sample_count=2,
        generation_mode=policy_schema.GENERATION_MODE_SEARCH_ORACLE,
        policy_temperature=1.0,
    )
    policy_schema.policy_value_player_context_path(data_path).write_text(
        '{"context":1}\n{"context":2}\n'
    )
    policy_schema.write_player_context_metadata(
        data_path, sample_count=2, recent_horizon=7, future_horizon=14
    )
    return data_path


@app.function(timeout=300)
def run_policy_value_sweep_remote(
    data_filename: str,
    supervision_mode: Literal["search_control", "player_context_primary"] = "search_control",
    run_specs: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    launched_runs: list[dict[str, object]] = []
    for spec in run_specs or []:
        call = train_policy_value_remote.spawn(
            data_filename=data_filename,
            run_id=cast(str, spec["run_id"]),
            supervision_mode=supervision_mode,
            batch_size=cast(int, spec["batch_size"]),
            num_workers=cast(int, spec["num_workers"]),
            max_epochs=cast(int, spec["max_epochs"]),
            lr=cast(float, spec["lr"]),
            weight_decay=cast(float, spec["weight_decay"]),
        )
        launched_runs.append({**spec, "call_id": call.object_id})

    return {
        "status": "launched",
        "profile": PROFILE.name,
        "data_filename": data_filename,
        "run_count": len(launched_runs),
        "runs": launched_runs,
    }


@app.local_entrypoint()
def launch_policy_value_pipeline(
    local_data_path: str = "training/training_data.bin",
    replay_dir: str = "data/replays",
    prepare_artifacts: bool = True,
    supervision_mode: Literal["search_control", "player_context_primary"] = "search_control",
    max_files: int = 0,
    prep_num_workers: int = 4,
    batch_size: int | None = None,
    num_workers: int | None = None,
    max_epochs: int = 50,
    lr: float = 3e-4,
    weight_decay: float = 1e-5,
) -> None:
    data_path = Path(local_data_path)
    if prepare_artifacts:
        prepare_policy_value_artifacts(
            local_data_path=str(data_path),
            replay_dir=replay_dir,
            max_files=max_files,
            num_workers=prep_num_workers,
            supervision_mode=supervision_mode,
        )
    artifact_paths = required_policy_value_artifact_paths(
        data_path, supervision_mode=supervision_mode
    )
    validate_policy_value_artifacts(data_path, supervision_mode=supervision_mode)
    uploaded: list[str] = []
    with data_vol.batch_upload() as batch:
        for artifact_path in artifact_paths:
            batch.put_file(str(artifact_path), artifact_path.name)
            uploaded.append(artifact_path.name)
    print(f"Uploaded {len(uploaded)} policy/value artifacts for {data_path.name}")
    call = run_policy_value_pipeline.spawn(
        data_filename=data_path.name,
        supervision_mode=supervision_mode,
        batch_size=batch_size,
        num_workers=num_workers,
        max_epochs=max_epochs,
        lr=lr,
        weight_decay=weight_decay,
    )
    print("Launched policy/value pipeline on Modal")
    print(f"  profile={PROFILE.name}")
    print(f"  gpu={PROFILE.resources.gpu}")
    print(f"  batch_size={batch_size or policy_value_batch_size_for_profile(PROFILE.name)}")
    print(f"  num_workers={num_workers or policy_value_num_workers_for_profile(PROFILE.name)}")
    print(f"  call_id={call.object_id}")


@app.local_entrypoint()
def launch_policy_value_sweep(
    local_data_path: str = "training/training_data.bin",
    replay_dir: str = "data/replays",
    prepare_artifacts: bool = False,
    supervision_mode: Literal[
        "search_control", "player_context_primary"
    ] = "player_context_primary",
    max_files: int = 0,
    prep_num_workers: int = 4,
    learning_rates: str = "3e-4,1e-4",
    weight_decays: str = "1e-5",
    batch_sizes: str = "",
    num_worker_values: str = "",
    max_epochs: int = 50,
) -> None:
    data_path = Path(local_data_path)
    if prepare_artifacts:
        prepare_policy_value_artifacts(
            local_data_path=str(data_path),
            replay_dir=replay_dir,
            max_files=max_files,
            num_workers=prep_num_workers,
            supervision_mode=supervision_mode,
        )

    artifact_paths = required_policy_value_artifact_paths(
        data_path, supervision_mode=supervision_mode
    )
    validate_policy_value_artifacts(data_path, supervision_mode=supervision_mode)
    with data_vol.batch_upload() as batch:
        for artifact_path in artifact_paths:
            batch.put_file(str(artifact_path), artifact_path.name)

    resolved_batch_sizes = (
        _parse_int_csv(batch_sizes)
        if batch_sizes.strip()
        else [policy_value_batch_size_for_profile(PROFILE.name)]
    )
    resolved_num_workers = (
        _parse_int_csv(num_worker_values)
        if num_worker_values.strip()
        else [policy_value_num_workers_for_profile(PROFILE.name)]
    )
    run_specs = _build_policy_value_run_specs(
        learning_rates=_parse_float_csv(learning_rates),
        weight_decays=_parse_float_csv(weight_decays),
        batch_sizes=resolved_batch_sizes,
        num_worker_values=resolved_num_workers,
        max_epochs=max_epochs,
        run_prefix=policy_value_run_id(f"policy-value-{PROFILE.name}"),
    )

    result = run_policy_value_sweep_remote.remote(
        data_filename=data_path.name,
        supervision_mode=supervision_mode,
        run_specs=run_specs,
    )
    print("Launched policy/value sweep on Modal")
    print(f"  profile={PROFILE.name}")
    print(f"  data_filename={data_path.name}")
    print(f"  run_count={result['run_count']}")
    for run in cast(list[dict[str, object]], result["runs"]):
        print(
            f"  run_id={run['run_id']} call_id={run['call_id']} "
            f"lr={run['lr']} weight_decay={run['weight_decay']} "
            f"batch_size={run['batch_size']} num_workers={run['num_workers']}"
        )


@app.local_entrypoint()
def launch_policy_value_sweep_smoke(
    local_data_path: str = "",
    supervision_mode: Literal[
        "search_control", "player_context_primary"
    ] = "player_context_primary",
) -> None:
    resolved_local_data_path = (
        local_data_path or f"/tmp/{policy_value_run_id('pv-sweep-smoke')}.bin"
    )
    smoke_path = _write_policy_value_sweep_smoke_artifacts(resolved_local_data_path)
    launch_policy_value_sweep(
        local_data_path=str(smoke_path),
        prepare_artifacts=False,
        supervision_mode=supervision_mode,
        learning_rates="3e-4,1e-4",
        weight_decays="1e-5",
        batch_sizes="1",
        num_worker_values="0",
        max_epochs=1,
    )


# ---------------------------------------------------------------------------
# Teacher training (Optuna HPO)
# ---------------------------------------------------------------------------


@app.function(
    image=training_image,
    gpu=PROFILE.resources.gpu,
    cpu=PROFILE.resources.cpu,
    memory=PROFILE.resources.memory_mib,
    timeout=PROFILE.resources.teacher_timeout_s,
    retries=PROFILE.resources.retries,
    scaledown_window=PROFILE.resources.scaledown_window_s,
    startup_timeout=PROFILE.resources.startup_timeout_s,
    volumes={DATA_DIR: data_vol, CKPT_DIR: ckpt_vol, COMPILE_CACHE_DIR: compile_cache_vol},
)
def train_teacher_trial(
    trial_number: int,
    n_trials: int = PROFILE.settings.trials_per_worker,
    max_epochs: int = PROFILE.settings.teacher_epochs,
) -> dict[str, object]:
    """Run Optuna trials for teacher hyperparameter search.

    Each worker runs n_trials sequential trials. Uses in-memory storage
    (each worker has its own independent study).

    Returns dict with best trial info from this worker.
    """
    import shutil
    import time

    import optuna

    try:
        from .optuna_objective import teacher_objective
    except ImportError:
        teacher_objective = import_module("training.scripts.optuna_objective").teacher_objective

    # Copy training data from Volume FUSE (~200 MB/s) to local NVMe (~5 GB/s).
    # This is a one-time cost per container that pays back on every epoch.
    # If file already exists with matching size, skip recopy.
    fuse_path = f"{DATA_DIR}/training_data.bin"
    local_path = "/tmp/training_data.bin"
    import os

    t0 = time.perf_counter()
    src_size = os.path.getsize(fuse_path)
    if os.path.exists(local_path) and os.path.getsize(local_path) == src_size:
        print(f"[worker {trial_number}] Reusing cached /tmp/training_data.bin")
    else:
        shutil.copy2(fuse_path, local_path)
    dt = time.perf_counter() - t0

    size_gb = os.path.getsize(local_path) / (1024**3)
    if dt > 0:
        print(
            f"[worker {trial_number}] Ready {size_gb:.1f} GB at /tmp in {dt:.1f}s ({size_gb / dt:.1f} GB/s)"
        )
    else:
        print(f"[worker {trial_number}] Ready {size_gb:.1f} GB at /tmp")

    # Each worker runs its own independent study with in-memory storage.
    # No shared DB needed — workers explore independently and we pick
    # the overall best result from all workers at the end.
    study = optuna.create_study(
        study_name=f"fusion-teacher-worker-{trial_number}",
        direction="minimize",
        pruner=optuna.pruners.HyperbandPruner(
            min_resource=5,
            max_resource=max_epochs,
            reduction_factor=3,
        ),
    )

    data_path = local_path

    study.optimize(
        lambda trial: teacher_objective(
            trial,
            data_path=data_path,
            checkpoint_dir=f"{CKPT_DIR}/worker_{trial_number}",
            max_epochs=max_epochs,
            accelerator="gpu",
            batch_size_choices=PROFILE.settings.teacher_batch_sizes,
            num_workers=PROFILE.settings.teacher_num_workers,
            prefetch_factor=PROFILE.settings.prefetch_factor,
            precision=PROFILE.settings.precision,
        ),
        n_trials=n_trials,
    )

    # Commit checkpoints and torch.compile kernel cache
    ckpt_vol.commit()
    compile_cache_vol.commit()

    best = study.best_trial
    return {
        "worker": trial_number,
        "best_value": best.value,
        "best_params": best.params,
        "best_trial_number": best.number,
        "total_trials": len(study.trials),
    }


# ---------------------------------------------------------------------------
# Fan-out: parallel workers
# ---------------------------------------------------------------------------


@app.local_entrypoint()
def fan_out(
    num_workers: int = PROFILE.settings.default_parallel_workers,
    trials_per_worker: int = PROFILE.settings.trials_per_worker,
    max_epochs: int = PROFILE.settings.teacher_epochs,
) -> None:
    """Launch parallel Optuna workers for teacher training.

    Usage: modal run training/scripts/modal_app.py::fan_out --num-workers 5

    Distributes trials across workers. Max 5 concurrent GPUs.
    """
    num_workers = min(num_workers, PROFILE.settings.max_parallel_workers)
    total_trials = trials_per_worker * num_workers

    print(f"Launching {num_workers} workers × {trials_per_worker} trials = {total_trials} total")
    print(f"Max epochs per trial: {max_epochs}")

    # Spawn parallel workers
    handles = []
    for i in range(num_workers):
        handle = train_teacher_trial.spawn(
            trial_number=i,
            n_trials=trials_per_worker,
            max_epochs=max_epochs,
        )
        handles.append(handle)

    # Collect results with per-worker fault tolerance
    results = []
    failures = []
    for worker_idx, handle in enumerate(handles):
        try:
            results.append(handle.get())
        except Exception as exc:
            failures.append({"worker": worker_idx, "error": str(exc)})
            print(f"[fan_out] worker {worker_idx} failed: {exc}")

    if not results:
        print("\nNo successful worker results.")
        if failures:
            print(f"Failures: {failures}")
        return

    # Find overall best
    best = min(results, key=lambda r: r["best_value"])
    print(f"\n{'=' * 60}")
    print(f"Best trial: #{best['best_trial_number']}")
    print(f"Best val loss: {best['best_value']:.6f}")
    print(f"Best params: {best['best_params']}")
    print(f"Total trials completed: {best['total_trials']}")
    if failures:
        print(f"Workers failed but tolerated: {len(failures)}")
    print(f"{'=' * 60}")


# ---------------------------------------------------------------------------
# Checkpoint discovery (runs on Modal to access volume)
# ---------------------------------------------------------------------------


@app.function(
    image=training_image,
    volumes={CKPT_DIR: ckpt_vol},
)
def find_best_checkpoint(worker_number: int, trial_number: int) -> str | None:
    """Find best checkpoint file for a given worker/trial on the Modal volume.

    Returns:
        Relative path (e.g. "worker_0/trial_2/best-epoch=50-val_total_loss=0.1234.ckpt")
        or None if not found.
    """
    trial_dir = Path(f"{CKPT_DIR}/worker_{worker_number}/trial_{trial_number}")
    ckpt_vol.reload()
    ckpt_files = sorted(trial_dir.rglob("best-*.ckpt"))
    if not ckpt_files:
        return None
    return str(ckpt_files[0]).removeprefix(f"{CKPT_DIR}/")


# ---------------------------------------------------------------------------
# Student distillation
# ---------------------------------------------------------------------------


@app.function(
    image=training_image,
    gpu=PROFILE.resources.gpu,
    cpu=PROFILE.resources.cpu,
    memory=PROFILE.resources.memory_mib,
    timeout=PROFILE.resources.student_timeout_s,
    retries=PROFILE.resources.retries,
    scaledown_window=PROFILE.resources.scaledown_window_s,
    startup_timeout=PROFILE.resources.startup_timeout_s,
    volumes={DATA_DIR: data_vol, CKPT_DIR: ckpt_vol, COMPILE_CACHE_DIR: compile_cache_vol},
)
def distill_student_remote(
    teacher_checkpoint: str,
    max_epochs: int = PROFILE.settings.student_epochs,
    batch_size: int = PROFILE.settings.student_batch_size,
    lr: float = 5e-4,
) -> str:
    """Run student distillation on Modal GPU.

    Args:
        teacher_checkpoint: Path relative to CKPT_DIR (e.g. "trial_42/best-epoch=50-val/total_loss=0.1234.ckpt")

    Returns:
        Path to best student checkpoint (relative to CKPT_DIR).
    """
    import shutil
    import time

    try:
        from .distill_student import distill_student
    except ImportError:
        distill_student = import_module("training.scripts.distill_student").distill_student

    # Reload checkpoint volume to see teacher checkpoints from HPO workers
    ckpt_vol.reload()

    # Copy training data from Volume FUSE to local NVMe.
    # If file already exists with matching size, skip recopy.
    fuse_path = f"{DATA_DIR}/training_data.bin"
    local_path = "/tmp/training_data.bin"
    import os

    t0 = time.perf_counter()
    src_size = os.path.getsize(fuse_path)
    if os.path.exists(local_path) and os.path.getsize(local_path) == src_size:
        print("[distill] Reusing cached /tmp/training_data.bin")
    else:
        shutil.copy2(fuse_path, local_path)
    dt = time.perf_counter() - t0

    size_gb = os.path.getsize(local_path) / (1024**3)
    if dt > 0:
        print(f"[distill] Ready {size_gb:.1f} GB at /tmp in {dt:.1f}s ({size_gb / dt:.1f} GB/s)")
    else:
        print(f"[distill] Ready {size_gb:.1f} GB at /tmp")

    teacher_path = f"{CKPT_DIR}/{teacher_checkpoint}"
    data_path = local_path
    output_dir = f"{CKPT_DIR}/student"

    best_path = distill_student(
        teacher_checkpoint=teacher_path,
        data_path=data_path,
        output_dir=output_dir,
        max_epochs=max_epochs,
        batch_size=batch_size,
        lr=lr,
        accelerator="gpu",
        num_workers=PROFILE.settings.student_num_workers,
        prefetch_factor=PROFILE.settings.prefetch_factor,
        precision=PROFILE.settings.precision,
    )

    ckpt_vol.commit()
    compile_cache_vol.commit()

    # Return path relative to CKPT_DIR so callers can reconstruct.
    # Lightning's best_model_path runs os.path.realpath() which resolves
    # the /checkpoints symlink to /__modal/volumes/vo-<id>/..., so simple
    # string prefix matching fails.  Use os.path to normalize robustly.
    import os

    real_ckpt_dir = os.path.realpath(CKPT_DIR)
    real_best = os.path.realpath(str(best_path))
    return os.path.relpath(real_best, real_ckpt_dir)


# ---------------------------------------------------------------------------
# Weight export
# ---------------------------------------------------------------------------


@app.function(
    image=training_image,
    volumes={CKPT_DIR: ckpt_vol},
)
def export_weights_remote(student_checkpoint: str) -> None:
    """Export student weights to flat f32 binary on Modal volume.

    The output .bin file can be downloaded from the checkpoint volume.
    """
    try:
        from .export_weights import export_student_weights
    except ImportError:
        export_student_weights = import_module(
            "training.scripts.export_weights"
        ).export_student_weights

    ckpt_path = f"{CKPT_DIR}/{student_checkpoint}"
    output_path = f"{CKPT_DIR}/student_weights.bin"

    n = export_student_weights(ckpt_path, output_path)
    print(f"Exported {n} floats ({n * 4} bytes) to {output_path}")

    ckpt_vol.commit()


# ---------------------------------------------------------------------------
# Full pipeline
# ---------------------------------------------------------------------------


@app.function(
    image=training_image,
    timeout=PIPELINE_TIMEOUT_S,
    volumes={DATA_DIR: data_vol, CKPT_DIR: ckpt_vol, COMPILE_CACHE_DIR: compile_cache_vol},
)
def run_pipeline(
    num_workers: int = PROFILE.settings.default_parallel_workers,
    trials_per_worker: int = PROFILE.settings.trials_per_worker,
    teacher_epochs: int = PROFILE.settings.teacher_epochs,
    student_epochs: int = PROFILE.settings.student_epochs,
) -> dict[str, object]:
    """Run the complete training pipeline on Modal: teacher HPO → distill → export.

    Runs entirely server-side — survives client disconnection.
    Returns dict with pipeline results.
    """
    num_workers = min(num_workers, PROFILE.settings.max_parallel_workers)

    # Phase 1: Teacher HPO
    print("=" * 60)
    print("Phase 1: Teacher Hyperparameter Search")
    print(
        f"  {num_workers} workers × {trials_per_worker} trials = {num_workers * trials_per_worker} total"
    )
    print("=" * 60)

    handles = []
    for i in range(num_workers):
        handle = train_teacher_trial.spawn(
            trial_number=i,
            n_trials=trials_per_worker,
            max_epochs=teacher_epochs,
        )
        handles.append(handle)

    results = []
    failures = []
    for worker_idx, handle in enumerate(handles):
        try:
            results.append(handle.get())
        except Exception as exc:
            failures.append({"worker": worker_idx, "error": str(exc)})
            print(f"[run_pipeline] worker {worker_idx} failed: {exc}")

    if not results:
        return {
            "status": "failed",
            "error": "No successful teacher workers",
            "worker_failures": failures,
        }

    best = min(results, key=lambda r: r["best_value"])

    print(f"Best teacher: worker={best['worker']} trial=#{best['best_trial_number']}")
    print(f"Best val loss: {best['best_value']:.6f}")
    print(f"Best params: {best['best_params']}")

    best_worker_num = best["worker"]
    best_trial_num = best["best_trial_number"]

    # Phase 2: Student Distillation
    print("\n" + "=" * 60)
    print("Phase 2: Student Distillation")
    print("=" * 60)

    teacher_ckpt_rel = find_best_checkpoint.remote(
        worker_number=best_worker_num, trial_number=best_trial_num
    )
    if teacher_ckpt_rel is None:
        msg = f"No checkpoint found for worker_{best_worker_num}/trial_{best_trial_num}/"
        print(f"WARNING: {msg}")
        return {"status": "failed", "error": msg, "teacher_results": results}

    print(f"Teacher checkpoint: {teacher_ckpt_rel}")

    student_best = distill_student_remote.remote(
        teacher_checkpoint=teacher_ckpt_rel,
        max_epochs=student_epochs,
    )

    print(f"Best student checkpoint: {student_best}")

    # Phase 3: Export weights
    print("\n" + "=" * 60)
    print("Phase 3: Weight Export")
    print("=" * 60)

    # distill_student_remote now returns relative path, but guard against absolutes
    student_ckpt_rel = student_best
    if student_ckpt_rel.startswith(CKPT_DIR):
        student_ckpt_rel = student_ckpt_rel.removeprefix(CKPT_DIR).lstrip("/")
    export_weights_remote.remote(student_checkpoint=student_ckpt_rel)

    print("\nPipeline complete!")
    print("Download: modal volume get fusion-training-checkpoints student_weights.bin")

    return {
        "status": "success",
        "teacher_results": results,
        "worker_failures": failures,
        "best_teacher": best,
        "teacher_checkpoint": teacher_ckpt_rel,
        "student_checkpoint": student_best,
    }


@app.local_entrypoint()
def launch_pipeline(
    num_workers: int = PROFILE.settings.default_parallel_workers,
    trials_per_worker: int = PROFILE.settings.trials_per_worker,
    teacher_epochs: int = PROFILE.settings.teacher_epochs,
    student_epochs: int = PROFILE.settings.student_epochs,
) -> None:
    """Thin launcher — schedules run_pipeline asynchronously and exits.

    Usage: modal run training/scripts/modal_app.py::launch_pipeline
    """
    print("Launching pipeline on Modal (async submit)...")
    print(
        "For disconnect-safe launch, use: modal run --detach training/scripts/modal_app.py::run_pipeline"
    )
    print(
        f"Profile={PROFILE.name} gpu={PROFILE.resources.gpu} cpu={PROFILE.resources.cpu} memory={PROFILE.resources.memory_mib}MiB"
    )
    print(f"Available profiles: {', '.join(sorted(gpu_profiles))}")
    call = run_pipeline.spawn(
        num_workers=num_workers,
        trials_per_worker=trials_per_worker,
        teacher_epochs=teacher_epochs,
        student_epochs=student_epochs,
    )
    print(f"\nSubmitted run_pipeline call_id={call.object_id}")


@app.local_entrypoint()
def show_profiles() -> None:
    print(f"Active profile: {PROFILE.name}")
    for name in sorted(gpu_profiles):
        profile = gpu_profiles[name]
        settings = profile.settings
        resources = profile.resources
        print(
            f"{name}: gpu={resources.gpu} cpu={resources.cpu} memory={resources.memory_mib}MiB "
            f"workers={settings.default_parallel_workers}/{settings.max_parallel_workers} trials_per_worker={settings.trials_per_worker} "
            f"teacher_batch_sizes={list(settings.teacher_batch_sizes)} student_batch_size={settings.student_batch_size} "
            f"precision={settings.precision}"
        )
        print(
            f"      policy_value_batch_size={policy_value_batch_size_for_profile(name)} "
            f"policy_value_num_workers={policy_value_num_workers_for_profile(name)}"
        )
