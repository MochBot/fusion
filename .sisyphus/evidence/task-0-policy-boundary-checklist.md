# Task 0 — S2 Policy Boundary Contract & Baseline Invariants

## Scope Lock (Task 0 only)
- Objective: freeze boundary/policy invariants and baseline behavior before any refactor.
- No runtime ranking/analysis behavior changes are allowed in this task.
- No new heuristics, no dependency additions, no search logic mutation.

## Canonical Source Precedence (authoritative order)
1. **Fusion code (current repo runtime behavior)**
2. **Fusion parity tests / fixtures (behavioral validation artifacts)**
3. **TetraStats code references**
4. **Wiki prose / conceptual docs (secondary only)**

If sources conflict, resolve strictly in the above order.

## Protected Modules (Task-0 boundary enforcement target)
- `src/search.rs` (critical move-selection policy path)
- `src/attack.rs` (Season 2 damage semantics used by search scoring)

## External Reference Anchor (read-only context)
- `/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/views/destination_calculator.dart`

Use this file only as a lower-precedence calibration/context source for future tasks; it does not override Fusion runtime/parity behavior.

## Boundary Denylist (prohibited in critical policy path unless explicitly re-approved later)
- Direct Season 1 policy constants/flags/aliases in protected modules (e.g., `S1`, `season1`, `s1_*`).
- Legacy toggles/keywords that bypass S2 policy ordering (e.g., `legacy` fallback branches).
- New heuristic branches introduced before invariant freeze (`experimental heuristic`, ad-hoc scoring injectors).
- Throughput-first shorthand metrics used as direct policy gates (`ppt`, `pps`, `apm`) without explicit S2 policy approval.

## Frozen Baseline Artifacts
- Baseline tests: `.sisyphus/evidence/task-0-baseline-test.txt`
  - Command: `cargo test --quiet`
  - Baseline status: **exit code 0** (all tests passing)
- Boundary grep sanity: `.sisyphus/evidence/task-0-boundary-grep.txt`
  - Result: denylist lexical checks show no matches in `src/search.rs` / `src/attack.rs`

## Invariants to Carry Forward (Tasks 1+)
- Preserve deterministic behavior while introducing S2 coaching state machinery.
- Keep lexicographic S2 policy intent explicit (survival/obligation/tactical/structure ordering work happens in later tasks only).
- Treat TetraStats/wiki as supporting references, not authority over established Fusion runtime + parity baseline.
