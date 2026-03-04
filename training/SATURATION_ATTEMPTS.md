# Fusion Saturation Attempt Log

Canonical ledger for every saturation benchmark attempt in `fusion-engine/training`.

## Logging Rules

- Record every attempt, even failed ones.
- Do not delete prior attempts; append corrections or follow-up notes.
- Keep the exact command, env-backed settings, and the resulting metrics together.
- One testing instance remains one GPU container; parallel attempts, if any, should be separate sections.

## Attempt 001

- Status: failed before remote execution
- Goal: baseline teacher-path saturation run on current default B200 probe settings
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 7e93bd2d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `python3 -m modal run training/scripts/gpu_saturation_probe.py::run_once`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=16384`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Failure at launch: `ModuleNotFoundError: No module named 'utils'`
  - Root cause: file-path `modal run training/scripts/...` treated the script as file-based execution, so sibling `training/utils` imports were not on `sys.path`.
  - Fix applied afterward: executable scripts now inject `TRAINING_ROOT` into `sys.path`, and future invocations should prefer Modal module mode.
- Notes:
  - This is the baseline reference point for later batch-size / loader-worker comparisons.
  - Modal dashboard/model page should be checked alongside the probe JSON for host-level time-series context.

## Attempt 002

- Status: failed before remote execution
- Goal: first substantive baseline teacher-path saturation run on current default B200 probe settings after fixing script execution imports
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 7e93bd2d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=16384`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
 - Result summary:
   - Failure at launch: Modal `InvalidError` because the image attempted a build step after `image.add_local_*`.
   - Root cause: `training/scripts/gpu_saturation_probe.py` built the image as `.pip_install(...).add_local_dir(...).add_local_dir(...).env(...)`; Modal requires `add_local_*` to be last unless `copy=True` is used.
   - Fix applied afterward: moved `.env({"PYTHONPATH": ...})` before the `.add_local_dir(...)` calls.
- Notes:
  - Module mode keeps import resolution aligned with the local package layout.
  - Modal dashboard/model page should be checked alongside the probe JSON for host-level time-series context.

## Attempt 003

- Status: failed during remote import
- Goal: rerun the baseline teacher-path saturation probe after fixing Modal image packaging order
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 7e93bd2d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=16384`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Failure inside the Modal container: `ImportError: cannot import name 'override' from 'typing'`.
  - Root cause: the unified training tree used `from typing import override`, but the Modal image is Python 3.11, where `typing.override` does not exist.
  - Fix applied afterward: changed affected files to use a compatibility fallback (`try: from typing import override; except ImportError: from typing_extensions import override`).
- Notes:
  - This was a genuine remote-only compatibility failure; local verification had not surfaced it because the local environment was newer than the Modal image.

## Attempt 004

- Status: completed successfully
- Goal: first successful baseline teacher-path saturation run on current default B200 probe settings
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun d10defc2`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-4rlyYNnDc9fptwWD638Cg4`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=16384`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful baseline benchmark on B200.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `16.57s` at `1.66 GiB/s`.
  - Throughput: `59,958 samples/s` and `3.66 batches/s` across `32` measured batches (`524,288` measured samples).
  - Timing split: `mean_data_time_s=0.0407`, `mean_step_time_s=0.2326`, `mean_cuda_step_time_s=0.2315`, `data_fraction=0.1489`, `step_fraction=0.8511`, `cuda_step_fraction=0.8473`.
  - Memory: `peak_cuda_allocated_gib=18.71`, `peak_cuda_reserved_gib=24.72`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Data-wait telemetry showed two notable stalls around step records `8` and `24` (`0.59s` and `0.71s` respectively) amid otherwise tiny fetch waits.
- Notes:
  - Baseline result suggests this configuration is primarily step/compute dominated rather than loader dominated on B200, but it is not perfectly smooth because there are intermittent data-wait spikes.
  - Reserved VRAM at `24.72 GiB` is already above a nominal 24 GiB class card budget, so this exact baseline is a poor fit for L4-class replication without reducing memory pressure.

## Next queued matrix

- Stage 1 objective: isolate the batch-size curve on B200 while keeping loader behavior fixed at the baseline (`num_workers=4`, `prefetch_factor=4`, `pin_memory=true`, `bf16=true`).
- Stage 1 rationale: Attempt 004 was mostly step/compute dominated (`step_fraction≈0.851`) and already reserved `24.72 GiB`, so the next priority is to map memory pressure and throughput slope before changing loader knobs.
- Stage 2 objective: after the best non-pathological Stage 1 batch size is known, sweep loader workers around that batch size to test whether the intermittent data stalls shrink materially.

## Attempt 005

- Status: succeeded
- Goal: lower-memory anchor below the successful baseline to measure throughput loss versus regained VRAM headroom
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 06934e76`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=8192 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-XdfOlLF4aEqIlXzdq4QLtt`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=8192`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful lower-batch benchmark on B200.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `21.31s` at `1.29 GiB/s`.
  - Throughput: `61,958 samples/s` and `7.56 batches/s` across `32` measured batches (`262,144` measured samples).
  - Timing split: `mean_data_time_s=0.0132`, `mean_step_time_s=0.1191`, `mean_cuda_step_time_s=0.1185`, `data_fraction=0.0995`, `step_fraction=0.9005`, `cuda_step_fraction=0.8962`.
  - Memory: `peak_cuda_allocated_gib=9.37`, `peak_cuda_reserved_gib=13.95`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: most fetch waits were sub-millisecond, with one notable stall around step record `24` (`0.41s`).
- Notes:
  - This run establishes the low-memory / lower-batch anchor for the single-GPU curve.
  - Compared with Attempt 004 (`batch_size=16384`), throughput slightly improved (`61.96k` vs `59.96k` samples/s) while reserved VRAM dropped sharply (`13.95 GiB` vs `24.72 GiB`).
  - This result fits comfortably under a nominal 24 GiB-class memory ceiling and suggests the 16384 baseline was already beyond the useful batch-size knee for this stack.

## Attempt 006

- Status: succeeded
- Goal: intermediate memory-pressure check between the low anchor and the current baseline
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 2325ebcb`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=12288 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-DaxW8FBZ2XilZzFsdJyGKL`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful intermediate batch benchmark on B200.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `19.35s` at `1.42 GiB/s`.
  - Throughput: `63,381.54 samples/s` and `5.16 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0183`, `mean_step_time_s=0.1755`, `mean_cuda_step_time_s=0.1748`, `data_fraction=0.0946`, `step_fraction=0.9054`, `cuda_step_fraction=0.9014`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: most fetch waits were tiny, with one notable `0.58s` stall around measured step record `13`.
- Notes:
  - This point helps interpolate the memory and throughput curve between 8192 and the 16384 baseline.
  - It is the most likely candidate to fit under a 24 GiB-class reserved-memory ceiling while preserving decent throughput.
  - Compared with Attempt 005 (`batch_size=8192`), throughput improved again (`63.38k` vs `61.96k`) while staying below the nominal 24 GiB class-card ceiling (`20.90 GiB` reserved).
  - This is now the best Stage 1 result so far.

## Attempt 007

- Status: succeeded
- Goal: higher-batch push to test whether B200 throughput keeps improving before instability or allocator pressure dominates
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 2325ebcb`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=24576 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-YDNZ36fTeIDBYjsGOSUpI4`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=24576`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Completed, but performance was pathological relative to smaller batches.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `17.52s` at `1.57 GiB/s`.
  - Throughput collapsed to `22,278.90 samples/s` and `0.91 batches/s` across `32` measured batches (`786,432` measured samples).
  - Timing split: `mean_data_time_s=0.7610`, `mean_step_time_s=0.3421`, `mean_cuda_step_time_s=0.3405`, `data_fraction=0.6899`, `step_fraction=0.3101`, `cuda_step_fraction=0.3087`.
  - Memory: `peak_cuda_allocated_gib=28.05`, `peak_cuda_reserved_gib=41.76`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: repeated multi-second data stalls appeared (e.g. ~`4.75s`, `2.42s`, `1.67s`, `1.60s`, `1.19s`, `0.78s`).
- Notes:
  - This run checks whether a materially larger batch pushes B200 into a better throughput regime or just inflates reserved memory.
  - Compare allocator peaks and stall patterns against Attempt 004 before considering an even larger batch.
  - Result: larger batch did not help. It drove memory far above the 24 GiB class-card target and introduced severe loader/data-wait behavior.
  - This point should be treated as beyond the useful knee for the current stack.

## Attempt 008

- Status: succeeded
- Goal: upper-batch limit check on B200 before any loader sweep or cross-GPU work
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 2325ebcb`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=32768 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-LzORjl3eutcvEh8wH9IPGU`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=32768`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=4`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful high-end pressure test on B200, but it underperformed the mid-range batches.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `19.83s` at `1.38 GiB/s`.
  - Throughput: `53,785.24 samples/s` and `1.64 batches/s` across `32` measured batches (`1,048,576` measured samples).
  - Timing split: `mean_data_time_s=0.1480`, `mean_step_time_s=0.4613`, `mean_cuda_step_time_s=0.4592`, `data_fraction=0.2429`, `step_fraction=0.7571`, `cuda_step_fraction=0.7538`.
  - Memory: `peak_cuda_allocated_gib=37.39`, `peak_cuda_reserved_gib=55.66`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: several >`1s` data stalls appeared (e.g. ~`1.10s`, `1.04s`, `1.44s`, `1.15s`) even though the step path remained the dominant fraction overall.
- Notes:
  - This is the high-end B200 pressure test and should be interpreted together with allocator peaks, any retries/OOMs, and step-time scaling.
  - If this run is unstable or shows poor scaling, Stage 2 should use the best result from Attempts 005-007 rather than pushing batch size higher.
  - Result: throughput remained below Attempts 005 and 006 while memory ballooned well beyond the target cross-GPU regime.

## Stage 1 conclusion

- Best current batch-size point: **Attempt 006 / batch_size=12288**.
- Evidence: it delivered the highest throughput so far (`63.38k samples/s`) while keeping `peak_cuda_reserved_gib=20.90`, which stays below a nominal 24 GiB class-card ceiling.
- Attempt 004 (`16384`) was already past the useful knee, Attempt 007 (`24576`) was pathological, and Attempt 008 (`32768`) inflated memory dramatically without improving throughput.
- Concurrency note: Attempts 006-008 were launched in parallel as separate GPU-container runs, so copy-time and some data-stall differences may include concurrent platform/storage effects. The batch-size ranking is still strong because the throughput and reserved-memory gaps are large.

## Next queued matrix (Stage 2)

- Stage 2 objective: keep `FUSION_PROBE_BATCH_SIZE=12288` fixed and sweep DataLoader worker count to see whether the remaining intermittent stalls shrink materially without regressing throughput.
- Stage 2 rationale: the batch-size curve is now mapped well enough to show `12288` as the best current knee, so the next useful axis is loader tuning around that point.

## Attempt 009

- Status: succeeded
- Goal: lower-worker anchor at the best current batch size to test whether fewer workers smooth out loader behavior
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 9977b2ac`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=2 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-jExSW1eZMXwV4p3bsd3nFY`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=2`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful low-worker benchmark at the current best batch size, but throughput collapsed.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `19.16s` at `1.43 GiB/s`.
  - Throughput: `22,400.45 samples/s` and `1.82 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.3712`, `mean_step_time_s=0.1773`, `mean_cuda_step_time_s=0.1759`, `data_fraction=0.6767`, `step_fraction=0.3233`, `cuda_step_fraction=0.3207`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: repeated large fetch stalls appeared (for example ~`0.35s`, `0.42s`, `0.99s`, `0.78s`, `1.09s`, and sustained ~`0.32s`-`0.42s` waits later in the run).
- Notes:
  - Reducing worker count to `2` materially hurt throughput without reducing memory pressure.
  - This indicates the loader side becomes the dominant limiter at this worker count for `batch_size=12288`.

## Attempt 010

- Status: succeeded
- Goal: moderate worker increase at the best current batch size to test whether loader overlap improves further
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 9977b2ac`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-BP63jOTzEFzeBsdgW3VSQg`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=6`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful moderate-worker benchmark, but still substantially worse than the Stage 1 winner.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `17.95s` at `1.53 GiB/s`.
  - Throughput: `31,030.93 samples/s` and `2.53 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.2221`, `mean_step_time_s=0.1739`, `mean_cuda_step_time_s=0.1730`, `data_fraction=0.5609`, `step_fraction=0.4391`, `cuda_step_fraction=0.4368`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: mostly tiny waits early, but large stalls appeared later (including ~`3.15s`, `1.04s`, `0.86s`, `0.82s`, `0.70s`).
- Notes:
  - Increasing workers from `2` to `6` helped materially, but it still did not restore the throughput seen at the original `num_workers=4` winner from Stage 1.
  - Loader-side instability remained visible despite the same reserved-memory footprint.

## Attempt 011

- Status: succeeded
- Goal: high-worker check at the best current batch size to see whether extra workers help or just add churn
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 9977b2ac`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-v6NJbtJSwuV54S04qyGCfY`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=4`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful high-worker benchmark and the best overall result so far.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `17.99s` at `1.53 GiB/s`.
  - Throughput: `64,842.82 samples/s` and `5.28 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0134`, `mean_step_time_s=0.1761`, `mean_cuda_step_time_s=0.1753`, `data_fraction=0.0708`, `step_fraction=0.9292`, `cuda_step_fraction=0.9251`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: fetch waits were almost entirely in the microsecond-to-millisecond range, with only small isolated bumps (~`0.32s` and `0.10s`).
- Notes:
  - Raising workers to `8` preserved the favorable memory footprint of Attempt 006 while further improving throughput.
  - This is now the strongest current single-GPU benchmark point.

## Stage 2 conclusion

- Best current settings: **Attempt 011 / batch_size=12288 / num_workers=8**.
- Evidence: it delivered the highest throughput so far (`64.84k samples/s`) while keeping `peak_cuda_reserved_gib=20.90`, still under a nominal 24 GiB-class reserved-memory ceiling.
- Stage 2 also clarified the loader sensitivity: `num_workers=2` and `6` both suffered substantial loader stalls, while `8` largely removed them without adding memory pressure.
- Concurrency note: Attempts 009-011 were launched in parallel as separate GPU-container runs, so copy-time variance may still include shared platform/storage effects. The worker-count ranking is still strong because throughput differences were large and memory stayed flat.

## Next queued matrix (Stage 3)

- Stage 3 objective: keep `FUSION_PROBE_BATCH_SIZE=12288` and `FUSION_PROBE_NUM_WORKERS=8` fixed, then sweep `prefetch_factor` to see whether the now-best worker count benefits from a different queue depth.
- Stage 3 rationale: Stage 2 made the worker bottleneck visible and selected `num_workers=8`; the next tight axis is `prefetch_factor`, which may either smooth the remaining tiny fetch noise or add unnecessary host pressure.

## Attempt 012

- Status: succeeded
- Goal: lower-prefetch anchor at the current best batch/worker point
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 0220eda3`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=2 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-z267M4rzWbt5hni9y4FrEu`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=2`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful lower-prefetch benchmark at the current best batch/worker point.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `17.22s` at `1.59 GiB/s`.
  - Throughput: `62,712.81 samples/s` and `5.10 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0201`, `mean_step_time_s=0.1759`, `mean_cuda_step_time_s=0.1751`, `data_fraction=0.1024`, `step_fraction=0.8976`, `cuda_step_fraction=0.8939`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: one notable fetch stall around measured step 13 (~`0.636s`), otherwise small waits.
- Notes:
  - `prefetch_factor=2` is viable but slower than the stronger prefetch settings at the same memory footprint.

## Attempt 013

- Status: succeeded
- Goal: higher-prefetch check at the current best batch/worker point
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 0220eda3`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-v5M1pnXrR8RoTD2Hc3UlgS`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful higher-prefetch benchmark and the best overall result so far.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `19.17s` at `1.43 GiB/s`.
  - Throughput: `65,383.72 samples/s` and `5.32 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0149`, `mean_step_time_s=0.1731`, `mean_cuda_step_time_s=0.1723`, `data_fraction=0.0791`, `step_fraction=0.9209`, `cuda_step_fraction=0.9166`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: almost all waits were tiny, with one moderate stall around step 13 (~`0.393s`) and a smaller bump around step 14 (~`0.080s`).
- Notes:
  - `prefetch_factor=6` is now the strongest current single-GPU configuration.
  - It improves throughput over Attempt 011 while keeping the same memory footprint and low-stall profile.

## Attempt 014

- Status: succeeded
- Goal: upper-prefetch pressure check at the current best batch/worker point
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 0220eda3`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=8 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-yR7SApGUU29zMCaipvGnzT`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=8`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful upper-prefetch benchmark, but it did not improve on the best point.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `22.32s` at `1.23 GiB/s`.
  - Throughput: `63,791.20 samples/s` and `5.19 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0199`, `mean_step_time_s=0.1727`, `mean_cuda_step_time_s=0.1719`, `data_fraction=0.1033`, `step_fraction=0.8967`, `cuda_step_fraction=0.8924`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: one major stall around measured step 13 (~`0.634s`), otherwise small waits.
- Notes:
  - Larger queue depth did not help further and slightly hurt the best-case throughput.

## Stage 3 conclusion

- Best current settings: **Attempt 013 / batch_size=12288 / num_workers=8 / prefetch_factor=6**.
- Evidence: it delivered the highest throughput so far (`65.38k samples/s`) while keeping `peak_cuda_reserved_gib=20.90`, still below a nominal 24 GiB-class reserved-memory ceiling.
- Stage 3 clarified that queue depth still matters at the chosen worker count, but the gain is modest: `prefetch_factor=6` beat `2` and `8` without changing memory pressure.
- Concurrency note: Attempts 012-014 were launched in parallel as separate GPU-container runs, so copy-time variance may still include shared platform/storage effects. The ranking remains strong because memory stayed flat and throughput differences were consistent.

## Next queued matrix (Stage 4)

- Stage 4 objective: hold the best B200 settings fixed (`batch_size=12288`, `num_workers=8`, `prefetch_factor=6`) and begin cross-GPU comparison.
- Stage 4 rationale: the B200-local knee is now mapped tightly enough that the next useful question is whether a cheaper GPU class can achieve similar throughput with the same memory-safe configuration.

## Attempt 015

- Status: succeeded
- Goal: first cheaper-GPU comparison using the current best B200 settings on L4
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun c5098946`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=L4 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-QSU2tmym44qjuYgrjYWMEr`
- Runtime settings:
  - `FUSION_PROBE_GPU=L4`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful L4 comparison run at the current best B200-local settings.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `43.39s` at `0.63 GiB/s`.
  - Throughput: `13,057.41 samples/s` and `1.06 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0290`, `mean_step_time_s=0.9121`, `mean_cuda_step_time_s=0.9084`, `data_fraction=0.0308`, `step_fraction=0.9692`, `cuda_step_fraction=0.9653`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: one major data stall around measured step 13 (~`0.920s`), but otherwise the run was overwhelmingly compute-dominated.
- Notes:
  - The configuration fits in memory on L4, but raw throughput is far below B200.
  - This makes L4 a poor wall-clock choice for this benchmark path, even though the memory-safe configuration ports cleanly.

## Attempt 016

- Status: succeeded
- Goal: first cheaper-GPU comparison using the current best B200 settings on A10
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun c5098946`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=A10 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-Td2swm2dML6xwmBziEW6Q0`
- Runtime settings:
  - `FUSION_PROBE_GPU=A10`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful A10 comparison run at the current best B200-local settings.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `21.22s` at `1.29 GiB/s`.
  - Throughput: `20,021.47 samples/s` and `1.63 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0178`, `mean_step_time_s=0.5960`, `mean_cuda_step_time_s=0.5943`, `data_fraction=0.0290`, `step_fraction=0.9710`, `cuda_step_fraction=0.9683`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.89`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: one larger stall around measured step 13 (~`0.564s`), but the run remained overwhelmingly compute-dominated.
- Notes:
  - A10 materially outperformed L4 on the same configuration but remained well behind B200 in absolute throughput.
  - It is the stronger cheaper-card candidate and therefore the right one to tune next.

## Stage 4 conclusion

- Cross-GPU ranking at the current best B200-local settings: **B200 > A10 > L4** in raw throughput.
- Evidence: B200 Attempt 013 reached `65.38k samples/s`, A10 Attempt 016 reached `20.02k samples/s`, and L4 Attempt 015 reached `13.06k samples/s`, all while fitting in roughly the same reserved-memory envelope (~`20.9 GiB`).
- Interpretation: these runs are overwhelmingly compute-dominated on the cheaper cards, not loader-dominated. The current configuration ports cleanly to both A10 and L4, but B200 remains the wall-clock winner by a large margin.
- Practical takeaway: A10 is now the only cheaper-card candidate worth tuning further; L4 is already behind enough that it is not the best next use of experiment budget.

## Next queued matrix (Stage 5)

- Stage 5 objective: tune the A10 locally, since it is the best cheaper-card candidate from Stage 4.
- Stage 5 rationale: the B200-local optimum may not be the A10-local optimum. Before concluding on hardware value, we should test whether A10 prefers a different batch size while keeping the strong loader settings (`num_workers=8`, `prefetch_factor=6`).

## Attempt 017

- Status: succeeded
- Goal: lower-batch A10 anchor to test whether the cheaper card prefers a smaller compute chunk
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun d8284fae`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=A10 FUSION_PROBE_BATCH_SIZE=8192 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-rLgiIR52owg4N6M1Jvts6V`
- Runtime settings:
  - `FUSION_PROBE_GPU=A10`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=8192`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful lower-batch A10 benchmark and the best A10-local point tested so far.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `21.49s` at `1.28 GiB/s`.
  - Throughput: `20,170.97 samples/s` and `2.46 batches/s` across `32` measured batches (`262,144` measured samples).
  - Timing split: `mean_data_time_s=0.0138`, `mean_step_time_s=0.3924`, `mean_cuda_step_time_s=0.3913`, `data_fraction=0.0339`, `step_fraction=0.9661`, `cuda_step_fraction=0.9634`.
  - Memory: `peak_cuda_allocated_gib=9.37`, `peak_cuda_reserved_gib=13.95`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: almost all fetch waits were tiny, with one notable stall around measured step 24 (~`0.435s`).
- Notes:
  - This slightly outperformed the prior A10 run at `batch_size=12288` while cutting reserved memory materially.
  - The run remained overwhelmingly compute-dominated, so A10 is bottlenecking on GPU compute rather than loader overlap at these settings.

## Attempt 018

- Status: succeeded
- Goal: higher-batch A10 check to see whether the cheaper card still benefits from larger compute batches
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun d8284fae`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=A10 FUSION_PROBE_BATCH_SIZE=16384 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-eKQ4kTB7DajL8YbyJdVtbl`
- Runtime settings:
  - `FUSION_PROBE_GPU=A10`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=16384`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful higher-batch A10 benchmark, but it underperformed the smaller-batch point.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `20.63s` at `1.33 GiB/s`.
  - Throughput: `19,643.31 samples/s` and `1.20 batches/s` across `32` measured batches (`524,288` measured samples).
  - Timing split: `mean_data_time_s=0.0398`, `mean_step_time_s=0.7942`, `mean_cuda_step_time_s=0.7920`, `data_fraction=0.0478`, `step_fraction=0.9522`, `cuda_step_fraction=0.9495`.
  - Memory: `peak_cuda_allocated_gib=18.71`, `peak_cuda_reserved_gib=21.58`; allocator summary reported `allocation_retries=1` and `oom_events=0`.
  - Telemetry: nearly all waits were tiny, with one larger stall around measured step 24 (~`0.727s`).
- Notes:
  - Larger batch size did not improve throughput on A10 and introduced allocator retry pressure.
  - This reinforces that the useful A10 knee is below `16384` for the current benchmark path.

## Stage 5 conclusion

- Best current A10-local settings: **Attempt 017 / batch_size=8192 / num_workers=8 / prefetch_factor=6**.
- Evidence: Attempt 017 reached `20,170.97 samples/s` at `peak_cuda_reserved_gib=13.95`, slightly beating Attempt 016 (`20,021.47`) while cutting reserved memory sharply; Attempt 018 fell to `19,643.31 samples/s` and triggered `allocation_retries=1`.
- Interpretation: the A10 branch is compute-dominated at these tuned loader settings, so pushing batch size higher does not max the card in a useful way; it just raises memory pressure without improving throughput.
- Practical takeaway: the global ranking remains **B200 Attempt 013 > A10 Attempt 017 > L4 Attempt 015**. If the goal is absolute throughput, stay on B200 at `12288 / 8 / 6`; if the goal is cheaper-card efficiency, the best validated A10 point is `8192 / 8 / 6`.

## Stage 6 — B200 high-pressure falsification

- Purpose: test whether the current single-instance benchmark path can materially exceed Attempt 013 by pushing batch pressure far above the practical knee while holding the best loader settings fixed.
- Rationale: Oracle judged that blindly chasing ~120 GiB reserved VRAM is probably wasteful on this benchmark path, but recommended one clean larger-batch falsification before concluding the path is intrinsically too small. Stage 6 therefore uses only B200 and changes only `FUSION_PROBE_BATCH_SIZE`, keeping `FUSION_PROBE_NUM_WORKERS=8` and `FUSION_PROBE_PREFETCH_FACTOR=6` fixed.
- Success criterion: a Stage 6 run only counts as a real improvement if it materially beats Attempt 013 (`65,383.72 samples/s`) rather than merely inflating reserved VRAM or creating pathological stalls.

## Attempt 019

- Status: succeeded
- Goal: first high-pressure B200 check above the current knee to see whether reserved VRAM can approach the ~120 GiB target without collapsing throughput
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 28cd9dc9`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_BATCH_SIZE=65536 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-xPA8eU82eXsClAm36Bh6Xc`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=65536`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful high-pressure B200 run that materially raised memory but did not beat the current best point.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `21.97s` at `1.25 GiB/s`.
  - Throughput: `46,740.17 samples/s` and `0.71 batches/s` across `32` measured batches (`2,097,152` measured samples).
  - Timing split: `mean_data_time_s=0.5920`, `mean_step_time_s=0.8101`, `mean_cuda_step_time_s=0.8060`, `data_fraction=0.4222`, `step_fraction=0.5778`, `cuda_step_fraction=0.5748`.
  - Memory: `peak_cuda_allocated_gib=74.75`, `peak_cuda_reserved_gib=77.92`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: repeated multi-second data stalls appeared throughout the run (for example ~`2.06s`, `2.72s`, `2.32s`, `2.51s`, `2.42s`, `2.39s`, `2.21s`, `2.32s`).
- Notes:
  - This run substantially increased reserved VRAM, but the throughput regressed badly versus Attempt 013.
  - It confirms that pushing batch pressure upward can inflate memory without producing a useful steady-state win.

## Attempt 020

- Status: succeeded
- Goal: second high-pressure B200 check to see whether the benchmark can push into the ~120 GiB reserved-memory regime at all
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 28cd9dc9`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_BATCH_SIZE=73728 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-QuEfymu7wIfNl00iTCkRWR`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=8`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=73728`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`

- Result summary:
  - Successful heavier-pressure B200 run that pushed reserved memory higher again, but still failed to beat the practical optimum.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `21.90s` at `1.25 GiB/s`.
  - Throughput: `44,418.54 samples/s` and `0.60 batches/s` across `32` measured batches (`2,359,296` measured samples).
  - Timing split: `mean_data_time_s=0.7493`, `mean_step_time_s=0.9106`, `mean_cuda_step_time_s=0.9060`, `data_fraction=0.4514`, `step_fraction=0.5486`, `cuda_step_fraction=0.5458`.
  - Memory: `peak_cuda_allocated_gib=84.09`, `peak_cuda_reserved_gib=90.56`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: the run showed persistent ~`2s` class loader stalls across many measured steps with only isolated tiny waits between them.
- Notes:
  - This run moved even further into the high-memory regime, but throughput fell below Attempt 019 and far below Attempt 013.
  - The benchmark still did not approach the requested ~`120 GiB` reserved-memory region, which strengthens the case that the single-instance TeacherNet path is intrinsically too small to justify further batch-only escalation.

## Stage 6 conclusion

- Oracle's falsification guidance held: larger-batch B200 runs increased reserved VRAM sharply but did **not** produce a better benchmark point.
- Attempt 019 reached `peak_cuda_reserved_gib=77.92` at `46,740.17 samples/s`; Attempt 020 reached `peak_cuda_reserved_gib=90.56` at `44,418.54 samples/s`; both were materially worse than Attempt 013 (`65,383.72 samples/s` at `20.90 GiB`).
- Interpretation: for this single-instance benchmark path, chasing much higher B200 memory occupancy with batch size alone is not productive. The useful B200 optimum remains **Attempt 013 / batch_size=12288 / num_workers=8 / prefetch_factor=6**.
- Next step returns to the cheaper-card branch: tune the A10 locally with Attempts 017 and 018 rather than continue escalating B200 pressure.

## Stage 7 — B200 host-input maximization

- Purpose: maximize **input throughput** for the current single-instance B200 benchmark path rather than chasing raw VRAM occupancy.
- Rationale: the benchmark code and the recorded attempts show that larger batch sizes pushed `data_wait_s` and `data_fraction` sharply upward without allocator retries or OOMs, which means the next limiter is the host-side feeder path (`np.memmap` -> per-sample CPU copies -> DataLoader workers -> pinned-memory handoff). The current best point remains Attempt 013 (`B200 / batch_size=12288 / num_workers=8 / prefetch_factor=6`), so Stage 7 keeps that compute point fixed and tests whether additional host resources can feed it faster.
- Success criterion: a Stage 7 run only counts as a real improvement if it materially beats Attempt 013 (`65,383.72 samples/s`) while keeping the same basic memory envelope (`peak_cuda_reserved_gib` roughly in the current ~`20.90 GiB` class) and without introducing allocator retries or pathological fetch stalls.

## Attempt 021

- Status: succeeded
- Goal: pure host-capacity check by increasing container CPU while holding the proven loader shape fixed
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 93411f13`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-RRZUVZHy1cXhwlW9uStCTm`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=12`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful CPU-only feeder expansion run, but it did **not** beat Attempt 013.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `18.61s` at `1.47 GiB/s`.
  - Throughput: `60,766.88 samples/s` and `4.95 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0265`, `mean_step_time_s=0.1757`, `mean_cuda_step_time_s=0.1749`, `data_fraction=0.1312`, `step_fraction=0.8688`, `cuda_step_fraction=0.8648`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: mostly tiny waits, but one large fetch stall appeared around measured step 13 (~`0.846s`).
- Notes:
  - This isolates whether the current best run is starved by host CPU availability even before raising worker count.
  - CPU `12` with workers still fixed at `8` was not enough to produce a real improvement.

## Attempt 022

- Status: succeeded
- Goal: second pure host-capacity check with a larger CPU bump while keeping the same loader shape fixed
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 93411f13`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=16 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=8 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-ps6DN9oadvpWztADqzCVYf`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=16`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=8`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful larger CPU-only feeder expansion run that nearly matched, but still did **not** beat, Attempt 013.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `19.69s` at `1.39 GiB/s`.
  - Throughput: `64,578.52 samples/s` and `5.26 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0175`, `mean_step_time_s=0.1728`, `mean_cuda_step_time_s=0.1720`, `data_fraction=0.0918`, `step_fraction=0.9082`, `cuda_step_fraction=0.9040`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: mostly tiny waits, with one notable stall around measured step 13 (~`0.556s`).
- Notes:
  - This tests whether the feeder still improves with substantially more host CPU even when worker count stays at 8.
  - CPU `16` helped versus CPU `12`, but pure CPU expansion alone still was not the new optimum.

## Attempt 023

- Status: succeeded
- Goal: coupled host-feeder test with more CPU and a modest worker bump
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 93411f13`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=10 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-RB59OD8M4VtHA8srrOcpm6`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=12`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=10`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful coupled feeder-expansion run and the **best overall result so far**.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `18.13s` at `1.51 GiB/s`.
  - Throughput: `66,345.35 samples/s` and `5.40 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0119`, `mean_step_time_s=0.1733`, `mean_cuda_step_time_s=0.1725`, `data_fraction=0.0644`, `step_fraction=0.9356`, `cuda_step_fraction=0.9313`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: mostly tiny waits, with one notable stall around measured step 13 (~`0.348s`) and a smaller bump around step 15 (~`0.031s`).
- Notes:
  - This is the first true feeder-expansion run: more CPU and more workers while holding batch/prefetch fixed at the known-good compute point.
  - It materially beat Attempt 013 (`66,345.35` vs `65,383.72 samples/s`) while staying in the same reserved-memory class, so this is a real productive improvement.

## Attempt 024

- Status: succeeded
- Goal: larger coupled host-feeder expansion to test the current practical ceiling of the input path
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 93411f13`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=16 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=12 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-peL84oUNuUXOgfUF4tQtvQ`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=16`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=12`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful upper feeder-expansion test, but it regressed materially from the best point.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `22.82s` at `1.20 GiB/s`.
  - Throughput: `60,467.87 samples/s` and `4.92 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0273`, `mean_step_time_s=0.1759`, `mean_cuda_step_time_s=0.1751`, `data_fraction=0.1346`, `step_fraction=0.8654`, `cuda_step_fraction=0.8617`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: mostly tiny waits, but one large fetch stall appeared around measured step 13 (~`0.872s`).
- Notes:
  - This is the upper host-feeder pressure test before any code-level dataset/collation optimization work.
  - Pushing both CPU and workers higher overshot the useful point; bigger host-side expansion is not automatically better.

## Stage 7 conclusion

- Best current settings: **Attempt 023 / B200 / batch_size=12288 / cpu=12 / num_workers=10 / prefetch_factor=6**.
- Evidence: it delivered the highest throughput so far (`66,345.35 samples/s`) while keeping `peak_cuda_reserved_gib=20.90`, matching the favorable memory class of Attempt 013.
- Interpretation: the feeder path was not "bad" or broken; it had a real but limited optimization headroom. A modest host-side expansion (`cpu 8 -> 12`, `workers 8 -> 10`) improved throughput productively, while larger expansions (Attempts 021, 022, 024) mostly added stalls or overhead without creating a new winner.
- Practical takeaway: the current single-instance optimum is now Attempt 023, not Attempt 013. If we continue from here, the next step should be a narrow refinement around the new feeder point rather than another broad escalation.

## Attempt 025

- Status: succeeded
- Goal: re-run the current best single-instance B200 baseline after eliminating per-sample eager copies in the dataset path
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 5bf1902c`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=10 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-nHRB5MUTKeAX9OAypIMxjS`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=12`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=10`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful baseline rerun after the loader copy-elimination refactor.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `22.18s` at `1.24 GiB/s`.
  - Throughput: `65,733.29 samples/s` and `5.35 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0137`, `mean_step_time_s=0.1733`, `mean_cuda_step_time_s=0.1728`, `data_fraction=0.0731`, `step_fraction=0.9269`, `cuda_step_fraction=0.9242`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: almost all fetch waits stayed tiny, with one notable stall around measured step 13 (~`0.362s`) and a smaller bump around step 15 (~`0.072s`).
- Notes:
  - This rerun isolates the code change that removed per-sample eager `np.array(..., copy=True)` work and moved materialization to a batched collate step.
  - Result: the refactor did **not** beat Attempt 023. Throughput fell slightly (`65,733.29` vs `66,345.35 samples/s`), while the timing and memory class remained almost unchanged.
  - Interpretation: this specific copy-elimination change was directionally valid and safe, but by itself it was not enough to create a new benchmark winner. The next standard move, if we continue optimizing the input path, should be to test whether the `/data -> /tmp` staging step is still necessary or to try a more batched host conversion path.

## Attempt 026

- Status: succeeded
- Goal: rerun the current best B200 baseline with direct mounted-volume reads instead of `/tmp` staging
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 0a26b064`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=10 FUSION_PROBE_PREFETCH_FACTOR=6 FUSION_PROBE_STAGE_TO_TMP=false python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-CsspzZrdcq7zVY5mO53fYA`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=12`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=10`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_STAGE_TO_TMP=false`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful direct-mounted-volume benchmark with staging explicitly disabled.
  - Copy stage: skipped; benchmark read directly from `/data/training_data.bin` (`copy_time_s=0`, `stage_to_tmp=false`).
  - Throughput: `61,307.82 samples/s` and `4.99 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0246`, `mean_step_time_s=0.1759`, `mean_cuda_step_time_s=0.1753`, `data_fraction=0.1226`, `step_fraction=0.8774`, `cuda_step_fraction=0.8747`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: almost all fetch waits stayed tiny, but there were two notable stalls around measured steps 13-14 (~`0.384s` and `0.398s`).
- Notes:
  - This run isolates the storage-path question by holding the proven Attempt 023 compute/loader settings fixed and disabling `/tmp` staging.
  - Result: direct mounted-volume reads were materially worse than staged local reads (`61,307.82` vs `66,345.35 samples/s`) while keeping the same memory class.
  - Interpretation: for this benchmark path, `/data -> /tmp` staging remains the better standard baseline. Direct reads are valid but slower, so future optimization work should keep `/tmp` staging and focus instead on a more batched host conversion path if further input-side gains are needed.

## Post-Stage 7 storage conclusion

- The direct mounted-volume validation did **not** replace the current best baseline.
- Best current settings remain **Attempt 023 / B200 / batch_size=12288 / cpu=12 / num_workers=10 / prefetch_factor=6 / stage_to_tmp=true**.
- Evidence: Attempt 026 proved direct reads from `/data/training_data.bin` are slower than the staged `/tmp/training_data.bin` path at the same compute point, so `/tmp` staging should remain the default for this benchmark.

## Attempt 027

- Status: succeeded
- Goal: test the plain-PyTorch `dataset_getitems` batched fetch path against the current best B200 baseline
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 5c207f23`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=10 FUSION_PROBE_PREFETCH_FACTOR=6 python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-d0riThtDCsLGLOX7eTicl9`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=12`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_STAGE_TO_TMP=true`
  - `FUSION_PROBE_FETCH_MODE=dataset_getitems` (implicit in the then-current code state)
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=10`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful benchmark of the first batch-aware plain-PyTorch candidate.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `17.60s` at `1.56 GiB/s`.
  - Throughput: `39,614.33 samples/s` and `3.22 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.1341`, `mean_step_time_s=0.1761`, `mean_cuda_step_time_s=0.1755`, `data_fraction=0.4322`, `step_fraction=0.5678`, `cuda_step_fraction=0.5658`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: repeated large fetch stalls after warmup (~`1.537s`, `0.848s`, `0.537s`, `0.453s`, plus other smaller spikes) while CUDA step time stayed close to the winning baseline.
- Notes:
  - This path followed PyTorch's standard `Dataset.__getitems__(indices)` optimization route, but for this benchmark it materially increased input wait.
  - Result: it was **not** a winner. Throughput collapsed far below Attempt 023 while staying in the same memory class, so this specific batched-fetch path is worse than the current baseline.

## Attempt 028

- Status: succeeded
- Goal: test the plain-PyTorch `sampler_batches` path (disabled automatic batching + sampler-yielded batch indices) against the current best B200 baseline
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 5c207f23`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 FUSION_PROBE_BATCH_SIZE=12288 FUSION_PROBE_NUM_WORKERS=10 FUSION_PROBE_PREFETCH_FACTOR=6 FUSION_PROBE_FETCH_MODE=sampler_batches python3 -m modal run -m training.scripts.gpu_saturation_probe::run_once`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-EAk2OHd9sz6HP5aV42a9we`
- Runtime settings:
  - `FUSION_PROBE_GPU=B200`
  - `FUSION_PROBE_CPU=12`
  - `FUSION_PROBE_MEMORY_MIB=40960`
  - `FUSION_PROBE_TIMEOUT_S=3600`
  - `FUSION_PROBE_SAMPLE_COUNT=262144`
  - `FUSION_PROBE_BATCH_SIZE=12288`
  - `FUSION_PROBE_STAGE_TO_TMP=true`
  - `FUSION_PROBE_FETCH_MODE=sampler_batches`
  - `FUSION_PROBE_WARMUP_STEPS=8`
  - `FUSION_PROBE_MEASURE_STEPS=32`
  - `FUSION_PROBE_NUM_WORKERS=10`
  - `FUSION_PROBE_PREFETCH_FACTOR=6`
  - `FUSION_PROBE_LR=1e-3`
  - `FUSION_PROBE_WEIGHT_DECAY=1e-4`
  - `FUSION_PROBE_USE_BF16=true`
  - `FUSION_PROBE_PIN_MEMORY=true`
  - `FUSION_PROBE_TELEMETRY_ENABLED=true`
  - `FUSION_PROBE_TELEMETRY_RECORD_PER_STEP=true`
- Result summary:
  - Successful benchmark of the second batch-aware plain-PyTorch candidate.
  - Copy stage: `/data/training_data.bin` -> `/tmp/training_data.bin` took `18.87s` at `1.45 GiB/s`.
  - Throughput: `60,660.17 samples/s` and `4.94 batches/s` across `32` measured batches (`393,216` measured samples).
  - Timing split: `mean_data_time_s=0.0292`, `mean_step_time_s=0.1734`, `mean_cuda_step_time_s=0.1728`, `data_fraction=0.1442`, `step_fraction=0.8558`, `cuda_step_fraction=0.8531`.
  - Memory: `peak_cuda_allocated_gib=14.04`, `peak_cuda_reserved_gib=20.90`; allocator summary reported `allocation_retries=0` and `oom_events=0`.
  - Telemetry: most waits were tiny, but one major stall appeared around measured step 13 (~`0.929s`) and smaller bumps also appeared around steps 9, 11, 14, and 20.
- Notes:
  - This path used the PyTorch-documented `batch_size=None` + sampler-yielded batch-indices approach to bypass normal automatic batching.
  - Result: it was better than Attempt 027 but still **not** a winner. Throughput stayed materially below Attempt 023 (`60,660.17` vs `66,345.35 samples/s`) at the same memory class.

## Batched host-conversion conclusion

- Two standard plain-PyTorch batched fetch paths were tested after the copy-elimination and storage-path work: **Attempt 027 (`dataset_getitems`)** and **Attempt 028 (`sampler_batches`)**.
- Neither beat the current best baseline. Attempt 027 regressed badly due to large steady-state fetch stalls, and Attempt 028 improved over 027 but still remained clearly below Attempt 023.
- Best current settings therefore remain **Attempt 023 / B200 / batch_size=12288 / cpu=12 / num_workers=10 / prefetch_factor=6 / stage_to_tmp=true**.
- Practical takeaway: the current best option is still the existing Attempt 023 baseline. The two most standard plain-PyTorch batched-fetch candidates have now been tested and neither replaced it.

## Attempt 029

- Status: succeeded
- Goal: measure total useful work on one B200 by packing **2** independent benchmark instances using the current best per-instance settings
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun a29c9364`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 12288 --fetch-mode sample_collate --num-workers 10 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-JWWHoUslJeLOryxipVqZRH`
- Runtime settings:
  - Per packed instance: `B200`, `cpu=12`, `batch_size=12288`, `fetch_mode=sample_collate`, `stage_to_tmp=true`, `num_workers=10`, `prefetch_factor=6`, `warmup_steps=8`, `measure_steps=32`, bf16 on, pin_memory on, telemetry on.
- Result summary:
  - Successful **2-way packed** benchmark on one B200 using the current best single-instance settings.
  - Aggregate throughput: `73,548.54 samples/s` and `5.9854 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=27.49`, `peak_cuda_reserved_gib=32.11`.
  - Mean per-instance loss: `-0.04877`.
  - Per-instance throughput split: one run reached `40,038.85 samples/s` (`data_fraction=0.0584`), the other `33,509.69 samples/s` (`data_fraction=0.0562`).
  - Telemetry on both packed instances still showed mostly step-dominated execution, with moderate but not catastrophic fetch stalls.
- Notes:
  - This is the first result that **materially beats** the single-instance ceiling in total useful work per B200.
  - Aggregate throughput improved by roughly **10.9%** over Attempt 023 (`73.55k` vs `66.35k samples/s`), even though each individual job slowed down relative to running alone.
  - Practical interpretation: 2-way packing appears to be the first real path past the single-instance ceiling for this workload.

## Attempt 030

- Status: succeeded
- Goal: measure total useful work on one B200 by packing **3** independent benchmark instances using the current best per-instance settings
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun a29c9364`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 3 --batch-size 12288 --fetch-mode sample_collate --num-workers 10 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-Yl15ra4RKIA23TYr8GCKBm`
- Runtime settings:
  - Per packed instance: `B200`, `cpu=12`, `batch_size=12288`, `fetch_mode=sample_collate`, `stage_to_tmp=true`, `num_workers=10`, `prefetch_factor=6`, `warmup_steps=8`, `measure_steps=32`, bf16 on, pin_memory on, telemetry on.
- Result summary:
  - Successful **3-way packed** benchmark on one B200 using the same per-instance settings.
  - Aggregate throughput: `68,707.56 samples/s` and `5.5914 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=40.94`, `peak_cuda_reserved_gib=46.23`.
  - Mean per-instance loss: `-0.04959`.
  - Per-instance throughput clustered around `22.86k`, `22.90k`, and `22.95k samples/s`, with all three runs remaining mostly step-dominated but clearly slower than the 2-way packed case.
- Notes:
  - 3-way packing still beats the original single-instance Attempt 023 in aggregate throughput, but it loses clearly to the 2-way packed result.
  - Compared to Attempt 029, it adds much more per-instance memory pressure (`46.23 GiB` reserved vs `32.11 GiB`) for a lower total return.
  - Practical interpretation: 3-way packing overshoots the useful point for this benchmark path.

## Packed B200 conclusion

- The current fastest validated path is now **Attempt 029**, not Attempt 023.
- Best known total useful work per B200 is **2 packed instances** of the current best single-instance configuration: `B200 / cpu=12 / batch_size=12288 / num_workers=10 / prefetch_factor=6 / stage_to_tmp=true / fetch_mode=sample_collate`.
- Evidence: Attempt 029 reached `73,548.54 samples/s` aggregate, materially above Attempt 023's `66,345.35 samples/s` and also above Attempt 030's `68,707.56 samples/s`.
- Practical takeaway: if the objective is **maximum total work per B200**, use the packed 2-instance path. If the objective is **best per-instance speed**, Attempt 023 remains the single-instance winner.

## Attempt 031

- Status: succeeded
- Goal: tune the winning 2-way packed shape by increasing shared container CPU from 12 -> 16 while keeping per-instance loader shape fixed (`num_workers=10`, `prefetch_factor=6`)
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 3901581d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=16 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 12288 --fetch-mode sample_collate --num-workers 10 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-ZDWj8ZL4fp4NK3gDkAxzrB`
- Result summary:
  - Aggregate throughput: `73,421.55 samples/s` and `5.9751 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=27.49`, `peak_cuda_reserved_gib=32.11`.
  - Mean per-instance loss: `-0.04925`.
- Notes:
  - This came very close to Attempt 029 but did **not** beat it.
  - Increasing shared CPU from 12 to 16 on the same 2-way packed shape did not create a new winner.

## Attempt 032

- Status: succeeded
- Goal: tune the winning 2-way packed shape by reducing per-instance workers from 10 -> 8 at the same shared CPU and prefetch
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 3901581d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 12288 --fetch-mode sample_collate --num-workers 8 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-5d9zbXDmoInhDVfnYeawPO`
- Result summary:
  - Aggregate throughput: `58,190.60 samples/s` and `4.7356 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=27.49`, `peak_cuda_reserved_gib=32.11`.
  - Per-instance `data_fraction` rose to roughly `0.19` and `0.20` with repeated multi-second stalls.
- Notes:
  - Lowering workers under 2-way packing was clearly worse.
  - This variant is strongly dominated by Attempt 029.

## Attempt 033

- Status: succeeded
- Goal: test whether higher shared CPU can rescue the lower-worker 2-way packed shape (`cpu=16`, `num_workers=8`)
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 3901581d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=16 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 12288 --fetch-mode sample_collate --num-workers 8 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-YlTTJ67wUYcc9HcbF5mfSu`
- Result summary:
  - Aggregate throughput: `46,566.23 samples/s` and `3.7896 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=27.49`, `peak_cuda_reserved_gib=32.69`.
  - One packed instance showed `data_fraction≈0.4706`, the other `≈0.4766`, meaning both spent nearly half of measured time waiting on input.
- Notes:
  - This was the worst 2-way packed tuning variant in the sweep.
  - Raising CPU did not compensate for the lower-worker configuration.

## Attempt 034

- Status: succeeded
- Goal: tune queue depth on the winning 2-way packed shape by lowering per-instance `prefetch_factor` from 6 -> 4
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 3901581d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 12288 --fetch-mode sample_collate --num-workers 10 --prefetch-factor 4 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-P9GWhBv7NKxLaRbe0XUv2x`
- Result summary:
  - Aggregate throughput: `68,369.66 samples/s` and `5.5639 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=27.49`, `peak_cuda_reserved_gib=30.92`.
  - Per-instance data fractions were low (`0.0451` and `0.0536`), but aggregate throughput still lagged Attempt 029.
- Notes:
  - Lower prefetch reduced reserved memory modestly but did not improve total useful work.
  - Attempt 029 remains better.

## Attempt 035

- Status: succeeded
- Goal: retune 2-way packed aggregate throughput by lowering per-instance batch size from `12288` -> `8192`
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 3901581d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 8192 --fetch-mode sample_collate --num-workers 10 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-NIAyy5wQi4EA1SFv64w6eD`
- Result summary:
  - Aggregate throughput: `65,575.40 samples/s` and `8.0048 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=18.35`, `peak_cuda_reserved_gib=21.53`.
  - Aggregate throughput fell below both Attempt 029 and even the single-instance Attempt 023.
- Notes:
  - Smaller packed batches reduced memory sharply but gave up too much total useful work.
  - `12288` remains better than `8192` for the packed 2-way shape.

## Attempt 036

- Status: succeeded
- Goal: retune 2-way packed aggregate throughput by raising per-instance batch size from `12288` -> `16384`
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 3901581d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 16384 --fetch-mode sample_collate --num-workers 10 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-jTKEvuDBiw7MPgU5BVvegw`
- Result summary:
  - Aggregate throughput: `44,201.04 samples/s` and `2.6978 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=36.64`, `peak_cuda_reserved_gib=43.55`.
  - Per-instance `data_fraction` exploded to roughly `0.54` and `0.53`, with repeated multi-second stalls.
- Notes:
  - Larger packed batches are clearly pathological for this 2-way shape.
  - This is a severe regression and confirms the packed batch knee remains below `16384`.

## Attempt 037

- Status: succeeded
- Goal: perform the last meaningful PyTorch-side A/B on the packed winner by disabling `pin_memory`
- Bookmark: `gpu-saturation-probe`
- Working-copy commit: `ywlqmvun 3901581d`
- Parent commit: `lpnqolno 8b064030` (`main`)
- Command:
  - `FUSION_PROBE_GPU=B200 FUSION_PROBE_CPU=12 FUSION_PROBE_PIN_MEMORY=false python3 -m modal run -m training.scripts.gpu_saturation_probe::packed_run --instance-counts 2 --batch-size 12288 --fetch-mode sample_collate --num-workers 10 --prefetch-factor 6 --warmup-steps 8 --measure-steps 32`
- Modal run URL:
  - `https://modal.com/apps/gladdonilli/main/ap-F7KFuWJZeLApMSpjFepss1`
- Result summary:
  - Aggregate throughput: `67,214.90 samples/s` and `5.4700 batches/s`.
  - Max per-instance memory: `peak_cuda_allocated_gib=27.49`, `peak_cuda_reserved_gib=32.57`.
  - Per-instance data fractions were `0.1391` and `0.1170`, both higher than the packed winner.
- Notes:
  - Disabling pinned memory regressed aggregate throughput relative to Attempt 029.
  - This confirms `pin_memory=true` remains the better packed default for this workload.

## Packed 2-way tuning conclusion

- The relevant packed-tuning sweep is now complete for the current benchmark shape: shared CPU, per-instance worker count, prefetch queue depth, packed batch size, and `pin_memory` have all been tested around the 2-way packed winner.
- None of Attempts **031–037** beat **Attempt 029**.
- Best current total useful work per B200 therefore remains **Attempt 029**: `2 packed instances / B200 / cpu=12 / batch_size=12288 / num_workers=10 / prefetch_factor=6 / pin_memory=true / stage_to_tmp=true / fetch_mode=sample_collate` at `73,548.54 aggregate samples/s`.
- Practical takeaway: we have now tuned the main still-plausible packed parameters around the winning 2-way shape, and Attempt 029 remains the fastest validated option.
