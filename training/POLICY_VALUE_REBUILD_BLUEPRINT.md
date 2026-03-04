# Policy+Value Rebuild Blueprint

Last updated: 2026-03-09 (UTC)

## Goal

Rebuild the current teacher/student coaching stack into a policy+value engine that can reach top-200 to top-100 decision quality first, then rebuild coaching on top of that stronger core.

Canonical Phase 0 contract reference: `training/PHASE0_STATE_CONTRACT.md`
Canonical Phase 1 supervision reference: `training/PHASE1_SEARCH_ALIGNED_SUPERVISION.md`

This document is a structural blueprint, not an implementation log. It defines what must change, what can be preserved, and what each phase must produce before the next phase starts.

---

## Why the Current Stack Caps Out

The current stack is structurally limited in four places:

1. **Data contract ambiguity**
   - `training/scripts/preprocess_replays.py` is the real source of truth for training examples, but the runtime contract is also partially redefined in `src/student_model.rs`.
   - Existing audit findings indicate likely opponent-context timing mismatch during extraction and replay-group correlation issues.

2. **Proxy supervision**
   - `training/models/lit_module.py::_derive_targets()` turns raw replay labels into hand-derived teacher targets.
   - The teacher is not trained primarily on move quality or search quality; it is trained on proxy abstractions.

3. **Value-only runtime use**
   - `src/search_expand.rs::evaluate_with_tt()` consumes only `StudentOutputs::value()`.
   - Learned inference currently acts as one board-evaluation subscore inside a handcrafted search/composite system.

4. **Coaching downstream of weak search truth**
   - `src/wasm.rs::evaluate_position_wasm()` and `src/analysis.rs` derive severity and insights from search-score deltas and coarse coaching states.
   - If engine truth is weak, coaching truth is weak.

External analogs support the same conclusion:

- **Lc0 / AlphaZero pattern**: strongest ceiling comes from policy+value owning search.
- **Stockfish / NNUE pattern**: learned evaluation must have first-class runtime authority.
- **Maia pattern**: human-likeness is useful for coaching, but it is not a substitute for strong-play modeling.

---

## End-State Architecture

The target system has four layers:

### 1. Canonical State/Data Layer

One versioned contract defines:

- pre-move board state
- opponent state at the same replay instant
- queue / hold / current piece
- progression scalars
- legal move set encoding
- replay metadata (`replay_id`, `round_id`, `player_id`, `frame_id`)

This contract must be defined once and shared by training and runtime.

### 2. Policy+Value Training Layer

The core model predicts:

- **Policy**: preference distribution over legal placements
- **Value**: continuation quality from the position or chosen move
- **Optional auxiliary heads**: attack, survival, tempo, or phase only if they help optimization

Teacher/student proxy imitation is no longer the core design. Search-aligned supervision replaces it.

### 3. Neural-Guided Search Layer

Search remains deterministic in legality and simulation, but learned outputs become first-class:

- policy controls expansion priority / beam allocation / pruning priority
- value controls continuation evaluation
- handcrafted eval/attack/context terms become fallback telemetry or auxiliary features, not co-equal controllers

### 4. Coaching Layer

Coaching is rebuilt after engine truth is stronger. It consumes:

- policy disagreement
- value loss
- continuation divergence
- best-vs-actual tactical branch differences

Severity and explanation are downstream products, not sources of truth.

---

## What Survives vs What Gets Replaced

### Preserve

- board simulator and legality logic in `src/`
- move generation infrastructure
- replay ingestion path as a source of raw data
- Modal orchestration patterns in `training/scripts/modal_app.py`
- WASM bridge shell in `src/wasm.rs`

### Replace or Heavily Rewrite

- `training/scripts/preprocess_replays.py` output contract
- `training/models/lit_module.py::_derive_targets()`
- current `TeacherNet` / `StudentNet` role split
- `training/scripts/distill_student.py` as the main training strategy
- `training/scripts/export_weights.py` current 9-output manifest assumptions
- `src/student_model.rs` current fixed student manifest and output semantics
- `src/search_expand.rs` value-only model usage
- `src/analysis.rs` as move-quality arbiter

### Demote to Fallback / Telemetry

- `src/eval.rs`
- `src/attack.rs`
- `src/analysis.rs::assemble_composite()`-style score blending
- handcrafted threshold-heavy coaching labels

---

## Phase Plan

## Phase 0 — Canonical State Contract

**Objective:** make training examples and runtime state encoding provably consistent.

**Deliverables:**

- versioned example schema
- one canonical feature/state contract
- alignment rules for opponent snapshots and progression fields
- replay-group metadata for split safety
- verification scripts/tests for schema integrity

**Exit condition:** training and runtime can both encode the same replay frame into the same canonical state representation with documented field ownership.

## Phase 1 — Search + Player-Context Supervision

**Objective:** replace proxy teacher targets with policy/value supervision that separates tactical search strength from elite-player intent.

**Deliverables:**

- **Phase 1a control lane:** preserved search-aligned labels and training path for move-quality baseline comparisons
- **Phase 1b player-context lane:** new sidecars carrying elite-player next-action and trajectory context without collapsing them into search truth
- versioned artifact contracts that keep search targets, player targets, and context fields semantically separate
- model output spec for policy + value + context auxiliaries with explicit loss boundaries
- training loop that can compare search-only, player-context, and hybrid-aux supervision without blending incompatible targets

**Exit condition:** the model is no longer trained primarily to imitate handcrafted teacher proxies, and Phase 1 artifacts cleanly distinguish search strength from player-intent supervision.

## Phase 2 — Neural Runtime Ownership

**Objective:** make learned outputs guide search itself.

**Deliverables:**

- runtime manifest for policy/value outputs
- search integration that uses policy for expansion ordering and value for continuation evaluation
- fallback heuristics retained only behind explicit boundaries
- new evaluation/debug telemetry exposing learned-vs-fallback behavior

**Exit condition:** best-move selection is primarily driven by neural-guided search rather than handcrafted board/attack/context mixing.

## Phase 3 — Coaching Rebuild

**Objective:** rebuild coaching as a downstream view over stronger engine truth.

**Deliverables:**

- new move-evaluation payload with policy disagreement, value loss, and branch divergence
- recalibrated severity logic
- explanation hooks designed around continuation differences, not heuristic tags

**Exit condition:** coaching no longer depends on weak upstream score deltas for its core judgment.

---

## Sequencing Rules

1. Do **not** redesign explanations before engine truth improves.
2. Do **not** scale the current student architecture before Phase 0 contract repair.
3. Do **not** preserve handcrafted score blending as a co-equal controller in Phase 2.
4. Do **not** treat auxiliary concept heads as substitutes for policy/value.

---

## Success Criteria

The rebuild is on the right path if, in order:

1. canonical replay examples become stable and verifiable
2. policy/value labels replace proxy teacher targets as the main truth source, with search and player-context supervision kept contractually distinct
3. runtime search consumes learned outputs directly for move choice
4. coaching metrics are rebuilt on top of that stronger decision core

Anything short of that may improve the product, but it does not remove the structural ceiling.
