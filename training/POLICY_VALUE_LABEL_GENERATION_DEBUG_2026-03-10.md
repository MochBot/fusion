# Policy/Value Label Generation Debug Report — 2026-03-10

## Exact observed failure state

Full-corpus artifact readiness before this fix was:

- `ready=False`
- `dataset_ready=True`
- `labels_ready=False`
- `sample_count=533314`

Exact readiness failure reasons:

- `policy/value metadata sample_count must be positive, got 0`
- `policy/value label count mismatch: expected 533314, got 315`

Exact artifact symptom from live inspection during failed/incomplete runs:

- `training/training_data.bin.policy_value.requests.jsonl`: `533,314` nonblank lines
- `training/training_data.bin.policy_value.jsonl`: partial output only (`315` lines in the original failing state; later interrupted retry showed `111` lines)
- `training/training_data.bin.policy_value.metadata.json`: stale metadata with `sample_count: 0`

## What was investigated and ruled out

### Not a malformed early request

The initial suspicion was that a bad request near the first partial-output boundary was crashing the Rust generator.

That hypothesis was disproved by direct single-request runs on real request lines `108` through `113`:

- line 108: success in `0.004s`
- line 109: success in `1.590s`
- line 110: success in `0.240s`
- line 111: success in `0.025s`
- line 112: success in `0.004s`
- line 113: success in `1.962s`

### Not a deterministic failure within the first 400 requests

Running the Rust generator on the first `400` real requests completed successfully and wrote valid labels/metadata.

## Proven root cause

The real operational root cause was that the Python wrapper in `training/scripts/generate_policy_value_labels.py` invoked:

```bash
cargo run --bin generate_policy_value_labels -- <requests> <labels>
```

That uses Cargo's default **debug/dev profile**, not the optimized release binary.

The Rust oracle workload is search-heavy enough that debug mode makes full-corpus generation look broken or stalled under realistic scale.

## Exact performance evidence

Measured on the same `400` real requests:

- debug binary (`target/debug/generate_policy_value_labels`): `460.87s`
- release binary (`target/release/generate_policy_value_labels`): `50.629s`

That is approximately a **9.1× slowdown** when using the debug build.

Further corrected-wrapper measurements:

- `2,000` real requests via updated wrapper (`num_workers=8`): `129.24s`
- `10,000` real requests via updated wrapper (`num_workers=10`): `208.3s`

Projected full-corpus runtime from the `10,000`-request measurement:

- `533,314 / 10,000 * 208.3s ≈ 11,106s ≈ 3.1 hours`

## Secondary bug introduced during fix work

An intermediate chunked-generation implementation contained a split/merge bug: the chunk writer only wrote the line immediately after chunk rollover because `chunk_handle.write(line)` was accidentally indented inside the rotation branch.

Effect of that bug:

- empty or severely truncated chunk files
- merged output collapsing to one or very few lines

That implementation bug was fixed before the final full-corpus rerun.

## Final code changes made

### `training/scripts/generate_policy_value_labels.py`

- stop using `cargo run` for the real workload path
- build/use `target/release/generate_policy_value_labels`
- add chunked parallel execution for large request corpora
- deterministically merge chunk outputs into the final labels file
- synthesize final metadata after merge with the existing Phase 1 schema contract
- surface chunk-specific failure context if a worker subprocess exits nonzero

### `training/tests/test_generate_policy_value_labels.py`

- update tests to validate the release-binary path
- add chunked-generation merge coverage

## Verification completed before full-corpus rerun

- `python3 -m unittest training.tests.test_generate_policy_value_labels -v` → passed all 7 tests
- `python3 -m py_compile training/scripts/generate_policy_value_labels.py training/tests/test_generate_policy_value_labels.py` → passed
- LSP diagnostics on the edited Python files were cleared before launching the full-corpus rerun

## Current training parameters to use after labels are ready

For the first real A10 training run, the established parameters remain:

- GPU profile: `a10`
- batch size: `1024`
- dataloader workers: `4`
- max epochs: `50`
- learning rate: `3e-4`
- weight decay: `1e-5`
- precision: `bf16-mixed`

Label-generation/oracle parameters remain:

- oracle profile: `stronger_offline_oracle`
- beam width: `2000`
- depth: `18`
- transposition table: `true`
- policy temperature: `1.0`

## Exact conclusion

There was no reproduced deterministic Rust exception in the early corpus.

The exact failure mode that made the system appear broken at full scale was:

1. the wrapper launched the Rust search-oracle generator in the **debug** profile via `cargo run`
2. the workload was therefore far slower than expected at corpus scale
3. interrupted/incomplete runs left partial `.policy_value.jsonl` output and stale metadata (`sample_count: 0`), which semantic readiness correctly rejected

The production-path fix is therefore to run the generator in **release mode** and parallelize chunk execution for the full corpus.
