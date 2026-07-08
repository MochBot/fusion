# Fusion Engine Training Pipeline

Policy/value training and labeling pipeline for Fusion engine experiments. The current active path is policy/value data preparation, labeling, training, export, and runtime smoke validation; the older teacher/student distillation material below is historical context unless explicitly referenced by a current contract.

Current contract references:
- `training/PHASE0_STATE_CONTRACT.md`
- `training/PHASE1_SEARCH_ALIGNED_SUPERVISION.md`
- `training/POLICY_VALUE_REBUILD_BLUEPRINT.md`
- `training/POLICY_VALUE_V2_CONTRACT_PLAN.md`

The 854-feature teacher/student path is legacy. Phase 0 established canonical schema and artifact ownership; Phase 1 established search-aligned policy/value supervision; Phase 2 added native ONNX runtime ownership and a Modal launcher for the policy/value path.

## Current active path

The active path is now policy/value, not teacher/student distillation:

- local artifact prep: `training/scripts/preprocess_replays.py`
- local label generation: `training/scripts/generate_policy_value_labels.py`
- local training entrypoint: `training/scripts/train_policy_value.py`
- local export entrypoint: `training/scripts/export_policy_value_onnx.py`
- remote policy/value launcher: `training/scripts/modal_app.py::launch_policy_value_pipeline` (default GPU profile is `l4`; override with `FUSION_GPU_PROFILE`)

Required artifact set for a policy/value corpus basename such as `training/training_data_triangle_v1.bin`:

- `<name>.bin`
- `<name>.bin.metadata.json`
- `<name>.bin.groups.u64`
- `<name>.bin.policy_value.requests.jsonl`
- `<name>.bin.policy_value.jsonl`
- `<name>.bin.policy_value.metadata.json`

Typical remote path:

```bash
python training/scripts/preprocess_replays.py data/replays-x-xplus training/training_data_triangle_v1.bin --workers 10
python training/scripts/generate_policy_value_labels.py training/training_data_triangle_v1.bin
modal run training/scripts/modal_app.py::launch_policy_value_pipeline --local-data-path training/training_data_triangle_v1.bin --replay-dir data/replays-x-xplus
```

Default policy/value launcher settings with `FUSION_GPU_PROFILE` unset:

- GPU profile: `l4`
- batch size: `512`
- dataloader workers: `4`
- max epochs: `50`
- lr: `3e-4`
- weight decay: `1e-5`
- precision: inherited from active Modal profile (`bf16-mixed` on L4)

## Active architecture

```
.ttrm replays
    │
    ▼
preprocess_replays.py ──► corpus bin + metadata/group sidecars
    │
    ▼
generate_policy_value_labels.py ──► policy/value request + label sidecars
    │
    ▼
train_policy_value.py / modal_app.py::launch_policy_value_pipeline
    │
    ▼
export_policy_value_onnx.py ──► ONNX + metadata for runtime smoke tests
```

### Active model family

The current policy/value path uses `training/scripts/train_policy_value.py`, `training/scripts/export_policy_value_onnx.py`, and the models under `training/models/policy_value.py`. Keep the corpus and replay ownership aligned with `DATA_INVENTORY.md`: the canonical baseline corpus is `training/training_data_triangle_v1.bin`, sourced from `data/replays-x-xplus/`.

### Historical teacher/student path

The following model notes describe the legacy 854-feature teacher/student pipeline and are retained for archaeology only.

**Teacher** (`models/teacher.py`) - Dual-CNN with fusion MLP.

- Two CNN encoders (player + opponent boards): `Conv2d(1→64→128→256)` with BatchNorm, ReLU, MaxPool, AdaptiveAvgPool → 256-dim each
- Fusion: concat(256 + 256 + 49 pieces + 5 scalars) = 566 → Linear(566,512) → Linear(512,256)
- 6 regression heads: value, attack_potential, defensive_solidity, efficiency, flexibility, tempo
- 1 classification head: phase (opener / midgame / survival)
- Loss: Kendall homoscedastic uncertainty weighting (learnable log-variance per task, Huber for regression, CE for classification)

**Student** (`models/student.py`) - 3-layer MLP for WASM.

- Architecture: 854 → 192 → 96 → 48 → 9 (SCReLU activations)
- Output: 6 regression values + 3 phase logits
- Exported as flat f32 binary: 187,785 floats = 734 KB
- Distillation loss: α·MSE(regression) + β·KL(phase logits with temperature)

### Feature Vector (854 floats)

| Offset | Size | Description |
|--------|------|-------------|
| 0–399 | 400 | Player board (10 cols × 40 rows, column-major, binary) |
| 400–799 | 400 | Opponent board (same layout) |
| 800–848 | 49 | Piece one-hot encoding (7 pieces × 7 slots: current + queue + hold) |
| 849–853 | 5 | Scalars: combo, b2b, lines cleared, garbage pending, bag position (all normalized) |

Labels (5 floats appended during preprocessing): game_outcome, lines_sent, b2b_after, position_normalized, time_to_topout.

Total sample: 859 floats = 3,436 bytes.

## Directory Structure

```
training/
├── TRAINING.md              # This file
├── training_data.bin         # Preprocessed replay data (mmap'd f32 binary)
├── pyproject.toml            # Python deps (torch, lightning, optuna)
├── data/
│   └── dataset.py            # FusionBinaryDataset - memory-mapped binary loader
├── models/
│   ├── teacher.py            # TeacherNet (dual-CNN + fusion MLP)
│   ├── student.py            # StudentNet (3-layer MLP, SCReLU)
│   └── lit_module.py         # TeacherLitModule + FusionDataModule (Lightning wrappers)
├── scripts/
│   ├── modal_app.py          # Modal app definition - all cloud functions
│   ├── optuna_objective.py   # Optuna trial definition for teacher HPO
│   ├── distill_student.py    # StudentDistillModule + distillation training
│   ├── preprocess_replays.py # .ttrm → training_data.bin (parallel, SRS simulation)
│   └── export_weights.py     # Lightning checkpoint → flat f32 binary
├── utils/
│   ├── config.py             # All dimension constants, label names, export order
│   ├── losses.py             # KendallMultiTaskLoss
│   └── activations.py        # SCReLU (Squared Clipped ReLU)
└── tests/
```

## Data Pipeline

### 1. Scrape Replays

```bash
cd scripts/replay_scraper
pip install -r requirements.txt
python scraper.py              # Fetches .ttrm files from TETR.IO API
python sort_by_rank.py         # Sorts into data/replays/0001-0100/ ... /0901-1000/
```

Current canonical baseline replay source: `data/replays-x-xplus/`. The legacy general replay corpus is `data/replays-legacy/`.

### 2. Preprocess Replays

Simulates TETR.IO game mechanics from initial board snapshot + key events. Extracts a sample at every hard drop for both players. Uses SRS rotation with wall kicks.

```bash
PYTHONPATH="$PWD" \
  python training/scripts/preprocess_replays.py \
    data/replays-x-xplus \
    training/training_data_triangle_v1.bin \
    --workers 10
```

Output: `training_data.bin` - contiguous f32 binary, plus `training_data.bin.metadata.json` and `training_data.bin.groups.u64` sidecars used for Phase 0 schema validation and replay-group-aware splitting.

### 3. Upload Data to Modal Volume

```bash
modal run training/scripts/modal_app.py::upload_data \
  --local-path training/training_data.bin
```

Uploads in 256 MB chunks to the `fusion-training-data` volume at `/data/training_data.bin`.

## Training on Modal

### Legacy teacher/student quick start

```bash
# Historical pipeline: HPO → distill → export
modal run training/scripts/modal_app.py::launch_pipeline

# Custom configuration
modal run training/scripts/modal_app.py::launch_pipeline \
  --num-workers 15 \
  --trials-per-worker 4 \
  --teacher-epochs 50 \
  --student-epochs 50
```

### Pipeline Phases

**Phase 1 - Teacher HPO** (`fan_out` → `train_teacher_trial` × N)

Each worker runs an independent Optuna study with HyperbandPruner. Workers explore hyperparameter space in parallel with no shared state (in-memory storage per worker).

Search space:

| Parameter | Range | Scale |
|-----------|-------|-------|
| Learning rate | 1e-5 – 1e-2 | Log |
| Weight decay | 1e-6 – 1e-2 | Log |
| Batch size | 12288 | Locked first real-training candidate |
| Dropout (fc1) | 0.1 – 0.5 | Uniform |
| Dropout (fc2) | 0.05 – 0.3 | Uniform |

Callbacks: ModelCheckpoint (top-1 by val/total_loss), EarlyStopping (patience=10), PruningCallback.

**Phase 2 - Student Distillation** (`distill_student_remote`)

Loads the best teacher checkpoint (frozen). Trains StudentNet to mimic teacher outputs:
- Regression: MSE loss between student and teacher
- Phase: KL divergence with temperature scaling
- Optimizer: AdamW with CosineAnnealingLR (T_max = max_epochs)
- EarlyStopping patience: 15

**Phase 3 - Weight Export** (`export_weights_remote`)

Extracts student weights from Lightning checkpoint into flat f32 binary for WASM consumption. Order defined in `config.WEIGHT_EXPORT_ORDER`.

### LARYNX Pattern

`run_pipeline` is decorated with `@app.function` (not `local_entrypoint`) so it runs inside a Modal container. This survives client disconnects - if your terminal dies, the pipeline continues. `launch_pipeline` is the thin `local_entrypoint` that calls `run_pipeline.remote()` and exits.

## B200 GPU Optimizations

### Hardware Configuration

| Parameter | Teacher | Student |
|-----------|---------|---------|
| GPU | B200 (192 GB VRAM) | B200 |
| CPU cores | 12 | 12 |
| RAM | 40 GB | 40 GB |
| Timeout | 10,800s (3h) | 7,200s (2h) |
| Retries | 2 | 2 |
| Scale-down window | 300s | 300s |
| Startup timeout | 600s | 600s |

### Ephemeral Disk Copy

Training data is copied from the Modal Volume (FUSE, ~200 MB/s) to the container's local NVMe (`/tmp`, ~5 GB/s) at container start. This eliminates network I/O during training.

```python
shutil.copy2("/data/training_data.bin", "/tmp/training_data.bin")
```

### DataLoader Tuning

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| `num_workers` | 10 | Best validated feeder point from the B200 benchmark before real-loop remeasurement |
| `pin_memory` | True | DMA transfer to GPU |
| `persistent_workers` | True | Avoid respawning between epochs |
| `prefetch_factor` | 6 | Best validated queue depth at the current B200 single-instance knee |
| `drop_last` | True (train) | Consistent batch sizes |

### torch.compile

Both TeacherLitModule and StudentDistillModule use `configure_model()` to compile the inner `nn.Module` before DDP wrapping:

```python
def configure_model(self):
    self.model = torch.compile(self.model, mode="default", dynamic=False)
```

- `mode='default'`: operator fusion and kernel optimization without CUDA Graphs
- `dynamic=False`: keeps the compile path stable while avoiding shape-mismatch issues when real batch sizes vary less but still are not guaranteed graph-safe
- Compile cache persisted to `fusion-compile-cache` Volume at `/compile-cache`

### Precision

`bf16-mixed` on both Trainer instances. BF16 Tensor Cores on B200 for matmul/conv, FP32 for reductions and loss computation. Chosen over `bf16-true` for gradient stability during training.

### CUDA Environment Variables

| Variable | Value | Purpose |
|----------|-------|---------|
| `PYTORCH_CUDA_ALLOC_CONF` | `expandable_segments:True` | Prevents CUDA memory fragmentation |
| `CUBLAS_WORKSPACE_CONFIG` | `:4096:8` | Deterministic cuBLAS operations |
| `TORCH_NCCL_AVOID_RECORD_STREAMS` | `1` | Reduces NCCL memory overhead |
| `NCCL_NVLS_ENABLE` | `0` | Avoids NVLink issues on single-GPU |
| `NCCL_CUMEM_ENABLE` | `0` | Avoids cuMem API issues |
| `TORCH_CUDNN_V8_API_ENABLED` | `1` | Enable cuDNN v8 backend |
| `TORCHINDUCTOR_CACHE_DIR` | `/compile-cache` | Persist torch.compile cache across containers |

### Volume Architecture

| Volume | Mount | Purpose |
|--------|-------|---------|
| `fusion-training-data` | `/data` | Training binary (read-only during training) |
| `fusion-training-checkpoints` | `/checkpoints` | Model checkpoints (cross-worker visibility) |
| `fusion-compile-cache` | `/compile-cache` | torch.compile/Inductor cache |

Checkpoint visibility between containers requires explicit `volume.commit()` after saving and `volume.reload()` before reading.

## Default Configuration

```
Workers:           15
Trials/worker:     4  (60 total trials)
Teacher epochs:    50
Student epochs:    100
GPU:               B200
Teacher batch:     12288 (locked first real-training candidate)
Student batch:     12288 (locked first real-training candidate)
Teacher/student workers: 10 / 10
Prefetch factor:   6
```

## Local Development

### Prerequisites

```bash
cd training
uv sync           # Install Python dependencies
```

### Run Preprocessing Locally

```bash
PYTHONPATH="$PWD" \
  python training/scripts/preprocess_replays.py data/replays-x-xplus training/training_data_triangle_v1.bin
```

### Export Weights Locally

```bash
PYTHONPATH="$PWD" \
  python training/scripts/export_weights.py path/to/checkpoint.ckpt output_weights.bin
```

### Run a Smoke Test

```bash
modal run training/scripts/modal_app.py::launch_pipeline \
  --num-workers 2 \
  --trials-per-worker 2 \
  --teacher-epochs 3 \
  --student-epochs 3
```

## Known Issues

- **Import resolution**: `training.scripts.*` / `training.utils.*` imports only resolve inside the Modal container (`.add_local_python_source("training")`). Local LSP will show import errors - this is expected.
- **PYTHONPATH**: Local scripts require `PYTHONPATH` set to the repo root for proper module resolution.
- **Large files**: `training_data.bin` (~30 GB) and `data/metadata/replay_manifest.json` (~5.5 MB) are in `.gitignore`. JJ snapshot limit bumped to 15 GiB.
