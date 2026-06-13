# Modal Failures, Tips, and Tricks

Focused notes for the active Mosaic policy/value path so we do not repeat the same Modal and artifact mistakes.

## Scope

This document is about the current policy/value training path, not the legacy teacher/student loop:

- local Phase 0: `/home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine/training/scripts/preprocess_replays.py`
- local Phase 1: `/home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine/training/scripts/generate_policy_value_labels.py`
- remote train: `/home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine/training/scripts/modal_app.py::train_policy_value_remote`
- remote full launcher: `/home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine/training/scripts/modal_app.py::launch_policy_value_pipeline`

Required artifact set for any dataset `<base>`:

- `<base>`
- `<base>.metadata.json`
- `<base>.groups.u64`
- `<base>.policy_value.requests.jsonl`
- `<base>.policy_value.jsonl`
- `<base>.policy_value.metadata.json`

## Durable failures we hit

### 1. Modal import fallback broke inside the container

Symptom:

- `ImportError: attempted relative import with no known parent package`
- followed by `ModuleNotFoundError: No module named 'scripts'`

Root cause:

- Modal executes `/root/modal_app.py` outside normal package context.
- Any surviving fallback import like `from scripts...` fails.
- Fixing only the header is not enough; inner lazy imports must also be package-correct.

Durable fix:

- In `training/scripts/modal_app.py`, stop using `scripts.*` fallback imports.
- Use `training.scripts.*` or `import_module("training.scripts....")` after adding repo root to `sys.path`.
- Treat **every** fallback import in `modal_app.py` as part of the same bug surface.

Rule:

- If a Modal function works locally but dies in container startup, grep `modal_app.py` for `from scripts.` first.

### 2. `batch_upload()` failed with `seek of closed file`

Symptom:

- `ValueError: seek of closed file`

Root cause:

- `upload_data()` and `_upload_policy_value_artifacts()` passed open file handles into `data_vol.batch_upload()`.
- Modal consumed them after the file objects had already been closed by the local loop/context.

Durable fix:

- Pass filesystem paths, not open handles:
  - `batch.put_file(str(path), remote_name)`

Rule:

- For Modal batch upload, prefer path-based uploads over temporary file objects.

### 3. Existence-only artifact checks caused false success

Symptom:

- prep printed `Policy/value artifacts ready.` even when artifacts were empty or semantically invalid.

Root cause:

- the old path only checked whether files existed.

Durable fix:

- `training/scripts/policy_value_pipeline.py` now uses semantic readiness/validation:
  - positive `sample_count`
  - dataset byte size matches `sample_count * BYTES_PER_SAMPLE`
  - `.groups.u64` size matches sample count
  - request line count matches sample count
  - label line count matches sample count
  - policy/value metadata sample count matches dataset sample count

Rule:

- Never trust file presence alone for Phase 0/1 artifacts.

### 4. Full-corpus preprocessing was fixed, but full-corpus label generation is still not done

What is healthy now:

- optimized full-corpus Phase 0 run completed over all 5,969 replays
- valid Phase 0 outputs were produced
- real small and medium subsets completed full Phase 1 successfully
- real remote A10 training path completed successfully on a valid two-group subset

What is still blocked:

- the full-corpus Rust label pass is not yet completing end-to-end
- observed state during a real full-corpus attempt:
  - dataset metadata sample count was large and valid
  - request sidecar had matching nonblank line count
  - policy/value metadata still reported `sample_count: 0`
  - label file only had a small fraction of expected lines

Interpretation:

- current blocker moved from preprocessing to full-scale Phase 1 label generation
- do not treat partial label output as benchmark-ready data

### 5. Tiny datasets can fail training for reasons unrelated to Modal

Symptom:

- `Early stopping conditioned on metric 'val/total_loss' which is not available`
- Lightning warning that validation dataloader length is zero

Root cause:

- tiny subsets can contain only one replay group
- `PolicyValueDataModule` splits by group; one-group subsets produce no validation loader

Durable fix:

- for smoke datasets, ensure at least two unique group IDs
- one-sample and naive front-truncated subsets are not reliable training smokes

Rule:

- validate group diversity before calling a remote train smoke meaningful.

### 6. Modal checkpoint paths may resolve under `/__modal/volumes/...`, not `/checkpoints`

Symptom:

- `ValueError: '.../__modal/volumes/.../file.ckpt' is not in the subpath of '/checkpoints'`

Root cause:

- code assumed checkpoint paths always came back through the symlink mount path `/checkpoints`
- Modal returned the resolved backing volume path instead

Durable fix:

- use resolved-path normalization before `relative_to(...)`
- current helper in `modal_app.py`: `checkpoint_relative_path(...)`

Rule:

- never assume Modal volume paths come back through the human-facing mount alias.

### 7. Cargo can exist but still be missing from shell `PATH`

Symptom:

- `FileNotFoundError: [Errno 2] No such file or directory: 'cargo'`

Root cause:

- Rust toolchain existed, but `cargo` was not available in the shell PATH used by the Python wrapper.

Durable fix:

- `training/scripts/generate_policy_value_labels.py` now falls back to:
  - `/home/li859/.cargo/bin/cargo`
- and prepends the resolved Cargo bin dir back into `PATH`

Rule:

- if Rust-backed Phase 1 dies immediately, check PATH before debugging the label logic.

## Telemetry we now care about for GPU decisions

The user explicitly wants enough information to tell, not just “it worked.”

Current intended benchmark metrics from `train_policy_value_remote()` logs:

- `profile`
- `device`
- `sample_count`
- `batch_size`
- `num_workers`
- `max_epochs`
- `elapsed_s`
- `peak_allocated_gib`
- `peak_reserved_gib`

Rule:

- elapsed-only runs are not enough for decision-grade GPU comparison.
- if a run does not emit VRAM telemetry, rerun it after fixing the instrumentation path.

## Current profile reality

Repo-grounded policy/value defaults remain:

- `t4`: batch `256`, workers `2`
- `l4`: batch `512`, workers `4`
- `a10`: batch `1024`, workers `4`
- `b200`: batch `2048`, workers `8`

Selector-only benchmark support was added for:

- `h100` → `H100!`
- `h200` → `H200`

Important constraint:

- these selector-only profiles are for hardware targeting, not new blessed policy/value defaults
- pass explicit `batch_size` / `num_workers` for H100/H200 benchmark runs

## Good benchmark workflow

1. Build or reuse a **semantically valid** artifact set.
2. Confirm at least two replay groups for any remote train smoke.
3. Upload artifacts to `fusion-training-data`.
4. Run `train_policy_value_remote` directly with explicit settings.
5. Capture telemetry from logs:
   - elapsed
   - peak allocated GiB
   - peak reserved GiB
6. Treat OOM, zero-validation, import failure, or upload failure as separate failure classes.

## Bad benchmark workflow

- benchmarking on placeholder artifacts
- trusting existence-only sidecars
- using one-group tiny subsets as if they prove training stability
- copying teacher/student tuning values into policy/value defaults without project-specific evidence
- reading only wall-clock time and ignoring VRAM use
- assuming a Modal import fix at the file header also fixed every inner fallback branch

## Absolute-path commands that were useful

### Check current large Phase 1 progress

```bash
python3 - <<'PY'
from pathlib import Path
base = Path('/tmp/pv_probe_training_data.bin')
label = Path(str(base)+'.policy_value.jsonl')
meta = Path(str(base)+'.policy_value.metadata.json')
print('label_exists', label.exists(), 'size', label.stat().st_size if label.exists() else -1)
print('meta_exists', meta.exists(), 'size', meta.stat().st_size if meta.exists() else -1)
if label.exists():
    print('lines', sum(1 for _ in label.open()))
PY
```

### Generate labels with Cargo fallback wrapper

```bash
PATH="/usr/bin:/bin" \
python3 "/home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine/training/scripts/generate_policy_value_labels.py" \
  "/tmp/pv_probe_small2.bin"
```

### Upload a valid artifact set

```bash
python3 -m modal run \
  /home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine/training/scripts/modal_app.py::upload_policy_value_artifacts \
  --local-data-path /tmp/pv_probe_small.bin
```

### Run a remote current-model train smoke

```bash
FUSION_GPU_PROFILE=a10 \
python3 -m modal run \
  /home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine/training/scripts/modal_app.py::train_policy_value_remote \
  --data-filename pv_probe_twogroup.bin \
  --run-id pv-twogroup-a10-smoke \
  --batch-size 2 \
  --num-workers 0 \
  --max-epochs 1 \
  --lr 0.0003 \
  --weight-decay 0.00001
```

## Short checklist before any future Modal benchmark run

- [ ] dataset has all 6 required files
- [ ] semantic validation passes
- [ ] dataset has enough replay-group diversity for validation split
- [ ] no `scripts.*` fallback imports remain on the active Modal code path
- [ ] upload helper uses path-based `batch.put_file(...)`
- [ ] telemetry line is present in the remote train logs
- [ ] checkpoint/export relpaths use resolved volume-path normalization
- [ ] results are large enough to be decision-grade for the question being asked

## Bottom line

The biggest repeatable lesson is that Modal failures here were rarely “GPU problems” first. They were usually packaging, import-surface, artifact-contract, or path-normalization bugs that happened before the GPU numbers meant anything. Fix those first, then trust the benchmark.
