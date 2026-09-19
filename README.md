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
It needs Python 3.11+, curl and either sha256sum or shasum on first use; intact cached opener inputs work offline.
The inputs stay ignored under `fixtures/openers/`, with no sibling application checkout or storage credentials required.
The pinned perf request predates the retired dealt-input/tail-queue fields, so hydration drops exactly those keys and checks the result against its expected output digest.
Before running Cargo test commands directly, run `sh scripts/hydrate-opener-tests.sh`.
Generated reports stay under ignored `evidence-out/`. The default library suite needs no
retained report: census compilation is pure and the ordinary recognition tests run without
one. The explicit `#[ignore]` census gate
(`openers::recognition::tests::full_catalog_census`) does join a historical path-A report at
`evidence-out/opener-recognition-census/census.json` and fails loudly when it is absent; that
report is not published with the source, so run that gate only where the retained report is
available.

The opener runtime accepts catalog bytes from its caller. Dataset downloads are test setup, not engine runtime behavior.
