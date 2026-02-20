# Fusion Engine Mass Cleanup Refactor

## TL;DR

> **Quick Summary**: Eliminate all clippy warnings (55→0), fix 2 UB sites, remove ~17 dead WASM exports, split 3 god-files (movegen/search/wasm), and narrow visibility of dead pub symbols — all without behavior changes or perft regression.
> 
> **Deliverables**:
> - Zero clippy warnings under `cargo clippy --all-targets -- -D warnings`
> - Zero unsafe UB (MaybeUninit properly initialized)
> - 15 transmute calls replaced with safe match-based conversions
> - 3 god-files split into focused modules (movegen, search, wasm)
> - ~17 dead WASM exports removed
> - 6 too-many-arguments functions struct-ified
> - Dead pub symbols narrowed to pub(crate)
> 
> **Estimated Effort**: Medium-Large
> **Parallel Execution**: YES - 5 waves
> **Critical Path**: Task 1 (snapshot) → Task 2 (UB fix) → Tasks 3-7 (clippy) → Tasks 8-12 (structural) → Final verification

---

## Context

### Original Request
User requested a mass cleanup refactor focused specifically on Fusion engine and fusion-related Mosaic integrations. Cleanup-only — no new features, no behavior changes. Motivated by 55 clippy lint errors (including 2 UB), god-files exceeding 900 lines, and dead WASM API surface.

### Interview Summary
**Key Discussions**:
- Scope: Fusion engine primary. Mosaic only for fusion-related WASM integration files.
- Must NOT touch non-fusion mosaic files (UI components, Triangle engine, upstream code).
- User treats all warnings as errors — zero-warning output is the acceptance gate.
- Atomic commits required (build+test pass at each commit).
- JJ VCS (not git).

**Research Findings**:
- 6 explore agents mapped: clippy lints (55), unsafe sites (20-21), god-files (8 candidates, cap at 3), dead WASM exports (~17), too-many-arguments (6 functions), WASM integration surface.
- D7 perft parity: 2,705,999,255 nodes (exact Cobra match) — must be preserved.
- NPS baseline: 33.7M serial — must not regress.
- MaybeUninit UB at movegen.rs:23/:96 causes clippy deny-by-default errors — blocks all clippy verification until fixed.

### Metis Review
**Identified Gaps** (addressed):
- Test baseline: 145 pass + 4 ignored (not 152). Corrected in acceptance criteria.
- Clippy UB is deny-by-default → compilation error, not warning. Made Task 0 blocker.
- Dead WASM exports undercounted: ~17 (not 7). Full audit included.
- Missing WASM build gate. Added AC4.
- Missing #[repr(u8)] verification for transmute replacements. Added to Task 3.
- Cap god-file splits at 3 (movegen, search, wasm). Others deferred.
- wasm-bindgen =0.2.100 pin must not be touched.

---

## Work Objectives

### Core Objective
Bring the Fusion engine codebase to zero clippy warnings, zero UB, clean module structure, and minimal dead code — all while preserving exact behavioral parity (perft D7 match, NPS non-regression, WASM compatibility).

### Concrete Deliverables
- `cargo clippy --all-targets -- -D warnings` → 0 errors/warnings
- `cargo clippy --all-targets --features wasm -- -D warnings` → 0 errors/warnings  
- `cargo test` → 0 failed (145+ pass)
- `cargo build --target wasm32-unknown-unknown` → 0 errors
- 3 god-files split: movegen.rs, search.rs, wasm.rs
- ~17 dead WASM exports removed
- 15 transmute calls → safe match conversions
- 6 functions struct-ified (too_many_arguments resolved)

### Definition of Done
- [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep -c "warning\|error\["` → 0
- [ ] `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"` in all test result lines
- [ ] `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown 2>&1 | grep -c "^error"` → 0
- [ ] D7 perft node count = 2,705,999,255 (run `cargo run --release --bin bench_perft`)
- [ ] `grep -rn "#\[allow" src/ | wc -l` ≤ pre-refactor count

### Must Have
- Zero clippy warnings (default + wasm features)
- MaybeUninit UB eliminated (movegen.rs:23, :96)
- All 15 transmute calls replaced with safe alternatives
- movegen.rs, search.rs, wasm.rs split into sub-modules
- Dead WASM exports removed (grep-verified against mosaic)
- Perft D7 parity preserved
- Atomic commits (each passes all gates)

### Must NOT Have (Guardrails)
- NO algorithm changes in movegen BFS, attack calculation, or search heuristics
- NO new features or behavior changes
- NO touching wasm-bindgen =0.2.100 version pin
- NO #[allow(clippy::...)] suppressions except documented false positives with // SAFETY: comments
- NO splitting beyond 3 god-files (movegen, search, wasm)
- NO touching non-fusion mosaic files
- NO new test coverage (only fix tests broken by refactoring)
- NO removing WASM exports without grep-verifying unused in mosaic-fusion-testing
- NO modifying Cargo.toml dependencies (except removing confirmed-unused ones)

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: Tests-after (verify after each task, no TDD)
- **Framework**: cargo test (145 pass, 4 ignored perft)
- **Perft gates**: D1-D3 + per-piece (passing), D4-D7 (ignored, run manually for regression check)

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

| Deliverable Type | Verification Tool | Method |
|------------------|-------------------|--------|
| Clippy lint fix | Bash (cargo clippy) | Run clippy, assert 0 warnings for target file |
| Unsafe elimination | Bash (cargo test) | Run tests, verify 0 failures + check assembly |
| God-file split | Bash (cargo test + cargo check) | Verify compilation + all tests pass |
| Dead export removal | Bash (cargo build --target wasm32) | WASM build succeeds |
| Struct-ification | Bash (cargo test) | All callers compile, tests pass |

### Pre-Refactor Baseline (record before Task 1)
```bash
# Record these values BEFORE any changes
CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "test result"     # Baseline test count
grep -rn "#\[allow" src/ | wc -l                             # Baseline allow count
grep -rn "unsafe" src/ | wc -l                               # Baseline unsafe count
grep -rn "transmute" src/ | wc -l                            # Baseline transmute count
```

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 0 (Sequential — gate for everything):
└── Task 1: Pre-cleanup snapshot + baseline recording [quick]

Wave 1 (Sequential — unblocks clippy):
└── Task 2: Fix MaybeUninit UB in movegen.rs [deep]

Wave 2 (After Wave 1 — clippy fixes, MAX PARALLEL):
├── Task 3: Replace all 15 transmute calls with safe conversions [unspecified-high]
├── Task 4: Fix gen.rs clippy lints (12 lints) [quick]
├── Task 5: Fix pathfinder.rs clippy lints (5 lints) [quick]
├── Task 6: Fix board.rs clippy lints (4 lints) [quick]
├── Task 7: Fix remaining scattered lints (search, attack, replay_validation_labels, tetrastats) [quick]
└── Task 8: Fix movegen.rs style lints (needless_range_loop, casts, contains) [unspecified-high]

Wave 3 (After Wave 2 — structural changes, PARALLEL):
├── Task 9: Struct-ify too_many_arguments functions (attack.rs + search.rs) [deep]
├── Task 10: Struct-ify movegen.rs too_many_arguments [deep]
├── Task 11: Remove dead WASM exports (grep-verified) [unspecified-high]
├── Task 12: Narrow dead pub symbols to pub(crate) [quick]
└── Task 13: impl Default for MoveBuffer + Inputs (new_without_default) [quick]

Wave 4 (After Wave 3 — god-file splits, SEQUENTIAL per file):
├── Task 14: Split movegen.rs → movegen/ directory [deep]
├── Task 15: Split search.rs → search/ directory [deep]
└── Task 16: Split wasm.rs → wasm/ directory [deep]

Wave FINAL (After ALL tasks — independent review, 4 parallel):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real QA — full gate suite (unspecified-high)
└── Task F4: Scope fidelity check (deep)

Critical Path: T1 → T2 → T3/T8 → T9/T10 → T14 → T15 → T16 → F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 6 (Wave 2)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|------------|--------|------|
| 1 | — | 2-16 | 0 |
| 2 | 1 | 3-16 | 1 |
| 3 | 2 | 9, 10, 14 | 2 |
| 4 | 2 | — | 2 |
| 5 | 2 | — | 2 |
| 6 | 2 | — | 2 |
| 7 | 2 | — | 2 |
| 8 | 2 | 10, 14 | 2 |
| 9 | 3, 7 | 15 | 3 |
| 10 | 3, 8 | 14 | 3 |
| 11 | 2 | 16 | 3 |
| 12 | 2 | — | 3 |
| 13 | 8 | 14 | 3 |
| 14 | 10, 13 | F1-F4 | 4 |
| 15 | 9 | F1-F4 | 4 |
| 16 | 11 | F1-F4 | 4 |
| F1-F4 | 14, 15, 16 | — | FINAL |

### Agent Dispatch Summary

| Wave | # Parallel | Tasks → Agent Category |
|------|------------|----------------------|
| 0 | **1** | T1 → `quick` |
| 1 | **1** | T2 → `deep` |
| 2 | **6** | T3 → `unspecified-high`, T4-T7 → `quick`, T8 → `unspecified-high` |
| 3 | **5** | T9-T10 → `deep`, T11 → `unspecified-high`, T12-T13 → `quick` |
| 4 | **3** | T14-T16 → `deep` (sequential within wave due to potential conflicts) |
| FINAL | **4** | F1 → `oracle`, F2-F3 → `unspecified-high`, F4 → `deep` |

---

## TODOs

> Implementation + Verification = ONE Task. Never separate.
> EVERY task MUST have: Recommended Agent Profile + Parallelization info + QA Scenarios.

- [x] 1. Pre-cleanup snapshot + baseline recording

  **What to do**:
  - Create JJ snapshot: `jj commit -m "snapshot: pre-cleanup baseline"`
  - Record baseline metrics to `.sisyphus/evidence/task-1-baseline.txt`:
    - `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "test result"` (capture exact pass/fail/ignored counts)
    - `grep -rn "#\[allow" src/ | wc -l` (baseline allow count)
    - `grep -rn "unsafe" src/ | wc -l` (baseline unsafe count)
    - `grep -rn "transmute" src/ | wc -l` (baseline transmute count)
    - `wc -l src/*.rs` (baseline line counts per file)
  - Verify D7 perft parity: `CLOUD_EXEC_SKIP=1 cargo run --release --bin bench_perft 2>&1 | grep "D7"` (must show 2,705,999,255)

  **Must NOT do**:
  - Do NOT modify any source files
  - Do NOT change any configuration

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple snapshot + data recording, no code changes
  - **Skills**: [`git-master`]
    - `git-master`: JJ commit operations

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 0 (sequential gate)
  - **Blocks**: Tasks 2-16
  - **Blocked By**: None

  **References**:
  - `Cargo.toml` — crate configuration, wasm-bindgen pin
  - `src/bin/bench_perft.rs` — perft benchmark binary
  - `tests/perft.rs` — perft integration tests

  **Acceptance Criteria**:
  - [ ] JJ snapshot commit created with message "snapshot: pre-cleanup baseline"
  - [ ] `.sisyphus/evidence/task-1-baseline.txt` exists with all baseline metrics

  **QA Scenarios**:
  ```
  Scenario: Baseline metrics captured
    Tool: Bash
    Preconditions: Clean working copy
    Steps:
      1. Run `jj log --limit 1` — verify snapshot commit message
      2. Read `.sisyphus/evidence/task-1-baseline.txt` — verify all 5 metric categories present
      3. Verify D7 line contains "2705999255"
    Expected Result: Snapshot commit exists, baseline file has test/allow/unsafe/transmute/linecount data
    Evidence: .sisyphus/evidence/task-1-baseline.txt
  ```

  **Commit**: YES
  - Message: `snapshot: pre-cleanup baseline`
  - Files: (none — snapshot only)

- [x] 2. Fix MaybeUninit UB in movegen.rs (CRITICAL — unblocks clippy)

  **What to do**:
  - Fix movegen.rs:23 — `MaybeUninit::uninit().assume_init()` on `[Move; 256]` array in MoveBuffer
    - Replace with `MaybeUninit::<[Move; 256]>::uninit()` and track length manually
    - Use `ptr::write` or `MaybeUninit::write` for element insertion
    - Use `assume_init_read` for element access (within tracked `len`)
    - OR: zero-init with `[Move::default(); 256]` if Move implements Default/Copy and perf is acceptable
  - Fix movegen.rs:96 — `MaybeUninit::uninit().assume_init()` on `[[[u64; 3]; 4]; 10]` collision map
    - Replace with `[[[0u64; 3]; 4]; 10]` (zero-init, since u64 zeroing is safe and cheap)
    - OR: `MaybeUninit::<[[[u64; 3]; 4]; 10]>::zeroed().assume_init()` if explicit
  - After fix: verify codegen equivalence on hot path
    - Run `CLOUD_EXEC_SKIP=1 cargo test` — all 145 tests must pass
    - Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "uninit_assumed_init"` — must return 0 matches
    - Run perft D5 timing to spot-check no major regression
  - Check if `#[repr(u8)]` exists on Piece/Rotation/Spin enums (needed for Task 3)

  **Must NOT do**:
  - Do NOT change MoveBuffer's public API (push, len, get methods)
  - Do NOT change movegen algorithm or BFS logic
  - Do NOT add #[allow(clippy::uninit_assumed_init)]

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Performance-critical hot path, UB elimination requires careful codegen analysis
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 1 (sequential — blocks everything)
  - **Blocks**: Tasks 3-16 (clippy verification impossible until UB fixed)
  - **Blocked By**: Task 1

  **References**:
  - `src/movegen.rs:13-60` — MoveBuffer struct definition and methods
  - `src/movegen.rs:90-100` — Collision map initialization
  - `src/movegen.rs:61-730` — BFS core (context for hot path assessment)
  - `src/header.rs` — Piece/Rotation/Spin enum definitions (check #[repr])
  - Rust std docs: `std::mem::MaybeUninit` — proper initialization patterns

  **Acceptance Criteria**:
  - [ ] `grep -n "uninit().assume_init()" src/movegen.rs` → 0 matches
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep -c "uninit_assumed_init"` → 0
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] D5 perft timing within 20% of baseline (no major regression)

  **QA Scenarios**:
  ```
  Scenario: UB eliminated, clippy unblocked
    Tool: Bash
    Preconditions: Task 1 snapshot exists
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "uninit_assumed_init"`
      2. Assert: 0 lines returned
      3. Run `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "test result"`
      4. Assert: "0 failed" in all result lines
      5. Run `CLOUD_EXEC_SKIP=1 cargo test --release perft_d5 2>&1`
      6. Assert: test passes, timing within 20% of baseline
    Expected Result: Zero UB lints, all tests pass, no performance regression
    Evidence: .sisyphus/evidence/task-2-ub-fix.txt

  Scenario: Enum repr check for Task 3 readiness
    Tool: Bash
    Preconditions: After UB fix
    Steps:
      1. Run `grep -n "#\[repr" src/header.rs`
      2. Document which enums have #[repr(u8)] and which don't
    Expected Result: Report on repr status for Piece, Rotation, Spin enums
    Evidence: .sisyphus/evidence/task-2-repr-check.txt
  ```

  **Commit**: YES
  - Message: `fix(movegen): eliminate MaybeUninit UB`
  - Files: `src/movegen.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test`

- [x] 3. Replace all 15 transmute calls with safe conversions

  **What to do**:
  - First: read Task 2's `.sisyphus/evidence/task-2-repr-check.txt` to check enum repr status
  - If enums have `#[repr(u8)]`: create `from_u8(val: u8) -> Self` methods on Piece, Rotation, Spin using match
  - If enums do NOT have `#[repr(u8)]`: add `#[repr(u8)]` to each enum, then create from_u8 methods
  - Replace all transmute sites:
    - gen.rs:32,72,230,275 (4 sites) — u8→Rotation/Piece transmutes
    - header.rs:168,173,180 (3 sites) — u32 bitfield extraction → enum transmutes
    - movegen.rs:104,131,175,493,539,578,681,789 (8 sites) — u8→Rotation transmutes
  - For header.rs bitfield sites: replace with safe bit-masking + from_u8() call
  - Verify all 15 transmute calls are eliminated: `grep -rn "transmute" src/ | wc -l` → 0
  - Run cargo test to verify no behavior change

  **Must NOT do**:
  - Do NOT change enum variant ordering or values
  - Do NOT change function signatures
  - Do NOT add panic paths in from_u8 (use unreachable_unchecked or debug_assert for invalid values)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 15 call sites across 3 files, mechanical but requires careful verification
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4-8)
  - **Blocks**: Tasks 9, 10, 14
  - **Blocked By**: Task 2

  **References**:
  - `src/header.rs:1-50` — Piece, Rotation, Spin enum definitions
  - `src/gen.rs:32,72,230,275` — Rotation/Piece transmute sites in move generation
  - `src/header.rs:168,173,180` — Bitfield extraction transmute sites
  - `src/movegen.rs:104,131,175,493,539,578,681,789` — Rotation transmute sites in BFS
  - `.sisyphus/evidence/task-2-repr-check.txt` — enum repr status from Task 2

  **Acceptance Criteria**:
  - [ ] `grep -rn "transmute" src/ | wc -l` → 0
  - [ ] `grep -rn "unsafe" src/ | wc -l` < baseline (from task-1-baseline.txt)
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep -c "missing_transmute"` → 0

  **QA Scenarios**:
  ```
  Scenario: All transmutes eliminated
    Tool: Bash
    Preconditions: Task 2 complete (UB fixed)
    Steps:
      1. Run `grep -rn "transmute" src/`
      2. Assert: 0 lines returned
      3. Run `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "test result"`
      4. Assert: "0 failed" in all result lines
      5. Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "missing_transmute"`
      6. Assert: 0 lines returned
    Expected Result: Zero transmute calls, all tests pass, no clippy transmute warnings
    Evidence: .sisyphus/evidence/task-3-transmute-elimination.txt

  Scenario: Safe conversion correctness
    Tool: Bash
    Preconditions: from_u8 methods added
    Steps:
      1. Run `CLOUD_EXEC_SKIP=1 cargo test -- --test-threads=1 2>&1`
      2. Assert: all 145+ tests pass (same count as baseline)
      3. Run D5 perft: `CLOUD_EXEC_SKIP=1 cargo test perft_d5 -- --ignored 2>&1`
      4. Assert: D5 node count matches baseline
    Expected Result: Behavioral parity preserved with safe conversions
    Evidence: .sisyphus/evidence/task-3-perft-verify.txt
  ```

  **Commit**: YES
  - Message: `refactor: replace all transmute with safe conversions`
  - Files: `src/header.rs`, `src/gen.rs`, `src/movegen.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test`

- [ ] 4. Fix gen.rs clippy lints (12 lints)

  **What to do**:
  - Fix 10 `unnecessary_cast` warnings: lines 84, 87, 95, 149, 219, 224, 226, 265, 269, 270
    - Remove redundant `as usize` casts on values that are already usize
  - Fix 1 `needless_range_loop` at line 229: convert to iterator-based loop
  - Fix 1 `needless_late_init` at line 277: initialize variable at declaration site
  - Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "gen.rs"` → 0 matches

  **Must NOT do**:
  - Do NOT change move generation logic or piece data tables
  - Do NOT rename functions or change signatures

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Mechanical lint fixes, low risk, single file
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3, 5-8)
  - **Blocks**: None
  - **Blocked By**: Task 2

  **References**:
  - `src/gen.rs:84-270` — All 12 lint locations
  - Clippy docs: `unnecessary_cast`, `needless_range_loop`, `needless_late_init`

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "gen.rs"` → 0 lines
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed

  **QA Scenarios**:
  ```
  Scenario: gen.rs clippy-clean
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep "gen.rs"`
      2. Assert: 0 lines
      3. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
      4. Assert: present in output
    Expected Result: Zero clippy warnings in gen.rs, all tests pass
    Evidence: .sisyphus/evidence/task-4-gen-clippy.txt
  ```

  **Commit**: NO (groups with Tasks 5-8)

- [ ] 5. Fix pathfinder.rs clippy lints (5 lints)

  **What to do**:
  - Fix 2 `manual_memcpy` at lines 194, 201: replace manual loop with `copy_from_slice()`
  - Fix 2 `if_same_then_else` at lines 238, 256: inspect branches — if intentionally identical (future differentiation), add `// NOTE: branches intentionally identical for [reason]` comment and suppress with `#[allow]` + `// SAFETY:` doc. If unintentional, merge branches.
  - Fix 1 `unnecessary_cast` at line 299: remove redundant cast

  **Must NOT do**:
  - Do NOT change pathfinding algorithm logic
  - Do NOT merge if_same_then_else branches without verifying they're unintentional duplicates

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 5 mechanical fixes, single file
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3, 4, 6-8)
  - **Blocks**: None
  - **Blocked By**: Task 2

  **References**:
  - `src/pathfinder.rs:194-299` — All 5 lint locations
  - `src/pathfinder.rs:238,256` — if_same_then_else branches (inspect intent before merging)

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "pathfinder.rs"` → 0 lines
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed

  **QA Scenarios**:
  ```
  Scenario: pathfinder.rs clippy-clean
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep "pathfinder.rs"`
      2. Assert: 0 lines
      3. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
    Expected Result: Zero clippy warnings in pathfinder.rs
    Evidence: .sisyphus/evidence/task-5-pathfinder-clippy.txt
  ```

  **Commit**: NO (groups with Tasks 4, 6-8)

- [ ] 6. Fix board.rs clippy lints (4 lints)

  **What to do**:
  - Fix 3 `unnecessary_cast` at lines 67, 210, 211: remove redundant casts
  - Fix 1 `inherent_to_string` at line 187: replace `fn to_string(&self) -> String` with `impl std::fmt::Display for Board` (delegates to existing logic). Remove the `to_string` method. All callers using `.to_string()` will automatically use Display's blanket impl.

  **Must NOT do**:
  - Do NOT change board data representation or indexing
  - Do NOT modify the board.rs:221 `as_bytes_mut` unsafe block (separate concern, deferred)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 4 simple fixes, single file
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3-5, 7-8)
  - **Blocks**: None
  - **Blocked By**: Task 2

  **References**:
  - `src/board.rs:67,187,210,211` — All 4 lint locations
  - `src/board.rs:187-228` — to_string method and debug output (convert to Display)

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "board.rs"` → 0 lines
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] `grep -n "fn to_string" src/board.rs` → 0 lines (replaced with Display impl)

  **QA Scenarios**:
  ```
  Scenario: board.rs clippy-clean + Display impl
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep "board.rs"`
      2. Assert: 0 lines
      3. `grep "impl.*Display.*Board" src/board.rs`
      4. Assert: 1 match (Display impl exists)
      5. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
    Expected Result: Display impl replaces to_string, zero warnings
    Evidence: .sisyphus/evidence/task-6-board-clippy.txt
  ```

  **Commit**: NO (groups with Tasks 4-5, 7-8)

- [ ] 7. Fix remaining scattered clippy lints (search, attack, replay_validation_labels, tetrastats)

  **What to do**:
  - search.rs:275 — `redundant_closure`: replace closure with direct function reference
  - search.rs:831 — `useless_vec`: replace `vec![...]` with array literal where possible
  - attack.rs — no style lints (only too_many_arguments, handled in Task 9)
  - replay_validation_labels.rs:29,33,37,41 — `manual_is_multiple_of`: replace `x % n == 0` with `x.is_multiple_of(n)` (4 sites)
  - tetrastats_features.rs:131 — `neg_multiply`: replace `-1.0 * x` with `-x`
  - Total: 7 lint fixes across 3 files

  **Must NOT do**:
  - Do NOT change search heuristics or scoring
  - Do NOT change attack calculation formulas
  - Do NOT change validation label logic

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 7 trivial mechanical fixes across 3 files
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3-6, 8)
  - **Blocks**: Task 9 (search.rs changes)
  - **Blocked By**: Task 2

  **References**:
  - `src/search.rs:275,831` — redundant_closure, useless_vec
  - `src/replay_validation_labels.rs:29,33,37,41` — manual_is_multiple_of
  - `src/tetrastats_features.rs:131` — neg_multiply

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep -E "(search|replay_validation_labels|tetrastats)"` → 0 lines (excluding too_many_arguments)
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed

  **QA Scenarios**:
  ```
  Scenario: Scattered lints resolved
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep -c "redundant_closure\|useless_vec\|manual_is_multiple\|neg_multiply"`
      2. Assert: 0
      3. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
    Expected Result: All 7 scattered lints fixed
    Evidence: .sisyphus/evidence/task-7-scattered-clippy.txt
  ```

  **Commit**: NO (groups with Tasks 4-6, 8)

- [ ] 8. Fix movegen.rs style lints (10 lints — excludes UB/transmute/args)

  **What to do**:
  - Fix 8 `needless_range_loop` at lines 102, 261, 402, 577, 596, 680, 699, 739: convert `for i in 0..len` + `arr[i]` to iterator patterns (`.iter()`, `.enumerate()`, `.iter_mut()`)
  - Fix 1 `manual_contains` at line 930: replace manual loop/match with `.contains()`
  - Fix 1 `unnecessary_cast` at line 901: remove redundant cast
  - Note: transmute lints handled in Task 3, too_many_arguments in Task 10, UB in Task 2, new_without_default in Task 13

  **Must NOT do**:
  - Do NOT change BFS algorithm logic
  - Do NOT modify MoveBuffer struct (handled in Task 2)
  - Do NOT change function signatures (handled in Task 10)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 10 fixes in the largest and most critical file (996 lines, hot path)
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3-7)
  - **Blocks**: Tasks 10, 14
  - **Blocked By**: Task 2

  **References**:
  - `src/movegen.rs:102,261,402,577,596,680,699,739` — needless_range_loop locations
  - `src/movegen.rs:930` — manual_contains location
  - `src/movegen.rs:901` — unnecessary_cast location

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "movegen.rs" | grep -c "needless_range_loop\|manual_contains\|unnecessary_cast"` → 0
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed

  **QA Scenarios**:
  ```
  Scenario: movegen.rs style lints resolved
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep "movegen.rs" | grep -v "too_many_arguments"`
      2. Assert: 0 lines (only too_many_arguments may remain, handled in Task 10)
      3. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
      4. Run D5 perft: `CLOUD_EXEC_SKIP=1 cargo test perft_d3 2>&1`
      5. Assert: passes (behavioral parity on hot path)
    Expected Result: 10 style lints fixed, tests pass, perft parity
    Evidence: .sisyphus/evidence/task-8-movegen-clippy.txt
  ```

  **Commit**: YES (groups Tasks 4-8 together)
  - Message: `cleanup: resolve all clippy lints`
  - Files: `src/gen.rs`, `src/pathfinder.rs`, `src/board.rs`, `src/search.rs`, `src/replay_validation_labels.rs`, `src/tetrastats_features.rs`, `src/movegen.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings` → only `too_many_arguments` remaining

- [ ] 9. Struct-ify too_many_arguments: SearchExpansionContext (search.rs)

  **What to do**:
  - Create `SearchExpansionContext<'a>` struct in search.rs containing shared params for `gen_and_eval_root` and `expand_node`:
    - `config: &'a SearchConfig`
    - `weights: &'a EvalWeights`
    - `zobrist_keys: &'a ZobristKeys`
    - `tt: &'a mut TranspositionTable`
    - `remaining_depth: usize`
    - (plus any other params shared between the two functions)
  - Refactor `gen_and_eval_root` (search.rs:400, 9 params) to accept `&mut SearchExpansionContext`
  - Refactor `expand_node` (search.rs:450, 10 params) to accept `&mut SearchExpansionContext`
  - Refactor `run_beam_search_iteration` (search.rs:170, 8 params) to accept `SearchIterationParams` struct
  - Update all callers (internal only — these are not pub functions)
  - Run tests to verify no behavior change

  **Must NOT do**:
  - Do NOT change search algorithm logic or scoring
  - Do NOT change public API (find_best_move signature stays the same)
  - Do NOT optimize or restructure the beam search

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 3 functions, shared struct design, careful refactoring of complex search code
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 10, 11, 12)
  - **Blocks**: Task 14 (search.rs god-file split)
  - **Blocked By**: Tasks 7, 8 (search.rs and movegen.rs lints must be done first)

  **References**:
  - `src/search.rs:15-60` — SearchConfig, SearchResult, EvalWeights type definitions
  - `src/search.rs:170` — run_beam_search_iteration signature
  - `src/search.rs:400` — gen_and_eval_root signature
  - `src/search.rs:450` — expand_node signature
  - `src/transposition.rs` — ZobristKeys, TranspositionTable types

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep "search.rs" | grep "too_many_arguments"` → 0 lines
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed

  **QA Scenarios**:
  ```
  Scenario: search.rs too_many_arguments resolved
    Tool: Bash
    Steps:
      1. `grep "struct SearchExpansionContext" src/search.rs`
      2. Assert: 1 match (struct exists)
      3. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep "search.rs"`
      4. Assert: 0 lines
      5. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
    Expected Result: Structs created, zero clippy warnings in search.rs
    Evidence: .sisyphus/evidence/task-9-search-structify.txt
  ```

  **Commit**: NO (groups with Task 10)

- [ ] 10. Struct-ify too_many_arguments: AttackContext + MoveGenState (attack.rs, movegen.rs)

  **What to do**:
  - Create `AttackContext` struct in attack.rs for `calculate_attack_full` (line 179, 8 params):
    - `lines: u32`, `spin: SpinType`, `b2b: u8`, `combo: u32`, `is_perfect_clear: bool`, etc.
    - Update ~14 test callers to use struct construction
  - Create `MoveGenState` struct in movegen.rs for `do_rotate_180` (line 373, 10 params) and `do_process_180` (line 669, 8 params):
    - Internal helper struct, not public
    - These are called from 1 site each
  - Also add `impl Default for MoveBuffer` (new_without_default lint at movegen.rs:20): delegate to `MoveBuffer::new()`
  - Run tests to verify no behavior change

  **Must NOT do**:
  - Do NOT change attack calculation formulas
  - Do NOT change movegen BFS algorithm
  - Do NOT make MoveGenState public

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multiple structs across 2 files, ~14 test caller updates for AttackContext
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 11, 12)
  - **Blocks**: Task 14
  - **Blocked By**: Tasks 3, 8 (transmute and movegen style lints must be done first)

  **References**:
  - `src/attack.rs:179` — calculate_attack_full signature
  - `src/attack.rs` — test module with ~14 callers
  - `src/movegen.rs:373` — do_rotate_180 signature
  - `src/movegen.rs:669` — do_process_180 signature
  - `src/movegen.rs:20` — MoveBuffer::new() (add Default impl)

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep -c "too_many_arguments"` → 0
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets 2>&1 | grep -c "new_without_default"` → 0
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed

  **QA Scenarios**:
  ```
  Scenario: All too_many_arguments and new_without_default resolved
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1 | grep -c "too_many_arguments\|new_without_default"`
      2. Assert: 0
      3. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
    Expected Result: Zero too_many_arguments warnings, zero new_without_default, all tests pass
    Evidence: .sisyphus/evidence/task-10-structify-attack-movegen.txt

  Scenario: Full clippy clean check
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1`
      2. Assert: exit code 0 (zero warnings, zero errors)
    Expected Result: ENTIRE CRATE is clippy-clean for the first time
    Evidence: .sisyphus/evidence/task-10-full-clippy-clean.txt
  ```

  **Commit**: YES (groups Tasks 9-10)
  - Message: `refactor: struct-ify too_many_arguments functions`
  - Files: `src/search.rs`, `src/attack.rs`, `src/movegen.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings` → exit code 0

- [ ] 11. Prune dead WASM exports

  **What to do**:
  - FIRST: grep mosaic-fusion-testing to re-confirm each export is unused:
    - `grep -rn "get\b\|\.get(" src/lib/fusion/ src/lib/components/ src/lib/analysis/ src/test/` for JsBoard.get
    - Repeat for each dead export candidate
  - Remove confirmed-dead `#[wasm_bindgen]` exports from wasm.rs:
    - JsBoard methods: `get(x,y)`, `set(x,y,val)`, `apply_move(m)`, `clear_lines()`
    - JsMove struct: entire struct + all 7 methods (if confirmed unused)
    - JsAttackConfig: `quick_play()` method (keep tetra_league if used)
    - Free functions: `get_all_moves(...)` (if unused)
  - For `find_best_move(...)`: keep if used in test-presim.mjs (it's a test utility, not dead)
  - Also remove any now-unused private helper functions that only served dead exports
  - Build WASM target to verify: `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown`
  - Run cargo test to verify no internal test breakage

  **Must NOT do**:
  - Do NOT remove active exports (init, from_rows, to_rows, calculateAttack, evaluate_board, evaluate_move, analyze_replay, evaluate_with_weights)
  - Do NOT modify mosaic source files
  - Do NOT change wasm-bindgen version pin (=0.2.100)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Cross-project verification (fusion + mosaic grep), API surface pruning
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 10, 12)
  - **Blocks**: Task 15 (wasm.rs god-file split)
  - **Blocked By**: Task 2 (clippy must be functional)

  **References**:
  - `src/wasm.rs` — all #[wasm_bindgen] exports (full file)
  - `/home/li859/projects/mosaic-fusion-engine-coaching/mosaic-fusion-testing/src/lib/fusion/wasm/fusion_wasm.d.ts` — TypeScript type definitions
  - `/home/li859/projects/mosaic-fusion-engine-coaching/mosaic-fusion-testing/src/lib/fusion/wasm/fusion_wasm.js` — JS bindings
  - `/home/li859/projects/mosaic-fusion-engine-coaching/mosaic-fusion-testing/src/lib/components/interface/ReplayViewer.svelte` — primary WASM consumer
  - `/home/li859/projects/mosaic-fusion-engine-coaching/mosaic-fusion-testing/src/lib/analysis/analyzer.ts` — analysis WASM consumer

  **Acceptance Criteria**:
  - [ ] Each removed export verified unused via grep in mosaic-fusion-testing
  - [ ] `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown` → 0 errors
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] `wc -l src/wasm.rs` < 917 (reduced from baseline)

  **QA Scenarios**:
  ```
  Scenario: Dead exports removed, WASM builds
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown 2>&1 | grep "error"`
      2. Assert: 0 lines
      3. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
      4. `wc -l src/wasm.rs`
      5. Assert: line count < 917
    Expected Result: WASM builds clean, tests pass, wasm.rs is smaller
    Evidence: .sisyphus/evidence/task-11-dead-exports.txt

  Scenario: Active exports still present
    Tool: Bash
    Steps:
      1. `grep -c "wasm_bindgen" src/wasm.rs`
      2. Assert: count > 0 (active exports remain)
      3. `grep "fn init\|fn from_rows\|fn evaluate_board\|fn analyze_replay" src/wasm.rs`
      4. Assert: all 4 found
    Expected Result: All active WASM exports preserved
    Evidence: .sisyphus/evidence/task-11-active-exports.txt
  ```

  **Commit**: YES
  - Message: `cleanup(wasm): remove dead exports`
  - Files: `src/wasm.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test && CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown`

- [ ] 12. Narrow pub visibility to pub(crate) for internal symbols

  **What to do**:
  - For each `pub fn` / `pub struct` / `pub enum` in non-wasm files:
    - Use `lsp_find_references` or `grep` to check if used outside its own module
    - If only used within the crate (not exposed via wasm.rs or lib.rs): change `pub` → `pub(crate)`
  - Priority files (most pub items, likely internal-only):
    - header.rs — enum/struct definitions (some used by wasm.rs, some internal)
    - gen.rs — move generation internals
    - board.rs — board manipulation (some used by wasm.rs)
    - pathfinder.rs — pathfinding internals
    - calibration.rs — coaching calibration internals
  - Do NOT change visibility of:
    - Anything re-exported in lib.rs
    - Anything used in wasm.rs (WASM boundary)
    - Anything used in tests/ directory (integration tests need pub)

  **Must NOT do**:
  - Do NOT make breaking changes to public API
  - Do NOT remove any symbols, only narrow visibility
  - Do NOT touch wasm.rs exports

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Cross-file reference checking across entire crate
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 10, 11)
  - **Blocks**: None
  - **Blocked By**: Task 2

  **References**:
  - `src/lib.rs` — public re-exports
  - `src/wasm.rs` — WASM boundary (uses from other modules)
  - `tests/` — integration tests (use pub items)

  **Acceptance Criteria**:
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] `CLOUD_EXEC_SKIP=1 cargo check --all-targets` → 0 errors
  - [ ] `grep -rn "pub fn\|pub struct\|pub enum" src/ | grep -v "pub(crate)" | wc -l` < baseline pub count

  **QA Scenarios**:
  ```
  Scenario: Visibility narrowed without breakage
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
      2. `CLOUD_EXEC_SKIP=1 cargo check --all-targets 2>&1 | grep "error"`
      3. Assert: 0 errors
    Expected Result: All tests pass, compilation clean
    Evidence: .sisyphus/evidence/task-12-visibility.txt
  ```

  **Commit**: YES
  - Message: `refactor: narrow pub to pub(crate) for internal symbols`
  - Files: `src/header.rs`, `src/gen.rs`, `src/board.rs`, `src/pathfinder.rs`, `src/calibration.rs` (and any others with internal-only pub items)
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test`

- [ ] 13. God-file split: movegen.rs → extract move_buffer.rs

  **What to do**:
  - Extract MoveBuffer struct + MoveList API from movegen.rs into new `src/move_buffer.rs`:
    - MoveBuffer struct definition (lines ~13-60)
    - MoveList type alias and associated methods (lines ~851-996)
    - impl Default for MoveBuffer (added in Task 10)
  - movegen.rs retains: BFS core (`generate()`, `do_rotate_180()`, `do_process_180()`, collision map logic)
  - Add `mod move_buffer;` to lib.rs
  - Update all imports in movegen.rs to use `crate::move_buffer::*` or specific imports
  - Update any other files importing MoveBuffer (search.rs, wasm.rs, etc.)
  - Follow the replay_validation split pattern: orchestration module + extracted sub-module

  **Must NOT do**:
  - Do NOT change BFS algorithm in movegen.rs
  - Do NOT change MoveBuffer API (method signatures stay the same)
  - Do NOT split the BFS core itself (it's cohesive)

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Splitting the largest, most critical file. Must not break perft parity.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4 (sequential — god-file splits)
  - **Blocks**: None
  - **Blocked By**: Tasks 3, 8, 10 (all movegen.rs changes must be complete)

  **References**:
  - `src/movegen.rs:13-60` — MoveBuffer struct definition
  - `src/movegen.rs:851-996` — MoveList API
  - `src/replay_validation.rs` — Pattern reference for god-file split (orchestration + sub-modules)
  - `src/lib.rs` — Module registration

  **Acceptance Criteria**:
  - [ ] `src/move_buffer.rs` exists with MoveBuffer + MoveList
  - [ ] `wc -l src/movegen.rs` < 850 (reduced from 996)
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings` → exit code 0
  - [ ] D5 perft parity: `CLOUD_EXEC_SKIP=1 cargo test perft_d3 2>&1` passes

  **QA Scenarios**:
  ```
  Scenario: movegen.rs reduced, move_buffer.rs extracted
    Tool: Bash
    Steps:
      1. `test -f src/move_buffer.rs && echo "EXISTS"`
      2. Assert: "EXISTS"
      3. `wc -l src/movegen.rs`
      4. Assert: < 850
      5. `grep "mod move_buffer" src/lib.rs`
      6. Assert: 1 match
      7. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
      8. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1`
      9. Assert: exit code 0
    Expected Result: Clean split, all tests pass, clippy clean
    Evidence: .sisyphus/evidence/task-13-movegen-split.txt
  ```

  **Commit**: YES
  - Message: `refactor(movegen): extract MoveBuffer and MoveList into move_buffer.rs`
  - Files: `src/movegen.rs`, `src/move_buffer.rs` (new), `src/lib.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test && CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`

- [ ] 14. God-file split: search.rs → extract search config + node expansion

  **What to do**:
  - Extract config/result types from search.rs into new `src/search_config.rs`:
    - SearchConfig struct (lines ~15-60)
    - SearchResult struct
    - EvalWeights struct
    - SearchExpansionContext struct (created in Task 9)
    - SearchIterationParams struct (created in Task 9)
  - Extract node expansion logic into new `src/search_expand.rs`:
    - `gen_and_eval_root` function (line ~400)
    - `expand_node` function (line ~450)
    - Helper functions only used by expansion
  - search.rs retains: `find_best_move()` orchestration, `run_beam_search_iteration()`, public API
  - Add modules to lib.rs, update all imports

  **Must NOT do**:
  - Do NOT change search algorithm logic or heuristics
  - Do NOT change find_best_move public signature
  - Do NOT restructure beam search flow

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Complex search module split, must preserve beam search behavior
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4 (sequential after Task 13)
  - **Blocks**: None
  - **Blocked By**: Tasks 9, 10 (struct-ification must be complete first)

  **References**:
  - `src/search.rs:15-60` — Config/result type definitions
  - `src/search.rs:170` — run_beam_search_iteration
  - `src/search.rs:400` — gen_and_eval_root
  - `src/search.rs:450` — expand_node
  - `src/replay_validation.rs` — Split pattern reference

  **Acceptance Criteria**:
  - [ ] `src/search_config.rs` exists with SearchConfig, SearchResult, EvalWeights
  - [ ] `src/search_expand.rs` exists with gen_and_eval_root, expand_node
  - [ ] `wc -l src/search.rs` < 500 (reduced from 938)
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings` → exit code 0

  **QA Scenarios**:
  ```
  Scenario: search.rs reduced, sub-modules extracted
    Tool: Bash
    Steps:
      1. `test -f src/search_config.rs && test -f src/search_expand.rs && echo "BOTH EXIST"`
      2. Assert: "BOTH EXIST"
      3. `wc -l src/search.rs`
      4. Assert: < 500
      5. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
      6. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1`
      7. Assert: exit code 0
    Expected Result: Clean three-way split, tests pass, clippy clean
    Evidence: .sisyphus/evidence/task-14-search-split.txt
  ```

  **Commit**: YES
  - Message: `refactor(search): extract config types and node expansion`
  - Files: `src/search.rs`, `src/search_config.rs` (new), `src/search_expand.rs` (new), `src/lib.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test && CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`

- [ ] 15. God-file split: wasm.rs → extract WASM type wrappers

  **What to do**:
  - Extract JsBoard struct + methods from wasm.rs into new `src/wasm_board.rs`:
    - JsBoard struct definition
    - All `#[wasm_bindgen]` impl methods on JsBoard
    - From/Into conversions
  - wasm.rs retains: `init()`, free FFI functions (calculateAttack, evaluate_board, evaluate_move, analyze_replay, evaluate_with_weights), JsAttackConfig
  - Add module to lib.rs with appropriate `#[cfg(feature = "wasm")]` gating
  - Update any cross-references

  **Must NOT do**:
  - Do NOT change any #[wasm_bindgen] export signatures
  - Do NOT change the wasm-bindgen version pin
  - Do NOT remove any active exports (only dead ones removed in Task 11)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: FFI module split, must preserve WASM boundary integrity
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4 (sequential after Task 14)
  - **Blocks**: None
  - **Blocked By**: Task 11 (dead exports must be pruned first)

  **References**:
  - `src/wasm.rs` — full file (917 lines, now smaller after Task 11)
  - `src/wasm.rs:212-289` — JsBoard struct and methods
  - `src/lib.rs` — module registration with cfg(feature) gating

  **Acceptance Criteria**:
  - [ ] `src/wasm_board.rs` exists with JsBoard struct + methods
  - [ ] `wc -l src/wasm.rs` < 600 (reduced from ~800 post-Task-11)
  - [ ] `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown` → 0 errors
  - [ ] `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed
  - [ ] `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings` → exit code 0

  **QA Scenarios**:
  ```
  Scenario: wasm.rs reduced, wasm_board.rs extracted
    Tool: Bash
    Steps:
      1. `test -f src/wasm_board.rs && echo "EXISTS"`
      2. Assert: "EXISTS"
      3. `wc -l src/wasm.rs`
      4. Assert: < 600
      5. `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown 2>&1 | grep "error"`
      6. Assert: 0 lines
      7. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "0 failed"`
    Expected Result: Clean split, WASM builds, tests pass
    Evidence: .sisyphus/evidence/task-15-wasm-split.txt
  ```

  **Commit**: YES
  - Message: `refactor(wasm): extract JsBoard into wasm_board.rs`
  - Files: `src/wasm.rs`, `src/wasm_board.rs` (new), `src/lib.rs`
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test && CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown`

- [ ] 16. Final regression verification + metrics comparison

  **What to do**:
  - Run ALL verification gates:
    - `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings` → exit code 0
    - `CLOUD_EXEC_SKIP=1 cargo test` → 0 failed, same pass count as baseline
    - `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown` → 0 errors
    - `CLOUD_EXEC_SKIP=1 cargo run --release --bin bench_perft` → D7 = 2,705,999,255
  - Record final metrics to `.sisyphus/evidence/task-16-final.txt`:
    - `grep -rn "#\[allow" src/ | wc -l` → compare with baseline
    - `grep -rn "unsafe" src/ | wc -l` → compare with baseline (should be 0 or near-0)
    - `grep -rn "transmute" src/ | wc -l` → 0
    - `wc -l src/*.rs` → compare line counts per file
    - Total file count
  - Generate diff summary: `jj diff --from [snapshot-commit] --stat`
  - Create final commit with all remaining uncommitted cleanup

  **Must NOT do**:
  - Do NOT change any source code in this task
  - Do NOT add new features or optimizations

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Comprehensive verification, metric comparison, must catch any regressions
  - **Skills**: [`verification-before-completion`]
    - `verification-before-completion`: Ensures all claims are backed by evidence

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 5 (final gate)
  - **Blocks**: Final Verification Wave
  - **Blocked By**: Tasks 13, 14, 15 (all god-file splits complete)

  **References**:
  - `.sisyphus/evidence/task-1-baseline.txt` — pre-refactor baseline metrics
  - `src/bin/bench_perft.rs` — perft benchmark binary
  - All `.sisyphus/evidence/task-*` files — per-task evidence

  **Acceptance Criteria**:
  - [ ] `cargo clippy --all-targets -- -D warnings` → exit code 0
  - [ ] `cargo test` → 0 failed
  - [ ] `cargo build --target wasm32-unknown-unknown` → 0 errors
  - [ ] D7 perft = 2,705,999,255 (exact match)
  - [ ] transmute count = 0
  - [ ] unsafe count < baseline
  - [ ] `.sisyphus/evidence/task-16-final.txt` exists with all metrics

  **QA Scenarios**:
  ```
  Scenario: Full regression verification
    Tool: Bash
    Steps:
      1. `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1`
      2. Assert: exit code 0
      3. `CLOUD_EXEC_SKIP=1 cargo test 2>&1 | grep "test result"`
      4. Assert: "0 failed" in all lines
      5. `CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown 2>&1 | grep "error"`
      6. Assert: 0 lines
      7. `CLOUD_EXEC_SKIP=1 cargo run --release --bin bench_perft 2>&1 | grep "D7"`
      8. Assert: contains "2705999255"
    Expected Result: All gates pass, metrics improved, zero regressions
    Evidence: .sisyphus/evidence/task-16-final.txt

  Scenario: Metric improvement summary
    Tool: Bash
    Steps:
      1. `grep -rn "transmute" src/ | wc -l`
      2. Assert: 0
      3. `grep -rn "unsafe" src/ | wc -l`
      4. Compare with task-1-baseline.txt unsafe count
      5. `wc -l src/*.rs | tail -1`
      6. Compare total lines with baseline
    Expected Result: transmute=0, unsafe reduced, line counts reflect splits
    Evidence: .sisyphus/evidence/task-16-metrics-comparison.txt
  ```

  **Commit**: YES
  - Message: `cleanup: fusion mass cleanup complete — zero clippy warnings, zero transmutes, god-files split`
  - Files: (any remaining uncommitted changes)
  - Pre-commit: `CLOUD_EXEC_SKIP=1 cargo test && CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings`

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings` + `CLOUD_EXEC_SKIP=1 cargo clippy --all-targets --features wasm -- -D warnings` + `CLOUD_EXEC_SKIP=1 cargo test`. Review all changed files for: `as any`/`@ts-ignore`, empty catches, console.log in prod, commented-out code, unused imports. Check for new #[allow] attributes. Verify zero transmute calls remain (except documented false positives).
  Output: `Clippy [PASS/FAIL] | Clippy+WASM [PASS/FAIL] | Tests [N pass/N fail] | Transmutes [N remaining] | VERDICT`

- [ ] F3. **Real QA — Full Gate Suite** — `unspecified-high`
  Execute EVERY verification gate from the plan:
  Gate 1: `cargo clippy --all-targets -- -D warnings` → 0
  Gate 2: `cargo clippy --all-targets --features wasm -- -D warnings` → 0
  Gate 3: `cargo test` → 0 failed
  Gate 4: `cargo build --target wasm32-unknown-unknown` → 0 errors
  Gate 5: `cargo run --release --bin bench_perft` → D7 = 2,705,999,255
  Gate 6: `grep -rn "#\[allow" src/ | wc -l` ≤ baseline
  Gate 7: `grep -rn "transmute" src/ | wc -l` = 0 (or documented exceptions)
  Gate 8: `grep -rn "MaybeUninit" src/` → only safe usage patterns
  Save all outputs to `.sisyphus/evidence/final-qa/`.
  Output: `Gates [N/8 pass] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (`jj log --stat`). Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check "Must NOT do" compliance: no algorithm changes, no new features, no wasm-bindgen version change. Detect cross-task contamination. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Scope [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

| After Task(s) | Message | Pre-commit Gate |
|---------------|---------|-----------------|
| 1 | `snapshot: pre-cleanup baseline` | N/A |
| 2 | `fix(movegen): eliminate MaybeUninit UB` | cargo test |
| 3 | `refactor: replace all transmute with safe conversions` | cargo test + cargo clippy (partial) |
| 4-8 (group) | `cleanup: resolve all clippy lints` | cargo clippy --all-targets -- -D warnings → 0 |
| 9-10 | `refactor: struct-ify too-many-arguments functions` | cargo test + cargo clippy |
| 11 | `cleanup(wasm): remove dead WASM exports` | cargo build --target wasm32 |
| 12-13 | `cleanup: narrow visibility + impl Default` | cargo test |
| 14 | `refactor(movegen): split into focused sub-modules` | cargo test + cargo clippy |
| 15 | `refactor(search): split into focused sub-modules` | cargo test + cargo clippy |
| 16 | `refactor(wasm): split into focused sub-modules` | cargo test + cargo clippy + cargo build --target wasm32 |

---

## Success Criteria

### Verification Commands
```bash
CLOUD_EXEC_SKIP=1 cargo clippy --all-targets -- -D warnings 2>&1        # Expected: 0 warnings/errors
CLOUD_EXEC_SKIP=1 cargo test 2>&1                                        # Expected: 0 failed
CLOUD_EXEC_SKIP=1 cargo build --target wasm32-unknown-unknown 2>&1       # Expected: 0 errors
CLOUD_EXEC_SKIP=1 cargo run --release --bin bench_perft 2>&1             # Expected: D7 = 2,705,999,255
grep -rn "transmute" src/ | wc -l                                        # Expected: 0
grep -rn "#\[allow" src/ | wc -l                                         # Expected: ≤ baseline
```

### Final Checklist
- [ ] All "Must Have" present (zero clippy, zero UB, splits done, dead exports removed)
- [ ] All "Must NOT Have" absent (no algorithm changes, no wasm-bindgen pin change, no scope creep)
- [ ] All tests pass (145+ pass, 0 fail)
- [ ] D7 perft parity: 2,705,999,255
- [ ] WASM build succeeds
- [ ] All commits atomic (each passes all gates)
