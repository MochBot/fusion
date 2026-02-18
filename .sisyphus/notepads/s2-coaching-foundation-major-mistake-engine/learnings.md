# Learnings

- 2026-02-18 Task -1 corpus gate: robust qualification required parsing each `.ttrm` JSON and enforcing `gamemode == "league"` plus age cutoff (`>= 2025-11-18T00:00:00Z`) rather than trusting file presence.
- 2026-02-18 Manual collection policy validated: when first replay pull for a player returns non-200 (observed 404), stopping that player and switching to another RD<70 candidate in the same rank successfully preserved progress and met rank constraints.
- 2026-02-18 Final qualified roster (10 ranks, 3 players each, 5 replays each) uses replacements where needed: `a/asdkjc`, `a+/rabenous`, `s-/e_munny20`, `s+/peingoin`.
- Task -1 gate completed with rank-depth roster B→U (10 ranks), 3 players/rank, and >=5 replays/player (152 total).
- `404` from replay endpoint consistently indicates replay unavailable/expired for that replay ID; replacement-player fallback by same rank is effective.
- Manual request policy (2.4s cooldown + stop on first error per run loop) is workable but requires multiple resume loops.

- Task 0 baseline freeze captured at `.sisyphus/evidence/task-0-baseline-test.txt` with `cargo test --quiet` exit code 0; this snapshot is the pre-refactor invariant anchor.
- Task 0 boundary contract is codified in `.sisyphus/evidence/task-0-policy-boundary-checklist.md` with protected modules `src/search.rs` and `src/attack.rs`.
- Canonical decision source precedence for S2 coaching work: Fusion runtime code > parity tests > TetraStats code (`/home/li859/projects/mosaic-fusion-engine-coaching/TetraStats/lib/views/destination_calculator.dart`) > wiki prose.
- Lexical boundary sanity checks were executed and persisted in `.sisyphus/evidence/task-0-boundary-grep.txt`; no denylist keyword matches were found in protected modules.

- 2026-02-18 Task 3: obligation transition now uses imminent garbage (`pending_garbage - lines_cleared`) instead of pending-garbage-only thresholds, so must-cancel/must-downstack gates are tied to next-window pressure.
- 2026-02-18 Task 3: spawn-envelope safety is modeled as a deterministic fatal gate via `GameState::spawn_envelope_blocked`, and search propagation carries that gate through coaching-state ranking.
- 2026-02-18 Task 3: targeted verification artifacts saved at `.sisyphus/evidence/task-3-must-cancel.txt` and `.sisyphus/evidence/task-3-spawn-envelope.txt`.

- 2026-02-18 Task 4: whitelist-only deterministic extractor added in `src/tetrastats_features.rs` with safe division guards that force finite outputs (no NaN/inf leak on zero denominators).
- 2026-02-18 Task 4: fixed-fixture parity tests under `tetrastats_feature_tests` validate DSP/DSS/cheese index, surge metrics, APL, opener-vs-midgame pace splits, playstyle axes, and cheese-line ratios.
- 2026-02-18 Task 4: no search hot-path coupling was introduced; `src/search.rs` and ranking/comparator logic remain untouched while extractor is exported for context/calibration usage only.

- 2026-02-18 Task 5: `src/analysis.rs` severity classification is now major-first and deterministic; fatality/obligation regressions can force `Blunder`/`Mistake` even when eval-loss is small, while keeping the existing enum contract.
- 2026-02-18 Task 5: fixture tests now explicitly verify (a) severe escalation on fatality/obligation transitions and (b) non-escalation for safe minor fixtures.

- 2026-02-18 Task 6: calibration artifact generation is deterministic on the qualified snapshot when using the same players manifest and ordered bucket serialization (`SkillBucket::ORDERED`); repeated runs produced identical SHA-256.
- 2026-02-18 Task 6: severity threshold application is now bucket-aware and deterministic through `analysis::evaluate_move_for_bucket`, using loaded `CalibrationProfile` thresholds with a deterministic default fallback.
- 2026-02-18 Task 6: players-manifest parsing must treat `],` inside nested arrays (e.g., `replay_ids`) as in-player content; parser now exits the players block only when not currently in a player object.

- 2026-02-18 Task 6: added deterministic calibration module `src/calibration.rs` with explicit artifact format/version (`format=skill_bucket_calibration`, `version=1`) plus stable source fingerprinting and ordered bucket rows (`b`..`u`).
- 2026-02-18 Task 6: added `calibrate_skill_buckets` binary to generate reproducible calibration artifacts from `.sisyphus/data/replay-corpus/.../players_manifest.json`; two same-snapshot runs produced identical SHA-256 (`0a7d01c47c1baf81fa454076e344d4c4733159a2585021449dc9241478995f2d`).
- 2026-02-18 Task 6: `src/analysis.rs` now supports calibrated threshold application via `evaluate_move_for_bucket(...)` while keeping default-path `evaluate_move(...)` behavior stable for existing callers.

- 2026-02-18 Task 7: `src/wasm.rs` contract now exports additive deterministic machine diagnostics objects (`coaching_before`, `coaching_after`, `best_coaching_state`) with explicit fields `fatality|obligation|surge|phase|plonk` mapped to stable serialized enums.
- 2026-02-18 Task 7: existing severity string contract remained unchanged (`none|inaccuracy|mistake|blunder`), and contract-shape tests now assert both backward-compat severity and additive diagnostic field presence/value stability.

- 2026-02-18 Task 8: deterministic replay gate harness added via `src/replay_validation.rs` + `src/bin/replay_validation_gate.rs`; report format includes explicit metrics, thresholds, pass/fail state, and deterministic hash.
- 2026-02-18 Task 8: strict promotion thresholds are enforced in code (`severe_recall >= 0.92`, `false_severe_rate <= 0.08`, `obligation_compliance >= 0.94`) and binary exits non-zero on violations.
- 2026-02-18 Task 8: replay harness was run twice against the pinned players manifest and produced identical determinism hash (`c31c373f80123fae`) and identical report SHA-256 across runs.
