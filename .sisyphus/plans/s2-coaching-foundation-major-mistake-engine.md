# TETR.IO Season 2 Coaching Foundation (Major-Mistake-First)

## TL;DR

> **Quick Summary**: Build a deterministic, backend-only Season 2 coaching core that prioritizes major mistakes (fatality/obligation failures) over micro-inefficiencies, while using TetraStats-derived signals for calibration/context (not hot move ranking).
>
> **Deliverables**:
> - Deterministic lexicographic decision kernel with strict priority gates
> - Season-2 state machines (fatality/obligation/surge/phase/plonk)
> - Major-mistake taxonomy + severity calibration pipeline
> - Replay validation harness (recall / false-severe / determinism)
>
> **Estimated Effort**: XL
> **Parallel Execution**: YES — 5 waves
> **Critical Path**: Task -1 → Task 0 → Task 1 → Task 2 → Task 3 → Task 5 → Task 6 → Task 8

---

## Context

### Original Request
User asked to gather all insights (including TetraStats formulas and wiki context), keep Season 2 as canonical mechanics scope, and generate a high-accuracy plan with Metis/Momus review for a perfection-grade coaching foundation.

### Interview Summary
**Key Discussions**:
- Major-mistake-first behavior is required; tiny inaccuracies should be de-emphasized.
- Lexicographic decision policy is preferred over pure weighted scalar blending.
- Season 2 mechanics (surge/cancel timing/opener interactions) must be first-class.
- TetraStats should be mined deeply, but relevance-filtered for coaching backend.

**Research Findings**:
- Existing Fusion code already has staged ranking and strategic reason tags:
  - `src/search.rs`
  - `src/analysis.rs`
  - `src/wasm.rs`
- Existing attack decomposition and S2 parity-oriented formulas:
  - `src/attack.rs`
- Current baseline defaults include danger/critical thresholds and deterministic beam controls:
  - `src/search.rs`
- TetraStats high-value formula sources:
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/nerd_stats.dart`
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/playstyle.dart`
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/minomuncher.dart`
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/views/destination_calculator.dart`
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/services/tetrio_crud.dart`
- Wiki reference reviewed (marked under development; treat as secondary reference):
  - `https://github.com/dan63047/TetraStats/wiki/Meaning-and-the-essence-of-stats`

### Metis Review
**Identified Gaps (addressed in this plan)**:
- Missing explicit obligation/cancel timing state in core decision layer
- Need hard S2 boundary guardrails to prevent season-leak behavior
- Need acceptance gates centered on severe-error recall and false-severe suppression
- Need replay-determinism checks for stable coaching output
- Need to constrain TetraStats usage to context/calibration unless ablation proves hot-path value

---

## Work Objectives

### Core Objective
Deliver a deterministic Season 2 coaching kernel that reliably detects major strategic mistakes, suppresses noise from tiny deltas, and is fully verifiable via backend-only replay/test harnesses.

### Concrete Deliverables
- New/updated decision-state model for S2 obligations and survival gates
- Lexicographic enforcement in search and analysis outputs
- Major mistake taxonomy with deterministic trigger conditions
- Skill-bucket-aware calibration module fed by replay/stat features
- Deterministic validation suite with pass/fail thresholds

### Definition of Done
- [x] All new logic passes backend test suite (`cargo test`) with deterministic outputs
- [x] Severe-error recall and false-severe metrics available from replay harness
- [x] S2 boundary checks prevent accidental S1 constant leakage into critical policy path
- [x] Coaching output includes machine-level reason fields and obligation/fatality context

### Must Have
- Strict lexicographic stack: survival > obligation > tactical/surge > structure
- Deterministic replay reproducibility checks
- Major-mistake taxonomy aligned to S2 mechanics
- No frontend phrasing dependencies

### Must NOT Have (Guardrails)
- No UI-language generation scope
- No replacing deterministic core with opaque model scoring
- No direct insertion of broad TetraStats aggregate metrics into hot move-ranking path without ablation proof
- No acceptance criteria requiring human manual verification

---

## Verification Strategy (MANDATORY)

> **UNIVERSAL RULE: ZERO HUMAN INTERVENTION**
>
> All tasks must be verifiable by commands, backend harnesses, or scripted assertions only.

### Test Decision
- **Infrastructure exists**: YES (Rust tests + integration tests)
- **Automated tests**: YES (Tests-after)
- **Framework**: `cargo test` (+ deterministic replay harness commands)

### Agent-Executed QA Scenarios (MANDATORY — ALL tasks)

Each task below includes explicit scenarios with exact tool/steps/assertions/evidence.

---

## Execution Strategy

### Parallel Execution Waves

```text
Wave 0 (Precondition Gate):
└── Task -1: Replay corpus acquisition + qualification gate

Wave 1 (After Wave 0):
├── Task 0: Baseline + S2 boundary contract
└── Task 4: TetraStats feature whitelist + extraction module design

Wave 2 (After Wave 1):
├── Task 1: State machine schema (fatality/obligation/surge/phase/plonk)
├── Task 2: Search lexicographic hard-gates + protected pruning
└── Task 5: Severity kernel v2 (major-first taxonomy)

Wave 3 (After Wave 2):
├── Task 3: Cancel-window/spawn-envelope integration
├── Task 6: Skill-bucket calibration pipeline
└── Task 7: WASM machine diagnostics contract updates

Wave 4 (After Wave 3):
└── Task 8: Replay validation harness + promotion gates

Critical Path: -1 -> 0 -> 1 -> 2 -> 3 -> 5 -> 6 -> 8
```

### Dependency Matrix

| Task | Depends On | Blocks | Parallelizable With |
|------|------------|--------|---------------------|
| -1 | None | 0,1,2,3,4,5,6,7,8 | None |
| 0 | -1 | 1,2,3,5,8 | 4 |
| 1 | 0 | 2,3,5,7 | 4 |
| 2 | 1 | 3,5,8 | 5 |
| 3 | 2 | 5,8 | 6,7 |
| 4 | -1,0 | 6 | 1,2 |
| 5 | 2,3 | 6,8 | 7 |
| 6 | 4,5 | 8 | 7 |
| 7 | 1,5 | 8 | 6 |
| 8 | 2,3,5,6,7 | Final | None |

### Agent Dispatch Summary

| Wave | Tasks | Recommended Agents |
|------|-------|--------------------|
| 0 | -1 | `task(category="unspecified-high", load_skills=["recursive-decomposition","verification-before-completion"], run_in_background=false)` |
| 1 | 0,4 | `task(category="unspecified-high", load_skills=["recursive-decomposition","writing-plans"], run_in_background=false)` |
| 2 | 1,2,5 | Same category; add `systematic-debugging` for gate logic |
| 3 | 3,6,7 | `unspecified-high` + `verification-before-completion` |
| 4 | 8 | `unspecified-high` + `verification-before-completion` |

---

## TODOs

- [x] -1. Acquire and qualify replay corpus (hard precondition)

  **What to do**:
  - Build calibration corpus before implementation tasks proceed.
  - Enforce user-approved corpus constraints:
    - rank-depth sampling from **B through U** (`b`, `b+`, `a-`, `a`, `a+`, `s-`, `s`, `s+`, `ss`, `u`)
    - 3 unique players per rank
    - 5 replays per player
    - 1v1 ranked only
    - replay age <= last 3 months
  - Generate deduplicated manifest keyed by replay ID + player ID.
  - Produce per-rank summary counts and qualification report.

  **Must NOT do**:
  - Do not proceed to implementation tasks if any bucket fails minimum constraints.
  - Do not mix non-1v1 ranked or stale replay windows into calibration corpus.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `recursive-decomposition`, `verification-before-completion`

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 0 (precondition gate)
  - **Blocks**: 0,1,2,3,4,5,6,7,8
  - **Blocked By**: None

  **References**:
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/services/tetrio_crud.dart` — replay retrieval and caching patterns.
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/tetrio_multiplayer_replay.dart` — replay structure/parsing assumptions.
  - `mosaic-fusion-testing/scripts/lockstep/replays/` — existing local replay artifacts.
  - `mosaic-fusion-testing/scripts/lockstep/replays50/` — additional local replay artifacts (dedup required).

  **Acceptance Criteria**:
  - [x] Rank manifest exists with exactly 10 ranks (`b`..`u`) and required metadata fields.
  - [x] Each rank has >= 3 unique players.
  - [x] Each selected player has >= 5 qualified replays (minimum corpus target: 150 replays).
  - [x] All replays pass mode/date filters (1v1 ranked only, <=3 months old).
  - [x] Qualification command exits 0; plan proceeds only if gate passes.

  **Collection Procedure (implemented)**:
  - Collection method: manual CLI requests only (no automation script).
  - Current output root template:
    - `.sisyphus/data/replay-corpus/s2-ranked-1v1-rd70-165/`
  - Current active run folder:
    - `.sisyphus/data/replay-corpus/s2-ranked-1v1-rd70-165/by-rank/`
  - Request policy used for collection:
    - 2-3 second cooldown between requests
    - stop immediately on first HTTP error and resume next loop after break
    - prioritize players with `RD < 70`
  - Generated artifacts:
    - rank/name folder structure with `.ttrm` payloads under `by-rank/<rank>/<username>/`

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Corpus qualification gate
    Tool: Bash
    Preconditions: manual replay pulls completed in rank/name folder layout
    Steps:
      1. Count replay files per selected player directory
      2. Aggregate per-rank counts
      3. Assert all constraints pass (rank coverage/player count/replay count/mode/date)
      4. Save machine-readable report
    Expected Result: Corpus gate PASS before any implementation task starts
    Evidence: .sisyphus/evidence/task-minus1-corpus-gate.json

  Scenario: Duplicate replay integrity check
    Tool: Bash
    Preconditions: manifest generated
    Steps:
      1. Count duplicate replay IDs across merged sources
      2. Assert duplicates removed in final qualified set
    Expected Result: No duplicate replay/player pairs in calibration corpus
    Evidence: .sisyphus/evidence/task-minus1-dedup-report.txt
  ```

- [x] 0. Establish S2 boundary contract and baseline invariants

  **What to do**:
  - Define explicit Season 2 policy boundary and denylist for S1 leakage in critical decision path.
  - Document canonical formulas/assumptions source precedence: Fusion code > parity tests > TetraStats code > wiki prose.
  - Lock baseline current behavior snapshots before refactor.

  **Must NOT do**:
  - Do not change move-ranking behavior yet.
  - Do not introduce new heuristics before invariants are frozen.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `writing-plans`, `recursive-decomposition`
  - **Skills Evaluated but Omitted**:
    - `frontend-ui-ux`: out of backend scope.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 4)
  - **Blocks**: 1,2,3,5,8
  - **Blocked By**: -1

  **References**:
  - `src/search.rs` — existing lexicographic comparator and gate-related defaults.
  - `src/attack.rs` — S2 damage decomposition already implemented.
  - `.sisyphus/plans/s2-coaching-foundation-major-mistake-engine.md` — active source-of-truth plan.
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/views/destination_calculator.dart` — reference formula ordering and surge parameters.
  - `https://github.com/dan63047/TetraStats/wiki/Meaning-and-the-essence-of-stats` — secondary conceptual reference only.

  **Acceptance Criteria**:
  - [x] `cargo test --quiet` runs with no new failures compared to baseline snapshot.
  - [x] A policy-boundary checklist file exists in plan artifacts and is consumed by later tasks.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Baseline freeze integrity
    Tool: Bash
    Preconditions: Repo builds locally
    Steps:
      1. Run: cargo test --quiet
      2. Capture pass/fail counts and save output
      3. Assert: exit code 0
    Expected Result: Baseline remains green before changes
    Evidence: .sisyphus/evidence/task-0-baseline-test.txt

  Scenario: S2 boundary lint sanity
    Tool: Bash (grep)
    Preconditions: boundary denylist prepared
    Steps:
      1. Run grep checks for prohibited S1 constants in critical files
      2. Assert: zero matches in protected modules
    Expected Result: No season leakage in protected path
    Evidence: .sisyphus/evidence/task-0-boundary-grep.txt
  ```

- [x] 1. Introduce deterministic coaching-state schema (fatality/obligation/surge/phase/plonk)

  **What to do**:
  - Add first-class state types for major-mistake detection.
  - Ensure state transitions are deterministic and replay-reconstructible.

  **Must NOT do**:
  - Do not add probabilistic/ML-only state labels in core path.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `recursive-decomposition`, `test-driven-development`

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 0)
  - **Parallel Group**: Wave 2
  - **Blocks**: 2,3,5,7
  - **Blocked By**: 0

  **References**:
  - `src/state.rs` — current minimal game state.
  - `src/search.rs` — current survival tier and tactical structure fields.
  - `src/analysis.rs` — strategic reason pipeline sink.
  - `tests/presim_validation.rs` — integration harness extension point.

  **Acceptance Criteria**:
  - [x] New state fields compile and serialize deterministically.
  - [x] Unit tests cover transition functions for each state machine.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: State transition determinism
    Tool: Bash
    Preconditions: New state tests added
    Steps:
      1. Run: cargo test state::tests -- --nocapture
      2. Re-run same command
      3. Compare outputs/hash of serialized state snapshots
    Expected Result: Identical transition outputs across runs
    Evidence: .sisyphus/evidence/task-1-state-determinism.txt
  ```

- [x] 2. Enforce hard lexicographic gates in search and protected pruning

  **What to do**:
  - Make survival and obligation gates non-overridable by lower-tier gains.
  - Update pruning protection rules to preserve high-priority obligations.

  **Must NOT do**:
  - Do not collapse back to a scalar-only comparator.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `systematic-debugging`, `test-driven-development`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 5 after interfaces align)
  - **Parallel Group**: Wave 2
  - **Blocks**: 3,5,8
  - **Blocked By**: 1

  **References**:
  - `src/search.rs` — comparator, pruning, expansion scoring.
  - `src/attack.rs` — tactical breakdown terms.
  - `src/eval.rs` — structure/static score base.

  **Acceptance Criteria**:
  - [x] Tests prove survival/obligation class wins even when scalar score is lower.
  - [x] Existing search tests remain green or are updated with rationale.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Comparator precedence enforcement
    Tool: Bash
    Preconditions: Comparator tests added/updated
    Steps:
      1. Run: cargo test search::tests::test_compare_prefers_survival_before_raw_score
      2. Run: cargo test search::tests::test_futility_preserves_best_survival_tier
      3. Assert: both pass
    Expected Result: High-priority policy is provably dominant
    Evidence: .sisyphus/evidence/task-2-lexicographic-tests.txt

  Scenario: Regression on full search tests
    Tool: Bash
    Preconditions: Search module compiles
    Steps:
      1. Run: cargo test search::tests
      2. Assert: exit code 0
    Expected Result: No search regressions
    Evidence: .sisyphus/evidence/task-2-search-suite.txt
  ```

- [x] 3. Implement cancel-window and spawn-envelope obligation modeling

  **What to do**:
  - Add explicit imminent-garbage and spawn-safety obligations to state and search ranking.
  - Encode must-cancel / must-downstack transitions.

  **Must NOT do**:
  - Do not infer obligations from height alone.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `recursive-decomposition`, `systematic-debugging`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 6/7 once interfaces stable)
  - **Parallel Group**: Wave 3
  - **Blocks**: 5,8
  - **Blocked By**: 2

  **References**:
  - `src/state.rs` — add incoming timing context.
  - `src/search.rs` — gate ranking integration.
  - `src/analysis.rs` — reason emission for obligation misses.
  - `src/board.rs` — spawn/occupancy checks.

  **Acceptance Criteria**:
  - [x] Must-cancel scenarios are deterministically identified.
  - [x] Spawn-envelope violation scenarios always classify as fatal tier.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Must-cancel detection
    Tool: Bash
    Preconditions: obligation fixtures added
    Steps:
      1. Run targeted test for incoming garbage + available clear
      2. Assert engine labels missed clear as obligation failure
    Expected Result: Obligation failures are consistently detected
    Evidence: .sisyphus/evidence/task-3-must-cancel.txt

  Scenario: Spawn-envelope fatality gate
    Tool: Bash
    Preconditions: fatality fixtures added
    Steps:
      1. Run targeted test with near-killbox and blocked spawn envelope
      2. Assert: classified as fatal regardless of tactical gain
    Expected Result: Fatality precedence enforced
    Evidence: .sisyphus/evidence/task-3-spawn-envelope.txt
  ```

- [x] 4. Implement TetraStats feature whitelist extraction (context/calibration only)

  **What to do**:
  - Implement deterministic extraction of selected metrics only:
    - DSP, DSS, cheese index family
    - surgeRate/surgeLength/surgeDS/APL
    - opener vs midgame pace splits
    - playstyle axes (opener/plonk/stride/infds)
  - Keep feature module isolated from hot move-ranking path.

  **Must NOT do**:
  - Do not import full TetraStats pipeline or bridge/network dependencies into Fusion runtime path.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `recursive-decomposition`, `test-driven-development`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1
  - **Blocks**: 6
  - **Blocked By**: 0

  **References**:
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/nerd_stats.dart`
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/playstyle.dart`
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/minomuncher.dart`
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/views/destination_calculator.dart`
  - `src/analysis.rs` (destination for calibration context usage)

  **Acceptance Criteria**:
  - [x] Feature extractor outputs deterministic values for fixed fixtures.
  - [x] No direct dependency from search ranking to non-whitelisted aggregate profile scores.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Formula parity on fixed fixtures
    Tool: Bash
    Preconditions: fixture vectors added
    Steps:
      1. Run: cargo test tetrastats_feature_tests -- --nocapture
      2. Assert computed values match expected tolerances
    Expected Result: Deterministic feature outputs
    Evidence: .sisyphus/evidence/task-4-feature-parity.txt
  ```

- [x] 5. Redesign severity kernel to major-first taxonomy

  **What to do**:
  - Add explicit major classes (e.g., lethal negligence, surge leak, opener obligation fail, plonk spiral).
  - Ensure obligation/fatality regressions force severe classification irrespective of tiny eval deltas.

  **Must NOT do**:
  - Do not rely solely on absolute eval-loss thresholds for major labels.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `systematic-debugging`, `verification-before-completion`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 7)
  - **Parallel Group**: Wave 2/3 bridge
  - **Blocks**: 6,8
  - **Blocked By**: 2,3

  **References**:
  - `src/analysis.rs` — classify and reason mapping.
  - `src/wasm.rs` — output contract extension.
  - `tests/presim_validation.rs` — scenario-level outcome checks.

  **Acceptance Criteria**:
  - [x] Major classes trigger deterministically on fixture scenarios.
  - [x] Existing reason tags remain backward-compatible where required.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Major mistake trigger validation
    Tool: Bash
    Preconditions: taxonomy tests added
    Steps:
      1. Run: cargo test analysis::tests -- --nocapture
      2. Assert severe class triggers on fatality/obligation fixtures
      3. Assert minor fixtures do not escalate to severe
    Expected Result: Major-first kernel behaves as intended
    Evidence: .sisyphus/evidence/task-5-major-taxonomy.txt
  ```

- [x] 6. Build skill-bucket calibration pipeline (deterministic)

  **What to do**:
  - Create deterministic calibration tables by skill bucket.
  - Use replay-derived context features to adjust noise tolerance while preserving severe recall.

  **Must NOT do**:
  - Do not introduce opaque model overrides in v1.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `recursive-decomposition`, `verification-before-completion`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 7)
  - **Parallel Group**: Wave 3
  - **Blocks**: 8
  - **Blocked By**: 4,5

  **References**:
  - `src/analysis.rs` — threshold application points.
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/tetra_league.dart` — profile model context.
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/aggregate_stats.dart` — deterministic profile aggregation pattern.

  **Acceptance Criteria**:
  - [x] Calibration output is deterministic and versioned.
  - [x] Bucket thresholds are loaded and applied without runtime nondeterminism.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Bucket threshold determinism
    Tool: Bash
    Preconditions: calibration fixtures prepared
    Steps:
      1. Run calibration generation command twice on same corpus snapshot
      2. Hash outputs
      3. Assert hashes match
    Expected Result: Stable calibration artifacts
    Evidence: .sisyphus/evidence/task-6-calibration-hash.txt
  ```

- [x] 7. Extend WASM machine diagnostics contract (backend outputs only)

  **What to do**:
  - Export machine-level fields needed by downstream advice layer:
    fatality/obligation classes, taxonomy code, confidence/context fields.

  **Must NOT do**:
  - Do not add natural-language generation to wasm outputs.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `verification-before-completion`, `systematic-debugging`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3
  - **Blocks**: 8
  - **Blocked By**: 1,5

  **References**:
  - `src/wasm.rs` — JSON schema and export wiring.
  - `src/analysis.rs` — source fields for output mapping.

  **Acceptance Criteria**:
  - [x] New wasm fields serialize correctly and remain backward compatible where required.
  - [x] Unit/integration tests validate presence and values for fixtures.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: WASM payload contract check
    Tool: Bash
    Preconditions: wasm tests or host-call tests available
    Steps:
      1. Run wasm-facing evaluation tests
      2. Parse JSON payloads
      3. Assert expected diagnostic fields exist with deterministic values
    Expected Result: Stable machine diagnostics in export contract
    Evidence: .sisyphus/evidence/task-7-wasm-contract.txt
  ```

- [x] 8. Build replay validation harness + promotion gates

  **What to do**:
  - Implement deterministic replay gate suite with metrics:
    - severe-error recall
    - false-severe rate
    - obligation compliance
    - determinism hash stability
  - Add promotion gate thresholds and CI command path.

  **Must NOT do**:
  - Do not approve changes that improve one metric by collapsing another without explicit tradeoff signoff.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `verification-before-completion`, `systematic-debugging`

  **Parallelization**:
  - **Can Run In Parallel**: NO (final integration)
  - **Parallel Group**: Wave 4
  - **Blocks**: Final
  - **Blocked By**: 2,3,5,6,7

  **References**:
  - `tests/presim_validation.rs` — expand with major-mistake fixtures.
  - `src/search.rs` / `src/analysis.rs` / `src/wasm.rs` — target behavior under test.
  - `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/data_objects/tetrio_multiplayer_replay.dart` — replay aggregation cues.

  **Acceptance Criteria**:
  - [x] Deterministic replay harness command exists and is reproducible.
  - [x] Metrics report generated with explicit thresholds.
  - [x] CI gate fails when thresholds are violated.

  **Agent-Executed QA Scenarios**:
  ```text
  Scenario: Deterministic replay-gate run
    Tool: Bash
    Preconditions: replay corpus snapshot pinned
    Steps:
      1. Run replay harness once; save report A
      2. Run replay harness again; save report B
      3. Assert A == B hash
      4. Assert thresholds met (recall >= target, FSR <= target)
    Expected Result: Stable and passing coaching quality gate
    Evidence: .sisyphus/evidence/task-8-replay-gate.txt
  ```

---

## Commit Strategy

| After Task Group | Message | Files | Verification |
|------------------|---------|-------|--------------|
| 0-2 | `feat(search): enforce s2 lexicographic obligation gates` | `src/search.rs`, `src/state.rs`, tests | `cargo test search::tests` |
| 3-5 | `feat(analysis): add major-first taxonomy and obligation logic` | `src/analysis.rs`, tests | `cargo test analysis::tests` |
| 6-7 | `feat(wasm): export deterministic coaching diagnostics` | `src/wasm.rs`, related tests | `cargo test` |
| 8 | `test(calibration): add replay quality promotion gates` | harness + tests | replay gate command |

---

## Success Criteria

### Verification Commands
```bash
cargo test
# Expected: all tests pass

cargo test search::tests
# Expected: lexicographic and futility gate tests pass

cargo test analysis::tests
# Expected: major taxonomy + reason tests pass

# (project-specific replay harness command)
# Expected: deterministic hash stable + thresholds satisfied
```

### Final Checklist
- [x] All major-mistake taxonomy rules are deterministic and tested
- [x] Severe/false-severe metrics are available and gated
- [x] S2 boundary checks are enforced in critical logic path
- [x] No human/manual verification required

---

## Defaults Applied (override allowed)
- Automated test strategy set to **Tests-after** (existing Rust test infrastructure).
- TetraStats features limited to context/calibration whitelist in v1.
- ML residual deferred from v1 core path.

## Decisions Applied
- **Skill bucket granularity (v1)**: rank-depth sampling approved (B through U, 10 ranks).
- **Calibration source (v1)**: replay-first + per-player stat snapshot context (local extraction path), not broad external ingestion by default.
- **Promotion gate strictness**: strict profile selected:
  - severe-error recall >= 92%
  - false-severe rate <= 8%
  - obligation compliance >= 94%
- **ML scope**: defer ML from v1 implementation; keep deterministic kernel as authoritative.
- **Calibration corpus minimum**:
  - 5 replays per player
  - 3 unique players per rank
  - derived minimum = 150 replays total (10 ranks × 3 players × 5 replays)
- **Eligible mode set (v1)**: 1v1 ranked only.
- **Replay age window (v1)**: last 3 months.
