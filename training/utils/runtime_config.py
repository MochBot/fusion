from __future__ import annotations

import os
from dataclasses import dataclass


def env_str(name: str, default: str) -> str:
    return os.environ.get(name, default)


def env_int(name: str, default: int) -> int:
    value = os.environ.get(name)
    if value is None:
        return default
    return int(value)


def env_float(name: str, default: float) -> float:
    value = os.environ.get(name)
    if value is None:
        return default
    return float(value)


def env_bool(name: str, default: bool) -> bool:
    value = os.environ.get(name)
    if value is None:
        return default
    normalized = value.strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    raise ValueError(f"Invalid boolean for {name}: {value}")


@dataclass(frozen=True)
class ProbeRuntimeConfig:
    app_name: str
    python_version: str
    remote_pythonpath: str
    remote_data_path: str
    local_data_path: str
    data_mount_path: str
    data_volume_name: str
    gpu: str
    cpu: int
    memory_mib: int
    timeout_s: int
    sample_count: int
    batch_size: int
    stage_to_tmp: bool
    fetch_mode: str
    warmup_steps: int
    measure_steps: int
    num_workers: int
    prefetch_factor: int
    lr: float
    weight_decay: float
    use_bf16: bool
    pin_memory: bool
    telemetry_enabled: bool
    telemetry_interval_s: float
    telemetry_gpu_index: int
    telemetry_record_per_step: bool
    sweep_batch_sizes: str
    sweep_num_workers: str


@dataclass(frozen=True)
class TrainingRuntimeConfig:
    app_name: str
    python_version: str
    data_volume_name: str
    checkpoint_volume_name: str
    data_mount_path: str
    checkpoint_mount_path: str
    gpu: str
    timeout_s: int
    default_num_workers: int
    optuna_secret_name: str


def load_probe_runtime_config() -> ProbeRuntimeConfig:
    remote_pythonpath = env_str("FUSION_PROBE_REMOTE_PYTHONPATH", "/root/fusion-training")
    data_mount_path = env_str("FUSION_PROBE_DATA_MOUNT_PATH", "/data")
    local_data_path = env_str("FUSION_PROBE_LOCAL_DATA_PATH", "/tmp/training_data.bin")
    remote_data_path = env_str(
        "FUSION_PROBE_REMOTE_DATA_PATH",
        f"{data_mount_path}/training_data.bin",
    )
    return ProbeRuntimeConfig(
        app_name=env_str("FUSION_PROBE_APP_NAME", "fusion-gpu-saturation-probe"),
        python_version=env_str("FUSION_PROBE_PYTHON_VERSION", "3.11"),
        remote_pythonpath=remote_pythonpath,
        remote_data_path=remote_data_path,
        local_data_path=local_data_path,
        data_mount_path=data_mount_path,
        data_volume_name=env_str("FUSION_PROBE_DATA_VOLUME", "fusion-training-data"),
        gpu=env_str("FUSION_PROBE_GPU", "B200"),
        cpu=env_int("FUSION_PROBE_CPU", 8),
        memory_mib=env_int("FUSION_PROBE_MEMORY_MIB", 40_960),
        timeout_s=env_int("FUSION_PROBE_TIMEOUT_S", 3_600),
        sample_count=env_int("FUSION_PROBE_SAMPLE_COUNT", 262_144),
        batch_size=env_int("FUSION_PROBE_BATCH_SIZE", 16_384),
        stage_to_tmp=env_bool("FUSION_PROBE_STAGE_TO_TMP", True),
        fetch_mode=env_str("FUSION_PROBE_FETCH_MODE", "sample_collate"),
        warmup_steps=env_int("FUSION_PROBE_WARMUP_STEPS", 8),
        measure_steps=env_int("FUSION_PROBE_MEASURE_STEPS", 32),
        num_workers=env_int("FUSION_PROBE_NUM_WORKERS", 4),
        prefetch_factor=env_int("FUSION_PROBE_PREFETCH_FACTOR", 4),
        lr=env_float("FUSION_PROBE_LR", 1e-3),
        weight_decay=env_float("FUSION_PROBE_WEIGHT_DECAY", 1e-4),
        use_bf16=env_bool("FUSION_PROBE_USE_BF16", True),
        pin_memory=env_bool("FUSION_PROBE_PIN_MEMORY", True),
        telemetry_enabled=env_bool("FUSION_PROBE_TELEMETRY_ENABLED", True),
        telemetry_interval_s=env_float("FUSION_PROBE_TELEMETRY_INTERVAL_S", 0.5),
        telemetry_gpu_index=env_int("FUSION_PROBE_TELEMETRY_GPU_INDEX", 0),
        telemetry_record_per_step=env_bool("FUSION_PROBE_TELEMETRY_RECORD_PER_STEP", True),
        sweep_batch_sizes=env_str("FUSION_PROBE_SWEEP_BATCH_SIZES", "8192,16384,32768"),
        sweep_num_workers=env_str("FUSION_PROBE_SWEEP_NUM_WORKERS", "2,4,8"),
    )


def load_training_runtime_config() -> TrainingRuntimeConfig:
    return TrainingRuntimeConfig(
        app_name=env_str("FUSION_TRAINING_APP_NAME", "fusion-training"),
        python_version=env_str("FUSION_TRAINING_PYTHON_VERSION", "3.11"),
        data_volume_name=env_str("FUSION_TRAINING_DATA_VOLUME", "fusion-training-data"),
        checkpoint_volume_name=env_str(
            "FUSION_TRAINING_CHECKPOINT_VOLUME",
            "fusion-training-checkpoints",
        ),
        data_mount_path=env_str("FUSION_TRAINING_DATA_MOUNT_PATH", "/data"),
        checkpoint_mount_path=env_str(
            "FUSION_TRAINING_CHECKPOINT_MOUNT_PATH",
            "/checkpoints",
        ),
        gpu=env_str("FUSION_TRAINING_GPU", "B200"),
        timeout_s=env_int("FUSION_TRAINING_TIMEOUT_S", 7_200),
        default_num_workers=env_int("FUSION_TRAINING_DEFAULT_NUM_WORKERS", 10),
        optuna_secret_name=env_str("FUSION_TRAINING_OPTUNA_SECRET", "optuna-neon-db"),
    )
