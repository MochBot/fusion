# Coaching Objective Layer Refactor — Additive Composite Search + Signal-First Coaching (formerly "V6 Eval Rework")

## TL;DR

> **Quick Summary**: Replace board-shape-dominant scoring with an offense/defense throughput-aware composite objective, then expose high-signal machine-readable coaching tags while explicitly cleaning legacy decision paths.
>
> **Deliverables**:
> - Composite scoring integrated into search expansion (TT-safe)
> - Three MVP insight primitives emitted as structured tags/metrics
> - Recalibrated severity behavior with regression gates
> - Legacy-path cleanup/deprecation with exact edit map and evidence
> - Rebuilt WASM artifacts at `mosaic-fusion-testing/src/lib/fusion/wasm/`
>
> **Estimated Effort**: Large
> **Parallel Execution**: YES — 3 implementation waves + final verification wave
> **Critical Path**: T2/T3/T4 → T7/T8/T9 → T11/T12/T13 → T16 → T14 → F1-F4

---

## Context

### Original Request
- Refactor the current plan into a more exact, cleanup-heavy architecture rollout, with precise edit targets and reduced legacy ambiguity.

### Interview Summary
**Key Decisions**:
- User preference: **signal quality over prose generation**. Implement detection tags/metrics first; narrative text is deferred.
- Architecture: **Option A additive composite scoring first**.
- Defer two-stage gates and policy-conditional objectives until additive rollout is validated.
- Enable **allspin-aware search detection** in V6 objective rollout (not T-only gating).
- Start with **fewer, higher-signal MVP primitives** and iterate with user feedback loops.
- Treat this as a **new objective layer** (naming beyond V5/V6 is acceptable); focus is architectural precision, not version-label continuity.

**Verified Codebase Anchors**:
- `src/search_expand.rs` (`gen_and_eval_root`, `expand_node`, `evaluate_with_tt`)
- `src/search.rs` (`find_best_move_with_scores_forced`, pruning/truncation path)
- `src/search_config.rs` (`SearchNode`, `SearchResultFull`, `SearchConfig`)
- `src/analysis.rs` (`classify_win_prob_drop`, `coaching_dp_multiplier`)
- `src/wasm.rs` (`MoveEvalResultJson`, `evaluate_position_wasm`)
- `src/attack.rs` (`calculate_attack`)
- `tests/presim_validation.rs` (calibration test harness)

### Metis Review (Applied)
**Gaps surfaced and pre-resolved in this plan**:
- Attack scoring is not yet wired into expansion score assembly.
- TT safety must be explicit: cache board eval only; add path terms outside TT.
- Composite-score range expansion requires futility/pruning retuning.
- Insight primitives need concrete output contract (tags/metrics), not stretch-goal prose.
- Guardrails needed to block scope creep (no NNUE/CMA-ES/policy-conditional branch gating).
- Legacy cleanup needed so old assumptions/paths don't silently conflict with new objective semantics.

### Design Rationale (Architecture Positioning)

#### Why this is NOT "just dropping weights"
- Weight dropping retunes static board heuristics; this refactor changes **search-time objective composition** with path-dependent terms (attack throughput, chain value, downstack efficiency intent gating).
- Decision quality is driven by sequence consequence over horizon, not only surface-board scalar shifts.

#### Why this is still presim-first (not rule-first)
- Presim beam search remains the ranking core (`best` vs `actual` from one search pass).
- Added signals are integrated into presim objective and interpretation, not replacing search with hand-authored rule trees.

#### Expected divergence vs MochBot-style closed-source engines
- Exact MochBot internals are unknown; this plan intentionally keeps low divergence in philosophy (deep-search truth) and higher divergence in explainability (explicit detector contracts and calibration evidence).
- This is a deliberate tradeoff to make model behavior auditable.

#### Gap vs ideal engine
- Ideal: full win-prob optimization with rich opponent/timing uncertainty modeling.
- This rollout: practical approximation with 14-piece horizon, offense/defense throughput proxies, and strict regression gates.
- Purpose: maximize signal quality now without destabilizing production.

#### JJ recovery invariant (execution safety)
- In this repo model, progress is not tied to staged state or only named commits; JJ records operation-level history for working-copy evolution.
- Recovery protocol for any suspected loss: inspect `jj op log`, then restore or extract with `jj op restore <op-id>` and/or `jj --at-op <op-id> file show -r @ <path>`.
- Planning guardrail: never claim work is unrecoverable until JJ operation history is checked; prefer JJ-native recovery over git-based state commands.

---

## Work Objectives

### Core Objective
Implement additive composite search scoring and emit high-value coaching insight tags so move quality reflects strategic consequence (attack/chain/context), while preserving presim-first architecture and verification rigor.

### Concrete Deliverables
- Composite score assembly in expansion path with TT-safe separation.
- Search result metadata for insight detection and calibration.
- Structured insight outputs in WASM contract (tags + numeric metrics).
- Single-player focus behavior defaulting coaching display to left player (P1) with settings override in Mosaic UI (external repo task).
- Legacy cleanup/deprecation pass removing stale objective-era assumptions and detector leftovers.
- Calibration evidence showing rank-tier behavior remains monotonic and useful.

### Definition of Done
- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test --lib` passes.
- [ ] `cargo test --test presim_validation` passes.
- [ ] WASM rebuild command succeeds and output files are updated under `mosaic-fusion-testing/src/lib/fusion/wasm/`.
- [ ] Task evidence files exist under `.sisyphus/evidence/` for every task.

### Must Have
- Composite score formula implemented in search expansion with explicit coefficients and bounded modifiers.
- Insight primitives implemented as **machine-readable tags/metrics**:
  - attack_window_miss
  - chain_break
  - downstack_efficiency_miss
- Severity pipeline remains sigmoid-based and rank-monotonic after recalibration.
- Forced-root behavior remains intact and comparable under new score distribution.
- Coaching visualization defaults to left player (P1) and never auto-shows both players simultaneously; user can switch target player in settings.

### Must NOT Have (Guardrails)
- No NNUE, CMA-ES, behavioral-cloning, or architecture pivots.
- No natural-language explanation generation in Rust/WASM path.
- No branch-gating architecture disguised as context modifiers.
- No broad frontend rewrites; only the scoped player-focus default behavior task is allowed on UI side.
- No speculative file references outside verified paths above.

## Exact Edit Map (Precision + Legacy Cleanup)

### Rust objective and scoring path (in-repo)
- `src/search_expand.rs`: integrate composite score assembly in `gen_and_eval_root` + `expand_node`; enable allspin-aware generation (`check_spin=true`), preserve TT-safe composition boundary.
- `src/search_config.rs`: own coefficient/config fields + composite metadata fields; no duplicate coefficient sources.
- `src/search.rs`: extraction/finality path for composite metrics and pruning behavior validation under expanded score range.
- `src/analysis.rs`: detector logic contracts and severity mapping interaction (no prose generation).
- `src/wasm.rs`: contract output fields for composite metrics/tags and wiring into `evaluate_position_wasm`.

### Legacy cleanup targets (in-repo)
- Remove stale detector references inconsistent with MVP scope (e.g., deferred primitives accidentally left in fixtures/contracts).
- Remove/retire old fallback assumptions that conflict with new intent-gated downstack detector semantics.
- Ensure no obsolete naming or dead fields remain in WASM output schema after detector consolidation.

### External Mosaic UI scope (cross-repo)
- Player-focus default behavior (P1) with explicit settings override, no auto-dual default rendering.
- Keep UI change narrow to targeting/focus flow; avoid unrelated visual refactors.

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — all verification steps are agent-executed.

### Test Decision
- **Infrastructure exists**: YES (Rust test + clippy + wasm build workflow already active)
- **Automated tests**: YES (**Tests-after**, default applied)
- **Framework**: `cargo test` + targeted integration tests in `tests/presim_validation.rs`

### QA Policy
- Every implementation task includes executable QA scenarios (happy + edge/failure).
- Evidence files written to `.sisyphus/evidence/task-{N}-*.txt`.
- Performance-sensitive gates capture wall-clock + pass/fail output snapshots.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start immediately — scaffolding and contracts, 6 parallel):
├── Task 1: Baseline snapshot + guardrail evidence [quick]
├── Task 2: Composite scoring config/constants surface [quick]
├── Task 3: SearchNode/SearchResult metadata extension [quick]
├── Task 4: Analysis scoring helpers + insight enum contracts [unspecified-high]
├── Task 5: Insight fixture scaffolding in presim tests [quick]
└── Task 6: WASM JSON contract placeholders for insight metrics [quick]

Wave 2 (After Wave 1 — core scoring integration, staged max-2 parallel):
├── Task 7: Root expansion composite scoring (TT-safe) [deep]
├── Task 8: Depth expansion path-attack carry + spin-mode wiring [deep]
├── Task 9: Pruning/truncation retune for widened score range [unspecified-high]
├── Task 10: Search result extraction for composite metrics [unspecified-high]
└── Task 11: WASM evaluation wiring for composite outputs [quick]

Wave 3 (After Wave 2 — signal detection + calibration + cleanup, staged max-3 parallel):
├── Task 12: Implement 3 MVP insight primitive detectors [deep]
├── Task 13: Calibration sweeps + threshold/coeff tuning [deep]
├── Task 14: WASM rebuild + integration evidence bundle [quick]
├── Task 15: Left-player default coaching focus in Mosaic UI (external repo) [visual-engineering]
└── Task 16: Legacy objective-path cleanup + deprecation sweep [unspecified-high]

Wave FINAL (After ALL implementation tasks — parallel independent review):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real QA gate suite replay (unspecified-high)
└── Task F4: Scope fidelity check (deep)

Critical Path: 2/3/4 → 7/8 → 9/10 → 11/12 → 13 → 16 → 14 → F1-F4
Parallel Speedup Target: ~40-50% over strictly sequential execution
Max Concurrent: 6 (Wave 1)
```

### Dependency Matrix (FULL)

| Task | Blocked By | Blocks |
|---|---|---|
| T1 | None | T13, F1 |
| T2 | None | T7, T8, T9, T10, T11 |
| T3 | None | T7, T8, T10, T11, T12 |
| T4 | None | T7, T8, T11, T12, T13 |
| T5 | None | T12, T13 |
| T6 | None | T11, T14 |
| T7 | T2,T3,T4 | T9, T10, T12, T13 |
| T8 | T2,T3,T4 | T9, T10, T12, T13 |
| T9 | T2,T7,T8 | T13 |
| T10 | T2,T3,T7,T8 | T11, T12, T13 |
| T11 | T2,T3,T4,T6,T10 | T14 |
| T12 | T3,T4,T5,T7,T8,T10 | T14 |
| T13 | T1,T4,T5,T7,T8,T9,T10 | T14, T16 |
| T14 | T6,T11,T12,T13,T16 | F1,F2,F3,F4 |
| T15 | None | F1,F2,F3,F4 |
| T16 | T3,T4,T11,T12,T13 | T14,F1,F2,F4 |
| F1 | T14,T15,T16 | completion |
| F2 | T14,T15,T16 | completion |
| F3 | T14,T15 | completion |
| F4 | T14,T15,T16 | completion |

### Agent Dispatch Summary

- **Wave 1 (6 agents)**:
  - T1 → `quick`
  - T2 → `quick`
  - T3 → `quick`
  - T4 → `unspecified-high`
  - T5 → `quick`
  - T6 → `quick`
- **Wave 2 (5 agents)**:
  - T7 → `deep`
  - T8 → `deep`
  - T9 → `unspecified-high`
  - T10 → `unspecified-high`
  - T11 → `quick`
- **Wave 3 (5 agents)**:
  - T12 → `deep`
  - T13 → `deep`
  - T14 → `quick`
  - T15 → `visual-engineering`
  - T16 → `unspecified-high`
- **Wave FINAL (4 agents)**:
  - F1 → `oracle`
  - F2 → `unspecified-high`
  - F3 → `unspecified-high`
  - F4 → `deep`

---

## TODOs

---

- [ ] 1. Baseline snapshot + guardrail evidence

  **What to do**:
  - Capture pre-change baseline outputs into `.sisyphus/evidence/task-1-baseline.txt`:
    - `CLOUD_EXEC_SKIP=1 cargo check`
    - `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`
    - `CLOUD_EXEC_SKIP=1 cargo test --lib`
    - `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation`
  - Record current constants and key anchors (SIGMOID_K, SIGMOID_C_BASE, futility_delta, beam_width, depth).
  - Lock guardrails in evidence: no NNUE/CMA-ES/policy-conditional branching/NLG generation in V6 scope.

  **Must NOT do**:
  - No source file modifications.
  - No threshold tuning in this task.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: command execution + evidence collation only.
  - **Skills**: [`verification-before-completion`]
    - `verification-before-completion`: ensures evidence-backed baseline before changes.
  - **Skills Evaluated but Omitted**:
    - `systematic-debugging`: no failing behavior under investigation yet.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2-T6)
  - **Blocks**: T13
  - **Blocked By**: None

  **References**:
  - `src/analysis.rs` - baseline sigmoid constants and severity classifier.
  - `src/search_config.rs` - baseline search/pruning defaults.
  - `src/search_expand.rs` - baseline expansion scoring path.
  - `tests/presim_validation.rs` - calibration and regression harness.

  **Acceptance Criteria**:
  - [ ] `.sisyphus/evidence/task-1-baseline.txt` exists with all baseline command outputs.
  - [ ] Guardrails are explicitly listed in the evidence file.

  **QA Scenarios**:
  ```
  Scenario: Baseline capture happy path
    Tool: Bash
    Preconditions: Clean working copy; project builds in current state
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo check`
      2. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`
      3. Run `CLOUD_EXEC_SKIP=1 cargo test --lib && CLOUD_EXEC_SKIP=1 cargo test --test presim_validation`
      4. Save outputs into `.sisyphus/evidence/task-1-baseline.txt`
    Expected Result: All commands exit 0 and evidence file contains command + result blocks
    Failure Indicators: Non-zero exit, missing command output block, missing file
    Evidence: .sisyphus/evidence/task-1-baseline.txt

  Scenario: Missing evidence failure
    Tool: Bash
    Preconditions: Evidence file deleted or empty
    Steps:
      1. Run `test -s .sisyphus/evidence/task-1-baseline.txt`
      2. If fail, mark task incomplete
    Expected Result: Command returns 0 only when file is non-empty
    Evidence: .sisyphus/evidence/task-1-baseline-validation.txt
  ```

  **Commit**: NO

- [ ] 2. Add composite scoring config surface

  **What to do**:
  - Extend `SearchConfig`/related config surfaces for additive objective coefficients and toggles needed by expansion scoring.
  - Add bounded defaults for initial rollout (conservative start) and document them inline.
  - Keep coefficient ownership centralized (single source of truth), not duplicated in multiple modules.

  **Must NOT do**:
  - No scoring logic changes in this task.
  - No pruning behavior changes in this task.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: data-structure and default wiring, low algorithmic risk.
  - **Skills**: [`verification-before-completion`]
    - `verification-before-completion`: confirms compile/lint after type changes.
  - **Skills Evaluated but Omitted**:
    - `property-based-testing`: not yet adding behavior-level invariants.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1,T3,T4,T5,T6)
  - **Blocks**: T7,T8,T9,T10,T11
  - **Blocked By**: None

  **References**:
  - `src/search_config.rs` - canonical location for search config defaults and structs.
  - `src/search.rs` - consumes config values in iteration path.

  **Acceptance Criteria**:
  - [ ] New config fields compile with defaults.
  - [ ] `cargo check` and clippy pass after config-only update.

  **QA Scenarios**:
  ```
  Scenario: Config wiring happy path
    Tool: Bash
    Preconditions: Config fields added in search_config.rs
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo check`
      2. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`
    Expected Result: Both commands exit 0; no missing-field or dead-code errors
    Failure Indicators: Struct init errors, unresolved fields, warning escalation
    Evidence: .sisyphus/evidence/task-2-config-compile.txt

  Scenario: Default bound validation
    Tool: Bash
    Preconditions: Defaults implemented
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation -- --nocapture`
      2. Check output has no panic from invalid coefficient bounds
    Expected Result: Test binary exits 0 with no bound assertion failures
    Evidence: .sisyphus/evidence/task-2-config-bounds.txt
  ```

  **Commit**: NO

- [ ] 3. Extend search metadata structures for composite outputs

  **What to do**:
  - Extend `SearchNode` and `SearchResultFull` to carry only the minimum additional metadata needed for downstream insight detection and WASM emission.
  - Keep TT-safe split explicit: board-eval cacheable component remains separate from path-dependent components.
  - Ensure existing forced-root and root_scores pathways continue to compile.

  **Must NOT do**:
  - No algorithmic rescoring here.
  - No changes to `eval.rs` feature set.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: schema evolution and compile-safety updates.
  - **Skills**: [`verification-before-completion`]
    - `verification-before-completion`: ensures all call sites are updated consistently.
  - **Skills Evaluated but Omitted**:
    - `systematic-debugging`: not diagnosing runtime bug yet.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1,T2,T4,T5,T6)
  - **Blocks**: T7,T8,T10,T11,T12
  - **Blocked By**: None

  **References**:
  - `src/search_config.rs` - `SearchNode` and `SearchResultFull` definitions.
  - `src/search.rs` - consumers/producers of `SearchResultFull`.
  - `src/wasm.rs` - downstream consumer of result metadata.

  **Acceptance Criteria**:
  - [ ] Struct changes compile across all dependent modules.
  - [ ] Existing tests that deserialize/use result structures still pass.

  **QA Scenarios**:
  ```
  Scenario: Metadata compile happy path
    Tool: Bash
    Preconditions: Struct fields updated with call-site propagation
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo check`
      2. Run `CLOUD_EXEC_SKIP=1 cargo test --lib`
    Expected Result: No missing-field errors and lib tests pass
    Failure Indicators: E0063/E0609 style field mismatches, failing search tests
    Evidence: .sisyphus/evidence/task-3-metadata-compile.txt

  Scenario: Backward-compat edge case
    Tool: Bash
    Preconditions: wasm.rs still compiles against updated structs
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`
      2. Verify no unreachable/default-fallback lint added for new fields
    Expected Result: Clippy exits 0
    Evidence: .sisyphus/evidence/task-3-metadata-clippy.txt
  ```

  **Commit**: NO

---

- [ ] 4. Add analysis-side composite helpers and insight contracts

  **What to do**:
  - Add helper functions in `src/analysis.rs` for:
    - chain value shaping (log/diminishing return)
    - context modifier shaping (bounded)
    - composite score assembly helpers used by search integration
  - Add/define insight tag contract enums/constants used by WASM output mapping.
  - Keep existing `classify_win_prob_drop` semantics intact unless Task 13 calibration requires bounded adjustment.

  **Must NOT do**:
  - No rewrite of severity taxonomy (`None/Inaccuracy/Mistake/Blunder`).
  - No prose rendering logic.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: medium-complexity scoring math with downstream coupling.
  - **Skills**: [`verification-before-completion`, `property-based-testing`]
    - `verification-before-completion`: keeps behavior assertions evidence-backed.
    - `property-based-testing`: helpful for monotonicity/boundedness checks.
  - **Skills Evaluated but Omitted**:
    - `brainstorming`: design phase already complete.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1,T2,T3,T5,T6)
  - **Blocks**: T7,T8,T11,T12,T13
  - **Blocked By**: None

  **References**:
  - `src/analysis.rs` - existing win-prob and multiplier functions.
  - `src/state.rs` - coaching state dimensions consumed by context modifier.

  **Acceptance Criteria**:
  - [ ] Helper functions compile and are unit-tested for bounded outputs.
  - [ ] Existing severity classifier behavior is preserved unless explicitly changed in calibration task.

  **QA Scenarios**:
  ```
  Scenario: Helper boundedness happy path
    Tool: Bash
    Preconditions: analysis helper functions added
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation -- --nocapture`
      2. Verify helper-bound assertions pass (no panic/out-of-range)
    Expected Result: Test exits 0, helper outputs stay within configured bounds
    Failure Indicators: panics, NaN, out-of-range coefficients
    Evidence: .sisyphus/evidence/task-4-helper-bounds.txt

  Scenario: Severity taxonomy regression edge case
    Tool: Bash
    Preconditions: analysis.rs modified
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --lib`
      2. Check severity-related tests still use canonical variants only
    Expected Result: No new/renamed severity levels introduced
    Evidence: .sisyphus/evidence/task-4-severity-regression.txt
  ```

  **Commit**: NO

- [ ] 5. Add deterministic insight fixture scaffolding in presim tests

  **What to do**:
  - Add deterministic board fixtures in `tests/presim_validation.rs` for each primitive family:
    - attack-window fixture
    - chain-break fixture
    - downstack-efficiency fixture
  - Add helper builders so calibration tasks can reuse fixtures without random seed search.
  - Keep fixtures minimal and reproducible.

  **Must NOT do**:
  - No random seed brute-force loops.
  - No flaky timing-dependent assertions.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: deterministic test scaffolding work.
  - **Skills**: [`verification-before-completion`]
    - `verification-before-completion`: ensures fixtures are deterministic and stable.
  - **Skills Evaluated but Omitted**:
    - `systematic-debugging`: this is forward test construction, not bug triage.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1,T2,T3,T4,T6)
  - **Blocks**: T12,T13
  - **Blocked By**: None

  **References**:
  - `tests/presim_validation.rs` - existing deterministic helper patterns.
  - `src/search.rs` - root score behavior to target in fixtures.

  **Acceptance Criteria**:
  - [ ] Fixture helpers compile and are reused by at least one primitive test placeholder each.
  - [ ] Repeated test runs are deterministic (same pass/fail outcome).

  **QA Scenarios**:
  ```
  Scenario: Deterministic fixture happy path
    Tool: Bash
    Preconditions: New fixture helpers added
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation -- --nocapture`
      2. Re-run the same command twice
      3. Compare pass/fail and key printed metrics for consistency
    Expected Result: Stable outputs across runs; no flaky failures
    Failure Indicators: intermittent failures, changing branch outcomes
    Evidence: .sisyphus/evidence/task-5-fixtures-deterministic.txt

  Scenario: Anti-randomness guard
    Tool: Bash
    Preconditions: Fixture task complete
    Steps:
      1. Run `grep -n "rand\|thread_rng\|seed" tests/presim_validation.rs`
      2. Ensure no new random-based fixture generation was introduced
    Expected Result: No newly added random fixture logic
    Evidence: .sisyphus/evidence/task-5-fixtures-no-random.txt
  ```

  **Commit**: NO

- [ ] 6. Extend WASM output contract placeholders for insight metrics

  **What to do**:
  - Extend `MoveEvalResultJson` in `src/wasm.rs` with placeholder fields for insight tags/metrics required by V6.
  - Populate safe defaults in serialization path before full detector wiring (done in later tasks).
  - Keep backward-compatible field naming conventions used in existing WASM exports.

  **Must NOT do**:
  - No frontend pipeline changes in this task.
  - No derived natural-language text fields.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: contract extension and serialization defaults.
  - **Skills**: [`verification-before-completion`]
    - `verification-before-completion`: ensures WASM contract remains build-safe.
  - **Skills Evaluated but Omitted**:
    - `frontend-ui-ux`: no UI work in this task.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1-T5)
  - **Blocks**: T11,T14
  - **Blocked By**: None

  **References**:
  - `src/wasm.rs` - `MoveEvalResultJson` and `evaluate_position_wasm` output assembly.
  - `mosaic-fusion-testing/src/lib/fusion/wasm/` - output artifacts path for eventual rebuild verification.

  **Acceptance Criteria**:
  - [ ] New JSON fields compile and serialize with defaults.
  - [ ] WASM build path remains valid after schema extension.

  **QA Scenarios**:
  ```
  Scenario: Contract compile happy path
    Tool: Bash
    Preconditions: wasm.rs contract fields added
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo check`
      2. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`
    Expected Result: No serialization/type errors
    Failure Indicators: missing field init or serde/wasm-bindgen failures
    Evidence: .sisyphus/evidence/task-6-contract-compile.txt

  Scenario: WASM schema edge case
    Tool: Bash
    Preconditions: Contract fields present
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 wasm-pack build --target web --out-dir mosaic-fusion-testing/src/lib/fusion/wasm --out-name fusion_wasm --features wasm --no-default-features`
      2. Verify command exits 0
    Expected Result: Build succeeds with extended schema
    Evidence: .sisyphus/evidence/task-6-contract-wasm-build.txt
  ```

  **Commit**: NO


- [ ] 7. Root expansion composite scoring (TT-safe)

  **What to do**:
  - In `gen_and_eval_root` (search_expand.rs), after `evaluate_with_tt` returns board_eval:
    1. Call `calculate_attack(lines_cleared, spin_type, next_b2b, next_combo, &ctx.attack_config, is_pc)` to get attack value for this root placement.
    2. Compute `chain_value = chain_value_bonus(next_b2b, next_combo)` using the helper from T4.
    3. Assemble composite score: `score = board_eval + β * attack_val + γ * chain_value`.
    4. Store `attack_val` and `chain_value` on the new SearchNode metadata fields (from T3), and initialize `root.path_attack = attack_val` for depth-consistent accumulation.
  - Enable `check_spin` globally for all piece placements: pass `true` as the spin-check flag to `generate()` so allspin opportunities are preserved.
  - Keep `evaluate_with_tt` caching board_eval ONLY — composite terms assembled outside TT.
  - Use coefficient values from SearchConfig (T2) — do not hardcode.

  **Must NOT do**:
  - No changes to `evaluate_with_tt` internals or TT key structure.
  - No changes to `eval.rs` features or weights.
  - No changes to `attack.rs` logic.
  - No piece-gated spin suppression (allspin support is required in this rollout).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: core algorithmic change touching hot path with TT-safety constraint.
  - **Skills**: [`verification-before-completion`, `systematic-debugging`]
    - `verification-before-completion`: ensures TT-safety and scoring correctness are evidence-backed.
    - `systematic-debugging`: methodical verification of composite score assembly.
  - **Skills Evaluated but Omitted**:
    - `test-driven-development`: tests come from T5 fixtures, not written here.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T8,T9,T10,T11)
  - **Blocks**: T9,T10,T12,T13
  - **Blocked By**: T2,T3,T4

  **References**:
  - `src/search_expand.rs` - `gen_and_eval_root` (line ~19), `evaluate_with_tt` (line ~119).
  - `src/attack.rs` - `calculate_attack` function.
  - `src/search_config.rs` - `SearchNode` struct, coefficient fields (from T2).
  - `src/gen.rs` - `generate()` function signature with `check_spin` parameter.

  **Acceptance Criteria**:
  - [ ] Root node scores include attack and chain_value components.
  - [ ] Root path attack is initialized to immediate `attack_val` so depth accumulation remains consistent.
  - [ ] Spin detection is enabled for all piece placements in root expansion.
  - [ ] TT cache stores board_eval only — verified by inspecting `evaluate_with_tt` call/return.
  - [ ] `cargo check` and `cargo clippy --all-targets -- -D warnings` pass.
  - [ ] `cargo test --lib` passes with no regressions.

  **QA Scenarios**:
  ```
  Scenario: Allspin-aware root scoring preserves high-value clears
    Tool: Bash
    Preconditions: Composite scoring wired in gen_and_eval_root
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation test_attack_integration -- --nocapture`
      2. Verify line-clearing moves outscore non-clearing moves in root_scores
    Expected Result: Test passes; line-clearing root scores > non-clearing root scores
    Failure Indicators: assertion failure, panic, or inverted ordering
    Evidence: .sisyphus/evidence/task-7-root-composite.txt

  Scenario: TT-safety verification
    Tool: Bash
    Preconditions: Same board evaluated via two different paths
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --lib test_transposition -- --nocapture`
      2. Verify TT hit returns same board_eval regardless of path
    Expected Result: TT-cached value matches fresh board_eval for same board hash
    Evidence: .sisyphus/evidence/task-7-tt-safety.txt
  ```

  **Commit**: NO

- [ ] 8. Depth expansion path-attack carry + spin-mode wiring

  **What to do**:
  - In `expand_node` (search_expand.rs), after `evaluate_with_tt` returns board_eval:
    1. Call `calculate_attack(lines_cleared, spin_type, next_b2b, next_combo, &ctx.attack_config, is_pc)` for the child placement.
    2. Accumulate: `child.path_attack = parent.path_attack + attack_val`.
    3. Compute `chain_value = chain_value_bonus(next_b2b, next_combo)`.
    4. Assemble composite: `score = board_eval + β * child.path_attack + γ * chain_value`.
  - Enable `check_spin` globally in `expand_node`'s `generate()` call: pass `true` so allspin paths remain available at depth.
  - Propagate `path_attack` through SearchNode so root_scores extraction can access cumulative attack for best path.

  **Must NOT do**:
  - No changes to TT internals.
  - No piece-gated spin suppression.
  - No path_attacks Vec (too expensive at 400K+ nodes) — use single f32 accumulator.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: path-dependent accumulation in hot loop with correctness constraints.
  - **Skills**: [`verification-before-completion`, `systematic-debugging`]
    - `verification-before-completion`: ensures path accumulation correctness.
    - `systematic-debugging`: catch accumulation bugs early.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T7,T9,T10,T11)
  - **Blocks**: T9,T10,T12,T13
  - **Blocked By**: T2,T3,T4

  **References**:
  - `src/search_expand.rs` - `expand_node` (line ~72), node construction.
  - `src/search_config.rs` - `SearchNode` metadata fields (path_attack from T3).
  - `src/attack.rs` - `calculate_attack`.

  **Acceptance Criteria**:
  - [ ] Child nodes accumulate parent's path_attack + own attack contribution.
  - [ ] Composite score at depth > 0 reflects cumulative path attack.
  - [ ] `cargo check` and `cargo clippy` pass.
  - [ ] `cargo test --lib` passes.

  **QA Scenarios**:
  ```
  Scenario: Path attack accumulates across depth
    Tool: Bash
    Preconditions: Depth expansion wired with path_attack carry
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --lib test_beam_search -- --nocapture`
      2. Verify best path's accumulated attack > 0 for boards with clearing opportunities
    Expected Result: Non-zero path_attack on best result for clearing-capable boards
    Failure Indicators: path_attack always 0, or NaN/inf values
    Evidence: .sisyphus/evidence/task-8-depth-carry.txt

  Scenario: Allspin detection at depth
    Tool: Bash
    Preconditions: check_spin enabled globally in expand_node
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation -- --nocapture`
      2. Check no regressions in existing presim tests
    Expected Result: All presim tests pass
    Evidence: .sisyphus/evidence/task-8-spin-depth.txt
  ```

  **Commit**: NO

- [ ] 9. Pruning/truncation retune for widened score range

  **What to do**:
  - Adjust `futility_delta` in SearchConfig to account for composite score range expansion.
    - Board eval range: ~-30 to 0. Attack adds ~0-20. Chain adds ~0-3. New range: ~-30 to +23.
    - Current delta 5.5 tuned for 30-unit range. New delta ≈ `5.5 * (53/30) ≈ 9.7`. Start with 10.0.
  - Verify pruning rate is reasonable: not over-pruning (killing attack paths) nor under-pruning (OOM/timeout).
  - Optionally add a `composite_futility_delta` field to SearchConfig if we want to keep the old delta for board-only fallback.
  - Verify forced-root protection still works correctly with wider score distribution.

  **Must NOT do**:
  - No algorithmic changes to pruning strategy (swap_remove + re-insert pattern stays).
  - No changes to beam truncation logic beyond delta value.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: empirical tuning with correctness verification.
  - **Skills**: [`verification-before-completion`]
    - `verification-before-completion`: ensures pruning behavior is evidence-backed.
  - **Skills Evaluated but Omitted**:
    - `property-based-testing`: useful but overkill for single-parameter tuning.

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T7/T8 scoring changes)
  - **Parallel Group**: Wave 2 (after T7,T8 complete)
  - **Blocks**: T13
  - **Blocked By**: T2,T7,T8

  **References**:
  - `src/search_config.rs` - `futility_delta` default.
  - `src/search.rs` - `apply_futility_pruning` logic.

  **Acceptance Criteria**:
  - [ ] Updated futility_delta compiles and doesn't cause test failures.
  - [ ] Beam search completes within 2× V5 wall-clock for standard test boards.
  - [ ] Forced-root move still survives pruning + truncation.

  **QA Scenarios**:
  ```
  Scenario: Pruning rate sanity check
    Tool: Bash
    Preconditions: futility_delta updated
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --lib -- --nocapture 2>&1 | grep -i prune`
      2. Run full test suite to ensure no timeout/OOM
    Expected Result: Tests complete within reasonable time; no OOM
    Failure Indicators: test timeout, memory errors, all nodes pruned
    Evidence: .sisyphus/evidence/task-9-pruning-retune.txt

  Scenario: Forced root survives wider distribution
    Tool: Bash
    Preconditions: Wider score range active
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --lib test_forced_root -- --nocapture`
      2. Verify forced move appears in final root_scores
    Expected Result: Forced move present in results despite wider pruning
    Evidence: .sisyphus/evidence/task-9-forced-root.txt
  ```

  **Commit**: NO

- [ ] 10. Search result extraction for composite metrics

  **What to do**:
  - At end of `run_beam_search_iteration` in search.rs, extract composite metadata from best node and root_scores:
    - `best_path_attack: f32` — cumulative attack along best path.
    - `best_chain_value: f32` — chain bonus at best leaf.
    - Update `root_scores` to carry composite score (already does via SearchNode.score).
  - Store extracted metrics in `SearchResultFull` fields (from T3).
  - Ensure `position_complexity` (V5) still computed correctly with new score distribution.

  **Must NOT do**:
  - No changes to beam search algorithm.
  - No new allocation-heavy data structures.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: metadata extraction from existing search output, moderate complexity.
  - **Skills**: [`verification-before-completion`]

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T7/T8 scoring)
  - **Parallel Group**: Wave 2 (after T7,T8)
  - **Blocks**: T11,T12,T13
  - **Blocked By**: T2,T3,T7,T8

  **References**:
  - `src/search.rs` - `run_beam_search_iteration` result assembly.
  - `src/search_config.rs` - `SearchResultFull` struct.

  **Acceptance Criteria**:
  - [ ] `SearchResultFull` carries `best_path_attack` and `best_chain_value`.
  - [ ] `position_complexity` still computes correctly.
  - [ ] `cargo check` and `cargo test --lib` pass.

  **QA Scenarios**:
  ```
  Scenario: Composite metadata extraction
    Tool: Bash
    Preconditions: Result extraction wired
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --lib test_position_complexity -- --nocapture`
      2. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation -- --nocapture`
    Expected Result: Both pass; position_complexity is non-negative
    Evidence: .sisyphus/evidence/task-10-result-extraction.txt
  ```

  **Commit**: NO

- [ ] 11. WASM evaluation wiring for composite outputs

  **What to do**:
  - In `evaluate_position_wasm` (wasm.rs), extract composite metrics from `SearchResultFull` and populate the extended `MoveEvalResultJson` fields (from T6):
    - `best_path_attack`, `best_chain_value` from full search result.
    - `actual_path_attack` — if actual move found in root_scores, extract its attack component.
  - Ensure coaching multiplier path still applies correctly with composite scores.
  - Populate insight tag placeholders with empty/default values (actual detection wired in T12).

  **Must NOT do**:
  - No insight detection logic in this task (that's T12).
  - No frontend/TS changes.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: WASM JSON field population from existing data.
  - **Skills**: [`verification-before-completion`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (after T10 completes)
  - **Parallel Group**: Wave 2 tail
  - **Blocks**: T14
  - **Blocked By**: T2,T3,T4,T6,T10

  **References**:
  - `src/wasm.rs` - `evaluate_position_wasm`, `MoveEvalResultJson`.
  - `src/search_config.rs` - `SearchResultFull` fields.

  **Acceptance Criteria**:
  - [ ] WASM output includes composite metric fields with correct values.
  - [ ] Coaching multiplier path unaffected.
  - [ ] `cargo check` passes.

  **QA Scenarios**:
  ```
  Scenario: WASM output includes composite fields
    Tool: Bash
    Preconditions: WASM wiring complete
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo check`
      2. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`
    Expected Result: Clean compile with new fields populated
    Evidence: .sisyphus/evidence/task-11-wasm-wiring.txt
  ```

  **Commit**: NO

---

- [ ] 12. Implement 3 MVP insight primitive detectors

  **What to do**:
  - Implement detection functions in `src/analysis.rs` (or a new `src/insights.rs` if cleaner) for each MVP primitive:

    1. **attack_window_miss**: Best path has `path_attack >= 4.0`, actual path has `path_attack < 1.0`. Tag fires when attack delta exceeds threshold.
    2. **chain_break**: Player's move has `b2b == 0` when parent had `b2b >= 2` AND best move maintains `b2b > 0`. Tag fires when B2B chain unnecessarily broken.
    3. **downstack_efficiency_miss**: Always compute `lpp = lines_cleared / pieces_used` over the 14-piece horizon for best vs actual path, but gate trigger by intent classification:
       - Compute `DefensePressure` and `OffenseOpportunity` per move using explicit inputs:
         - `DefensePressure = f(pending_garbage, stack_height_ratio, coaching_state ∈ {Fatal, Critical}, garbage_access_penalty)`
         - `OffenseOpportunity = f(best_path_attack, chain_value, b2b_continuation_score, board_cleanliness_ratio)`
       - Primary gate (2-signal):
         - `intent = upstack` if `OffenseOpportunity - DefensePressure > τ`
         - `intent = downstack` otherwise
       - Tiebreak only when near tie (`|OffenseOpportunity - DefensePressure| <= ε`): use `move_effect_delta` (garbage access + immediate LPP delta) to choose intent.
       - Initial defaults for calibration start: `τ = 0.30`, `ε = 0.10` (tuned in T13).
       - Trigger tag only when `intent = downstack` and `best_lpp - actual_lpp >= lpp_threshold`; suppress when `intent = upstack`.

  - Defer `intuition_trap` to iteration 2 after MVP detector stability is confirmed.

  - Wire detectors into `analyze_move_inner` and `evaluate_position_wasm` output paths.
  - Output as structured tags on `MoveEvalResultJson` (Vec<String> or similar).

  **Must NOT do**:
  - No natural-language generation.
  - No new search passes (detection uses existing search result data only).
  - No `setup_investment_payoff` or `intuition_trap` in MVP detector pass (deferred to iteration 2).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: multi-primitive detection logic with correctness constraints.
  - **Skills**: [`verification-before-completion`, `systematic-debugging`, `test-driven-development`]
    - `verification-before-completion`: each primitive needs evidence of correct triggering.
    - `systematic-debugging`: edge case debugging for false positives/negatives.
    - `test-driven-development`: write detector tests against T5 fixtures.

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T13 if fixtures ready)
  - **Parallel Group**: Wave 3
  - **Blocks**: T14
  - **Blocked By**: T3,T4,T5,T7,T8,T10

  **References**:
  - `src/analysis.rs` - existing severity pipeline, coaching multiplier.
  - `src/wasm.rs` - MoveEvalResultJson output path.
  - `tests/presim_validation.rs` - insight fixtures from T5.

  **Acceptance Criteria**:
  - [ ] 3 MVP detectors implemented (attack_window_miss, chain_break, downstack_efficiency_miss).
  - [ ] Each detector has at least one test that triggers it and one that doesn't.
  - [ ] Tags appear in MoveEvalResultJson when conditions are met.
  - [ ] `cargo test --lib` and `cargo test --test presim_validation` pass.

  **QA Scenarios**:
  ```
  Scenario: Insight primitive detection happy path
    Tool: Bash
    Preconditions: Detectors wired, fixtures from T5 present
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation test_insight -- --nocapture`
      2. Verify each fixture triggers exactly the expected tag
    Expected Result: Each primitive fires on its fixture, doesn't fire on others
    Failure Indicators: wrong tag, missing tag, false positive on unrelated fixture
    Evidence: .sisyphus/evidence/task-12-insight-detectors.txt

  Scenario: No false positives on clean board
    Tool: Bash
    Preconditions: Detectors active
    Steps:
      1. Run analysis on clean/empty board with best move
      2. Verify insight_tags is empty
    Expected Result: No insight tags fire when player plays the best move
    Evidence: .sisyphus/evidence/task-12-no-false-positives.txt
  ```

  **Commit**: NO

- [ ] 13. Calibration sweeps + threshold/coefficient tuning

  **What to do**:
  - Run corpus calibration using `test_calibration_severity_distributions` test with updated composite scoring.
  - Verify V5 calibration targets still hold with composite scores:
    - X+ ≥ 60% None, ≤ 5% Blunder
    - D ≥ 15% Mistake+Blunder
    - Monotonic: X+ ≥ S ≥ D in None%
  - If targets break, adjust in order:
    1. SIGMOID_C_BASE (shift sigmoid inflection).
    2. classify_win_prob_drop thresholds (0.05/0.12/0.22).
    3. Coefficient values (β, γ) if score distribution is fundamentally different.
  - Save calibration evidence to `.sisyphus/evidence/task-13-*` files.
  - Compare against T1 baseline to quantify distribution shift.

  **Must NOT do**:
  - No more than 3 calibration iterations (time-box).
  - No CMA-ES or automated parameter optimization.
  - No changes to insight detectors during calibration (those are independent).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: iterative tuning with evidence-backed convergence.
  - **Skills**: [`verification-before-completion`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T12 once T7-T10 done)
  - **Parallel Group**: Wave 3
  - **Blocks**: T14,F1,F3
  - **Blocked By**: T1,T4,T5,T7,T8,T9,T10

  **References**:
  - `src/analysis.rs` - SIGMOID_K, SIGMOID_C_BASE, classify_win_prob_drop.
  - `tests/presim_validation.rs` - calibration test harness.
  - `.sisyphus/evidence/task-1-baseline.txt` - V5 baseline for comparison.

  **Acceptance Criteria**:
  - [ ] Calibration test passes with composite scoring.
  - [ ] Distribution shift documented relative to T1 baseline.
  - [ ] Evidence files exist: `task-13-xrank.txt`, `task-13-drank.txt`, `task-13-monotonic.txt`.

  **QA Scenarios**:
  ```
  Scenario: Calibration target compliance
    Tool: Bash
    Preconditions: Composite scoring active, coefficients set
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation test_calibration -- --nocapture`
      2. Check X+ None% ≥ 60%, X+ Blunder% ≤ 5%, D M+B% ≥ 15%, monotonic
    Expected Result: All calibration assertions pass
    Failure Indicators: assertion failure on percentage thresholds
    Evidence: .sisyphus/evidence/task-13-calibration-pass.txt

  Scenario: Distribution shift report
    Tool: Bash
    Preconditions: Both baseline and post-composite results available
    Steps:
      1. Compare task-1-baseline.txt against task-13 outputs
      2. Document shift in each tier's None/Inac/Mistake/Blunder percentages
    Expected Result: Documented comparison with delta values
    Evidence: .sisyphus/evidence/task-13-distribution-shift.txt
  ```

  **Commit**: NO

- [ ] 14. WASM rebuild + integration evidence bundle

  **What to do**:
  - Rebuild WASM with all V6 changes:
    ```
    CLOUD_EXEC_SKIP=1 wasm-pack build --target web --out-dir mosaic-fusion-testing/src/lib/fusion/wasm --out-name fusion_wasm --features wasm --no-default-features
    ```
  - Verify all V6 exports are present (existing + new insight/composite fields).
  - Run `cargo clean -p direct-cobra-copy && rm -rf target/wasm32-unknown-unknown` before rebuild to clear stale compiled code.
  - Bundle evidence:
    - WASM binary size (must be ≤ 250KB).
    - Export list verification.
    - Sample `evaluate_position` output showing composite fields and insight tags.

  **Must NOT do**:
  - No frontend/UI changes.
  - No source code modifications in this task.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: build command execution + evidence collection.
  - **Skills**: [`verification-before-completion`]

  **Parallelization**:
  - **Can Run In Parallel**: NO (final integration gate)
  - **Parallel Group**: Wave 3 (after T11,T12,T13)
  - **Blocks**: F1,F2,F3,F4
  - **Blocked By**: T6,T11,T12,T13

  **References**:
  - `mosaic-fusion-testing/src/lib/fusion/wasm/` - output artifacts path.
  - `src/wasm.rs` - export surface.

  **Acceptance Criteria**:
  - [ ] WASM build succeeds with exit code 0.
  - [ ] Binary size ≤ 250KB.
  - [ ] All V5 exports still present + V6 composite/insight fields in output.
  - [ ] Evidence file: `.sisyphus/evidence/task-14-wasm-build.txt`.

  **QA Scenarios**:
  ```
  Scenario: WASM build + export verification
    Tool: Bash
    Preconditions: All V6 source changes complete
    Steps:
      1. Run `cargo clean -p direct-cobra-copy && rm -rf target/wasm32-unknown-unknown`
      2. Run WASM build command
      3. Check binary size: `ls -la mosaic-fusion-testing/src/lib/fusion/wasm/fusion_wasm_bg.wasm`
      4. Verify exports present
    Expected Result: Build succeeds, ≤250KB, all exports present
    Failure Indicators: build failure, oversized binary, missing exports
    Evidence: .sisyphus/evidence/task-14-wasm-build.txt
  ```

  **Commit**: NO

- [ ] 15. Left-player default coaching focus in Mosaic UI (external repo)

  **What to do**:
  - Implement this in the full Mosaic UI repository (external to this checkout), not in `fusion-engine` source files.
  - Change analysis display behavior so coaching focus defaults to **left player / player index 0 (P1)**.
  - Remove automatic simultaneous display for both players; keep a settings control to switch the focused target player explicitly.
  - Ensure worker/result plumbing respects target-player mode so interaction/timing overlays are interpreted from one active perspective at a time.

  **Must NOT do**:
  - No removal of the ability to switch players in settings.
  - No permanent hard-lock to P1 (default-only, user-overridable).
  - No Rust scoring changes in this task.

  **Recommended Agent Profile**:
  - **Category**: `visual-engineering`
    - Reason: UI state/default behavior and settings UX wiring.
  - **Skills**: [`frontend-ui-ux`, `verification-before-completion`]
    - `frontend-ui-ux`: targeted UI behavior change with minimal confusion.
    - `verification-before-completion`: evidence-backed browser validation.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (can start early; independent of T12/T13/T14)
  - **Blocks**: F1,F2,F3,F4
  - **Blocked By**: None

  **References**:
  - `mosaic-fusion-testing/src/lib/fusion/wasm/` - in-repo artifact output path that UI consumes.
  - **External repo required**: full Mosaic UI source tree containing ReplayViewer/settings/worker orchestration.

  **Acceptance Criteria**:
  - [ ] Opening analysis defaults to P1/left-player focus.
  - [ ] Settings can switch focus to other player(s) without reloading session.
  - [ ] No simultaneous dual-player auto-display in default mode.
  - [ ] Evidence captured from UI run demonstrating default and override behaviors.

  **QA Scenarios**:
  ```
  Scenario: Default target-player behavior
    Tool: Playwright (external Mosaic repo)
    Preconditions: Replay loaded with two players
    Steps:
      1. Open replay viewer and start analysis
      2. Observe initial focused player indicator and coaching pins/overlays
      3. Verify focus is P1 (left) by default
    Expected Result: Only P1-focused coaching signals shown initially
    Failure Indicators: dual auto-display or defaulting to non-P1
    Evidence: .sisyphus/evidence/task-15-default-player-focus.txt

  Scenario: Settings override behavior
    Tool: Playwright (external Mosaic repo)
    Preconditions: Analysis results present
    Steps:
      1. Open settings and switch focused player to P2
      2. Verify displayed coaching markers/insights swap to selected player context
      3. Switch back to P1 and re-verify
    Expected Result: Controlled single-player focus follows settings selection
    Failure Indicators: stale indicators, mixed dual-player overlays, no switch effect
    Evidence: .sisyphus/evidence/task-15-settings-override.txt
  ```

  **Commit**: NO

- [ ] 16. Legacy objective-path cleanup + deprecation sweep

  **What to do**:
  - Remove stale references and placeholders that no longer match the new objective layer:
    - Old/deferred detector identifiers that leaked into tests/contracts/comments.
    - Legacy fallback assumptions that conflict with intent-gated `downstack_efficiency_miss` behavior.
  - Ensure schema and docs only reference active MVP detectors:
    - `attack_window_miss`, `chain_break`, `downstack_efficiency_miss`
  - Keep cleanup surgical: remove dead/stale paths, do not redesign APIs.

  **Must NOT do**:
  - No broad module rewrites.
  - No feature additions outside cleanup scope.
  - No silent behavior changes beyond removing stale legacy paths.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: broad but precise cleanup across multiple touched modules.
  - **Skills**: [`verification-before-completion`, `systematic-debugging`]
    - `verification-before-completion`: validate no stale references remain.
    - `systematic-debugging`: avoid accidental regression during cleanup.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with T12,T13,T14,T15)
  - **Blocks**: T14,F1,F2,F4
  - **Blocked By**: T3,T4,T11,T12,T13

  **References**:
  - `src/analysis.rs` - detector enums/constants and classification path.
  - `src/wasm.rs` - output schema fields and serialization.
  - `tests/presim_validation.rs` - fixture names and detector assertions.
- `.sisyphus/plans/coaching-objective-layer-refactor.md` - active detector scope and guardrails.

  **Acceptance Criteria**:
  - [ ] No stale detector names remain in active code/test paths.
  - [ ] Cleanup does not break compile/lint/tests.
  - [ ] Cleanup evidence file exists with grep-based verification output.

  **QA Scenarios**:
  ```
  Scenario: Stale-reference cleanup verification
    Tool: Bash
    Preconditions: Cleanup edits complete
    Steps:
      1. Run `grep -Rin "intuition_trap\|setup_investment_payoff\|false_safety_clear" src tests || true`
      2. Verify only explicitly deferred/documented references remain (if any)
      3. Save output to evidence file
    Expected Result: No unintended stale references in active implementation paths
    Failure Indicators: stale identifiers still present in runtime/test contracts
    Evidence: .sisyphus/evidence/task-16-cleanup-scan.txt

  Scenario: Regression guard after cleanup
    Tool: Bash
    Preconditions: Cleanup applied
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo check`
      2. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`
      3. Run `CLOUD_EXEC_SKIP=1 cargo test --lib && CLOUD_EXEC_SKIP=1 cargo test --test presim_validation`
    Expected Result: All gates pass after cleanup
    Failure Indicators: compile errors, lint errors, test regressions
    Evidence: .sisyphus/evidence/task-16-cleanup-regression.txt
  ```

  **Commit**: NO
---

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  - Verify every Must Have/Must NOT Have against code and evidence artifacts.
  - Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  - Run clippy/tests/build gates and inspect for anti-patterns (`as any`, dead code, debug prints, hidden fallback logic).
  - Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N/N] | VERDICT`

- [ ] F3. **Real QA Gate Suite** — `unspecified-high`
  - Execute all task QA scenarios end-to-end; ensure evidence artifacts are present and reproducible.
  - Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  - Compare final diffs against task scope; flag scope creep, cross-task contamination, and unaccounted edits.
  - Output: `Tasks [N/N compliant] | Scope [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

- **Commit 1 (Wave 1)**: `feat(coaching-layer): add composite scoring scaffolding and contracts`
- **Commit 2 (Wave 2)**: `feat(search): integrate composite scoring into expansion and pruning`
- **Commit 3 (Wave 3A)**: `feat(analysis): add mvp insight primitives and calibration gates`
- **Commit 4 (Wave 3B)**: `refactor(cleanup): remove stale objective-era detector paths`
- **Commit 5 (Wave 3C)**: `build(wasm): rebuild coaching-layer wasm artifacts and evidence bundle`

---

## Success Criteria

### Verification Commands
```bash
CLOUD_EXEC_SKIP=1 cargo check
CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings
CLOUD_EXEC_SKIP=1 cargo test --lib
CLOUD_EXEC_SKIP=1 cargo test --test presim_validation
CLOUD_EXEC_SKIP=1 wasm-pack build --target web --out-dir mosaic-fusion-testing/src/lib/fusion/wasm --out-name fusion_wasm --features wasm --no-default-features
```

### Final Checklist
- [ ] Composite scoring is active in root + depth expansion.
- [ ] Insight tags/metrics are present in WASM output and evidence.
- [ ] Calibration outputs meet monotonic and severity targets.
- [ ] Legacy detector/objective stale paths are removed or explicitly deferred.
- [ ] All automated gates pass.
- [ ] No deferred-scope items were pulled in.
