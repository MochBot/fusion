# Fusion engine

Rust engine and Python training utilities for TETR.IO replay coaching. The Rust package is `direct-cobra-copy`; it owns move generation, search, board evaluation, S2 attack, and native/WASM runtime paths. The `training/` directory contains policy/value data and Modal training/labeling tools.

## Fresh clone setup

Install Rust with rustup and install `uv` for the Python training environment. From the repository root, use the Cargo environment prefix when your shell does not load it automatically:

```bash
source "$HOME/.cargo/env"
CLOUD_EXEC_SKIP=1 cargo metadata --no-deps --format-version 1
CLOUD_EXEC_SKIP=1 cargo test --lib
CLOUD_EXEC_SKIP=1 cargo test --test presim_validation
CLOUD_EXEC_SKIP=1 cargo clippy -- -D warnings
```

Python setup lives under `training/`:

```bash
cd training
uv sync
uv run pytest tests
```

Run training scripts from `training/` or set `PYTHONPATH` to the current checkout, not to a machine-specific path:

```bash
cd training
PYTHONPATH="$PWD" uv run python -m pytest tests
```

## Optional private resources

Copy `.env.example` to `.env` only if you need replay collection, Modal training, or a local override for the policy/value label generator. Normal Rust tests, clippy, and Python unit tests do not require private env vars.

Large generated artifacts are intentionally not tracked. `training/training_data_triangle_v1.bin`, replay corpora under `data/`, and generated label sidecars must be staged locally or pulled from the shared artifact store before running full training jobs. Runtime model files that are present under `models/` are tracked with their metadata; keep the ONNX and `.metadata.json` side by side when running runtime smoke commands.

## Useful commands

```bash
source "$HOME/.cargo/env"
CLOUD_EXEC_SKIP=1 cargo run --bin policy_value_runtime_smoke -- models/pvc-real-r03.onnx.metadata.json
cd training && uv run python scripts/generate_policy_value_labels.py training_data_triangle_v1.bin
```

The smoke command requires the matching ONNX file next to the metadata file. The label command requires the training data and request sidecars described in `DATA_INVENTORY.md` and `training/TRAINING.md`.
