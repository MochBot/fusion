# Training

Policy/value training for the engine. Big artifacts (replay corpora, `*.bin`, label sidecars) are untracked; stage them locally first.

```bash
cd training && uv sync && uv run pytest tests
```

Pipeline, in order (run from the repo root with `PYTHONPATH="$PWD"`):

```bash
python training/scripts/preprocess_replays.py data/replays-x-xplus training/training_data_triangle_v1.bin --workers 10
python training/scripts/generate_policy_value_labels.py training/training_data_triangle_v1.bin
modal run training/scripts/modal_app.py::launch_policy_value_pipeline --local-data-path training/training_data_triangle_v1.bin --replay-dir data/replays-x-xplus
python training/scripts/export_policy_value_onnx.py path/to/checkpoint.ckpt
```

Runtime smoke test against an exported model (ONNX and its `.metadata.json` staged side by side):

```bash
CLOUD_EXEC_SKIP=1 cargo run --bin policy_value_runtime_smoke -- path/to/model.onnx.metadata.json
```

Replay collection scripts live in `training/recovered/replay-collector/`. Canonical corpus: `data/replays-x-xplus/`.

Note: `training.scripts.*` imports resolve inside the Modal container; local LSP import errors are expected.
