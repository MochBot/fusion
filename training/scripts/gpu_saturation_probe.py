from __future__ import annotations

from collections.abc import Iterator, Sequence
import json
import shutil
from dataclasses import asdict, dataclass
from pathlib import Path
import sys
from threading import Lock
from time import perf_counter
from typing import Callable, Protocol, cast

import modal
import numpy as np
import torch

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from utils.runtime_config import load_probe_runtime_config


RUNTIME = load_probe_runtime_config()

REMOTE_DATA_PATH = Path(RUNTIME.remote_data_path)
LOCAL_DATA_PATH = Path(RUNTIME.local_data_path)
REMOTE_PYTHONPATH = RUNTIME.remote_pythonpath
REMOTE_MODELS_PATH = f"{REMOTE_PYTHONPATH}/models"
REMOTE_UTILS_PATH = f"{REMOTE_PYTHONPATH}/utils"


app = modal.App(RUNTIME.app_name)

image = (
    modal.Image.debian_slim(python_version=RUNTIME.python_version)
    .pip_install("numpy", "torch")
    .env({"PYTHONPATH": REMOTE_PYTHONPATH})
    .add_local_dir(TRAINING_ROOT / "models", remote_path=REMOTE_MODELS_PATH)
    .add_local_dir(TRAINING_ROOT / "utils", remote_path=REMOTE_UTILS_PATH)
)

data_volume = modal.Volume.from_name(RUNTIME.data_volume_name, create_if_missing=True)
STAGING_LOCK = Lock()


@dataclass(frozen=True)
class ProbeConfig:
    sample_count: int = RUNTIME.sample_count
    batch_size: int = RUNTIME.batch_size
    stage_to_tmp: bool = RUNTIME.stage_to_tmp
    fetch_mode: str = RUNTIME.fetch_mode
    warmup_steps: int = RUNTIME.warmup_steps
    measure_steps: int = RUNTIME.measure_steps
    num_workers: int = RUNTIME.num_workers
    prefetch_factor: int = RUNTIME.prefetch_factor
    lr: float = RUNTIME.lr
    weight_decay: float = RUNTIME.weight_decay
    use_bf16: bool = RUNTIME.use_bf16
    pin_memory: bool = RUNTIME.pin_memory
    telemetry_enabled: bool = RUNTIME.telemetry_enabled
    telemetry_interval_s: float = RUNTIME.telemetry_interval_s
    telemetry_gpu_index: int = RUNTIME.telemetry_gpu_index
    telemetry_record_per_step: bool = RUNTIME.telemetry_record_per_step


class ProbeFuture(Protocol):
    def get(self) -> dict[str, object]: ...


class ProbeRunHandle(Protocol):
    def remote(self, config: ProbeConfig) -> dict[str, object]: ...

    def spawn(self, config: ProbeConfig) -> ProbeFuture: ...


class ProbeHandle(Protocol):
    run: ProbeRunHandle


def infer_num_samples(file_path: Path, bytes_per_sample: int) -> int:
    file_size = file_path.stat().st_size
    if file_size % bytes_per_sample != 0:
        raise ValueError(
            f"{file_path} size {file_size} is not divisible by {bytes_per_sample}"
        )
    return file_size // bytes_per_sample


def build_strided_indices(total_samples: int, sample_count: int) -> list[int]:
    if total_samples <= 0:
        raise ValueError("total_samples must be positive")
    if sample_count <= 0:
        raise ValueError("sample_count must be positive")
    if sample_count >= total_samples:
        return list(range(total_samples))
    return [index * total_samples // sample_count for index in range(sample_count)]


def parse_int_csv(value: str) -> list[int]:
    items = [chunk.strip() for chunk in value.split(",")]
    parsed = [int(item) for item in items if item]
    if not parsed:
        raise ValueError("expected at least one integer")
    return parsed


def build_batch_index_groups(sample_indices: Sequence[int], batch_size: int) -> list[list[int]]:
    if batch_size <= 0:
        raise ValueError("batch_size must be positive")
    groups: list[list[int]] = []
    for start in range(0, len(sample_indices), batch_size):
        group = list(sample_indices[start : start + batch_size])
        if len(group) == batch_size:
            groups.append(group)
    return groups


def mean_or_zero(values: list[float]) -> float:
    if not values:
        return 0.0
    return sum(values) / len(values)


def numeric_series(records: list[dict[str, object]], key: str) -> list[float]:
    values: list[float] = []
    for record in records:
        value = record.get(key)
        if isinstance(value, (int, float)):
            values.append(float(value))
    return values


def summarize_step_records(step_records: list[dict[str, object]]) -> dict[str, object]:
    if not step_records:
        return {
            "record_count": 0,
            "mean_step_wall_s": 0.0,
            "mean_step_cuda_s": 0.0,
            "mean_data_wait_s": 0.0,
            "max_cuda_allocated_gib": 0.0,
            "max_cuda_reserved_gib": 0.0,
        }

    step_wall = numeric_series(step_records, "step_wall_s")
    step_cuda = numeric_series(step_records, "step_cuda_s")
    data_wait = numeric_series(step_records, "data_wait_s")
    cuda_allocated = numeric_series(step_records, "cuda_allocated_gib")
    cuda_reserved = numeric_series(step_records, "cuda_reserved_gib")

    return {
        "record_count": len(step_records),
        "mean_step_wall_s": mean_or_zero(step_wall),
        "mean_step_cuda_s": mean_or_zero(step_cuda),
        "mean_data_wait_s": mean_or_zero(data_wait),
        "max_cuda_allocated_gib": max(cuda_allocated),
        "max_cuda_reserved_gib": max(cuda_reserved),
    }


def phase_class_from_features(features: np.ndarray, num_phase_classes: int) -> int:
    return int(abs(float(features[-1]) * 1_000_000.0)) % num_phase_classes


def phase_classes_from_feature_batch(
    feature_batch: np.ndarray, num_phase_classes: int
) -> np.ndarray:
    phase_values = np.abs(feature_batch[:, -1].astype(np.float64)) * 1_000_000.0
    return np.remainder(phase_values.astype(np.int64), num_phase_classes)


def build_probe_tensors(
    feature_batch: np.ndarray,
    num_regression_heads: int,
    num_phase_classes: int,
) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor]:
    contiguous_batch = np.ascontiguousarray(feature_batch)
    features_tensor = torch.from_numpy(contiguous_batch)
    regression_targets = features_tensor[:, :num_regression_heads]
    phase_targets = torch.from_numpy(
        phase_classes_from_feature_batch(contiguous_batch, num_phase_classes)
    ).to(dtype=torch.long)
    return features_tensor, regression_targets, phase_targets


def collate_probe_batch(
    batch: Sequence[tuple[np.ndarray, int]],
    num_regression_heads: int,
    num_phase_classes: int,
) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor]:
    feature_rows = [features for features, _phase_class in batch]
    feature_batch = np.stack(feature_rows, axis=0)
    return build_probe_tensors(feature_batch, num_regression_heads, num_phase_classes)


def finalize_probe_batch(
    batch: object,
    num_regression_heads: int,
    num_phase_classes: int,
) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor]:
    if (
        isinstance(batch, tuple)
        and len(batch) == 3
        and all(isinstance(item, torch.Tensor) for item in batch)
    ):
        return batch
    if isinstance(batch, Sequence):
        return collate_probe_batch(batch, num_regression_heads, num_phase_classes)
    raise TypeError("unexpected batch type")


def prepare_training_data(
    remote_path: Path = REMOTE_DATA_PATH,
    local_path: Path = LOCAL_DATA_PATH,
    stage_to_tmp: bool = RUNTIME.stage_to_tmp,
) -> dict[str, object]:
    if not remote_path.exists():
        raise FileNotFoundError(f"Missing training data at {remote_path}")

    remote_size = remote_path.stat().st_size
    if not stage_to_tmp:
        return {
            "remote_path": str(remote_path),
            "local_path": str(remote_path),
            "remote_size_bytes": remote_size,
            "reused_local_copy": True,
            "copy_time_s": 0.0,
            "copy_throughput_gib_s": 0.0,
            "stage_to_tmp": False,
        }

    copy_started_at = perf_counter()

    with STAGING_LOCK:
        reused = local_path.exists() and local_path.stat().st_size == remote_size
        if not reused:
            local_path.parent.mkdir(parents=True, exist_ok=True)
            _ = shutil.copy2(remote_path, local_path)

    elapsed_s = perf_counter() - copy_started_at
    throughput_gbps = 0.0
    if not reused and elapsed_s > 0:
        throughput_gbps = remote_size / elapsed_s / (1024**3)

    return {
        "remote_path": str(remote_path),
        "local_path": str(local_path),
        "remote_size_bytes": remote_size,
        "reused_local_copy": reused,
        "copy_time_s": elapsed_s,
        "copy_throughput_gib_s": throughput_gbps,
        "stage_to_tmp": True,
    }


@app.cls(
    image=image,
    gpu=RUNTIME.gpu,
    cpu=RUNTIME.cpu,
    memory=RUNTIME.memory_mib,
    timeout=RUNTIME.timeout_s,
    volumes={RUNTIME.data_mount_path: data_volume},
)
@modal.concurrent(max_inputs=4, target_inputs=2)
class TeacherSaturationProbe:
    @modal.enter()
    def setup(self) -> None:
        if not REMOTE_DATA_PATH.exists():
            raise FileNotFoundError(f"Missing training data at {REMOTE_DATA_PATH}")

    @modal.method()
    def run(self, config: ProbeConfig) -> dict[str, object]:
        from torch.utils.data import DataLoader, Dataset, Sampler

        from models.teacher import TeacherNet
        from utils.config import (
            BYTES_PER_SAMPLE,
            FEATURES_PER_SAMPLE,
            FLOATS_PER_SAMPLE,
            NUM_PHASE_CLASSES,
            NUM_REGRESSION_HEADS,
        )
        from utils.losses import KendallMultiTaskLoss

        copy_stats = prepare_training_data(stage_to_tmp=config.stage_to_tmp)
        local_path = copy_stats["local_path"]
        if not isinstance(local_path, str):
            raise TypeError("copy_stats.local_path must be a string")
        local_data_path = Path(local_path)

        total_file_samples = infer_num_samples(local_data_path, BYTES_PER_SAMPLE)
        sample_indices = build_strided_indices(total_file_samples, config.sample_count)

        class ProbeDataset(Dataset[tuple[np.ndarray, int]]):
            file_path: Path
            indices: list[int]

            def __init__(self, file_path: Path, indices: list[int]) -> None:
                self.file_path = file_path
                self.indices = indices
                self._rows: np.memmap | None = None

            def _memmap_rows(self) -> np.memmap:
                if self._rows is None:
                    sample_total = infer_num_samples(self.file_path, BYTES_PER_SAMPLE)
                    self._rows = np.memmap(
                        self.file_path,
                        dtype=np.float32,
                        mode="r",
                        shape=(sample_total, FLOATS_PER_SAMPLE),
                    )
                return self._rows

            def __len__(self) -> int:
                return len(self.indices)

            def __getitem__(self, dataset_index: int) -> tuple[np.ndarray, int]:
                row = self._memmap_rows()[self.indices[dataset_index]]
                features = row[:FEATURES_PER_SAMPLE]
                phase_value = phase_class_from_features(features, NUM_PHASE_CLASSES)
                return features, phase_value

        class GetitemsProbeDataset(ProbeDataset):
            def __getitems__(
                self, dataset_indices: Sequence[int]
            ) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor]:
                sample_indices = [self.indices[dataset_index] for dataset_index in dataset_indices]
                feature_batch = self._memmap_rows()[sample_indices, :FEATURES_PER_SAMPLE]
                return build_probe_tensors(
                    feature_batch,
                    NUM_REGRESSION_HEADS,
                    NUM_PHASE_CLASSES,
                )

        class BulkProbeDataset(Dataset[tuple[torch.Tensor, torch.Tensor, torch.Tensor]]):
            file_path: Path
            indices: list[int]

            def __init__(self, file_path: Path, indices: list[int]) -> None:
                self.file_path = file_path
                self.indices = indices
                self._rows: np.memmap | None = None

            def _memmap_rows(self) -> np.memmap:
                if self._rows is None:
                    sample_total = infer_num_samples(self.file_path, BYTES_PER_SAMPLE)
                    self._rows = np.memmap(
                        self.file_path,
                        dtype=np.float32,
                        mode="r",
                        shape=(sample_total, FLOATS_PER_SAMPLE),
                    )
                return self._rows

            def __len__(self) -> int:
                return len(self.indices)

            def __getitem__(
                self, dataset_indices: int | Sequence[int]
            ) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor]:
                if isinstance(dataset_indices, int):
                    sample_indices = [self.indices[dataset_indices]]
                else:
                    sample_indices = list(dataset_indices)
                feature_batch = self._memmap_rows()[sample_indices, :FEATURES_PER_SAMPLE]
                return build_probe_tensors(
                    feature_batch,
                    NUM_REGRESSION_HEADS,
                    NUM_PHASE_CLASSES,
                )

        class BatchIndexSampler(Sampler[list[int]]):
            def __init__(self, batch_index_groups: list[list[int]]) -> None:
                self.batch_index_groups = batch_index_groups

            def __iter__(self) -> Iterator[list[int]]:
                return iter(self.batch_index_groups)

            def __len__(self) -> int:
                return len(self.batch_index_groups)

        if config.fetch_mode == "dataset_getitems":
            dataset = GetitemsProbeDataset(local_data_path, sample_indices)
            if config.num_workers > 0:
                dataloader = DataLoader(
                    dataset,
                    batch_size=config.batch_size,
                    shuffle=False,
                    num_workers=config.num_workers,
                    pin_memory=config.pin_memory,
                    drop_last=True,
                    persistent_workers=True,
                    prefetch_factor=config.prefetch_factor,
                    collate_fn=lambda batch: finalize_probe_batch(
                        batch,
                        NUM_REGRESSION_HEADS,
                        NUM_PHASE_CLASSES,
                    ),
                )
            else:
                dataloader = DataLoader(
                    dataset,
                    batch_size=config.batch_size,
                    shuffle=False,
                    num_workers=0,
                    pin_memory=config.pin_memory,
                    drop_last=True,
                    collate_fn=lambda batch: finalize_probe_batch(
                        batch,
                        NUM_REGRESSION_HEADS,
                        NUM_PHASE_CLASSES,
                    ),
                )
        elif config.fetch_mode == "sampler_batches":
            dataset = BulkProbeDataset(local_data_path, sample_indices)
            batch_index_groups = build_batch_index_groups(sample_indices, config.batch_size)
            if config.num_workers > 0:
                dataloader = DataLoader(
                    dataset,
                    batch_size=None,
                    sampler=BatchIndexSampler(batch_index_groups),
                    num_workers=config.num_workers,
                    pin_memory=config.pin_memory,
                    persistent_workers=True,
                    prefetch_factor=config.prefetch_factor,
                    collate_fn=lambda batch: finalize_probe_batch(
                        batch,
                        NUM_REGRESSION_HEADS,
                        NUM_PHASE_CLASSES,
                    ),
                )
            else:
                dataloader = DataLoader(
                    dataset,
                    batch_size=None,
                    sampler=BatchIndexSampler(batch_index_groups),
                    num_workers=0,
                    pin_memory=config.pin_memory,
                    collate_fn=lambda batch: finalize_probe_batch(
                        batch,
                        NUM_REGRESSION_HEADS,
                        NUM_PHASE_CLASSES,
                    ),
                )
        elif config.fetch_mode == "sample_collate":
            dataset = ProbeDataset(local_data_path, sample_indices)
            if config.num_workers > 0:
                dataloader = DataLoader(
                    dataset,
                    batch_size=config.batch_size,
                    shuffle=False,
                    num_workers=config.num_workers,
                    pin_memory=config.pin_memory,
                    drop_last=True,
                    persistent_workers=True,
                    prefetch_factor=config.prefetch_factor,
                    collate_fn=lambda batch: finalize_probe_batch(
                        batch,
                        NUM_REGRESSION_HEADS,
                        NUM_PHASE_CLASSES,
                    ),
                )
            else:
                dataloader = DataLoader(
                    dataset,
                    batch_size=config.batch_size,
                    shuffle=False,
                    num_workers=0,
                    pin_memory=config.pin_memory,
                    drop_last=True,
                    collate_fn=lambda batch: finalize_probe_batch(
                        batch,
                        NUM_REGRESSION_HEADS,
                        NUM_PHASE_CLASSES,
                    ),
                )
        else:
            raise ValueError(f"Unknown fetch mode: {config.fetch_mode}")
        device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        if device.type != "cuda":
            raise RuntimeError("GPU saturation probe requires CUDA")

        model = TeacherNet().to(device)
        loss_module = KendallMultiTaskLoss().to(device)
        optimizer = torch.optim.AdamW(
            list(model.parameters()) + list(loss_module.parameters()),
            lr=config.lr,
            weight_decay=config.weight_decay,
        )

        total_steps = config.warmup_steps + config.measure_steps
        data_wait_times: list[float] = []
        step_times: list[float] = []
        cuda_step_times: list[float] = []
        measured_losses: list[float] = []
        measured_samples = 0
        step_records: list[dict[str, object]] = []
        iterator: Iterator[tuple[torch.Tensor, torch.Tensor, torch.Tensor]] = iter(dataloader)

        torch.cuda.reset_peak_memory_stats(device)

        for step_idx in range(total_steps):
            fetch_started_at = perf_counter()
            try:
                batch = next(iterator)
            except StopIteration:
                iterator = iter(dataloader)
                fetch_started_at = perf_counter()
                batch = next(iterator)
            fetch_finished_at = perf_counter()

            features, regression_targets, phase_targets = batch
            features = features.to(device, non_blocking=config.pin_memory)
            regression_targets = regression_targets.to(device, non_blocking=config.pin_memory)
            phase_targets = phase_targets.to(device, non_blocking=config.pin_memory)

            step_started_at = perf_counter()
            optimizer.zero_grad(set_to_none=True)
            step_start_event = torch.cuda.Event(enable_timing=True)
            step_end_event = torch.cuda.Event(enable_timing=True)
            step_start_event.record()
            with torch.autocast(
                device_type="cuda",
                dtype=torch.bfloat16,
                enabled=config.use_bf16,
            ):
                outputs = model(features)
                loss_dict = loss_module(
                    outputs["regression"],
                    regression_targets,
                    outputs["phase_logits"],
                    phase_targets,
                )
                loss = loss_dict["total_loss"]
            loss.backward()
            _ = optimizer.step()
            step_end_event.record()
            torch.cuda.synchronize(device)
            step_finished_at = perf_counter()
            step_cuda_s = step_start_event.elapsed_time(step_end_event) / 1000.0

            if step_idx >= config.warmup_steps:
                data_wait_s = fetch_finished_at - fetch_started_at
                step_wall_s = step_finished_at - step_started_at
                data_wait_times.append(fetch_finished_at - fetch_started_at)
                step_times.append(step_wall_s)
                cuda_step_times.append(step_cuda_s)
                loss_value = float(loss.detach().cpu().item())
                measured_losses.append(loss_value)
                batch_samples = int(features.shape[0])
                measured_samples += batch_samples
                if config.telemetry_enabled and config.telemetry_record_per_step:
                    step_records.append(
                        {
                            "step_index": step_idx - config.warmup_steps,
                            "data_wait_s": data_wait_s,
                            "step_wall_s": step_wall_s,
                            "step_cuda_s": step_cuda_s,
                            "loss": loss_value,
                            "batch_samples": batch_samples,
                            "cuda_allocated_gib": torch.cuda.memory_allocated(device) / (1024**3),
                            "cuda_reserved_gib": torch.cuda.memory_reserved(device) / (1024**3),
                            "cuda_max_allocated_gib": torch.cuda.max_memory_allocated(device)
                            / (1024**3),
                            "cuda_max_reserved_gib": torch.cuda.max_memory_reserved(device)
                            / (1024**3),
                        }
                    )

        total_data_time = sum(data_wait_times)
        total_step_time = sum(step_times)
        total_measured_time = total_data_time + total_step_time
        measured_batches = len(step_times)
        if measured_batches == 0 or total_measured_time <= 0:
            raise ValueError("measure_steps must be positive")

        memory_stats = torch.cuda.memory_stats(device)
        allocator_metrics = {
            "allocated_bytes_current": int(memory_stats.get("allocated_bytes.all.current", 0)),
            "allocated_bytes_peak": int(memory_stats.get("allocated_bytes.all.peak", 0)),
            "reserved_bytes_current": int(memory_stats.get("reserved_bytes.all.current", 0)),
            "reserved_bytes_peak": int(memory_stats.get("reserved_bytes.all.peak", 0)),
            "active_bytes_peak": int(memory_stats.get("active_bytes.all.peak", 0)),
            "requested_bytes_peak": int(memory_stats.get("requested_bytes.all.peak", 0)),
            "allocation_retries": int(memory_stats.get("num_alloc_retries", 0)),
            "oom_events": int(memory_stats.get("num_ooms", 0)),
        }

        return {
            "config": asdict(config),
            "copy_stats": copy_stats,
            "dataset": {
                "file_path": str(local_data_path),
                "file_samples": total_file_samples,
                "probe_samples": len(sample_indices),
                "coverage_ratio": len(sample_indices) / total_file_samples,
                "bytes_per_sample": BYTES_PER_SAMPLE,
            },
            "metrics": {
                "measured_batches": measured_batches,
                "measured_samples": measured_samples,
                "mean_loss": sum(measured_losses) / measured_batches,
                "mean_data_time_s": total_data_time / measured_batches,
                "mean_step_time_s": total_step_time / measured_batches,
                "mean_cuda_step_time_s": sum(cuda_step_times) / measured_batches,
                "samples_per_s": measured_samples / total_measured_time,
                "batches_per_s": measured_batches / total_measured_time,
                "data_fraction": total_data_time / total_measured_time,
                "step_fraction": total_step_time / total_measured_time,
                "cuda_step_fraction": sum(cuda_step_times) / total_measured_time,
                "effective_input_gib_s": (
                    measured_samples * BYTES_PER_SAMPLE / total_measured_time / (1024**3)
                ),
                "peak_cuda_allocated_gib": torch.cuda.max_memory_allocated(device) / (1024**3),
                "peak_cuda_reserved_gib": torch.cuda.max_memory_reserved(device) / (1024**3),
            },
            "telemetry": {
                "platform_time_series": "Use Modal GPU/CPU/memory charts for container-level over-time metrics.",
                "allocator_metrics": allocator_metrics,
                "step_records": step_records,
                "step_summary": summarize_step_records(step_records),
            },
        }


def probe_handle() -> ProbeHandle:
    return cast(Callable[[], ProbeHandle], TeacherSaturationProbe)()


@app.local_entrypoint()
def run_once(
    sample_count: int = RUNTIME.sample_count,
    batch_size: int = RUNTIME.batch_size,
    stage_to_tmp: bool = RUNTIME.stage_to_tmp,
    fetch_mode: str = RUNTIME.fetch_mode,
    warmup_steps: int = RUNTIME.warmup_steps,
    measure_steps: int = RUNTIME.measure_steps,
    num_workers: int = RUNTIME.num_workers,
    prefetch_factor: int = RUNTIME.prefetch_factor,
    lr: float = RUNTIME.lr,
    weight_decay: float = RUNTIME.weight_decay,
    use_bf16: bool = RUNTIME.use_bf16,
    telemetry_enabled: bool = RUNTIME.telemetry_enabled,
    telemetry_record_per_step: bool = RUNTIME.telemetry_record_per_step,
) -> None:
    probe = probe_handle()
    result = probe.run.remote(
        ProbeConfig(
            sample_count=sample_count,
            batch_size=batch_size,
            stage_to_tmp=stage_to_tmp,
            fetch_mode=fetch_mode,
            warmup_steps=warmup_steps,
            measure_steps=measure_steps,
            num_workers=num_workers,
            prefetch_factor=prefetch_factor,
            lr=lr,
            weight_decay=weight_decay,
            use_bf16=use_bf16,
            telemetry_enabled=telemetry_enabled,
            telemetry_record_per_step=telemetry_record_per_step,
        )
    )
    print(json.dumps(result, indent=2, sort_keys=True))


def summarize_packed_results(
    instance_count: int,
    results: Sequence[dict[str, object]],
) -> dict[str, object]:
    aggregate_samples_per_s = 0.0
    aggregate_batches_per_s = 0.0
    max_reserved_gib = 0.0
    max_allocated_gib = 0.0
    mean_losses: list[float] = []

    for result in results:
        metrics = result.get("metrics")
        if not isinstance(metrics, dict):
            raise TypeError("probe result is missing metrics")
        samples_per_s = metrics.get("samples_per_s")
        batches_per_s = metrics.get("batches_per_s")
        peak_reserved = metrics.get("peak_cuda_reserved_gib")
        peak_allocated = metrics.get("peak_cuda_allocated_gib")
        mean_loss = metrics.get("mean_loss")
        if not isinstance(samples_per_s, (int, float)):
            raise TypeError("probe result metrics are missing samples_per_s")
        if not isinstance(batches_per_s, (int, float)):
            raise TypeError("probe result metrics are missing batches_per_s")
        if not isinstance(peak_reserved, (int, float)):
            raise TypeError("probe result metrics are missing peak_cuda_reserved_gib")
        if not isinstance(peak_allocated, (int, float)):
            raise TypeError("probe result metrics are missing peak_cuda_allocated_gib")
        if not isinstance(mean_loss, (int, float)):
            raise TypeError("probe result metrics are missing mean_loss")
        aggregate_samples_per_s += float(samples_per_s)
        aggregate_batches_per_s += float(batches_per_s)
        max_reserved_gib = max(max_reserved_gib, float(peak_reserved))
        max_allocated_gib = max(max_allocated_gib, float(peak_allocated))
        mean_losses.append(float(mean_loss))

    return {
        "instance_count": instance_count,
        "aggregate_samples_per_s": aggregate_samples_per_s,
        "aggregate_batches_per_s": aggregate_batches_per_s,
        "max_per_instance_reserved_gib": max_reserved_gib,
        "max_per_instance_allocated_gib": max_allocated_gib,
        "mean_per_instance_loss": mean_or_zero(mean_losses),
    }


@app.local_entrypoint()
def sweep(
    batch_sizes: str = RUNTIME.sweep_batch_sizes,
    num_workers_list: str = RUNTIME.sweep_num_workers,
    sample_count: int = RUNTIME.sample_count,
    stage_to_tmp: bool = RUNTIME.stage_to_tmp,
    fetch_mode: str = RUNTIME.fetch_mode,
    warmup_steps: int = RUNTIME.warmup_steps,
    measure_steps: int = RUNTIME.measure_steps,
    prefetch_factor: int = RUNTIME.prefetch_factor,
    use_bf16: bool = RUNTIME.use_bf16,
    telemetry_enabled: bool = RUNTIME.telemetry_enabled,
    telemetry_record_per_step: bool = RUNTIME.telemetry_record_per_step,
) -> None:
    probe = probe_handle()
    results: list[dict[str, object]] = []

    def samples_per_second(result: dict[str, object]) -> float:
        metrics = result.get("metrics")
        if not isinstance(metrics, dict):
            raise TypeError("probe result is missing metrics")
        samples_per_s = metrics.get("samples_per_s")
        if not isinstance(samples_per_s, (int, float)):
            raise TypeError("probe result metrics are missing samples_per_s")
        return float(samples_per_s)

    for batch_size in parse_int_csv(batch_sizes):
        for num_workers in parse_int_csv(num_workers_list):
            config = ProbeConfig(
                sample_count=sample_count,
                batch_size=batch_size,
                stage_to_tmp=stage_to_tmp,
                fetch_mode=fetch_mode,
                warmup_steps=warmup_steps,
                measure_steps=measure_steps,
                num_workers=num_workers,
                prefetch_factor=prefetch_factor,
                use_bf16=use_bf16,
                telemetry_enabled=telemetry_enabled,
                telemetry_record_per_step=telemetry_record_per_step,
            )
            results.append(probe.run.remote(config))

    results.sort(key=samples_per_second, reverse=True)
    print(json.dumps(results, indent=2, sort_keys=True))


@app.local_entrypoint()
def packed_run(
    instance_counts: str = "2,3",
    sample_count: int = 262_144,
    batch_size: int = 12_288,
    stage_to_tmp: bool = True,
    fetch_mode: str = "sample_collate",
    warmup_steps: int = 8,
    measure_steps: int = 32,
    num_workers: int = 10,
    prefetch_factor: int = 6,
    lr: float = 1e-3,
    weight_decay: float = 1e-4,
    use_bf16: bool = True,
    telemetry_enabled: bool = True,
    telemetry_record_per_step: bool = True,
) -> None:
    probe = probe_handle()
    packed_results: list[dict[str, object]] = []
    for instance_count in parse_int_csv(instance_counts):
        config = ProbeConfig(
            sample_count=sample_count,
            batch_size=batch_size,
            stage_to_tmp=stage_to_tmp,
            fetch_mode=fetch_mode,
            warmup_steps=warmup_steps,
            measure_steps=measure_steps,
            num_workers=num_workers,
            prefetch_factor=prefetch_factor,
            lr=lr,
            weight_decay=weight_decay,
            use_bf16=use_bf16,
            telemetry_enabled=telemetry_enabled,
            telemetry_record_per_step=telemetry_record_per_step,
        )
        futures = [probe.run.spawn(config) for _ in range(instance_count)]
        results = [future.get() for future in futures]
        packed_results.append(
            {
                "config": asdict(config),
                "aggregate": summarize_packed_results(instance_count, results),
                "per_instance_results": results,
            }
        )

    print(json.dumps(packed_results, indent=2, sort_keys=True))
