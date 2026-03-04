# GPU Decisions, Settings, Errors, and Fixes (ULW Summary)

Last updated: 2026-03-07 (UTC)

## 1) Executive Summary

- We restored the richer real training pipeline onto `main` and locked the first real B200 training candidate to the best validated single-instance benchmark shape.
- The pipeline is now profile-driven (`FUSION_GPU_PROFILE`) with separate configs for `l4`, `a10`, `b200` (archived), and `t4`.
- Reliability hardening was applied for overnight runs: larger teacher timeout on L4/A10, OOM-to-pruned trials, per-worker failure tolerance, and shape-stability improvements.
- A critical operational issue was identified: Modal volume currently contains `training_data.bin` at **11.6 GiB**, while local rebuilt data is **27.44 GiB** (8,575,773 samples). Runs using the current volume file train on the smaller dataset unless volume is refreshed.
- Benchmark-informed real-training starting point on B200 is now: `cpu=12`, `teacher/student_num_workers=10`, `prefetch_factor=6`, `teacher_batch_sizes=(12288,)`, `student_batch_size=12288`, `precision=bf16-mixed`, `/tmp` staging enabled.

---

## 2) GPU Strategy Decisions (What and Why)

## Final default strategy

- **Default GPU profile:** `l4`
- **Reason:** best cost-efficiency for this model size (high utilization with significantly lower hourly cost than B200).

## Why B200 became archived for default use

- B200 delivered high absolute throughput but was often over-provisioned for this model class.
- Cost per completed job was not favorable for routine runs compared to L4 horizontal scaling.

## When to switch off L4

- Switch to H100/B200 only when strict wall-clock SLA matters more than cost efficiency.
- Do **not** switch only because utilization is high; high stable utilization is generally desirable when training is progressing.

---

## 3) Current Profile Config (Source of Truth)

File: `training/scripts/gpu_profiles.py`

### L4 (default)
- GPU: `L4`
- CPU: `6`
- Memory: `32768 MiB`
- Teacher timeout: `21600s`
- Student timeout: `7200s`
- Precision: `bf16-mixed`
- Teacher batch choices: `(2048, 4096, 8192, 16384)`
- Student batch size: `16384`
- DataLoader workers: teacher/student = `4/4`
- Prefetch factor: `4`
- Parallel workers cap: `max_parallel_workers=30`
- Default launch shape: `default_parallel_workers=15`, `trials_per_worker=4`
- Epoch defaults: teacher `50`, student `100`

### A10
- Same logical settings as L4 for stability; fallback when L4 supply is constrained.
- Teacher timeout also raised to `21600s`.

### B200 (tuned real-training candidate)
- Premium wall-clock profile for the first integrated real training runs.
- Resource envelope: `gpu=B200`, `cpu=12`, `memory=40960 MiB`.
- Teacher batch choices: `(12288,)`
- Student batch size: `12288`
- DataLoader workers: teacher/student = `10/10`
- Prefetch factor: `6`
- Precision: `bf16-mixed`

### T4 (budget fallback)
- Precision: `16-mixed` (no native bf16 support).

---

## 4) Modal Orchestration and Runtime Behavior

File: `training/scripts/modal_app.py`

- Active profile loaded from `FUSION_GPU_PROFILE`.
- Worker count enforcement:
  - `num_workers = min(num_workers, PROFILE.settings.max_parallel_workers)`
  - Now allows up to 30 workers due to profile cap update.
- Pipeline orchestrator is `run_pipeline` (`@app.function`), launcher is `launch_pipeline` (`@app.local_entrypoint`).
- `launch_pipeline` now submits asynchronously via `run_pipeline.spawn(...)` (non-blocking local launcher).

---

## 5) Data Path and Throughput Optimizations

## Volume → NVMe copy strategy

- Training data is copied from Modal Volume (`/data/training_data.bin`) to local NVMe (`/tmp/training_data.bin`) before training/distillation.
- Rationale: one-time copy cost usually repays quickly over many epochs due faster local I/O.

## Copy reuse optimization

- Added size-based reuse check in both teacher and distill paths:
  - If `/tmp/training_data.bin` exists and size matches source, skip recopy.
- Logs now report `Ready ... GB at /tmp` and show reuse when applicable.

---

## 6) Errors Encountered and How They Were Resolved

## A) OOM from oversized teacher batch sampling (L4)
- Symptom: CUDA OOM during teacher trials when larger candidates were sampled.
- Fixes:
  1. Narrowed L4/A10 teacher batch search to `(2048, 4096, 8192, 16384)`.
  2. Wrapped `trainer.fit()` in Optuna objective to catch `torch.OutOfMemoryError` and raise `optuna.TrialPruned` instead of crashing worker.
- File: `training/scripts/optuna_objective.py`

## B) Worker timeout cancellations on overnight runs
- Symptom: `Task's current input ... hit its timeout of 10800s` for teacher workers running multiple sequential trials (`n_trials=4`).
- Fix: raised L4/A10 `teacher_timeout_s` from `10800` to `21600`.
- File: `training/scripts/gpu_profiles.py`

## C) Pipeline fragility: single worker failure aborted whole run
- Symptom: all-or-nothing result collection (`[h.get() for h in handles]`) caused full pipeline failure when one worker died/preempted.
- Fix: changed to per-worker `try/except`, collect successes, track `worker_failures`, continue pipeline when at least one worker succeeds.
- File: `training/scripts/modal_app.py`

## D) TorchDynamo recompilation churn due partial batch shapes
- Symptom: warnings like size mismatch (`expected 2048, actual 1452`) and frequent recompiles.
- Fix: set `drop_last=True` in validation dataloader to keep shape stability.
- File: `training/models/lit_module.py`

## E) Local disconnect/interrupt lifecycle issues
- Symptom: attached runs stop on local interruption/disconnect (`KeyboardInterrupt`, app stop messages).
- Mitigation:
  - `launch_pipeline` changed to async submit pattern.

## F) Surrogate benchmark findings promoted into real training defaults (B200)
- The saturation benchmark is not the real training loop, but it was sufficient to choose the first integrated infra settings.
- Validated carryovers:
  1. Keep `/data -> /tmp` staging enabled (direct mounted reads were slower).
  2. Keep `pin_memory=True` and `bf16-mixed`.
  3. Use the best single-instance feeder shape as the first real-training candidate: `cpu=12`, `num_workers=10`, `prefetch_factor=6`, `batch_size=12288`.
  4. Do **not** keep the broad B200 batch search space from the older profile for the first integrated run; lock to `12288` first and re-measure on the real objective.
  - Prefer detached command for disconnect-safe execution when acceptable.
- File: `training/scripts/modal_app.py`

## F) Checkpoint naming/path correctness
- Symptom class: slash (`/`) in checkpoint metric template created nested paths and broke lookup/export.
- Fix: switched filename metric key to underscore alias (`val_total_loss`) and robust path normalization with `realpath/relpath` handling for Modal symlinked volumes.

## G) Export key mismatch with torch.compile wrappers
- Symptom: `_orig_mod.` and module-prefix differences in checkpoint keys.
- Fix: ordered prefix stripping in export logic to handle wrapped/unwrapped variants.

---

## 7) Warning Triage (Benign vs Actionable)

## Usually benign/noise in current context
- `LeafSpec` deprecation warning from Lightning internals.
- `Not enough SMs to use max_autotune_gemm mode` (TorchInductor falls back; not a hard failure by itself).
- `Checkpoint directory exists and is not empty` on resumed worker trial dirs.

## Actionable/fatal indicators
- `OutOfMemoryError` (mitigated by OOM→pruned trial handling).
- `hit its timeout of ...` (mitigated by timeout increase).
- `No successful teacher workers` (pipeline now returns structured failure with worker errors).

---

## 8) Dataset Size Reality Check (Critical)

- Local rebuilt dataset:
  - Path: `training/training_data.bin`
  - Size: **29,466,356,028 bytes** (**27.44 GiB**)
  - Samples: **8,575,773** (`3436 bytes/sample`)
- Modal volume dataset currently observed:
  - Volume: `fusion-training-data`
  - Path: `/training_data.bin`
  - Size: **11.6 GiB**

Implication:
- Any run reading `/data/training_data.bin` from current volume trains on the **smaller dataset** until volume upload is refreshed.

---

## 9) Practical Launch Guidance (Current)

Attached run (user-preferred when avoiding detach behavior):

```bash
FUSION_GPU_PROFILE=l4 python3 -m modal run training/scripts/modal_app.py::run_pipeline \
  --num-workers 25 --trials-per-worker 2 --teacher-epochs 50 --student-epochs 100
```

Before launch, ensure volume data freshness if full dataset is intended:

```bash
python3 -m modal run training/scripts/modal_app.py::upload_data --local-path training/training_data.bin
```

---

## 10) What Was Improved Overall

- GPU selection moved to cost-aware, profile-driven control.
- Timeout and fault handling now match long-running multi-worker reality.
- Data pipeline improved with copy reuse and static-shape validation batching.
- Pipeline now degrades gracefully on partial worker failures instead of all-or-nothing abort.
- Known state mismatch (11.6 GiB volume vs 27.44 GiB local dataset) has been identified clearly for operational correction before next full run.
