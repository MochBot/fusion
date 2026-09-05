# fusion

Rust engine for TETR.IO replay coaching: move generation, search, eval, S2 attack, native and WASM builds. Python training tools live under `training/`. Movegen uses cobra-movegen's smear-board reachability approach.

## Setup

Rust via rustup, Python via `uv`.

```bash
CLOUD_EXEC_SKIP=1 sh scripts/test.sh
CLOUD_EXEC_SKIP=1 cargo clippy -- -D warnings
cd training && uv sync && uv run pytest tests
```

Copy `.env.example` to `.env` only for replay collection, Modal training, or the label generator; tests and clippy run without it.

Large artifacts (replay corpora, training bins, label sidecars, ONNX models) are not tracked; keep a model's `.metadata.json` next to it. See `training/TRAINING.md` for the training pipeline.

The test command verifies and caches checksum-pinned opener inputs before running the library suite.
It needs curl and either sha256sum or shasum on first use; intact cached inputs work offline.
The inputs stay ignored under `fixtures/openers/`, with no sibling application checkout or storage credentials required.
Use `sh scripts/hydrate-opener-tests.sh` before running Cargo test commands directly.
Generated reports stay under ignored `evidence-out/`. The ignored historical census comparison requires
its separately retained baseline report; normal compiler tests do not read that report.

The opener runtime accepts catalog bytes from its caller. Dataset downloads are test setup, not engine runtime behavior.
