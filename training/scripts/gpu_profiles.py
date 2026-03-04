from __future__ import annotations

from dataclasses import dataclass
from typing import Literal


@dataclass(frozen=True)
class ModalResources:
    gpu: str
    cpu: int
    memory_mib: int
    teacher_timeout_s: int
    student_timeout_s: int
    retries: int = 2
    scaledown_window_s: int = 300
    startup_timeout_s: int = 600


@dataclass(frozen=True)
class TrainingSettings:
    teacher_batch_sizes: tuple[int, ...]
    student_batch_size: int
    teacher_num_workers: int
    student_num_workers: int
    prefetch_factor: int
    precision: Literal["bf16-mixed", "16-mixed"]
    max_parallel_workers: int
    default_parallel_workers: int
    trials_per_worker: int
    teacher_epochs: int
    student_epochs: int


@dataclass(frozen=True)
class GpuProfile:
    name: str
    description: str
    resources: ModalResources
    settings: TrainingSettings


SELECTOR_ONLY_HIGH_END_SETTINGS = TrainingSettings(
    teacher_batch_sizes=(12288,),
    student_batch_size=12288,
    teacher_num_workers=10,
    student_num_workers=10,
    prefetch_factor=6,
    precision="bf16-mixed",
    max_parallel_workers=30,
    default_parallel_workers=15,
    trials_per_worker=4,
    teacher_epochs=50,
    student_epochs=100,
)


GPU_PROFILES: dict[str, GpuProfile] = {
    "l4": GpuProfile(
        name="l4",
        description="Cost-optimized default for small-model HPO and horizontal scaling.",
        resources=ModalResources(
            gpu="L4",
            cpu=6,
            memory_mib=32768,
            teacher_timeout_s=21600,
            student_timeout_s=7200,
        ),
        settings=TrainingSettings(
            teacher_batch_sizes=(2048, 4096, 8192, 16384),
            student_batch_size=16384,
            teacher_num_workers=4,
            student_num_workers=4,
            prefetch_factor=4,
            precision="bf16-mixed",
            max_parallel_workers=30,
            default_parallel_workers=15,
            trials_per_worker=4,
            teacher_epochs=50,
            student_epochs=100,
        ),
    ),
    "a10": GpuProfile(
        name="a10",
        description="Balanced fallback when L4 availability is constrained.",
        resources=ModalResources(
            gpu="A10",
            cpu=6,
            memory_mib=32768,
            teacher_timeout_s=21600,
            student_timeout_s=7200,
        ),
        settings=TrainingSettings(
            teacher_batch_sizes=(2048, 4096, 8192, 16384),
            student_batch_size=16384,
            teacher_num_workers=4,
            student_num_workers=4,
            prefetch_factor=4,
            precision="bf16-mixed",
            max_parallel_workers=30,
            default_parallel_workers=15,
            trials_per_worker=4,
            teacher_epochs=50,
            student_epochs=100,
        ),
    ),
    "b200": GpuProfile(
        name="b200",
        description="Wall-clock optimized premium profile based on benchmark-informed real-training defaults.",
        resources=ModalResources(
            gpu="B200",
            cpu=12,
            memory_mib=40960,
            teacher_timeout_s=10800,
            student_timeout_s=7200,
        ),
        settings=TrainingSettings(
            teacher_batch_sizes=(12288,),
            student_batch_size=12288,
            teacher_num_workers=10,
            student_num_workers=10,
            prefetch_factor=6,
            precision="bf16-mixed",
            max_parallel_workers=30,
            default_parallel_workers=15,
            trials_per_worker=4,
            teacher_epochs=50,
            student_epochs=100,
        ),
    ),
    "h100": GpuProfile(
        name="h100",
        description="Selector-only H100 profile for policy/value benchmarking; use explicit run settings.",
        resources=ModalResources(
            gpu="H100!",
            cpu=12,
            memory_mib=40960,
            teacher_timeout_s=10800,
            student_timeout_s=7200,
        ),
        settings=SELECTOR_ONLY_HIGH_END_SETTINGS,
    ),
    "h200": GpuProfile(
        name="h200",
        description="Selector-only H200 profile for policy/value benchmarking; use explicit run settings.",
        resources=ModalResources(
            gpu="H200",
            cpu=12,
            memory_mib=40960,
            teacher_timeout_s=10800,
            student_timeout_s=7200,
        ),
        settings=SELECTOR_ONLY_HIGH_END_SETTINGS,
    ),
    "t4": GpuProfile(
        name="t4",
        description="Budget fallback; uses fp16 mixed precision (no native bf16 support).",
        resources=ModalResources(
            gpu="T4",
            cpu=4,
            memory_mib=24576,
            teacher_timeout_s=14400,
            student_timeout_s=10800,
        ),
        settings=TrainingSettings(
            teacher_batch_sizes=(2048, 4096, 8192, 16384),
            student_batch_size=8192,
            teacher_num_workers=4,
            student_num_workers=4,
            prefetch_factor=4,
            precision="16-mixed",
            max_parallel_workers=30,
            default_parallel_workers=20,
            trials_per_worker=3,
            teacher_epochs=50,
            student_epochs=100,
        ),
    ),
}

DEFAULT_PROFILE_NAME = "l4"


def get_profile(name: str) -> GpuProfile:
    key = name.strip().lower()
    if key not in GPU_PROFILES:
        valid = ", ".join(sorted(GPU_PROFILES))
        raise ValueError(f"Unknown GPU profile '{name}'. Valid profiles: {valid}")
    return GPU_PROFILES[key]
