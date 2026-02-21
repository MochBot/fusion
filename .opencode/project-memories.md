# Supermemory Export

**Exported:** 2026-02-24T18:36:40.569Z
**Scope:** project

---

## Project Memories (125)

### [utn1MKEswWMSSbQ3MKqsgB] (error-solution)

[PROJECT] error-solution: Z-scoring desync in relabel.ts — applyPhaseZScoringForPlayer was modifying flag.severity but NOT result.severity, causing analysis results to show original severity while timeline flags showed reclassified severity. Fix: after flag loop, sync result.severity by finding primaryFlag with non-none severity and copying it to result.severity, or setting result.severity='none' if all flags were downgraded.

*Created: 2026-02-24T18:35:03.966Z*

### [jgYViUPv9PBsE4K1zDBXmB] (architecture)

[PROJECT] architecture: Composite display in ReplayViewer.svelte uses path accumulators (path_attack, path_chain, path_context) NOT leaf-node channel scores (attack_score, chain_score, context_score). Leaf values are ~0 at depth 14 because they reflect the last move in beam search, not the root move. Path accumulators sum along the full search path root→leaf. board_score is still leaf value (valid since it's the final board eval). Visibility gate: `board_score !== 0 || path_attack !== 0 || path_chain !== 0 || path_context !== 0`.

*Created: 2026-02-24T18:35:00.578Z*

### [733gcuQ6UxZvsmje7bfa1L] (architecture)

[PROJECT] architecture: combo_before wiring complete (2026-02-24). Pre-tick combo captured in snapshotWorker.ts (line after holdLockedBefore), passed as 16th arg to evaluatePositionSafe in analyzer.ts, which forwards it as `combo_before: comboBefore ?? null` in the WASM frame context object (ReplayFrameContextJson). Rust side reads combo_before from ctx for InsightDetectorInput, enabling ChainBreak detector to compare pre-lock vs post-lock combo (was previously dead code reading same post-lock value for both).

*Created: 2026-02-24T18:34:55.366Z*

### [tU3FA8Y32xZEYingH1Jhss] (error-solution)

[PROJECT] error-solution: ReplayViewer.svelte had TWO stray closing braces (at original lines 280 and 316) that caused "Unexpected else" Svelte compilation errors (ERROR 500). Root cause: previous edits to the if/else-if chain (COMPLETE → ANALYSIS_RESULT → ANALYSIS_COMPLETE → ERROR) left extra `}` braces that prematurely closed blocks before their `else if` continuations. Fix: removed both stray `}` lines. The brace chain structure must be: each `} else if` opens/closes in sequence without intermediate closing braces from inner if blocks leaking out.

*Created: 2026-02-24T18:34:49.330Z*

### [a7MDoortgHSW5LHi8wMQGU] (error-solution)

[PROJECT] error-solution: Oracle+Deep review (2026-02-24) found 3 CRITICAL bugs in insight detectors: (1) ChainBreak DEAD — wasm.rs reads ctx.combo for BOTH combo_before and combo_after (same post-lock value), so combo_before>0 && combo_after==0 is impossible. Need pre-lock combo from frontend. (2) AttackWindowMiss STARVED — uses leaf-node attack_score (~0 at depth 14) instead of path_attack. Threshold needs bump from 1.0 to 2.0-3.0 for path scale. (3) DownstackEfficiencyMiss INVALID — board_gap = best_board_score (absolute) - board_eval_delta (1-depth delta), dimension mismatch. Also: composite display shows leaf values (~0), should show path accumulators. Progress toast uses frames*playerCount (~20K) instead of lock frames (~1000). These bugs explain "no pins" on 30-sec replays.

*Created: 2026-02-24T14:31:52.997Z*

### [rhVt7R6d4iFbaLn4QK9YMw] (architecture)

[PROJECT] architecture: Coaching objective layer refactor COMPLETE (T1-T16, F1-F4) + post-E2E fixes. Final architecture: composite scoring with DIFFERENTIATED weights (board_weight=1.0, attack_weight=0.25, chain_weight=0.15, context_weight=0.10) wired through assemble_composite via SearchConfig (no longer hardcoded DEFAULT_*_WEIGHT=1.0). Context signal now threaded: shape_context_modifier(next_combo - current_combo) instead of hardcoded 0.0. 3 path accumulators: path_attack + path_chain + path_context on SearchNode/SearchResultFull/MoveEvalResultJson. chain_break insight threshold raised 0.1→0.3 + requires actual_combo_before>0 (was 85% noise, now fires only when player actually broke an active combo). InsightDetectorInput gained actual_combo_before field. Frontend MoveEvalResult in types.ts has board_score/attack_score/chain_score/context_score/path_attack/path_chain/path_context/insight_tags. SIGMOID_K=0.10. All gates pass: 139/139 lib, 14/14 presim (3 ignored), 1/1 e2e, clippy clean, WASM 227KB.

*Created: 2026-02-24T13:28:14.362Z*

### [WjwpLc4gXirtpu39JNTViq] (architecture)

[PROJECT] E2E QUALITY VALIDATION RESULTS (2026-02-24): Two real replays (firestorm 124 moves, tiki2tgt 135 moves) tested through full WASM pipeline (beam=800, depth=14). Severity classification working (93-95% none, 5-7% flagged). chain_break insight fires 85%+ of moves — TOO NOISY, threshold 0.1 needs raising to 0.3+ or adding combo_before>0 precondition. attack_window_miss and downstack_efficiency_miss fire appropriately. T-spin detection works correctly (F1803/F1827 in tiki2tgt replay). Composite weights still placeholder (DEFAULT_*_WEIGHT=1.0 in assemble_composite, not using SearchConfig differentiated weights). Context modifier hardcoded to shape_context_modifier(0.0). path_attack accumulation works (max ~34.7 across both replays). E2E test file: mosaic-fusion-testing/src/test/e2e-quality.test.ts.

*Created: 2026-02-24T12:52:38.316Z*

### [utBPTtjhMcp5NJBNR6aHSD] (architecture)

[PROJECT] architecture: Coaching objective layer refactor COMPLETE (T1-T16, F1-F4). Final architecture: composite scoring (board_weight=1.0, attack_weight=0.25, chain_weight=0.15, context_weight=0.10) in search_expand.rs gen_and_eval_root + expand_node. 3 MVP insight detectors (AttackWindowMiss, ChainBreak, DownstackEfficiencyMiss) in analysis.rs detect_insights(). SIGMOID_K=0.10 (calibrated down from 0.15 for wider composite range). WASM pkg/ output 227KB. All gates pass: 139/139 lib tests, 14/14 presim (3 ignored fixtures), clippy -D warnings clean, WASM build clean.

*Created: 2026-02-24T12:07:51.322Z*

### [SRp7XHzftk8VJd6BibxaKz] (error-solution)

[PROJECT] error-solution: T13 calibration sweep — composite scoring (board+attack+chain+context) widened score gaps causing 13.3% blunder rate (limit ≤5%) in test_calibration_severity_distributions. Fix: reduced SIGMOID_K from 0.15 to 0.10 in analysis.rs:127. K controls sigmoid steepness — lower K = less sensitivity to score gaps, compensating for ~2x wider composite range. SIGMOID_C_BASE (-13.5) unchanged. classify_win_prob_drop thresholds (5%/12%/22%) unchanged. All presim_validation tests pass with K=0.10.

*Created: 2026-02-24T11:56:21.101Z*

### [z28bs8ERkPCYZFgCnB2qjP] (architecture)

[PROJECT] architecture: T12 MVP insight detectors implemented in analysis.rs — 3 detectors: AttackWindowMiss (best_attack>1.0 + loss>0.5), ChainBreak (best_chain>0.1 + combo_after=0), DownstackEfficiencyMiss (board_gap>2.0 + delta<0). InsightDetectorInput struct takes best channel scores + actual_score + frame context (combo, lines, board eval delta). detect_insights() returns Vec<InsightResult> with tag/severity/delta. Wired into wasm.rs evaluate_position_wasm via 11-tuple destructuring. insight_tags populated as Vec<String> from InsightTag::to_str().

*Created: 2026-02-24T11:56:15.976Z*

### [CMvaxfMuRB6ecmA5NRgao4] (architecture)

[PROJECT] architecture: T11 WASM wiring complete — MoveEvalResultJson now has board_score/attack_score/chain_score/context_score (f32) and insight_tags (Vec<String>). Fields populated from SearchResultFull in evaluate_position_wasm via expanded destructuring tuple. insight_tags defaults to Vec::new() until T12 wires detectors. SearchResultFull already had these fields from T10 (copies from best node's composite scores).

*Created: 2026-02-24T11:47:15.368Z*

### [DkZcdJoj8e43PPh25SZDCm] (learned-pattern)

[USER] learned-pattern: OMO todo continuation hook fires independently of subagent task() completion. When task() returns, the file-change summary may reflect the full repo JJ diff (all uncommitted changes) rather than just the subagent's edits. If two parallel task() calls return with identical massive file-change lists, this is the JJ colocated diff showing ALL uncommitted work — not evidence of scope creep. Always verify by reading the actual target files rather than trusting the diff summary.

*Created: 2026-02-24T11:20:38.378Z*

### [9hNqGXFmWRW4a9CWZv7FjP] (error-solution)

[PROJECT] error-solution: T6 (MoveEvalResultJson extension) was marked complete in Wave 1 compressed summary but fields board_score/attack_score/chain_score/context_score/insight_tags are NOT present in wasm.rs MoveEvalResultJson struct (verified via grep). T6 needs to be re-done as part of T11 (WASM wiring). Always verify compressed summaries against actual code.

*Created: 2026-02-24T11:13:31.501Z*

### [KRUzRfdDTJjaREa4LPTzyG] (architecture)

[PROJECT] architecture: T7+T8 implementation requires threading SearchConfig through SearchExpansionContext (add `config: &'a SearchConfig` field), then using it in gen_and_eval_root and expand_node to call calculate_attack(), shape_chain_value(), shape_context_modifier(), and assemble_composite(). Key: SearchExpansionContext construction is in search.rs run_beam_search_iteration (~L157-162), params.config is already available there. Both gen/expand functions call generate() with check_spin=false that needs to become true.

*Created: 2026-02-24T11:13:31.117Z*

### [xUV8SWcXYqXKReDwwfmTto] (learned-pattern)

[PROJECT] learned-pattern: Subagent task() delegation can return with "no assistant response found" when the subagent was still reading files — the file change list in the output shows the entire JJ working copy diff (all uncommitted changes), NOT new changes by the subagent. Always verify actual source file state with `read` before assuming scope creep. The jj diff --stat shows cumulative uncommitted changes across all waves, not per-task deltas.

*Created: 2026-02-24T11:13:23.778Z*

### [KEJzzxgCe4BL84GUWeuKe6] (learned-pattern)

[PROJECT] learned-pattern: JJ colocated repo file recovery — `jj op restore` reverts the ENTIRE working copy to that operation's state, not just one change. Files created between the restore target and current state disappear from the working copy but are NEVER lost. Recovery path: (1) `jj --at-op <id> file list/show` to check visible revisions at each op, (2) if not found there, use `git log --all -- '<path>'` on the colocated git backend to find JJ's automatic working-copy snapshot commits (unreferenced git objects), then `git show <hash>:<path>` to extract. To undo just ONE operation, prefer `jj undo` (undoes only last op) or `jj op restore` to the op immediately before the bad one. Concrete case: `coaching-objective-layer-refactor.md` lost during restore to 16hr-old state, recovered from git commit `51f4968`.

*Created: 2026-02-24T10:32:24.156Z*

### [ZUN6cM33f2kZveuuNCqUqq] (learned-pattern)

[PROJECT] learned-pattern: When using `jj op restore` to revert one specific change, be aware it reverts the ENTIRE working copy to that operation's state — not just the one change you're targeting. Files created between the restore target and the current state will disappear from the working copy. Always use `jj op log` to find the most recent clean state, not an arbitrarily old one. To undo just ONE operation, prefer `jj undo` (which undoes only the last op) or use `jj op restore` to the operation immediately before the bad one. The `coaching-objective-layer-refactor.md` plan file was lost because restore targeted an op from 16 hours ago instead of the op immediately before the bad `jj describe`.

*Created: 2026-02-24T10:28:31.073Z*

### [KWrjNGUbMxNy8xYoxk7xCw] (error-solution)

[PROJECT] learned-pattern: Atlas orchestrator executed wrong plan (fusion-mass-cleanup, already complete) instead of the active coaching-objective-layer-refactor plan. Root cause: boulder.json still pointed to fusion-mass-cleanup from prior sessions, and the orchestrator followed it blindly instead of checking conversation context. Recovery: jj op restore to undo the bad jj describe, then deleted stale plans/boulder/notepads/evidence. Lesson: always verify boulder.json matches the user's current intent before executing, and delete completed plan artifacts after a plan is done.

*Created: 2026-02-24T10:26:02.707Z*

### [ncGPDLbt1Q9RzpECX6ECcy] (architecture)

[PROJECT] Plan identity + detector scope update (2026-02-24): planning artifact is now `.sisyphus/plans/coaching-objective-layer-refactor.md` (renamed from `v6-eval-rework.md`) to reflect a larger architecture-layer shift rather than a minor version bump. Allspin is explicitly global in expansion (`check_spin=true` root/depth), and detector MVP scope is `attack_window_miss`, `chain_break`, `downstack_efficiency_miss` to prioritize high-signal stability. This narrows first-pass complexity while preserving iterative extension later.

*Created: 2026-02-24T07:42:07.009Z*

### [NVfH1CT5iv3B7snyXHSvKv] (architecture)

[PROJECT] Offense/defense intent model lock (2026-02-24): `downstack_efficiency_miss` is always computed over the 14-piece horizon using `lpp = lines_cleared / pieces_used`, but it only fires when move intent is classified as downstack. Intent classifier is 2-signal primary (`OffenseOpportunity` vs `DefensePressure`) with `move_effect_delta` used only as near-tie tiebreak, preventing circular logic and reducing overfit risk. This lock preserves deliberate upstack burst lines while still penalizing true downstack inefficiency under pressure.

*Created: 2026-02-24T07:41:44.271Z*

### [aNVfJwETSgWG7d7GHUY7Su] (architecture)

[PROJECT] High-accuracy review consolidation (2026-02-24): Momus returned [OKAY] on `.sisyphus/plans/coaching-objective-layer-refactor.md`, then Oracle identified three planning tighten-ups that were applied: (1) define downstack intent inputs explicitly (`DefensePressure` vs `OffenseOpportunity`) with default gate params `τ=0.30`, `ε=0.10`; (2) fix dependency realism by removing unnecessary T13<-T12 coupling and making T15 independent; (3) explicitly initialize `root.path_attack = attack_val` for root/depth consistency. Parallelism estimate was revised from ~60-70% to ~40-50% to match actual staged dependencies.

*Created: 2026-02-24T07:41:44.254Z*

### [SCxSPB8MjPSKx6fufiHV29] (architecture)

[PROJECT] V6 plan decision update (2026-02-24): allspin is now explicitly in scope for composite-scoring rollout, replacing earlier T-only spin-gating assumptions. Plan tasks were updated so `generate(..., check_spin)` is enabled globally in both root and depth expansion paths, because spin-driven chain maintenance is considered core meta signal. Detector scope was intentionally narrowed to an MVP set (attack_window_miss, chain_break, false_safety_clear) to improve early signal quality and reduce noisy overreach before iterative refinements.

*Created: 2026-02-24T06:49:32.261Z*

### [xYuESx2CXtuqfH3zkvEhtW] (architecture)

[PROJECT] ATTACK SCORING STATUS CORRECTION (2026-02-23): Current `src/search_expand.rs` still scores nodes with `evaluate_with_tt(...)` only; `calculate_attack()` is not yet part of expansion score assembly. The b2b/combo/coaching propagation exists, so integration is straightforward, but it has not been wired into `SearchNode.score` yet. V6 rollout should treat this as an implementation task and keep TT-safe composition by caching board-eval only and adding path-dependent terms outside TT.

*Created: 2026-02-24T04:53:19.609Z*

### [obWfsSoxKBGcBUdgXZxd8s] (architecture)

[PROJECT] V6 planning constraint discovered in Metis gap analysis (2026-02-23): spin detection in search generation is controlled by a single `check_spin` boolean at `generate(...)`, so enabling it changes branching behavior materially and requires explicit policy (piece-gated vs global). Composite scoring introduces path-dependent terms, therefore `evaluate_with_tt` should remain board-eval caching only, with attack/chain/context terms composed outside TT to avoid cache invalidation artifacts. Because score variance increases, pruning settings (notably `futility_delta`) need retuning as part of the same rollout or pruning behavior becomes unreliable.

*Created: 2026-02-24T04:52:58.803Z*

### [8y8hbLo8iABwc3wxBzhUfb] (architecture)

V6 architecture review outcome (2026-02-23 brainstorming): For presim coaching objective upgrade, recommended rollout is additive-composite scoring first (board + strategic/path terms) before two-stage gates or policy-conditional objectives. Reason: lowest control-flow disruption and easiest calibration surface while still addressing current board-shape dominance. Two-stage and state-conditional objectives are deferred due to threshold cliffs/score comparability risks; validate additive behavior first with coefficient sweeps and regression checks for over-aggression vs under-correction.

*Created: 2026-02-24T04:38:47.585Z*

### [NJfuBo5PGoKu8SwKtQdcmj] (architecture)

[PROJECT] TETRIS RANK-SPECIFIC COACHING PRIORITIES (2026-02-23): D-C (<1.0 PPS): flat stacking, avoiding covering holes, recognizing garbage holes — don't penalize B2B breaks. B-A (1.0-1.4 PPS): 9-0/6-3 structure, basic TSD, one opener — penalize missing obvious TSDs. S-SS (1.5-2.0 PPS): B2B maintenance, opener proficiency, queue reading — strongly penalize dropping B2B for unnecessary singles ("skimming"). U-X (2.5-3.5+ PPS): timing, opponent tracking, cheese survival, efficiency — penalize attacking into ready opponent, flag missed spike opportunities. Midgame 3-phase framework: Build/Plonk (opponent clean → build setups), Surge/Spike (opponent vulnerable → burst), Downstack/Cheese (you're high → survive efficiently). Maps directly to CoachingState (Surge/Fatal/Build phases).

*Created: 2026-02-23T17:41:53.573Z*

### [28E9hDL9ggC52u2hprXLfE] (architecture)

[PROJECT] COACHING ENGINE PEDAGOGY RESEARCH (2026-02-23): Key findings from chess/Tetris coaching research: (1) Lichess uses Win Probability Drop (sigmoid-normalized) not raw centipawn — maps to our classify_win_prob_drop. (2) Chess.com "Brilliant" uses depth discrepancy (shallow=bad, deep=good) — maps to T-spin setups that look messy at depth 2 but resolve at depth 14. (3) Lc0 "high prior + low posterior" = move looks natural but is tactically flawed — most valuable coaching signal, identifies intuition traps. (4) Maia behavioral cloning: 9 models per 100-Elo band, surprise=-log(P) from peer model — recommending X+ moves to B-rank is mathematically optimal but pedagogically useless. (5) MochBot "no eval just presim": deep search naturally discovers penalties because bad moves constrain board 12+ moves later — show CONSEQUENCES not HEURISTIC VIOLATIONS. (6) Deliberate practice research: aggregate across games, identify patterns ("60% of blunders when stack>row 12"), ONE thing done wrong most often >> showing optimal play for every move.

*Created: 2026-02-23T17:41:46.193Z*

### [YFBhMtLwfgL4pdPdXCMX7d] (architecture)

[PROJECT] V6 EVAL REWORK — CORE INSIGHT (2026-02-23): The fundamental problem with V5 presim is that SearchNode.score = evaluate(board) which is pure board-shape heuristic (9 features: holes, height, bumpiness, well_depth, etc). The beam search optimizes for "flattest board in 14 moves" instead of "most damage / best survival path." All infrastructure for path-aware scoring ALREADY EXISTS in codebase: calculate_attack() in attack.rs is pure arithmetic (cheap), search_expand.rs already tracks lines_cleared/b2b/combo/spin_type via Move.spin() and next_chain_values(), CoachingState::transition() runs at every expansion. The only missing wire is calling calculate_attack() during expansion and adding result to score. check_spin=false in generate() prevents T-spin detection during search — must enable for T-pieces at minimum.

*Created: 2026-02-23T17:41:35.952Z*

### [5S3dCXcG9CgZk6zLJcKB32] (architecture)

[PROJECT] Fusion Debug Panel Removal (2026-02-23): Deleted src/lib/fusion/debug/ directory (FusionDebugPanel.svelte, AttackCalculator.svelte, BoardEvaluator.svelte, MoveEvalDisplay.svelte, index.ts). Removed from ReplayViewer.svelte: import line 30, showFusionDebug state + currentBoardRows derived (lines 563-572), settings toggle (lines 1333-1365), modal block (lines 1484-1551). Added analysis progress toast using toast.loading+update pattern: tracks analysisFramesProcessed counter, updates every 10 frames with "Analyzing... N/total moves (X%)", dismisses + shows success on ANALYSIS_COMPLETE. The edit tool repeatedly fails with "undefined is not an object (evaluating 'ref.trim')" on this file — use sed for line deletions instead.

*Created: 2026-02-23T17:12:10.340Z*

### [iw9DWSZuwRTeg823d7uoLE] (architecture)

[PROJECT] Mosaic Frontend - Fusion Debug Panel Architecture (2026-02-23): Debug panel lives at src/lib/fusion/debug/ with FusionDebugPanel.svelte, AttackCalculator.svelte, BoardEvaluator.svelte, MoveEvalDisplay.svelte, and index.ts re-export. Controlled by `showFusionDebug` state in ReplayViewer.svelte (line 564). Toggle in Settings section lines 1344-1371, modal rendering lines 1495-1562. Toast library at src/lib/web/toast.ts wraps external _toast — loading() returns {dismiss, update} for progress tracking. Video generation progress pattern at line 708 uses update() callback. Analysis flow uses `analyzing` boolean + `analysisPlayersExpected/Completed` tracking per worker ANALYSIS_RESULT/ANALYSIS_COMPLETE messages.

*Created: 2026-02-23T17:04:58.548Z*

### [HjPZjKwuzPjtoLvGBsF5YD] (error-solution)

[PROJECT] WASM Cache Gotcha: `cargo clean -p direct-cobra-copy` only cleans native targets, NOT the wasm32-unknown-unknown target. When WASM binary has stale compiled code despite source changes, must `rm -rf target/wasm32-unknown-unknown` before `wasm-pack build`. Vite dev server also caches WASM modules aggressively — clearing `node_modules/.vite` AND restarting the dev server is needed. Verify with `strings fusion_wasm_bg.wasm | grep <removed_symbol>` to confirm binary is actually rebuilt.

*Created: 2026-02-23T16:51:23.154Z*

### [AbHP2EvZYWJW7fHTsBSBFf] (project-config)

[PROJECT] WASM Build V5 Final: PlonkState removed from coaching system (state.rs, analysis.rs, wasm.rs). WASM rebuilt at 223KB via `CLOUD_EXEC_SKIP=1 wasm-pack build --target web --out-dir mosaic-fusion-testing/src/lib/fusion/wasm --out-name fusion_wasm --features wasm --no-default-features`. Output at mosaic-fusion-testing/src/lib/fusion/wasm/. 12 exports: JsAttackConfig, JsBoard, JsMove, calculateAttack, evaluate_board, evaluate_position, evaluate_with_weights, find_best_move, get_all_moves, init, initSync, default. V5 evaluate_position returns 11 fields including position_complexity and coaching states (no plonk). Remaining coaching multipliers: Fatality 1.5×, Surge 1.4×, Obligation 1.3×.

*Created: 2026-02-23T16:45:42.987Z*

### [Nn6YseXEN8URKPuS9Ykhh5] (architecture)

[PROJECT] PlonkState REMOVED from coaching system: User clarified "plonk" means timing/waiting for counterattack, not sloppy placement. The streak-of-non-clears heuristic (PlonkState::Stable/Drifting/Spiral based on consecutive non-clearing moves) was actively harmful — penalized intentional quad/tspin setup. Removed from state.rs (enum + fields + transition logic + serialization), analysis.rs (plonk_mul branch in coaching_dp_multiplier, test fixtures), wasm.rs (plonk_to_contract fn, MachineDiagnosticsJson plonk field, PlonkState import). Remaining multipliers: Fatality 1.5×, Surge 1.4×, Obligation 1.3×. Serialization format stays "v2|..." with 6 fields (fatality|obligation|surge|phase|ply).

*Created: 2026-02-23T16:42:54.277Z*

### [H4ny42uvNKTRSeJQywFocW] (project-config)

[PROJECT] V5 Calibration Values (final): SIGMOID_K=0.15 (was 0.12), SIGMOID_C_BASE=-13.5 (was -15.0). compute_sigmoid_c uses ln(pps) for exponential skill separation: D-rank c≈-13.0, S-rank c≈-18.3, X+-rank c≈-24.9. Calibration results: X+ 92% None / 0% Blunder, S 68% None / 8% M+B, D 60% None / 20% M+B. classify_win_prob_drop thresholds unchanged: ≥0.22 Blunder, ≥0.12 Mistake, ≥0.05 Inaccuracy. coaching_dp_multiplier max-across-dims: Fatal=1.5, Active=1.4, MustCancel=1.3, Spiral=1.2.

*Created: 2026-02-23T16:34:18.471Z*

### [GdfpsMAcH3cREyNNczvrF6] (architecture)

[PROJECT] V5 Presim Coaching Architecture: Frontend has TWO analysis paths — snapshotWorker.ts (production) passes all 13 coaching args (player stats, b2b, combo, lines_cleared, hold_used, pending_garbage) to evaluatePositionSafe, while analyzer.ts runAnalysis (legacy/main-thread) only passes 5 args (board, postBoard, piece, queue, hold). Only the worker path benefits from V5 coaching multipliers and forced root move. position_complexity is returned by WASM but NOT consumed by any frontend component yet — available for future UI integration.

*Created: 2026-02-23T16:34:12.064Z*

### [F799shvbkBZUMrYs4pxxAB] (error-solution)

[PROJECT] Presim Coaching V5 Calibration Test: tests/presim_validation.rs (708 lines) contains 3 calibration tests for Task 9. Key compilation fixes: Board doesn't implement Copy (use .clone()), root_scores is Vec<(Move, f32)> not HashMap (use .iter().map(|(_, s)| *s)), GameState.b2b is u8 not bool (use 0 not false), find_best_move_with_scores returns Option<SearchResultFull> (needs .unwrap()). search_root_scores() uses SearchConfig::default() (beam_width=800, depth=14) which is slow but accurate for calibration.

*Created: 2026-02-23T16:23:32.376Z*

### [dzrG8MebPscPL2mfzkNmnz] (learned-pattern)

[PROJECT] Task 9 Calibration Approach: WASM-based replay analysis is too slow (beam_width=800, depth=14 takes 100+ sec per replay in Node.js). Chose native Rust calibration test instead — create diverse board scenarios, run find_best_move_with_scores natively, classify severity at D/S/X+ tiers via classify_win_prob_drop. PlayerSkill default is S-rank (pps=1.57, app=0.48, dsp=0.20). compute_sigmoid_c values: D(pps=0.69)≈-18.9, S(pps=1.57)≈-20.3, X+(pps=3.27)≈-24.9. Task 9 acceptance: X+≥60% None ≤5% Blunder, D≥15% Mistake+Blunder, monotonic worsening X+→D.

*Created: 2026-02-23T16:14:10.617Z*

### [JAorjvAZaQj52nUWYCujpc] (project-config)

[PROJECT] Calibration Corpus Infrastructure: 49 rank-labeled .ttrm replay files exist at mosaic-fusion-testing/src/test/fixtures/replays/bulk/ with filename prefixes encoding rank tiers (u__=U/X+, ss__=SS/X+, s__=S, s-__=S-, a+__=A+, a-__=A-, b+__=B+, b__=B). bulk-replay.test.ts reads all files, runs analyzeReplayFile() for all rounds × both players, collects severity distributions (none/inaccuracy/mistake/blunder counts + avgEvalLoss). Run via `npx vitest run src/test/bulk-replay.test.ts` from mosaic-fusion-testing/. Current WASM build includes all Wave 1+2 changes (SIGMOID_K=0.15, beam_width=800, depth=14, futility_delta=5.5, coaching multipliers, position complexity, root move forcing).

*Created: 2026-02-23T16:06:06.408Z*

### [wfBp6tqCxhgsgFwS2LQAJH] (architecture)

[PROJECT] Responsibility Split for Calibration: Fusion (Rust) owns search/eval/scoring/sigmoid/severity classification — pure board-state math exposed via WASM. Mosaic (TS/Triangle) owns replay parsing, board reconstruction, simulation, display, and calling Fusion WASM with board states + player stats. For corpus calibration (Task 9), data collection runs through Mosaic's existing pipeline (harness.ts → evaluatePositionSafe → WASM). Rust-side constants (SIGMOID_K, classify_win_prob_drop thresholds, compute_sigmoid_c coefficients) are tuned based on Mosaic output distributions. Don't overthink — Mosaic already handles everything needed for extraction/simulation/display.

*Created: 2026-02-23T16:03:59.767Z*

### [joyANYSg449eNn6XmMywGs] (project-config)

[PROJECT] WASM Build & Deploy Config: Build command is `wasm-pack build --target web --out-dir <path> --out-name fusion_wasm --features wasm --no-default-features`. Output goes to `mosaic-fusion-testing/src/lib/fusion/wasm/` (NOT static/pkg/ as plan originally stated). Crate name is `direct-cobra-copy` but `--out-name fusion_wasm` renames output. Frontend imports from `$lib/fusion/wasm/fusion_wasm` in 3 files: harness.ts, fusion/index.ts, fusion/config.ts. WASM exports: init, calculateAttack, evaluate_board, evaluate_with_weights, evaluate_position, find_best_move, get_all_moves, JsAttackConfig, JsBoard, JsMove. The `evaluate_move` export was removed in V4 cleanup — dangling import in index.ts was fixed by removing that re-export line.

*Created: 2026-02-23T16:01:32.238Z*

### [u1ZwjHfHCk2hZuVwZA969C] (architecture)

[PROJECT] Replay Analysis Pipeline Architecture: The full replay→board→analysis pipeline lives in mosaic-fusion-testing (TypeScript), NOT in fusion-engine (Rust). Triangle.js (`src/lib/triangle/`) handles replay parsing and board state reconstruction via Engine class. The analysis flow is: parse .ttrm → Triangle Engine → captureSpawnContext/captureLockContext (lifecycle.ts) → detectBoardChange → buildCleanPostBoard → evaluatePositionSafe (calls WASM). The test harness at `src/test/harness.ts` has `analyzeReplay(ttrm, playerIndex, roundIndex)` that runs the full pipeline. Replay API at `src/lib/api/remote/replay.remote.ts` fetches replays via `api.replay`. The Rust fusion-engine only receives board states via WASM bridge (evaluate_position_wasm) — it never parses replays directly. For Task 9 corpus calibration, must either: (a) run the TS pipeline to generate board states, then feed to Rust tests, or (b) use synthetic boards in Rust that simulate rank-appropriate difficulty levels.

*Created: 2026-02-23T15:56:53.200Z*

### [gZs55Ypjv78goMC1L4b3ay] (learned-pattern)

[PROJECT] Rogue subagent prevention: 6/6 delegation attempts failed across presim-v5 and fusion-mass-cleanup. gemini-3-flash produces '(No text output)' and crashes silently. gemini-3.1-pro-high goes rogue modifying 17+ source files. JJ snapshots dirty working copy on crash. Recovery: jj op restore <op-id>. Strategy: direct implementation by orchestrator only, no more delegation.

*Created: 2026-02-23T15:35:28.167Z*

### [ELeKi8QrBgVbbjzQNsZ9y9] (error-solution)

[PROJECT] Flaky test test_attack_integration_tspin_scores_higher: Random seed search (8000 seeds, depth 1) finds candidate_pairs=0 after search param upgrades (futility_delta 3.0→5.5, beam_width 300→800). The wider beam and higher futility threshold change which moves survive pruning at depth 1. Fix: replace random seed approach with deterministic T-spin-ready board construction using board_from_bottom_rows + row_with_gap helpers already in presim_validation.rs.

*Created: 2026-02-23T15:35:23.170Z*

### [4LZTmybqJDw7gg1mMhb91V] (architecture)

[PROJECT] Presim V5 Wave 2 Complete: Tasks 5 (root move forcing), 6 (coaching ΔP multipliers), 7 (SIGMOID_K 0.15), 8 (position complexity) all implemented. search.rs=841 lines, analysis.rs=726 lines, wasm.rs=607 lines, search_config.rs=92 lines. 132 lib tests pass, clippy clean. Key values: beam_width=800, depth=14, futility_delta=5.5, CABS start width=200, time_budget_ms=None.

*Created: 2026-02-23T15:35:18.414Z*

### [VofSpYygcgYgepkUEBZcmj] (architecture)

[PROJECT] CURRENT EVAL PIPELINE STATE (2026-02-23 post-cleanup): evaluate_position_wasm (wasm.rs): pre_board + post_board + piece + frame_context → find_best_move_with_scores (depth-12, width 300, 200ms budget) → SearchResultFull { best: SearchResult, root_scores: Vec<(Move, f32)> sorted desc } → generate-and-compare to identify actual move → lookup in root_scores → eval_loss = best_search_score - actual_search_score → win_prob sigmoid with skill-adaptive c(pps,app,dsp) → classify_win_prob_drop (5/12/22% ΔP thresholds) → coaching state escalation via classify_major_first → MoveEvalResultJson. If actual move not found in root_scores: severity=none, eval_loss=0 (no static eval fallback). Sigmoid: k=0.12, c = -15.0 + -5.0×ln(pps) + -2.0×app + -8.0×dsp. eval.rs has 9 features: holes(-4.0), cell_coveredness(-0.5), height(-0.2), height_upper_half(-1.0), height_upper_quarter(-5.0), bumpiness(-0.3), bumpiness_sq(-0.1), row_transitions(-0.3), well_depth(+0.2). 272 lines total. Frontend: snapshotWorker.ts passes player stats + coaching context → ReplayViewer.svelte receives ANALYSIS_RESULT → phase z-scoring at ANALYSIS_COMPLETE → rebuildDisplayFlagsForPlayer → Slider.svelte renders severity-colored pins.

*Created: 2026-02-23T07:21:22.801Z*

### [9m5ah7r4TE3a99YD1156nr] (learned-pattern)

[PROJECT] RESEARCH FINDINGS — COACHING ENGINE PATTERNS (2026-02-23): Key patterns from 10 research agents across chess/Go/poker coaching engines: (1) Lichess: CPLoss = search_score(best) - search_score(actual), Win% sigmoid 1/(1+10^(-cp/400)), classify by WIN PROBABILITY DROP ≥30%=Blunder, ≥20%=Mistake, ≥10%=Inaccuracy. (2) Chess.com: depth discrepancy for Brilliant/Great detection (shallow vs deep eval). (3) KataGo: phase-relative z-scoring — z=(loss-μ_phase)/σ_phase, z>2.0=Blunder. Directly applicable to Tetris: "massive misdrop before death correlates to causing factor." (4) Maia Chess: 9 models per 100-Elo band, behavioral cloning predicts what player WOULD play, move surprise = -log(P). Transfer to Tetris: 10×20 grid, ~34 placements/frame, train on TTRM replays grouped by TR band. (5) Lc0: policy (prior/intuition) vs MCTS (posterior/calculation) — high prior + low posterior = natural mistake. (6) MochBot/MochEngine2: "No eval, just presim" — trivial static eval, depth 12-20, deep search naturally discovers penalties. Luke is creator, fully closed source. (7) Cold Clear 2: CMA-ES-tuned linear eval, SOTA for competitive Tetris bots. (8) ML feasibility in WASM: tiny NN (50→64→64→1) ~5-15ms for 10K evals, tract crate for ONNX in wasm32. But consensus: linear eval + presim remains best for Tetris.

*Created: 2026-02-23T07:21:12.333Z*

### [f8JYWRYSzizehhx6DduhG3] (architecture)

[PROJECT] RESEARCH SYNTHESIS — NEXT STEPS FOR PRESIM COACHING (2026-02-23): 10 research agents + 2 Gemini Deep Research reports + Oracle consultation synthesized into actionable next steps. COMPLETED: (A) Pipeline fixes — eval_loss=0 bug, severity calibration (70%→28% blunders→4%), auto-pause severity filtering, timeline pins. (B) Search-score quality scoring — find_best_move_with_scores returns SearchResultFull with per-root-move max leaf scores from beam. Quality delta = best_search_score - actual_search_score (FREE from single search pass since beam width 300 > ~34 placements). (C) Stat-adaptive sigmoid — c(pps,app,dsp) shifts inflection per player skill from TTRM aggregatestats. (D) Phase z-scoring in relabel.ts — reclassifies severity within opening/midgame/endgame phases. (E) Coaching state hardcodes fixed — lines_cleared/b2b/combo/hold_used/pending_garbage now passed from JS to WASM. REMAINING (ordered priority): (1) Verify quality metric uses full search score including attack — search.rs already integrates S2 attack in gen_and_eval_root/expand_node via calculate_attack, but confirm leaf scores propagate attack value into root_scores. (2) Sigmoid recalibration — current k=0.12, deep research recommends k=0.15 with skill-scaled c. Thresholds 5/12/22% may need tuning against replay corpus. (3) Coaching state multipliers — Fatality 1.5x, Surge 1.4x, Obligation 1.3x, Plonk 1.2x on ΔP before classification (from deep research V3). (4) Forced-root search for beam-pruned moves — 15ms secondary search (depth 6, width 50) when player's move not in root_scores. (5) Position complexity — variance of top 10 root scores indicates forced vs creative positions, adjust classification tolerance. (6) 7-bag queue extension — track bag state to predict 1-3 pieces beyond visible queue. (7) Maia-style behavioral cloning — deferred, requires training data pipeline on Colab.

*Created: 2026-02-23T07:20:57.001Z*

### [vHgVngSicMBg7FHLtesxp8] (architecture)

[PROJECT] V4 CLEANUP COMPLETE (2026-02-23): Removed all V4 eval bloat that contradicted presim-first architecture. Changes: (1) eval.rs: Removed 6 zeroed V4 features (aggregate_height, height_variance, almost_complete_rows, t_slot, blockade_depth, col_transitions), their 5 helper functions, BoardFeatures struct, and extract_features(). File reduced 438→272 lines. Original 9 features kept intact (holes, cell_coveredness, height, height_upper_half, height_upper_quarter, bumpiness, bumpiness_sq, row_transitions, well_depth). (2) wasm.rs: Removed FeatureDeltasJson struct, extract_features import/calls, delta computation, feature_deltas field from MoveEvalResultJson. Fixed quality metric fallback bug — when actual move not in root_scores, severity="none" + eval_loss=0 instead of falling back to static eval (depth-1 vs depth-12 scale mismatch). (3) Frontend (types.ts, snapshotWorker.ts, ReplayViewer.svelte): Removed FeatureDeltas interface, CoachingInsight interface, DELTA_LABELS, generateCoachingInsights function, featureDeltas from MoveFlag, coaching insights HTML block. All changes uncommitted in JJ working copy.

*Created: 2026-02-23T07:20:38.936Z*

### [xpvhDrUcc8xEaMjaL16RSJ] (architecture)

[PROJECT] EVAL ARCHITECTURE DECISIONS — IMMUTABLE (2026-02-23): User chose Option C (pure presim, search-score quality scoring). These decisions are FINAL and must not be drifted from:

1. NO EVAL WEIGHT TUNING: MochBot philosophy — "no eval, just presim." Search depth IS the evaluation. Do NOT add eval features for tuning (CMA-ES, NNUE, etc.). The V4 eval features (aggregate_height, height_variance, almost_complete_rows, t_slots, blockade_depth, col_transitions) added with zeroed weights were a MISTAKE — they contradict presim-first architecture.

2. QUALITY METRIC MUST USE FULL SEARCH SCORE: The search score from depth-12 beam search includes both board eval AND attack value (calculate_attack with S2 mechanics, 99.99% parity). Quality delta = best_search_score - actual_search_score. Do NOT use static eval (depth-1) for quality comparison.

3. ATTACK AND LINE CLEARS MUST FACTOR INTO QUALITY: search.rs already integrates S2 attack mechanics (gen_and_eval_root, expand_node call calculate_attack with b2b/combo propagation). The quality scoring pipeline must surface this — a T-spin triple sacrifice that the search correctly values should NOT be flagged as a mistake.

4. FEATURE DELTAS ARE NOT INSIGHTS: The extract_features → FeatureDeltasJson pipeline is debugging information, not coaching insights. Either remove from WASM hot path or don't compute at all. Do not present as coaching content.

5. SEARCH SCORE = GROUND TRUTH: Leaf node search score at depth 12 is the quality metric. No secondary eval, no percentile ranking, no threshold tables. Win% sigmoid on search scores for classification.

*Created: 2026-02-23T06:55:21.257Z*

### [3vRo3JG7wu71Zu4ENpV5hh] (architecture)

[PROJECT] MOCHBOT: Luke is the creator of MochBot. MochBot currently uses MochEngine 2. MochBot is fully closed source. Cold Clear 2, Zetris, and Misamino are all 4-5 years old and considered outdated by the user.

*Created: 2026-02-23T04:39:28.862Z*

### [8tQKCPVcqXGQZKnDy6D2ZE] (architecture)

[PROJECT] EVAL ARCHITECTURE: User chose Option C (pure presim / search-score quality scoring with win% sigmoid). Budget is NOT a hard constraint for replay analysis — performance should not be sacrificed for arbitrary time limits. Increase budget as needed.

*Created: 2026-02-23T01:24:57.642Z*

### [UhhGhhQQXKYwx58HiZZhrn] (project-config)

[PROJECT] ANTIGRAVITY MODEL CONFIG (2026-02-20): Correct upstream model names found in C:\Users\li859\.antigravity_tools\accounts\*.json (accessible via /mnt/c/Users/li859/.antigravity_tools/accounts/ in WSL). Gemini 3.1 Pro variants: `gemini-3.1-pro-high` and `gemini-3.1-pro-low`. No plain `gemini-3.1-pro` exists. Using `gemini-3.1-pro-high` everywhere. Config updated: opencode.json (compaction + model definition key), oh-my-opencode.jsonc (multimodal-looker, visual-engineering, deep, artistry), CONFIG_SNAPSHOT.md. Thinking level is baked into model name — no variant needed. Proxy needs Windows-side update before models will route (currently returns "not available on this version"). Other available upstream models: claude-opus-4-6-thinking, claude-sonnet-4-6, gemini-3-flash, gemini-2.5-pro, gemini-2.5-flash, gemini-2.5-flash-lite, gemini-2.5-flash-thinking, gemini-3-pro-image.

*Created: 2026-02-20T21:39:45.207Z*

### [3PAixmofnadA46EcwnV38g] (architecture)

[PROJECT] MOSAIC DEAD-CODE CLEANUP (2026-02-19): Deleted unused src/lib/analysis/sim.ts. Extracted temporal relabeling from ReplayViewer.svelte into src/lib/analysis/relabel.ts (ReplayViewer imports from $lib/analysis/relabel). Reduced export surface in workerProtocol.ts, mapping.ts, snapshotWorker.ts (internalized unused helpers). Fixed malformed signature in overlay.ts. Removed unused imports in bulk-replay.test.ts. Updated .gitignore for .ttrm exclusion. Committed as "cleanup: dead code removal, relabel extraction, and reduced export surface".

*Created: 2026-02-20T04:28:22.277Z*

### [97zGfKFHriHQtm2kRC9376] (architecture)

[PROJECT] REPLAY_VALIDATION GOD-FILE SPLIT (2026-02-19): replay_validation.rs split into 3 modules: replay_validation.rs (289 lines, orchestration layer), replay_validation_labels.rs (42 lines, label classification logic), replay_validation_manifest.rs (96 lines, manifest handling). All 152 tests pass. Committed as "cleanup: dead code removal and replay_validation god-file split".

*Created: 2026-02-20T04:28:17.249Z*

### [iSyJMMwWH9PfFkYbVb2oUq] (architecture)

[PROJECT] UNSAFE ELIMINATION STATUS (2026-02-19): Safe conversion functions rotation_from_u8() and spin_from_u8() added in wasm.rs. However, 15 transmute calls REMAIN: gen.rs (4: lines 32,72,230,275), header.rs (3: lines 168,173,180), movegen.rs (8: lines 104,131,175,493,539,578,681,789). 2 MaybeUninit in movegen.rs:23 and :96 (MoveBuffer). piece_from_u8 does not exist. ~20+ unsafe blocks remain across codebase. Should be audited for soundness.

*Created: 2026-02-17T19:45:46.231Z*

### [GnKtbfhAQJuhTXWuWnGKUs] (architecture)

[PROJECT] MOSAIC-FUSION-TESTING TECH STACK (2026-02-17): Svelte 5.49.1, Vite 7.3.1, Tailwind 4.1.18, PIXI.js 8.15.0, Vitest 4.0.18. Uses Svelte 5 runes ($state, $derived, $effect). WASM loaded via Web Worker. Triangle engine at src/lib/triangle/ for replay simulation.

*Created: 2026-02-17T19:45:43.450Z*

### [r4ePgd4D4g2vJmc6bs3aUN] (architecture)

[PROJECT] BUG FIXES APPLIED (2026-02-17): Bug 1 (impossible placements) - evaluate_position validates best move against post-lock board via obstructed_move(), suppresses suggestions blocked by garbage. Bug 3 (P2 analysis) - ReplayViewer iterates ALL players in moveflagsPerPlayer, auto-switches selectedPlayer. Bug 4 (eval calibration) - thresholds widened from 0.5/1.5/3.0 to 5/15/30 to match board eval scale where swings reach 20-50 points.

*Created: 2026-02-17T19:45:39.849Z*

### [Wx24ye7zLovPsCe76Kgjef] (architecture)

[PROJECT] FUSION V2 WASM DEPLOYMENT: wasm-pack builds to pkg/ with package name direct-cobra-copy. Files are RENAMED when deployed to mosaic: fusion_wasm.js + fusion_wasm_bg.wasm at src/lib/fusion/wasm/. App imports via fusion_wasm.js (NOT direct_cobra_copy names). wasm-pack 0.14.0 changed --out-dir to --artifact-dir (requires nightly); use default pkg/ output instead. wasm-opt = false in Cargo.toml metadata.

*Created: 2026-02-17T19:45:35.493Z*

### [BH3HbFhwK22YNScYv91qC7] (architecture)

[PROJECT] FUSION V2 CURRENT STATE (2026-02-19): 27 source files, 8,721 total lines, 152 tests. Package direct-cobra-copy. Attack-aware beam search with evaluate_position(). Eval uses CoachingState (Fatal/Critical/Safe) tiering. 15 transmute calls remain (see unsafe elimination memory). Stack-allocated SearchNode paths and RemainingPieces. SearchConfig: beam_width=300, depth=12, futility_delta=3.0, time_budget_ms(Option), use_tt(bool), extend_queue_7bag(bool), attack_config(AttackConfig). God-file split: replay_validation.rs(289) + replay_validation_labels.rs(42) + replay_validation_manifest.rs(96).

*Created: 2026-02-17T19:45:31.489Z*

### [8mQUxfxDPk63hm6bdxQewQ] (error-solution)

[PROJECT] EVAL POPUP PRE-LOCK TIMING FIX (2026-02-17): ReplayViewer.svelte now indexes moveflags by Math.max(1, preLockFrame - 1) instead of resultFrame (lock frame). This makes the eval popup pause playback 1-2 frames BEFORE the piece visually locks. Removed the old jumpTo(rewindTo) rewind logic at lines 661-668 — no more jarring frame jumps. The preLockFrame comes from the worker's ANALYSIS_RESULT message (spawn.frame captured before engine.tick()).

*Created: 2026-02-17T16:16:15.293Z*

### [ds9zN7xPydwkZfXYaDjzyM] (architecture)

[PROJECT] ATTACK SCORING INTEGRATION (2026-02-19): SearchNode tracks b2b(u8), combo(u32), pending_garbage, coaching(CoachingState), root_move, root_hold_used, path. NO attack_total field on SearchNode. SearchConfig uses attack_config: AttackConfig::tetra_league() (not direct attack_scale/attack_cap fields). gen_and_eval_root and expand_node compute spin type from Move.spin(), call calculate_attack() with proper b2b/combo propagation. Root nodes inherit b2b/combo from GameState. 152 tests pass.

*Created: 2026-02-17T16:16:10.319Z*

### [21hkrNrWxDET3Pj6qQv6rP] (error-solution)

[PROJECT] BUTTON FIX LESSON (2026-02-17): In ReplayViewer.svelte, snapshot worker error handlers (ERROR message + onerror) killed the worker via terminate() but never reset `analyzing = false`. Since `ready = $derived(splitFrames.length > 0 && frames > 0 && !analyzing)` gates ALL button handlers with `if (!ready) return`, any worker error permanently disabled every playback button. Fix: always set `analyzing = false` BEFORE calling `snapshotWorker?.terminate()` in both error paths. RULE: Any state that gates UI interactivity MUST be reset in ALL error/cleanup paths, not just happy paths.

*Created: 2026-02-17T15:47:08.939Z*

### [qd8BYZJGq3T32V4CTMdTWB] (error-solution)

[PROJECT] VITE CACHE BUG: When mosaic-fusion-testing buttons stop working / page shows only MOSAIC logo / browser shows "Internal Error", the root cause is stale Vite dependency cache. Fix: `rm -rf node_modules/.vite` and restart dev server. Symptom: `buffer_.js` returns 504 "Outdated Optimize Dep", SvelteKit app.js fails to dynamically import, hydration never completes, zero buttons in DOM.

*Created: 2026-02-17T14:35:31.485Z*

### [fmt5BgwGhuDxoWGh2ivf6a] (preference)

[PROJECT] Testing direction: User wants to replace Playwright-facing tests with backend-only TTRM simulation tests that detect lock frames and run beam-search evaluation without frontend server/manual checks.

*Created: 2026-02-17T10:37:57.086Z*

### [9Hrj6pTm4tpWYtxX9MXQvP] (error-solution)

[PROJECT] ANALYZER.TS OFF-BY-ONE BUG (2026-02-16): Two timing bugs in mosaic-fusion-testing/src/lib/analysis/analyzer.ts: (1) preLockFalling captured BEFORE engine.tick() had mid-flight Y coordinate, but piece locks at different Y during tick. Fix: board XOR diff (old vs new board rows) derives actual locked position — deriveLockedPosition() helper added at bottom of file. (2) Queue captured AFTER tick — engine.tick() calls nextPiece() which does queue.shift(), so captured queue is missing the just-spawned piece. Fix: capture preLockQueueIds and preLockHoldId BEFORE tick. (3) evaluateMoveSafe previously passed frameIndex (number) as 4th arg to WASM evaluate_move where Rust expected {queue, hold} object. Fix: pass mapped queue/hold arrays. DESPITE THESE FIXES, eval values still spike in browser (284, 349, 242 losses). Further debugging needed.

*Created: 2026-02-17T03:50:48.149Z*

### [nFu8cF82keMfkAWMhNEiKL] (error-solution)

[PROJECT] WASM CRASH FIX CHAIN (2026-02-16): Three bugs causing WASM unreachable trap at frame 97 during replay analysis: (1) board.rs place() had no bounds checking — negative coords from external replay data cast to usize → array index OOB → WASM trap. Fixed: added `if xu < COL_NB && yu < BOARD_HEIGHT` guards. Also added `if !is_ok_move(m) { return 0; }` release-mode guard in do_move(). (2) wasm.rs had no panic guard — any Rust panic kills entire WASM instance. Fixed: wrapped evaluate_move_wasm and analyze_replay_wasm in std::panic::catch_unwind(AssertUnwindSafe(...)). Returns JsValue::NULL on panic. (3) Cargo.toml wasm-opt caused build failure — added `[package.metadata.wasm-pack.profile.release] wasm-opt = false`.

*Created: 2026-02-17T03:50:41.485Z*

### [hrVHtRwxwE2yxRGDCo7gjT] (architecture)

[PROJECT] PRESIM IMPLEMENTATION COMPLETE (2026-02-19): All 7 presim optimization tasks implemented and verified. 152 tests pass, 0 fail. (1) beam_width=300, depth=12. (2) TranspositionTable 64K entries (DEFAULT_TT_SIZE=65536), ZobristKeys 400 keys, depth-preferred replacement. (3) futility_delta=3.0 default. (4) CABS iterative widening: width starts 100, doubles to max_width. (5) BagTracker with extend_queue (predicts when remaining.len()<=2). (6) TT opt-in (use_tt default false), bag opt-in (extend_queue_7bag default true). (7) WASM time_budget_ms:Some(50). New files: src/transposition.rs, src/bag.rs.

*Created: 2026-02-16T10:57:35.440Z*

### [QBuiMgynqJ7bwZbpvxzpxe] (learned-pattern)

[PROJECT] GEMINI DR: PRESIM ALGORITHM RESEARCH (2026-02-16): Key findings beyond Oracle analysis: (1) TDS (Transposition-Table-Driven Scheduling) is superior parallel architecture — hash-partition state ownership across threads, lock-free TT, 138x speedup reported on 128 cores. (2) SIMD batch eval: load 8-16 successor states into vector registers, evaluate simultaneously, divides eval time by vector width. Works in WASM via simd128. (3) CABS (Complete Anytime Beam Search) is formal framework for iterative widening that converges to completeness. (4) Probe TT and futility BEFORE eval call — saves 50ns per pruned node. (5) PEXT unavailable in WASM — use magic bitboards instead. (6) Macro-moves: combine deterministic multi-step sequences into single logical jumps to reduce effective tree depth. (7) Achievable depth 20+ with full optimization stack. (8) IDA*/RBFS/A* mathematically precluded at b=35 — confirmed. Paper refs: BMCTS (Winands 2012), Memory-Bounded BFBS (AAAI SOCS 2023), MCTS Survey (Browne 2012).

*Created: 2026-02-16T10:41:25.699Z*

### [BsGFSEo1fmVRLVyM1DvEZb] (architecture)

[PROJECT] PRESIM ARCHITECTURE DECISIONS (2026-02-16): Research completed on optimal presim algorithm. Key decisions: (1) Beam search is optimal for single-agent deterministic search — 2-4x faster than MCTS per BMCTS paper (Winands 2012). Skip MCTS, IDA*, RBFS. (2) Current search uses 5.6% of compute budget (168K nodes of 3M available at 30M NPS WASM). Sweet spot: width 300, depth 12, ~2.5M nodes, ~83ms. (3) 7-bag queue extension: track bag state to predict 1-3 pieces beyond visible queue, extending effective depth. (4) Opponent-as-environment architecture: don't simulate both players. Inject opponent data (GarbageTimeline, board height, pending garbage) as environment context into single-player presim. Branching stays ~35. (5) Three context signals: incoming garbage injection at correct depths, opponent vulnerability weighting, cancellation opportunity scoring. (6) Score = board_eval + α*attack*urgency + β*cancellation (α=0.3, β=0.5 start). (7) Implementation priority: fill budget (depth 6→12) → futility pruning → 7-bag extension → iterative deepening → TT experiment. Total ~5-9h for presim, ~8h for opponent context. (8) Advice generation layer deferred to another team member — presim outputs structured MoveAnalysis data.

*Created: 2026-02-16T10:04:47.354Z*

### [q18w83w16bne2vc4P9dSFY] (learned-pattern)

[PROJECT] WASM PERFORMANCE BUDGET FOR FUSION ENGINE (2026-02-16): Native ~113M NPS serial / ~812M NPS parallel. Expected WASM: 1.4-2.0x slowdown → ~65-80M NPS serial / ~480-580M NPS parallel. Key optimizations: (1) Use raw #[no_mangle] exports NOT wasm-bindgen for hot paths (2.5-3x faster), (2) wasm-opt -O4 with --flexible-inline-max-function-size for 40% speedup, (3) wasm-bindgen-rayon for parallelism requires nightly+build-std+COOP/COEP headers, (4) SIMD (wasm-simd128) is production-ready in Chrome 91+/Firefox 89+/Safari 16.4+. Main thread cannot block with threads — run WASM in workers only. For depth 6 beam search: native 668ms → WASM serial ~900ms-1.1s → WASM parallel ~350-450ms. Budget 30-50ms per move for depth 4-5 presimulation in browser. Sources: ar5iv.org/html/1901.09056 (45-55% WASM overhead on SPEC CPU), RReverser/wasm-bindgen-rayon, binaryen wasm-opt docs.

*Created: 2026-02-16T09:49:25.969Z*

### [Ly5qTE79wsCHVkEBUxyNWC] (architecture)

[PROJECT] MOCHBOT vs FUSION CLARIFICATION: MochBot/fusion is the upstream Tetris engine repo. Fusion V2 is our standalone Rust crate (package: direct-cobra-copy) at /home/li859/projects/mosaic-fusion-engine-coaching/fusion-engine -- a ground-up rewrite, NOT inside fusion-test-local anymore. MochBot/mosaic is the upstream frontend. Our fork is mosaic-fusion-testing.

*Created: 2026-02-16T09:47:20.260Z*

### [CxY1pixL6TQaCbStYC2a3D] (error-solution)

[PROJECT] ATTACK PARITY STATUS (2026-02-16): All 4 TetraStats gaps FIXED: (1) Surge release implemented. (2) Combo minifier fixed — max(log_floor, damage) not additive. (3) >5 line scaling implemented. (4) Garbage clear boost implemented. B2B eligibility guard removed (trust caller's b2b value). Result: 0 attack mismatches across 50 replays (~80K locks). 4 remaining mismatches are board-placement only (movegen BFS no-candidates). Overall: 99.99%+.

*Created: 2026-02-16T09:40:10.760Z*

### [oukL71ekKF7bD3qECPdyM7] (architecture)

[PROJECT] S2 TETRIS BOT LANDSCAPE 2026: Three viable open-source engines: (1) Fusion (MochBot, Rust, best-in-class, S2 attack tables, ~240K NPS, modular eval engine), (2) Teapot (danielyx-z/Tetrio-Bot, Python, ~25K NPS, exposed heuristics.py for weight tuning, screen-capture input), (3) Blockfish (blockfish/blockfish, Rust, specialist for cheese/downstack/defense, updated July 2025, no Surge understanding). CC2 is 4 years old and NOT S2-aware. No public bot fully implements Surge potential valuation. Key S2 eval requirements: (a) Surge level state variable with nonlinear weight w_surge * S_l^k, (b) All-Mini maintenance clears valued highly for B2B chain preservation, (c) line clear penalty for non-spin singles/doubles in spike meta, (d) spike efficiency over APM. Anti-cheat: input fuzzing (Gaussian ~30ms±5ms), finesse degradation, reaction delay (~200ms). Neural network evaluators emerging in private sector but no public implementations.

*Created: 2026-02-16T07:15:55.684Z*

### [qVngkptTuQT7AW3MJmTis8] (error-solution)

[PROJECT] MOSAIC LOADING FIX (2026-02-16): Root cause of loading screen stuck was missing PUBLIC_IS_PROD env var — SvelteKit's $env/static/public module crashed with SyntaxError preventing entire client JS from loading. Fix: created .env with PUBLIC_IS_PROD=0. Secondary fix: removed skin texture gate in +layout.svelte that blocked loading when PIXI textures failed (WSL no GPU). Also: npm run dev is the correct command (not bun run dev). Triangle submodule requires bun install + bun run build:typia for typia code generation before app works.

*Created: 2026-02-16T05:56:43.547Z*

### [eFbBGPMsKVkiXKLE1xH2km] (learned-pattern)

[PROJECT] SVELTE 5 + TAILWIND 4 + PIXI OVERLAY REPLAY ANALYSIS UX PATTERNS (2026-02-16):

## Lichess-style Mistake Classification Thresholds
- Inaccuracy (?!) : ≥50 centipawns (0.5 pawns) loss from best move
- Mistake (?) : ≥100 centipawns (1.0 pawns) loss
- Blunder (??) : ≥300 centipawns (3.0 pawns) loss
- Source: lichess.org discussions and StackExchange

## Svelte 5 Runes Best Practices
1. `$state` for reactive primitives and deep objects (auto-proxified)
2. `$state.raw()` for non-reactive objects that need reassignment pattern
3. `$derived` for computed values (push-pull reactivity - lazy evaluation)
4. `$effect` for side effects with auto-cleanup via return function
5. Use `.svelte.ts` extension for files containing runes
6. TypeScript: strongly typed props use `interface Props { field: type }` with `$props<Props>()`

## Tailwind CSS 4 Dark Mono Design System
1. CSS-first config via `@theme` directive in main CSS
2. Dark mode: `@variant dark (&:where(.dark, .dark *))` for class selector
3. Design tokens as CSS variables: `--color-fg: var(--text-fg)`
4. Use `light-dark()` CSS function for automatic light/dark switching
5. Built-in cascade layers, OKLCH colors, container queries

## PIXI.js + Svelte Integration
1. Use `$effect` to manage PIXI lifecycle - creates on mount, cleanup on return
2. Container as base class (v8) - not DisplayObject
3. `cacheAsTexture()` for static containers (performance)
4. `interactiveChildren = false` when no interaction needed
5. Rectangle masks fastest (scissor), Graphics masks second (stencil)

## Keyboard Accessibility (WCAG 2.1 AA)
- Arrow keys: left/right for prev/next move, up/down for first/last
- Space: play/pause or play best move
- Shift+arrow: navigate branches
- Letters for actions: f=flip, ?=help, l=toggle eval
- Must have visible focus indicator, no keyboard traps
- All controls accessible without mouse

## WASM Memory Management (wasm-bindgen)
1. WASM classes have `.free()` method in JS to trigger Rust Drop
2. Always call `.free()` in finally block: `try { ... } finally { obj.free() }`
3. Rust Drop impl handles cleanup: `impl Drop for T { fn drop(&mut self) { ... } }`
4. Closures auto-invalidate when dropped - use `forget()` to prevent
5. JsValue from JS to Rust: GC handles when dropped in Rust

*Created: 2026-02-16T03:02:59.584Z*

### [oY57hcFJBy4PGiu1jt58wc] (error-solution)

[PROJECT] ATTACK PARITY FIX (2026-02-16): Fixed B2B eligibility check in src/attack.rs — removed redundant `is_b2b_eligible` guard from B2B bonus application. Root cause: Fusion's calculate_attack double-checked eligibility (spin||lines>=4) before applying B2B bonus, but Triangle's garbageCalcV2 applies B2B unconditionally when b2b>0. This matters for PC triples where b2b counter stays positive (brokeB2B=false prevents reset). Fix: trust caller's b2b value — if b2b>0, apply bonus. Test updated: test_b2b_applied_when_caller_passes_positive. Also fixed garbagespecialbonus default to true in parity.ts:78 and converter.ts:63. Result: 111 attack mismatches → 0 across 50 replays (~80K locks). 4 remaining mismatches are all board-placement (no-candidates pattern, movegen BFS issue), not attack. Overall parity: 99.99%+ (4/~80K = 0.005% board-only mismatches).

*Created: 2026-02-16T00:08:50.126Z*

### [8exiMvSkHHYvUktzFBXMCm] (project-config)

[PROJECT] LOCKSTEP PARITY RESULTS (2026-02-15): Fusion 2 (direct-cobra-copy) achieves 99.98% lockstep parity vs Triangle (60,746 locks, 15 mismatches, 0 board divergences) compared to Fusion v1's 98.7%. Zero board-unreachable. 15 remaining attack-only mismatches are edge cases. Combo minifier was the main fix: Triangle uses raw `Math.log1p(combo*1.25)` as floor (no .floor()), compared directly against multiplied damage (not base+log). Piece ID translation layer maps between internal ordering (I=0,O=1,T=2,L=3,J=4,S=5,Z=6) and Fusion v1/external ordering (I=0,O=1,T=2,S=3,Z=4,J=5,L=6). WASM js_name attributes use snake_case to match Fusion v1 API contract. wasm-bindgen pinned at =0.2.100 to avoid externref issue with Rust 1.93.

*Created: 2026-02-15T23:10:26.115Z*

### [61U5QMDE4PdZRr6UmoyqW9] (learned-pattern)

[PROJECT] WASM-BINDGEN SINGLE-CRATE FEATURE-GATED SETUP GUIDE (2026-02-15): 

## Cargo.toml Pattern for Feature-Gated WASM
```toml
[package]
name = "my-crate"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]  # cdylib for wasm, rlib for native

[features]
default = []
wasm = ["wasm-bindgen", "serde-wasm-bindgen", "js-sys"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
# Core deps always included

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = { version = "0.2", optional = true }
serde-wasm-bindgen = { version = "0.4", optional = true }
js-sys = { version = "0.3", optional = true }
# OR use feature flag approach:
# wasm-bindgen = { version = "0.2", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
# Native-only deps here

[dev-dependencies]
wasm-bindgen-test = "0.3"  # Works on both wasm and native
```

## Export Patterns with Conditional Compilation
```rust
// src/lib.rs
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;
#[cfg(feature = "wasm")]
use serde_wasm_bindgen;

// Core struct - shared between wasm and native
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GameState {
    pub board: Vec<u8>,
    pub score: u32,
}

// WASM-only export wrapper
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn get_game_state() -> Result<JsValue, JsValue> {
    let state = compute_state();
    serde_wasm_bindgen::to_value(&state)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

// Native-only function
#[cfg(not(feature = "wasm"))]
pub fn get_game_state_native() -> GameState {
    compute_state()
}

// Shared implementation
fn compute_state() -> GameState { /* ... */ }
```

## serde_wasm_bindgen Best Practices
- Use `serde_wasm_bindgen::to_value(&struct)` for Rust→JS (returns JsValue)
- Use `serde_wasm_bindgen::from_value::<T>(js_val)` for JS→Rust
- For JSON-compatible output: `Serializer::json_compatible().serialize(&val)`
- HashMap→JS Map by default; use `serialize_maps_as_objects(true)` for plain objects
- u64/i64→number (safe int range) or bigint; use `serialize_large_number_types_as_bigints(true)` for bigint

## Testing WASM Exports
```rust
// tests/wasm.rs
use wasm_bindgen_test::*;

#[wasm_bindgen_test]
fn test_game_state_serialization() {
    let state = GameState { board: vec![1,2,3], score: 100 };
    let js_val = serde_wasm_bindgen::to_value(&state).unwrap();
    // Test round-trip
    let recovered: GameState = serde_wasm_bindgen::from_value(js_val).unwrap();
    assert_eq!(state.score, recovered.score);
}

// Runs as #[test] on native, #[wasm_bindgen_test] on wasm32
#[wasm_bindgen_test(unsupported = test)]
fn test_on_all_targets() { /* ... */ }
```

## Node.js Target Deployment
- Use `--target nodejs` for CommonJS require()
- Use `--target experimental-nodejs-module` for ESM import
- Generated JS shims can be required directly

## Stable JS API Contract Patterns
1. Version exports in separate module: `src/wasm_exports.rs`
2. Use wrapper types that don't expose internal Rust types
3. All public WASM functions return JsValue (serialized) or primitives
4. Avoid returning Result<T, E> directly - map to Result<JsValue, JsValue>
5. Use tsify crate for TypeScript type generation

## Parity Harness Strategy for Replacing Existing WASM Package
1. Create `src/wasm_compat.rs` with same function signatures as old package
2. Map old API to new internal implementation
3. Write comparison tests: call both old and new, assert equality
4. Use feature flags: `old-wasm` vs `new-wasm` for A/B testing
5. Benchmark both in Node.js using same test harness
6. Gradual migration: export both APIs, deprecate old one

*Created: 2026-02-15T22:37:41.430Z*

### [PuvzhMQmJDwmg9QsJ2JANW] (architecture)

[PROJECT] TRIANGLE ROLE CLARIFICATION: Triangle.js is a REPLAY SIMULATION ENGINE, NOT TETR.IO's actual game engine. It simulates/replays games from replay data and serves as ground truth for lockstep parity comparison against Fusion. The lockstep comparator runs Triangle alongside Fusion on the same replay to verify board state and attack calculation parity.

*Created: 2026-02-15T22:23:18.913Z*

### [MGbkuCCVRgiy6tFqMCpFYQ] (architecture)

[PROJECT] TETRASTATS DATA MODEL SCHEMA (2026-02-15): TETR.IO API models from TetraStats for Rust struct design. TetrioPlayer: userId/username/role/registrationTime/badges/bio/country/gamesPlayed/gamesWon/gameTime/xp/level(derived)/supporterTier/verified. TetraLeague: gamesPlayed/gamesWon/winrate(derived)/bestRank/decaying/tr/glicko/rd(default60.9)/gxe/s1tr(gxe*250)/rank/percentileRank/percentile/standing/apm/pps/vs/season + derived nerdStats/estTr/playstyle. ReplayStats: seed/linesCleared/piecesPlaced/inputs/holds/score/topCombo/topBtB/topSpike(derived)/tspins/roundLength/clears/garbage/finesse + derived kpp/kps/spp/finessePercentage. Clears has 17 fields (singles through tSpinMiniQuads). Garbage: sent/received/attack/cleared/sent_normal/maxspike. Finesse: combo/faults/perfectPieces. Handling: arr/das/sdf/dcd/cancel/safeLock. API: ch.tetr.io/api/users/{id}, /summaries, /records/{stream}; inoue.szy.lol/api/replay/{id} for replay download. Rate limit via cached_until.

*Created: 2026-02-15T22:09:28.259Z*

### [d4GtwS2H4DhV8doGQUVQ6j] (error-solution)

[PROJECT] SEARCH HOLD-STATE BUG FIX (2026-02-15): In `src/search.rs`, depth>0 expansion previously ignored hold-state evolution. Fix: `expand_node` now accepts and propagates `new_hold: Option<Piece>` so hold swaps correctly set held piece to the swapped-out current piece on child nodes. This prevents invalid multi-ply hold simulation drift in beam search. Included in Layer1+2 commit `0d808b11`.

*Created: 2026-02-15T21:50:00.523Z*

### [TELTBxYsM34KWGnifBnYmj] (architecture)

[PROJECT] S2 ATTACK + ALLSPIN INTEGRATION (2026-02-15): Added `src/attack.rs` and rewired eval/search/analysis to use real TETR.IO Season 2 attack mechanics instead of hand-tuned clear weights. `calculate_attack(lines, spin, b2b, combo, config, is_pc)` is piece-agnostic for spins (`spin != NoSpin`), so allspin is naturally supported. Surge uses logarithmic B2B chaining (`ln_1p` with 0.8 constant) and combo handling supports Multiplier/Classic/Modern/None tables. `GameState.b2b` changed from bool to u8 surge level. Commit: `e4078fbf`.

*Created: 2026-02-15T21:50:00.511Z*

### [6xjqqSsnxNfVtWCqwMpBqE] (learned-pattern)

[PROJECT] TETRASTATS COMPLETE FORMULA EXTRACTION (2026-02-15): Source: dan63047/TetraStats @ 1f7dc91. KEY FORMULAS: (1) NerdStats: APP=apm/(pps*60), VS/APM=vs/apm, DS/S=(vs/100)-(apm/60), DS/P=DS/S/pps, CheeseIndex=(dsp*150)+((vsapm-2)*50)+(0.6-app)*125, GbE=app*dsp*2, nyaapp=app-5*tan(rad((cheese/-30)+1)), Area=apm*1+pps*45+vs*0.444+app*185+dss*175+dsp*450+gbe*315. (2) EstTR: srarea=pps*135+app*290+dsp*700, statrank=11.2*atan((srarea-93)/130)+1. (3) Playstyle: 6 normalized metrics (nmapm/nmpps/nmapp/nmdsp/nmgbe/nmvsapm) against statrank, then Opener/Plonk/Stride/InfDS axes. (4) Replay microstats: opener/midgame APM/PPS splits, APL variants (cheese/downstack/upstack), efficiency (i/t/allspin), surge metrics (9 ratios), stack speed (up/down PPS + ratio), garbage handling (7 ratios), PPS variance via GMM (BurstPPS/PlonkPPS/PPSCoeff), attackCheesiness via sigmoid, KPP/KPS, finesse%, biggest spike (60-frame window). (5) Rank cutoffs: X+=0.002 to D=1.0 percentiles; TR targets: X+=24000 to D=0.

*Created: 2026-02-15T21:50:00.492Z*

### [Cq3Jzs4W7FPoMJ51rChNEz] (architecture)

[PROJECT] Y-AXIS CONVENTION UNIVERSAL SURVEY (2026-02-15): ALL major Tetris implementations use Y-UP (row 0 = bottom, Y increases going up). Confirmed for: SRS Guideline (tetris.wiki: "positive y upwards"), TETR.IO (SRS+ true rotations), Cobra C++ (bb(y)=1<<y, softdrop=y-1, spawn=21), Cold Clear (sonic_drop decreases y), MisaMino (gem_beg_y=1 near bottom), Zetris (LogBoard prints y=30→0), Fusion 1 (column-major [u64;10]). direct-cobra-copy board.rs comment was incorrectly labeled "Y-down" — FIXED to "Y-up (row 0 = bottom)". No known Tetris engine uses Y-down.

*Created: 2026-02-15T04:37:49.706Z*

### [PHaQv8vfpiYAtUEc72D8T4] (architecture)

[PROJECT] Perft Engine Optimization Best Practices (2026-02-14): (1) TT Design: 32-byte cache-aligned clusters, 2 entries/cluster, depth-preferred+always-replace. Store partial_key(u16)+depth+nodes. DISABLE for parity verification or use thread-local. (2) Zobrist: Incremental XOR updates stored in BoardState, fixed-seed PRNG initialization, 64-bit keys sufficient. (3) Cache Layout: Column-major [u64;10], hot data first, TT clusters fit cache lines, huge pages for large TT. (4) Collision Updates: Source subtraction for first-valid kick, canonical collapse for O/S/Z, softdrop/hshift/rotation stages. (5) SIMD: Line clear via bitwise shift with mask, portable_simd for AVX2 column processing, AND reduction for full-row detection. (6) Recursion Elimination: Bulk counting at depth=1 (return moves.len()), apply/unapply with UndoInfo snapshot, iterative stack-based perft option. (7) Monomorphization: Rust const generics match C++ templates, match dispatch for runtime piece selection, #[inline] on hot paths, PGO for branch hints. (8) Verification: Never HashMap (use BTreeMap/fixed-hashbrown), wrapping_add for signed overflow, cross-platform D1-D7 checkpoints with as-of date/commit/command. Cobra reference: D7=2,705,999,255 @ d7054ef. (9) Parallel: Rayon depth-2 work split, thread-local TT or disabled, AtomicU64 or par_iter().sum(). Key insight: generate() is 80% of time, move/unmove eliminates clone, bulk counting is critical.

*Created: 2026-02-15T04:05:38.537Z*

### [rs6ADkt25QrkWa4ywhSGDh] (architecture)

[PROJECT] Direct Cobra Copy D1-D7 perft parity (2026-02-14): 1:1 Rust port at /home/li859/projects/mosaic-fusion-engine-coaching/direct-cobra-copy/ passes all perft depths. D1=17, D2=153, D3=5266, D4=188561, D5=3500883, D6=67088390, D7=2705999255 — all exact Cobra match (delta=0). D5 release 0.14s, D7 release 101.5s single-threaded. Tests in tests/perft.rs, D4+ marked #[ignore]. Unit tests 21/21 pass.

*Created: 2026-02-15T01:58:19.718Z*

### [rUQ35t8j5QCd2cZMEjKxsL] (architecture)

[PROJECT] CANONICAL WORKSPACE LAYOUT (2026-02-15): All Fusion/Mosaic projects consolidated under parent directory ~/projects/mosaic-fusion-engine-coaching/ with shared supermemory-id 9a1f5f1f8a273899 across parent and all subdirs. Subdirectories: (1) fusion-test-local (1.1GB) — primary dev workspace, JJ colocated, Gladdonilli/fusion-test-local remote, v1 engine codebase (column-major Y-up). (2) fusion-rewrite-groundup-session (719MB) — standalone Cargo project for fusion2 rewrite, master plan docs, fusion2-* crates, reference corpus. FAILED approach — user abandoned this rewrite. (3) fusion-upstream-clean (218MB) — clean git clone for upstream PRs, Gladdonilli/fusion-test-local remote. (4) cobra-movegen (3.6MB) — Cobra C++ reference repo, Kixenon/cobra-movegen, verified up-to-date with upstream at d7054ef. (5) mosaic-fusion-testing (338MB) — MochBot/mosaic clone with lockstep comparator tooling under scripts/lockstep. (6) mosaic-upstream-clean (17MB) — clean git clone for mosaic PRs, Gladdonilli/mosaic-local remote. NEXT: direct-cobra-copy dir for 1:1 Rust port of Cobra C++.

*Created: 2026-02-14T23:22:51.366Z*

### [RGk9qtsgunkb7ZuCKzq6fV] (error-solution)

[PROJECT] Fusion PR#14 spawn-block correctness fix (2026-02-14): In /home/li859/projects/fusion-upstream-clean commit d5ba5769 changed crates/engine/src/movegen_bitboard.rs so seed_initial_states returns false when spawn is blocked (removed board-wide fallback seeding). Added regression tests test_spawn_blocked_returns_no_moves_t and test_spawn_blocked_returns_no_moves_i. Verified with cargo build --workspace and CLOUD_EXEC_SKIP=1 cargo test -p fusion-engine (98 passed, 0 failed, 34 ignored).

*Created: 2026-02-14T08:43:19.219Z*

### [FbCZDNBPxUNDQBWC7Gsgc7] (learned-pattern)

[PROJECT] AI Cleanup for Upstream skill created (2026-02-14): Project-specific skill at `.opencode/skills/ai-cleanup-upstream/SKILL.md` in fusion-test-local. Codifies the full upstream push cleanup workflow: file removal (.sisyphus/.opencode/AGENTS.md/Macroscope.md), AI marker scan (git grep pattern), header/separator cleanup in dotfiles, comment normalization (strip all except auto-generated and discord-neutral-tone), history shape selection (non-orphan for Fusion chain, branch-from-main for Mosaic), force-push with verification, and rollback procedure. Anti-patterns documented: orphan branches cause "N commits behind", AI-style section headers, verbose comments. Clean clone locations: fusion-upstream-clean and mosaic-upstream-clean.

*Created: 2026-02-14T08:19:28.095Z*

### [7nf29We8Uskd3CuJfA3o5C] (learned-pattern)

[PROJECT] Rust/WASM Rewrite Workflow Best Practices (2026-02-14): (1) Test gating: Use cargo-nextest with retries for flaky tests, configure in .config/nextest.toml with [profile.ci] retries = 2 and exponential backoff for remote API tests. (2) Ignored tests: Run with --ignored flag separately from main suite; document why tests are ignored (slow, requires external deps, flaky). (3) Benchmark reproducibility: Use Criterion with sample_size, warm_up_time, measurement_time configured; compare against baseline with --save-baseline and --baseline flags. (4) Checkpoints: Record as-of date, branch/commit, command, and expected values in docs; rerun full gate suite before accepting changes. (5) Rollback: Keep patch iteration log with observed deltas per change; revert immediately on D1-D4 regression; never commit failed combined lanes.

*Created: 2026-02-14T08:15:53.192Z*

### [JbT3oGm6D4HXqeBf9pz8DR] (learned-pattern)

[PROJECT] AI-trace scrub pattern for gated branches: remove AGENTS/Macroscope/supermemory/opencode/sisyphus/PR-bot config docs, then verify with grep scan over tracked text files for markers (`AGENTS.md|Macroscope|.opencode|supermemory|ChatGPT|Claude|OpenCode|greptile|qodo|assistant`) before commit.

*Created: 2026-02-14T00:21:49.360Z*

### [HmRwHkwbbqaeyu6hpa31uu] (error-solution)

[PROJECT] Noise containment fix for upstream prep: when working trees are saturated with `.sisyphus/.opencode` artifacts, clone into fresh sibling repos and do cleanup/commit there instead of editing .gitignore/reverting large local trees. This preserves deterministic diffs and avoids accidental inclusion of local tooling traces.

*Created: 2026-02-14T00:21:49.328Z*

### [E2PSWLkKm2ULySetoGqyEs] (project-config)

[PROJECT] Gated-clean PR workflow (2026-02-14): Used isolated clean clones `/home/li859/projects/fusion-upstream-clean` and `/home/li859/projects/mosaic-upstream-clean`, reset `origin` to `https://github.com/Gladdonilli/fusion-test-local.git` and `https://github.com/Gladdonilli/mosaic-local.git`, created branch `gated-clean-local-trace-cleanup`, and opened PRs fusion#14 + mosaic#5.

*Created: 2026-02-14T00:21:49.307Z*

### [J3oztFTSB38SP2fT7mNU24] (learned-pattern)

[PROJECT] Lockstep comparator parity fix (2026-02-13): In scripts/lockstep/comparator.ts, move candidate board comparison must apply Triangle lock-time incoming garbage (`res.garbageAdded`) to trial board rows before matching against boardTargetRows. Added applyGarbageAddedRows(rows, garbageAdded) and used it on each trial candidate. Result: attack-only mismatches eliminated in 400-run window; residual deterministic mismatches are board-unreachable I mini triple cases with expectedAttack == fusionAttack.

*Created: 2026-02-13T11:11:47.789Z*

### [xkJc1sUWQqMPMMasb6JhRu] (architecture)

[PROJECT] TETR.IO Attack Calculation External References (2026-02-13): High-confidence sources: (1) tetrio.wiki.gg/Mechanics - B2B charging (surge_base TL=4, QP=1), combo multiplier formula base(1+0.25x) or ln(1+1.25x) for single-combos, rounding modes DOWN vs RNG. (2) Triangle.js damageCalc/index.ts L3-30 - base attack table (single=0,double=1,triple=2,quad=4,tspinSingle=2,tspinDouble=4,tspinTriple=6), combo tables classic/modern/none, B2B chaining logarithmic constant 0.8, combo logarithmic constant 1.25. (3) skysomorphic attack calculator - interactive reference for testing. (4) Fusion TETRIO_RULES.md - attack formula order, combo table mapping (0=multiplier,1=classic,2=modern,3=none), config fields. Caveats: no single authoritative source for all S2 nuances; some contradictions exist between wiki and implementation (allClear constant shows 10 in Triangle.js but TL=5/QP=3 in Fusion config).

*Created: 2026-02-13T10:55:11.674Z*

### [9fbqMocSu8jm2M8hvYAXyV] (architecture)

[PROJECT] Attack Calculation Timing/Order References: (1) Triangle.js garbageCalcV2 (damageCalc/index.ts L32-183) shows exact order: base_attack → spin_bonus → b2b_bonus → combo_bonus → target_bonus. All values use POST-LOCK state (b2b/combo counters already incremented). (2) Fusion attack.rs L121-147 matches this order. (3) TETR.IO wiki mechanics page confirms components: lines cleared + spin + B2B + Combo + margin_time affect garbage. (4) Tetris wiki Garbage page shows guideline base values + B2B/PC bonuses. (5) skysomorphic attack calculator confirms B2B chaining formula: floor(1+ln(1+b2b*0.8)) for levels 0-8. Critical: no pre/post lock ambiguity - all calculations use state AFTER piece lock and line clear.

*Created: 2026-02-13T10:30:50.033Z*

### [rijrqzBEcErnhu6hhKRRY8] (learned-pattern)

[PROJECT] Upstream relationship clarification (2026-02-13): /home/li859/projects/mosaic-fusion-testing is a direct clone tracking MochBot/mosaic (origin points to upstream repo), not a fork. Divergence from upstream should be interpreted as local commits/working-tree changes only. Current local checkpoint includes lockstep tooling + comparator classification under scripts/lockstep and package.json scripts.

*Created: 2026-02-13T10:14:28.604Z*

### [sAfD4QPefFADwuBZKPrbKt] (architecture)

[PROJECT] I-piece CCW/CW asymmetry geometric condition (2026-02-12): In SRS/SRS+ systems, I-piece rotation asymmetry creates scenarios where CCW is legal but CW illegal from same pre-rotation state. Root causes: (1) Grid intersection pivot (I/O pieces rotate around line intersections, not cell centers), (2) 4 distinct geometric states (North/East/South/West don't occupy same cells due to pivot wobble), (3) Asymmetric kick tables (Standard SRS 0→L and 0→R both kick LEFT; SRS+ modifies CCW to kick RIGHT first). Example: I-piece in North rotation adjacent to right wall. CW 0→R tests kicks (-2,0),(-1,0) - both LEFT (fails if wall blocks). SRS+ CCW 0→L tests (+2,0),(-1,0) - RIGHT first then LEFT (succeeds if right side has clearance). Key references: TETRIO_RULES.md L128-L139 (kick tables), SRS_PLUS_REFERENCE.md L36-L48 (grid pivot), COBRA_IMPLEMENTATION.md L156-178 (SRS+ implementation). Formula: CCW kick = (-CW_opposite.x, CW_opposite.y).

*Created: 2026-02-13T01:13:51.675Z*

### [CSeodZKNtM6hZnNoUTJmvF] (learned-pattern)

[PROJECT] Frame-locked rotation test patterns (2026-02-12): 6 high-signal templates for Tetris engine rotation testing. (1) Single kick validation: fixed board, one rotation, verify kick_index + final position. (2) CW/CCW symmetry: same board/pose, test both directions, compare kick_index. (3) T-spin mini/full: 3-corner board setup, rotate, verify spin_type. (4) Kick exhaustion: fully blocked board, verify rotation fails (None). (5) Pattern-based board: ASCII string → board, test specific kick offset. (6) Kick order verification: CW/CCW kick table symmetry check. Key invariant: no intermediate movement between rotation start and end verification.

*Created: 2026-02-13T00:49:46.609Z*

### [HiXB9pgzY34eTJjinwPYsd] (error-solution)

[PROJECT] I-piece BFS sealed-gap bug (2026-02-12): After garbage sync fix, only 5 board-unreachable cases remain from 25,521 locks across 20 replays. ALL are I-piece vertical placements kicked into sealed column gaps (column open from y=a to y=b but blocked at y=b+1). Player reaches via soft-drop to horizontal near gap + rotateCCW (N→W kick) + hardDrop. Fusion's BFS generates 18-19 candidates but misses this specific kicked position. Board orientation note: JSONL boardBefore/boardAfter strings are REVERSED (string[i] = x=(9-i)). This bug correlates exactly with the I-piece isolation perft delta (-12,107 at D5). Fix requires changes to propagate_rotation_cobra in crates/engine/src/movegen_bitboard.rs.

*Created: 2026-02-12T11:02:17.672Z*

### [UjVZeiS8BcuCfg7uiJujge] (error-solution)

[PROJECT] Lockstep comparator garbage sync fix (2026-02-12): Always resync fusionBoard to expectedRows after EVERY lock, not just on mismatch. Previous conditional carry-forward caused fusionBoard to miss garbage row insertions between locks (garbage applied by Triangle but not reflected in Fusion's carried board). Fix in mosaic-fusion-testing/scripts/lockstep/comparator.ts lines 300-310. Result: board-unreachable dropped from 1,441 to 5 (99.7% reduction), total mismatches from 1,705 to 315 (81.5% reduction), exact match rate from 93.3% to 98.7%. Remaining 5 board-unreachable are ALL I-piece vertical kick placements in sealed gaps (true movegen bugs). 319 attack-only mismatches remain (separate issue).

*Created: 2026-02-12T10:40:47.772Z*

### [LSYSR61qjvryDW97WgXRoZ] (error-solution)

[PROJECT] I-PIECE KICK ORDERING BUG: Runtime get_i_kicks() in crates/engine/src/kicks.rs had kicks 1,2 swapped for I-piece N→E (CW) and N→W (CCW) transitions. Runtime had (1,0),(-2,0) but SRS+ spec requires (-2,0),(1,0) for N→E CW. Fixed by swapping to match the const SRS_PLUS_KICKS table which was correct. Also fixed test_i_kicks and test_i_srsplus_kicks assertions. The test_const_kicks_match_runtime test was previously failing silently. All other 6 CW/CCW + 4 180° transitions were correct. Fix verified: 187 tests pass, 0 fail.

*Created: 2026-02-12T09:30:40.415Z*

### [hVJAaNJU45zYLcY44c7gvP] (error-solution)

[PROJECT] PARITY MATRIX STALE EXPECTATIONS: 6 parity_matrix tests had stale expected values from pre-B2B-chaining formula. Updated: test_3_2 (5→6), test_3_4 (5→7), test_3_5 (7→8), test_6_1 (8→9), test_6_2 (10→13), test_6_3 (10→12). All B2B and B2B+combo scenarios. Engine values verified correct via S2 logarithmic B2B chaining formula: floor(1 + ln(1 + b2b*0.8)).

*Created: 2026-02-12T09:30:40.359Z*

### [rYyncqTHYjiFcvGuYj3cuu] (learned-pattern)

[PROJECT] Lockstep comparator evolution (2026-02-12): Three iterations of comparator fixes: (1) logical-pass: use preClearBoard for lines==0, expectedRows for lines>0 (9,559→8,429 mismatches). (2) garbage sync: always resync fusionBoard to expectedRows after EVERY lock (8,429→315 mismatches, board-unreachable 1,441→5). (3) The carry-forward-on-match pattern from iteration 1 is now irrelevant because iteration 2 always resyncs. Current state: 98.7% exact, 1.2% attack-only, 0.02% board-unreachable (5 I-piece cases).

*Created: 2026-02-12T09:04:51.346Z*

### [bsEkAnvz2yHAWcCUk4F5ap] (learned-pattern)

[PROJECT] Supermemory cleanup gotcha: `forget` must target v4 memory IDs (nested `memory.id` from `supermemory get`), not top-level exported document IDs. Document IDs can appear to succeed but leave underlying memories active.

*Created: 2026-02-11T03:58:05.176Z*

### [49MhQM8MV4e1Aa6UnE2iTh] (architecture)

COBRA MOVE EMISSION & DEDUP SEMANTICS (Non-T Pieces) — Authoritative Summary from Cobra source d7054efe: (1) Non-T pieces have ZERO spin tracking: checkSpin=false at line 16, spinSet has zero size at line 26. (2) Non-T emits exactly ONE move per placement: direct emission at lines 77-80 (fast path) and 136-151 (harddrop), no spin variant loop (that's T-piece only, lines 233-246). (3) Canonical rotation mapping: group2 (I/S/Z) uses r&1 (N/E→0, S/W→1), O→NORTH, L/J/T→r (distinct 4 states). (4) Canonical offset: I(SOUTH=(1,0), WEST=(0,-1)), S/Z(SOUTH=(0,1), WEST=(1,0)), L/J/T=all(0,0). Purpose: align non-canonical rotations with canonical coordinate system for dedup. (5) Dedup key: (x, canonical_rotation, y) via bitset moveSet[x][canonical_r], NOT (rotation, x, y) tuple. Implicit dedup via bitset OR: moveSet[x][r1] |= m, & ~moveSet[x][r1] masks already-seen bits. (6) Placement counted when: softdrop reachable AND downshift by 1 collides. Collision check uses cm[x][r1] where r1=canonical_r<p>(r) — canonical rotation collision map. (7) Rotation canonical offset application: off = canonical_offset(source_rotation) - canonical_offset(target_rotation). Applied to position: x1 = x + kicks[i].x + off.x, y1 = threshold + kicks[i].y + off.y. (8) NO source subtraction: Cobra does NOT subtract source (x,y) coordinates. Only offset difference off is used and added to kick offset. Full details in docs/COBRA_MOVE_EMISSION_SEMANTICS.md with source links.

*Created: 2026-02-10T18:37:47.516Z*

### [dt4BRBK3EZWAZjcug2YhP1] (learned-pattern)

[PROJECT] CANONICAL OFFSET DERIVATION (2026-02-10): Verified correct additive-convention canonical offsets by tracing actual mino cell positions through the BFS x_idx/y coordinate system. Formula: cx_idx = x_idx + off.0, cy = y + off.1. Verified values: I South=(-1,0), I West=(0,1), S/Z South=(0,1), S/Z West=(1,0), all others=(0,0). Key insight: Cobra uses SUBTRACTIVE convention (cx = x - off), so Cobra's raw values are NEGATED from our additive convention. Previous sessions kept flip-flopping on signs because they mixed conventions. ALWAYS derive from first principles: find which (x_idx_target, y_target) in canonical rotation produces same physical cells as (x_idx_source, y_source) in source rotation, then off = target - source.

*Created: 2026-02-10T07:33:31.771Z*

### [PXkKFZX5Fu28qxoKZ5Em7r] (project-config)

[PROJECT] FUSION ACCEPTANCE GATES (2026-02-10): User no longer chases exact D7 node match — Cobra's count may itself be imperfect. New gates: (1) D1-D5 exact parity with Cobra (ACHIEVED on new-baseline branch). (2) Performance: D5 < 70ms, must beat Cobra's D5 time. (3) Correctness: SRS+ rules (minos, kicks, spin detection) must be spec-correct per TETR.IO reference. D7 is informational only.

*Created: 2026-02-10T07:05:47.222Z*

### [oQXYgkVSNWGFiGaxmrx1eV] (learned-pattern)

[PROJECT] COBRA PARITY CONTRACT CORRECTIONS (2026-02-10): ALL kick tables (JLSZT, I-SRS, I-SRS+) are identity-first — Test 1 = (0,0). Previous claim of "I CW/CCW non-identity-first" was WRONG. Corrected in docs/COBRA_PARITY_CONTRACT.md with per-class identity_offset_first_cw_ccw semantics. Source: Cobra gen.hpp kick arrays verified against harddrop/tetrio.wiki.

*Created: 2026-02-10T07:05:42.534Z*

### [KRe5y23MQHGCvGLmvBuHPX] (architecture)

[PROJECT] COBRA T-PIECE SPIN MOVEGEN (2026-02-10): Cobra generates MULTIPLE moves for the same T-piece physical position when reachable via different paths. spinSet[x][r][SPIN_NB] tracks arrival method during BFS. Shift/drop path → NO_SPIN, rotation path → MINI or FULL (based on 3-corner rule). Same (x,y,r) can produce 2 distinct moves: NO_SPIN+MINI or NO_SPIN+FULL. NEVER MINI+FULL (corner check is position-dependent, not path-dependent). Non-T pieces: zero spin tracking, always 1 move per position. This means T-piece perft counts include spin-variant duplicates — Fusion must implement equivalent tracking for parity. Source: movegen.cpp:16 (checkSpin = p1==TSPIN), :26 (spinSet zero-sized for non-T), :233-246 (triple-loop emission over NO_SPIN/MINI/FULL). Confirmed by kise screenshot showing same T position as "both a mini and non spin".

*Created: 2026-02-10T06:24:22.910Z*

### [63fpBeXBSHrzfAnNneo4eF] (error-solution)

[PROJECT] COBRA SPIN DETECTION — DEFINITIVE (2026-02-10): Cobra returns MINI/FULL ONLY for T-pieces. Non-T pieces ALWAYS get NO_SPIN. Evidence from source: (1) compile-time gate checkSpin = p1 == TSPIN, (2) spinSet is zero-sized for non-T, (3) spin() getter returns 0 for all non-TSPIN pieces, (4) runtime assert p == TSPIN || !fullspin. T-piece emits multiple moves per position: same (x,y,r) can produce NO_SPIN (via shift) + MINI/FULL (via rotation with 3-corner rule). Kise's screenshot showing "both mini and non spin" was about T-piece dual-move emission. Previous memory claiming non-T CAN get Mini was WRONG and poisoned multiple sessions. SUPERSEDES old "CRITICAL CORRECTION" memory.

*Created: 2026-02-10T06:18:15.444Z*

### [yqKEnPW972QY8PUKD22NvR] (architecture)

[PROJECT] SRS+ REFERENCE DATA: Authoritative kick table reference for all pieces. NOTE: Y-axis convention is Y-UP across all engines (fusion, cobra, Triangle) -- some external SRS+ docs incorrectly state Y-down. See memory Cq3Jzs4W for Y-axis convention confirmation.

*Created: 2026-02-10T06:04:18.006Z*

### [aDUTvBqyF9kVsvJasMWnvJ] (learned-pattern)

[PROJECT] PERFT DELTA DEBUGGING BEST PRACTICES (2026-02-10): Key research findings for diagnosing tiny perft deltas in movegen engines:

**1. DELTA DEBUGGING ALGORITHM (Zeller 2002)**: Binary search approach to minimize failure-inducing inputs. Split failing test case into subsets, test each, recursively keep failing half. Guarantees finding 1-minimal test case in O(n²) tests. Applied to perft: isolate specific board states causing D7 divergence by testing subtrees and halving until minimal failing leaf is found.

**2. PERFT DEBUGGING WORKFLOW (from perftree/chess programming)**: Perft is primarily for debugging move generation correctness, not just performance. Compare against trusted oracle (Cobra for Tetris). Use divide algorithm: for each move at current position, compute perft(depth-1) and compare node counts. Missing numbers indicate divergent subtrees - drill down recursively.

**3. REGRESSION TESTING GATES**: 
- Shallow (smoke): Quick D1/D2 checks after any build change. Catches basic breakage in minutes.
- Sanity: Focused tests on changed modules (e.g., spin detection after rotation fix). Narrow but deep.
- Deep regression: Full D5/D7 verification gates before committing. Wide coverage but slow.
- Selective regression: Only test paths affected by recent changes. Saves time while ensuring quality.

**4. RUST/WASM COMPATIBILITY VALIDATION**:
- After engine changes: always run `cargo test` AND `wasm-pack test --headless --chrome` - they test different code paths.
- Use snapshot testing (insta-rs) for board state serialization regression detection.
- Build validation: ensure wasm-bindgen bindings still compile and FFI layer works.

**5. MINIMAL FIXES PRINCIPLE**: Delta debugging finds minimal change causing failure. For perft deltas, this means avoiding broad mino/kick table changes. Focus on isolated logic bugs (spin classification edge cases, canonical rotation indexing, rotation propagation). Test shallow gates (D1/D2) immediately before attempting deep (D7).

*Created: 2026-02-10T06:02:35.881Z*

### [XG6hgmFuN8hHtgPaHqc4kH] (learned-pattern)

[PROJECT] D7 PERFT VERIFICATION LIMITATIONS (2026-02-10): No independent D7 perft verification exists — Cobra's 2,705,999,255 is the ONLY published reference. Cold Clear, Zetris, shakkar23, ImpleLee — none publish perft D7 counts. Additionally, perft counts are dedup-strategy-dependent: two correct movegens can produce different counts if they deduplicate differently (e.g., 4-state I-piece vs 2-state collapsed). To verify movegen correctness across implementations, compare board-state sets at leaf nodes, not raw counts. D5 board-state comparison is the sweet spot (~28MB, <30s).

*Created: 2026-02-10T04:46:21.422Z*

### [9jVBHJuuVGR7dcnq1VQ1qX] (architecture)

[PROJECT] PERFT TEST REFACTORING (2026-02-10): perft.rs refactored from 3,298→402 lines (88% reduction). 36→11 tests. Deleted 23 debug investigation artifacts. All tests now use assert_eq! (previously many just println!). Single COBRA_REF constant with provenance citation. Single STANDARD_QUEUE. Unified into one test module. run_benchmark() helper shared by D5/D6/D7. Tests: depth_0, d1_per_piece, 3 variant consistency, cobra_parity_d1_to_d4, benchmark_d5/d6/d7 (#[ignore]), d7_spin_invariance (#[ignore]), d7_parallel_consistency (#[ignore]).

*Created: 2026-02-10T04:46:14.981Z*

### [fS8uEkfAS6JjZBzBUaNRJq] (architecture)

[PROJECT] COBRA I-PIECE 4-STATE MINO REFERENCES (2026-02-10): I-piece mino definition: src/header.hpp:181 = C(-1,0), C(1,0), C(2,0) for NORTH. 4 distinct states via rotate() transform at header.hpp:192-199: NORTH=(-1,0,1,2), EAST=(0,-1,0,0,1,2), SOUTH=(1,0,-1,-2), WEST=(0,1,0,0,-1,-2). Critical: NORTH≠SOUTH — reversed cell ordering. Canonical rotation r&1 mapping: src/gen.hpp:40-46 (N→0, E→1, S→0, W→1). Canonical offsets: src/gen.hpp:48-63 — I: SOUTH={1,0}, WEST={0,-1}; S/Z: SOUTH={0,1}, WEST={1,0}. Offset usage: src/movegen.cpp:174-178 (off = canonical_offset[r] - canonical_offset[r1]). Move storage: src/movegen.cpp:136-146. Spawn row=21: src/default_ruleset.cpp:10. Commit: d7054efe1ffcaa3f7b97ed2d0b3ac36ecc41c433.

*Created: 2026-02-10T04:14:59.558Z*

### [kC7va9dyVNP2i8fPFV1tEu] (architecture)

[PROJECT] TETRIS MOVEGEN CROSS-REFERENCE (2026-02-09): Found independent implementations for SRS/SRS+ correctness verification:

IMPLEMENTATIONS FOUND:
1. Cold Clear (MinusKelvin/cold-clear) - Rust library with piece rotation, SRS kicks, T-spin detection. No perft results published.
2. MisaMino (misakamm/MisaMino) - C++ tetris AI with wallkick data in gamepool.h, supports 180° rotation (trySpin180() function).
3. tetris-movegen (citrus610/tetris-movegen) - C++ movegen using Dijkstra, includes SRS kick tables.
4. TETR.IO bots (ahmedrangel/tetrio-bot, danielyx-z/Tetrio-Bot) - TypeScript implementations using @haelp/teto library.

I-PIECE COORDINATES COMPARISON:
Cold Clear: I-piece North=[(-1,0),(0,0),(1,0),(2,0)], East=[(0,1),(0,0),(0,-1),(0,-2)]
MisaMino: I-piece defined via gem.bitmap[] + GEM_COL_H[] arrays
tetris-movegen: I-piece UP=[(-1,0),(0,0),(1,0),(2,0)], RIGHT=[(0,1),(0,0),(0,-1),(0,-2)]
tetrio-bot (El-Tetris): I-piece as binary parse("1111") for North (width=4, height=1), [1,1,1,1] for East

SRS KICK TABLES:
Cold Clear: rotation_points() - I-piece special kicks, J/L/S/Z/T/O standard 5-kick CW/CCW + identity (0,0)
MisaMino: Iwallkickdata[4][2][4][2] and wallkickdata[4][2][4][2] - I-piece 4 kicks, others 4 kicks
tetris-movegen: SRS_LUT[2][4][5][2] - I-piece special kicks, others standard
kick-visualizer: Supports multiple systems: SRS, SRSX, JSTris, ASCDX, TETRIO (I-piece kicks differ in TETRIO)

180° ROTATION:
MisaMino: trySpin180() function present in tetris.h (returns true/false)
kick-visualizer: SRSX includes 180° rotations (L.NS, L.EW, L.SN, L.WE, etc. for all pieces including I-piece)
Cold Clear: No explicit 180° in rotation system (uses CW/CCW chain)

PERFT RESULTS:
No independent tetris implementations with published perft depth 5/6/7 node counts found. Cobra's D7=2,705,999,255 remains unique reference.

*Created: 2026-02-10T03:58:19.503Z*

### [atcKqHbBRWG26pNFLVZPpr] (architecture)

[PROJECT] COBRA SOURCE EXTRACTION CORRECTIONS: All corrections from cobra C++ to Rust port have been applied. Port completed at commit 97240ec with D1-D7 perft parity. Historical reference only.

*Created: 2026-02-09T23:35:17.179Z*

### [trepMZvSb4ZeZcfo4fSQdA] (learned-pattern)

[PROJECT] PARITY-SAFE OPTIMIZATION ORDER (2026-02-09): For Fusion parity-first rewrite, prioritize packed-row board + frozen mino/kick/spawn fixtures + parity gates before advanced optimization. Safe transfer techniques from chess/perf work: transposition table (careful keying), sparse bit iteration, popcount, SIMD with scalar fallback. Forbidden for perft parity: alpha-beta/null-move/futility pruning and any branch-skipping optimization.

*Created: 2026-02-09T21:46:49.237Z*

### [khLMK14QcjGQo5jRDy5KH4] (learned-pattern)

[PROJECT] SRS I-piece anchor convention research (2026-02-09): The I-piece pivot in SRS is at the grid INTERSECTION (1.5, 0.5 in 4x4 bounding box), NOT at any cell center. This is unique to I and O pieces. Because the pivot is between cells, implementations must pick which cell to call the "anchor" — two common conventions: Convention A (Fusion): anchor at 2nd cell (index 1), minos = [(-1,0),(0,0),(1,0),(2,0)] North, [(0,-1),(0,0),(0,1),(0,2)] East. Convention B: anchor at 3rd cell (index 2), minos = [(-2,0),(-1,0),(0,0),(1,0)] North, [(0,-2),(0,-1),(0,0),(0,1)] East. Both produce identical physical cells for same board position — difference is purely which y value is reported as anchor. The systematic +1 y offset seen in D7 I-piece divergence test is likely this convention difference leaking into comparison code. SRS+ does NOT change piece rotation states or mino definitions — only kick tables (adds 180° kicks, tweaks some offsets). The earlier cell-fingerprint test that "proved physical mismatch" was likely feeding Cobra's anchor y through Fusion's mino function, creating apparent shift.

*Created: 2026-02-09T21:21:25.759Z*

### [L5yRSRPcDD4fqHEeyNEAHF] (architecture)

[PROJECT] STME RESEARCH FINDINGS (2026-02-09): STME = "Same Type Multiple Element" — shakkar23's software SIMD abstraction in fast-reachability fork. `stme<T, N>` wraps `std::array<T,N>` with element-wise bitwise operators. shakkar23's fork uses stme<u64,4> for board; ImpleLee's original uses std::experimental::simd (hardware SIMD). BOTH use identical algorithm: `binary_bfs` = bitboard flood-fill (NOT traditional BFS). This is structurally IDENTICAL to Fusion's SSA (movegen_bitboard.rs) — Cobra-style bitboard flood-fill with fixpoint expansion via shift+AND. Board layout: packed 10-bit rows into u64 chunks (6 rows per u64, 4 chunks = 24 rows). Key insight: SSA/binary_bfs IS the state-of-the-art algorithm; the optimization frontier is in the BOARD REPRESENTATION and VECTORIZATION BACKEND (stme vs hardware SIMD vs scalar), not the algorithm itself.

*Created: 2026-02-09T20:51:38.827Z*

### [AwRehhT8ZaM1A8gXQJy287] (learned-pattern)

[PROJECT] JJ LINEAGE CONFUSION ANTI-PATTERN (2026-02-09): Do NOT use jj restore/file show/diff across different lineages to transplant files - the surrounding engine files (kicks.rs, piece.rs, collision.rs) differ between lineages and cause massive regressions when mixed. D7=-50M resulted from mixing rewrite-branch base with d7-6142-fix movegen_bitboard.rs. Each lineage is a self-consistent unit; cherry-picking individual files breaks parity. Also: jj restore --from can trigger unexpected rebase chains and move the working copy to wrong commits. Use jj undo immediately if this happens.

*Created: 2026-02-09T20:22:06.113Z*

### [nfsHZV6ewzBJDySBdAGk5u] (architecture)

[PROJECT] BOARD REPRESENTATION HISTORY (2026-02-10): Originally column-major [u64;10]. Row-major rewrite spec v2 decided on [u64;4] packed-row (6 rows per u64, 4 chunks = 24 rows). Current new-baseline branch uses the packed-row representation. Column-major decision is superseded.

*Created: 2026-02-09T04:48:27.560Z*

### [1gWrKtS5TZgmbsJ4ADkGB3] (project-config)

[PROJECT] COLD CLEAR 2 ARCHITECTURE ANALYSIS 2026-02-08T21:22:00Z

## BOARD REPRESENTATION
- Type: Column-major bitboards ([u64; 10])
- Each column is 64-bit, representing 40 height (bits 0-39)
- Collision: `board.cols[x] & 1 << y != 0` (bit test)
- Line clears: Bitwise AND across all columns, then shift columns
- Special: BMI2 `_pext_u64` instruction for fast line clear on x86_64

## MOVEGEN ALGORITHM
- Lock-delay-free: Generates all final placements directly
- Fast mode: When stack < 16 rows, iterates all (x, rotation) combos at y=19
- Full mode: BFS from spawn position using BinaryHeap for cost ordering (soft_drops)
- Cost function: Minimizes soft drops (gravity moves before lock)
- Collision detection: Precomputed CollisionMaps for all 4 rotations
- CollisionMaps: `[[u64; 10]; 4]` - bitfield per column per rotation

## MOVE DEDUPLICATION
- Canonical rotation mapping per piece type (data.rs:198-248)
- T/J/L pieces: All 4 rotations considered distinct
- O piece: Maps 4 rotations to canonical (shift positions)
- S/Z pieces: Map {N,E}→N, {S}→N-1y, {W}→E-1x
- I piece: Map {N,E}→N, {S}→N-1x, {W}→E+1y
- Underground locks: AHashMap tracks minimum soft_drops to canonical placement

## SPIN DETECTION
- T-piece only: Special logic in movegen.rs:265-286
- Corner counting: Checks 4 corners (-1,-1), (1,-1), (-1,1), (1,1) relative to piece center
- Mini T-spin: 2+ corners occupied, kick index != 4
- Full T-spin: 3+ corners occupied OR kick index = 4
- Other pieces: Spin::None

## SRS KICKS
- Precomputed LUT: `kicks(piece, from, to)` returns 5 (dx, dy) offsets
- Rotation logic: offsets(piece, rotation) defines per-rotation cell positions
- Kick calculation: Subtract "from" offsets from "to" offsets
- Test order: 5 kick offsets per rotation, return first valid placement
- Custom offsets: O and I pieces have non-standard SRS+ kicks

## TRANSPOSITION TABLE
- StateMap<V>: Sharded hash table (4096 buckets)
- Sharding: `(hash >> 32) % 4096` bucket index
- Locks: Each bucket has RwLock for thread-safe access
- Hasher: ahash::RandomState (fast non-crypto)
- Purpose: Cache evaluated GameState → value to avoid recomputation

## MEMORY MANAGEMENT
- Bumpalo Herd: Arena allocator for per-thread allocations
- Self-referencing: ouroboros crate for lifetime-safe arenas
- Layers: Lazy<Box<LayerCommon>> - deferred expansion
- Speculation: EnumMap<Piece, Vec<ChildData>> - all 7 pieces

## SEARCH STRATEGY
- DAG-based: Transposition-aware game tree
- Two layer types:
  - Known: Concrete game states from actual moves
  - Speculated: Hypothetical states for MCTS-style exploration
- Selection: UCB-like with exploration parameter and random noise
- Backprop: Updates parent evaluations when child improves

## SIMD USAGE
- BMI2 only: `_pext_u64` instruction for line clear (data.rs:316-322)
- No AVX/SSE: No vectorized piece operations
- Fallback: Loop-based line clear on non-BMI2 systems

## BENCHMARKS
- Criterion framework: benches/movegen.rs
- Test boards: Empty, T-spin setup, DTD setup, terrible setup
- Metrics: Moves/second per piece type
- Latest optimization: CollisionMaps lookup to avoid collision loops (commit ed8b193)

## CODE STATISTICS
- Total Rust: ~2675 lines
- Main modules: data.rs (334), movegen.rs (357), dag.rs (318)
- Transpositions: map.rs (98), sharded StateMap
- Workers: 1 thread spawn in lib.rs:120-125

## KEY INSIGHTS
- Column-major bitboards optimal for line clears (AND across columns)
- Precomputed CollisionMaps major speedup (no per-cell collision checks)
- Canonical rotation dedup reduces search space ~2-4x
- Sharded transposition table enables multithreaded scaling
- Fast mode when stack is low (<16 rows) skips BFS entirely
- Underground locks track minimum path cost for final placements

*Created: 2026-02-09T03:28:12.316Z*

### [ajAk6Z1ZWM2Je67HKRmUNa] (unknown)

[PROJECT] Fastest Tetris Engines and Bots Comparison 2026

PERFORMANCE HIERARCHY:

Tier 1: Competitive bots (>100M nodes/second):
- Cold Clear 2 (Rust): Modern, multithreaded, transposition-aware
- Cold Clear (Original): Well-established, Tetris Bot Protocol
- Zetris (MisaMino): TETR.IO focused, C# with full ruleset
- MisaMino: Reference implementation, T-spin pioneer

Tier 2: High-performance engines:
- Tetris Bitboard (C): Bitboard specialist, 28-row cache optimization
- Guideline-Tetris-AI (C++): BFS-based AI with 6-feature heuristics

Tier 3: Perfect clear solvers:
- perfect-tetris (Zig): ~93ms for 4-height PC, specialized for perfect clears
- TGM secret grade solver: Brute-force approach, not optimized for general play

KEY INSIGHTS:

1. Bitboards + precomputed tables = fastest collision detection
2. Column-major layout optimal for TETR.IO line clears
3. Lock-delay-free movegen critical for search speed
4. TETR.IO complex (spins, combos) requires full SRS + attack tables
5. WASM bindgen with Rust: Use slice passing, inline hot paths, precomputed tables
6. SIMD provides 1.5x-2.5x speedup for vectorizable operations
7. Cache-friendly data structures reduce memory access time
8. Transposition tables (MCTS-style) avoid redundant evaluation

NOTABLE ALGORITHMS:

Lock-delay-free movegen:
- Generate all possible final placements for current piece
- Precompute all 4 rotations and all possible x positions
- Test collision for each (bitboard AND/OR)
- Return list of valid final states
- Skip intermediate gravity simulation entirely

Canonical rotation deduplication:
- Map 4 rotations to canonical set (e.g., spawn, 90°, 180°, 270°)
- Use hash or Zobrist key for transposition lookup
- Only evaluate unique canonical states per piece
- Can reduce search space by 2-4x

SRS Wall Kick System:
- 5 kick tests per rotation: basic rotation then offsets
- Test order: 0, 1, 2, 3, 4 (or until success)
- Precompute for speed: offset tables in read-only memory
- T-piece: 4-point test system for T-Spin detection
- Offset encoding: (x, y) with positive=right/up

Bloom Filters for Duplicate Elimination:
- Hash-based deduplication with probabilistic guarantee
- 335,333,533 prime number for filter size
- O(1) memory: single integer array lookup
- Reduces state explosion in search trees

CODE PATTERNS FROM TOP IMPLEMENTATIONS:

Cold Clear 2 (Rust):
- async/await with futures for concurrent bot operations
- Arc<BotConfig> for thread-safe shared state
- EnumSet for efficient bag randomization
- Zero-allocation patterns in critical paths

Tetris Bitboard (C):
- Inline functions with __attribute__((always_inline))
- O2 compilation flag for speed
- ROWS=28 defines board height (24 visible + bottom wall)
- 64-bit aligned board struct for cache line optimization
- Bitwise AND/OR operations: board->rows[row] & (pieces[piece] >> col)

MisaMino (C++):
- Hybrid C++/Lua for scripting flexibility
- External JSON configuration for rulesets
- DLL-based API for external integration

WASM COMPILATION STRATEGY:

```toml
[package.metadata.wasm-pack.profile.release]
wasm-opt = ["-Oz", "-O3"]
lto = true
wasm-simd = true

[dependencies.web-sys]
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.2"

[lib]
crate-type = ["cdylib", "rlib"]
```

This combination provides:
- Maximum runtime optimization
- SIMD acceleration
- Small binary size
- Fast JS interop
```

*Created: 2026-02-09T03:14:54.285Z*
