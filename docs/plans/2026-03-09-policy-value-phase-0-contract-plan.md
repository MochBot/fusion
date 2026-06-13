# Policy+Value Phase 0 Contract Repair Implementation Plan

**Goal:** Repair and canonicalize the training/runtime state contract so later policy+value work is built on trustworthy examples rather than mismatched replay state.

**Architecture:** Introduce one versioned example schema for replay decision points, move feature ownership out of scattered ad hoc definitions, and verify that preprocessing, dataset loading, and runtime encoding all agree on the same state semantics. This phase does not train a new model; it establishes the source-of-truth contract required for every later phase.

**Tech Stack:** Python preprocessing pipeline, Rust runtime encoder, markdown schema docs, unit/integration tests.

---

### Task 1: Define the canonical Phase 0 contract document

**Files:**
- Create: `training/PHASE0_STATE_CONTRACT.md`
- Reference: `training/TRAINING.md`
- Reference: `training/POLICY_VALUE_REBUILD_BLUEPRINT.md`

**Step 1: Write the contract sections**

Include exact headings for:
- Example identity
- Replay alignment rule
- Board ownership rule
- Opponent snapshot rule
- Queue/hold/current ownership
- Scalar field definitions
- Legal move indexing contract
- Split-group metadata
- Schema versioning

**Step 2: Define canonical example fields**

Specify the example object fields explicitly, including:
- `schema_version`
- `replay_id`
- `round_id`
- `player_id`
- `frame_id`
- `pre_state`
- `opponent_state`
- `legal_moves`
- `chosen_move`
- `progression`
- `group_id`

**Step 3: Record current contract violations**

Document the known risks already identified:
- likely opponent timing mismatch in preprocessing
- scalar index inconsistency in teacher target derivation
- replay-group correlation risk
- duplicated contract ownership between Python training and Rust runtime

**Step 4: Review for ambiguity**

Ensure every field answers: owner, type, derivation time, and consumer.

**QA:** The document must be sufficient for an engineer to implement the schema without re-reading the architecture audit.

---

### Task 2: Add a shared schema module in training utils

**Files:**
- Create: `training/utils/example_schema.py`
- Modify: `training/utils/config.py`
- Test: `training/tests/test_example_schema.py`

**Step 1: Define schema dataclasses/constants**

Add structured definitions for:
- schema version constant
- scalar field order
- piece order
- board encoding constant
- example identity fields

**Step 2: Add validation helpers**

Implement validation functions for:
- scalar order correctness
- piece order correctness
- required field presence
- move-index contract invariants

**Step 3: Write tests first**

Add tests asserting:
- schema version exists
- scalar order is explicit and stable
- piece order matches training contract
- validator rejects missing/invalid fields

**Step 4: Run targeted tests**

Run: `python3 -m unittest training.tests.test_example_schema -v`

**QA:** Schema validation must fail loudly on contract drift.

---

### Task 3: Refactor preprocessing to emit canonical examples, not anonymous flat rows

**Files:**
- Modify: `training/scripts/preprocess_replays.py`
- Reference: `training/utils/example_schema.py`
- Test: `training/tests/test_preprocess_contract.py`

**Step 1: Introduce an intermediate canonical example object**

Do not write raw flat features immediately after simulation. Create a structured per-decision example first.

**Step 2: Make replay alignment explicit**

Add one clearly named helper responsible for deciding which opponent state belongs to a player decision point.

**Step 3: Attach split-safe grouping metadata**

Emit stable `group_id` or equivalent replay/round grouping metadata with each example.

**Step 4: Serialize from canonical example to flat features only as a final transformation**

Keep flat export only as a derived representation, not the primary contract.

**Step 5: Write tests**

Add tests that assert:
- each emitted example has identity metadata
- scalar ordering matches schema
- piece ordering matches schema
- opponent snapshot alignment helper is called in one place

**QA:** Preprocessing must have one explicit source of truth for alignment and feature ownership.

---

### Task 4: Make dataset loading schema-aware

**Files:**
- Modify: `training/data/dataset.py`
- Modify: `training/utils/config.py`
- Test: `training/tests/test_dataset_contract.py`

**Step 1: Teach the dataset layer to verify schema metadata**

If the dataset format remains flat binary for now, add a sidecar metadata file or header validation path.

**Step 2: Enforce split-group handling hooks**

Prepare the dataset layer for replay-group-aware splitting even if Phase 0 does not fully switch the split strategy yet.

**Step 3: Add contract tests**

Assert that dataset loading rejects mismatched scalar order / version / feature count assumptions.

**QA:** Dataset code should no longer silently accept ambiguous training artifacts.

---

### Task 5: Unify Rust runtime encoding with the canonical schema

**Files:**
- Modify: `src/student_model.rs`
- Modify: `src/wasm.rs`
- Test: `src/student_model.rs` tests

**Step 1: Move hardcoded contract details behind one named schema block**

Ensure scalar order, piece order, board encoding, and schema version are explicitly tied to the same canonical definitions used by training.

**Step 2: Rename runtime manifest semantics if needed**

If `StudentModelManifest` is now broader than the legacy student, prepare naming/comments for future policy+value generalization.

**Step 3: Add runtime contract tests**

Assert:
- scalar order matches documented Phase 0 contract
- piece order remains remapped correctly
- runtime manifest rejects stale/legacy mismatches clearly

**QA:** Rust runtime must no longer independently evolve the feature contract from Python.

---

### Task 6: Remove target-derivation ambiguity from the current training path

**Files:**
- Modify: `training/models/lit_module.py`
- Test: `training/tests/test_teacher_target_contract.py`

**Step 1: Stop implicit scalar-index assumptions**

Replace positional scalar access with named access derived from the shared schema.

**Step 2: Add regression tests for scalar meaning**

Write tests ensuring bag-related logic cannot accidentally read normalized lines or other scalar slots.

**QA:** Even before policy+value training exists, current training code must read scalar semantics through shared names rather than raw positional magic numbers.

---

### Task 7: Add cross-layer parity tests

**Files:**
- Create: `training/tests/test_runtime_training_contract_parity.py`
- Reference: `training/scripts/preprocess_replays.py`
- Reference: `src/student_model.rs`

**Step 1: Define parity fixtures**

Create minimal replay/frame fixtures where the expected scalar order, piece order, and board encoding are known.

**Step 2: Assert Python-side encoding and Rust-side encoding agree**

If direct cross-language execution is too heavy, assert agreement through a shared documented vector fixture checked from both sides.

**Step 3: Run parity tests**

Use the lightest possible command(s) that validate both sides.

**QA:** Phase 0 is not complete until one fixture proves both layers encode the same state identically.

---

### Task 8: Update planning and training docs

**Files:**
- Modify: `training/TRAINING.md`
- Modify: `training/POLICY_VALUE_REBUILD_BLUEPRINT.md`
- Modify: `training/PHASE0_STATE_CONTRACT.md`

**Step 1: Mark legacy sections clearly**

Annotate old teacher/student assumptions as legacy where necessary.

**Step 2: Link the new contract docs**

Point readers to the canonical contract and this Phase 0 plan.

**QA:** An engineer landing in the repo should know which documents are current and which assumptions are legacy.

---

## Acceptance Criteria

Phase 0 planning is ready for enactment when all of the following are true:

1. One markdown contract defines canonical example ownership and schema versioning.
2. Training utilities expose shared schema constants/validators instead of scattered positional assumptions.
3. Preprocessing emits canonical structured examples before any flat export step.
4. Dataset loading validates contract metadata instead of silently trusting artifact shape.
5. Rust runtime encoding is explicitly tied to the same schema semantics.
6. Current target derivation no longer uses ambiguous scalar indexes.
7. At least one parity test proves training/runtime encoding agreement on the same fixture.

---

## Notes

- Phase 0 does **not** introduce the final policy+value model.
- Phase 0 does **not** rewrite search.
- Phase 0 exists to make later phases trustworthy.
- If a task reveals that the current binary format cannot carry the required metadata safely, changing the artifact format is allowed in Phase 0 because correctness takes priority over backward convenience.
