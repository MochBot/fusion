# Task 4-5 Player-Context Smoke — 2026-03-10

## Scope

This record captures the end-to-end smoke verification for Task 4 and Task 5 of the Phase 1 player-context lane:

- Task 4: dataset/model contract for `player_context_primary` with search policy/value auxiliary supervision.
- Task 5: pipeline/readiness/training integration, including mode-aware artifact validation and Modal entrypoints.

## What was implemented

The following files were updated to support explicit supervision modes:

- `training/data/policy_value_dataset.py`
- `training/models/policy_value.py`
- `training/models/policy_value_lit_module.py`
- `training/scripts/train_policy_value.py`
- `training/scripts/policy_value_pipeline.py`
- `training/scripts/modal_app.py`

The new lane is `player_context_primary`, where:

- primary loss = player action over the existing search candidate set
- auxiliary losses = search policy KL + search value MSE
- readiness requires both the existing search sidecars and the new player-context sidecars

The existing `search_control` lane remains available as the backward-compatible control path.

## Local verification

Focused unit coverage passed after implementation:

```bash
python3 -m unittest \
  training.tests.test_policy_value_dataset \
  training.tests.test_policy_value_model \
  training.tests.test_policy_value_pipeline -v
```

Bytecode validation also passed for the edited files.

Local end-to-end training smoke was not decision-grade in the shell environment because bare `python3` on the VM does not have `lightning` installed.

## Real Modal smoke

### Synthetic artifact set

A tiny synthetic dataset was created at:

- `/tmp/pv_player_context_smoke.bin`

with the complete `player_context_primary` artifact family:

- `.metadata.json`
- `.groups.u64`
- `.policy_value.requests.jsonl`
- `.policy_value.jsonl`
- `.policy_value.metadata.json`
- `.policy_value.player_context.jsonl`
- `.policy_value.player_context.metadata.json`

### Upload command

```bash
FUSION_GPU_PROFILE=a10 python3 -m modal run \
  training/scripts/modal_app.py::upload_policy_value_artifacts \
  --local-data-path /tmp/pv_player_context_smoke.bin \
  --supervision-mode player_context_primary
```

This succeeded, which verified that the mode-aware upload path correctly required and staged the extra player-context sidecars.

### First training smoke: runtime bug found

```bash
FUSION_GPU_PROFILE=a10 python3 -m modal run \
  training/scripts/modal_app.py::train_policy_value_remote \
  --data-filename pv_player_context_smoke.bin \
  --run-id player-context-smoke-<timestamp> \
  --supervision-mode player_context_primary \
  --batch-size 1 \
  --num-workers 0 \
  --max-epochs 1
```

The first remote smoke exposed a real runtime bug in `collate_policy_value_batch`:

```text
RuntimeError: Boolean value of Tensor with more than one value is ambiguous
```

Root cause:

- Python `or` was used on tensor-valued fallbacks while selecting `search_policy_probs` / `best_value` aliases.
- Unit tests had not caught this because the fallback path was exercised with test data that did not reproduce the real DataLoader worker behavior.

Fix applied:

- replaced tensor truthiness fallback logic with explicit key-presence branching

## Successful training smoke after fix

The same Modal training smoke was rerun after the collate fix.

Observed success output included:

```text
Using bfloat16 Automatic Mixed Precision (AMP)
GPU available: True (cuda), used: True
LOCAL_RANK: 0 - CUDA_VISIBLE_DEVICES: [0]
`Trainer.fit` stopped: `max_epochs=1` reached.
BENCH_TELEMETRY profile=l4 device='NVIDIA A10' sample_count=2 batch_size=1 num_workers=4 max_epochs=1 elapsed_s=24.273 peak_allocated_gib=0.072 peak_reserved_gib=0.086
```

This confirms that the new lane can:

1. validate `player_context_primary` artifacts,
2. upload the required sidecars,
3. stage them inside Modal,
4. build batches with the new dataset contract,
5. execute remote training successfully.

## Environment decision

For artifact generation:

- player-context artifact regeneration should run on VM/local preprocessing, not Modal CPU, because replay preprocessing is comparatively cheap, deterministic, and already local-friendly.
- heavy search-label generation should not use the recently unstable Modal CPU shard path as the default production route; that path previously suffered from worker preemption and inconsistent volume visibility.
- GPU training should remain a Modal job.

This split matches both the implementation and the observed operational behavior during the Phase 1 work.
