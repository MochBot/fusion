# fusion

Rust engine for TETR.IO replay coaching: move generation, search, eval, S2 attack, native and WASM builds. Python training tools live under `training/`. Movegen uses cobra-movegen's smear-board reachability approach.

## Setup

Rust via rustup, Python via `uv`.

```bash
CLOUD_EXEC_SKIP=1 cargo test --lib
CLOUD_EXEC_SKIP=1 cargo clippy -- -D warnings
cd training && uv sync && uv run pytest tests
```

Copy `.env.example` to `.env` only for replay collection, Modal training, or the label generator; tests and clippy run without it.

Large artifacts (replay corpora, training bins, label sidecars) are not tracked. Models under `models/` keep the ONNX and its `.metadata.json` side by side. See `training/TRAINING.md` for the training pipeline.
