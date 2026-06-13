# Policy+Value Phase 1 Search-Aligned Supervision Plan

**Goal:** Replace legacy proxy teacher supervision with policy/value targets produced from current search outputs, while keeping runtime search integration unchanged for now.

**Architecture:** Phase 1 adds a new training-side supervision artifact parallel to the Phase 0 canonical example dataset. The initial oracle is the existing engine search: `root_scores` define policy supervision and `best.score` defines value supervision. This phase does not yet replace the runtime model or the Rust search path; it establishes the new truth source and the data/training surfaces needed to consume it.

Locked decisions:
- oracle profile: **stronger offline oracle**
- policy head shape: **candidate ranking over legal/root moves**, not a fixed global action head

**Tech Stack:** Python preprocessing/training pipeline, existing Rust search semantics as oracle source, markdown contract docs, unit tests.

---

### Task 1: Write the Phase 1 supervision contract

**Files:**
- Create: `training/PHASE1_SEARCH_ALIGNED_SUPERVISION.md`
- Modify: `training/POLICY_VALUE_REBUILD_BLUEPRINT.md`
- Modify: `training/TRAINING.md`

**Steps:**
1. Define the default truth source as search-oracle supervision.
2. Define the policy target as a distribution over `root_scores` keyed by `Move.raw()`.
3. Define the value target as `SearchResultFull.best.score`.
4. Define artifact metadata and normalization rules.

**QA:** Contract text must make it impossible to confuse replay labels with Phase 1 truth.

---

### Task 2: Introduce a shared policy/value schema module

**Files:**
- Create: `training/utils/policy_value_schema.py`
- Test: `training/tests/test_policy_value_schema.py`

**Steps:**
1. Define schema constants and sidecar naming.
2. Define dataclasses for root-move search targets.
3. Add metadata validators and target validators.
4. Add temperature-softmax normalization helpers.

**QA:** The schema must fail loudly on invalid move IDs, empty root score lists, or invalid probability normalization.

---

### Task 3: Add target-generation scaffolding for search outputs

**Files:**
- Create: `training/scripts/generate_policy_value_labels.py`
- Test: `training/tests/test_generate_policy_value_labels.py`

**Steps:**
1. Define a training-side builder that turns root search scores into policy/value targets.
2. Preserve both raw root scores and normalized policy probabilities.
3. Serialize/deserialize targets in a durable JSONL-compatible artifact shape.

**QA:** Given a small synthetic root-score set, the generator must produce deterministic best move, best value, and normalized policy probabilities.

---

### Task 4: Wire Phase 1 docs into the current training docs

**Files:**
- Modify: `training/TRAINING.md`
- Modify: `training/POLICY_VALUE_REBUILD_BLUEPRINT.md`

**Steps:**
1. Link the new Phase 1 contract doc.
2. Mark current teacher/distillation flow as legacy relative to the new supervision path.

**QA:** A new engineer should be able to tell what Phase 1 changes and what it deliberately does not change yet.

---

## Acceptance Criteria

1. One canonical Phase 1 document defines policy/value truth source and move-ID contract.
2. A shared schema module validates policy/value metadata and targets.
3. A generation scaffold exists for converting root scores into training targets.
4. Tests cover normalization, validation, and serialization.
5. Docs clearly position Phase 1 as replacing proxy supervision without yet replacing runtime search.
