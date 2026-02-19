# Supermemory Export

**Exported:** 2026-02-20T03:54:47.852Z
**Scope:** project

---

## Project Memories (81)

### [z4nrm3nAdseYXsX3AjEBgk] (architecture)

[PROJECT] EXCLUDING ARTIFACTS FROM REFACTORING (2026-02-19):

## Standard Exclusion Patterns
```
# .refactorignore (custom, used by IDE/tools)
node_modules/
dist/
build/
target/
pkg/              # wasm-pack output
*.generated.ts
*.generated.rs
*.d.ts
__generated__/
fixtures/
*.wasm
*.png
*.jpg
*.pdf
third-party/
vendor/
```

## Tool-Specific Config
**knip**: `ignoreFiles: ["src/generated/**", "fixtures/**"]`, `ignoreDependencies: ["@internal/pkg"]`
**cargo-machete**: `package.metadata.cargo-machete.ignored = ["prost"]` (build.rs deps)
**clippy**: `#[allow(dead_code)]` on FFI exports

## Detection vs Exclusion
- Run tools FIRST to identify candidates
- Add false positives to exclusion lists
- Review exclusions quarterly (stale entries)

## Binary/Large Fixture Handling
- Never include in refactor scopes
- Keep separate `.gitignore` entries
- Use Git LFS if tracked

*Created: 2026-02-19T23:16:20.955Z*

### [ZLHiHasXg166nh11EHGKPv] (architecture)

[PROJECT] ATOMIC COMMIT SEQUENCING FOR REFACTORS (2026-02-19):

## Atomic Commit Definition
- Single purpose, cannot be subdivided
- Builds AND tests pass
- Self-documenting message

## Refactor-Only Commit Rules
1. **No behavior changes** (input→output identical)
2. **One extraction per commit** (Extract Method, Move Class, etc.)
3. **Message format**: `refactor(scope): extract X from Y for Z reason`

## Sequencing Pattern
```
commit 1: refactor(auth): extract PasswordValidator from UserAuthService
commit 2: test(auth): add characterization tests for PasswordValidator
commit 3: refactor(auth): move PasswordValidator to validators/
commit 4: refactor(auth): remove deprecated wrapper from UserAuthService
```

## Safe Merge Strategy
- Squash only if commits span <1 day and single author
- Keep characterization test commits separate
- Never squash across test failures

## JJ Workflow
```bash
jj describe -m "refactor: extract PasswordValidator"
jj new  # clean commit boundary
```

## PR Review Optimization
- Group related commits in PR description
- "Refactor chain: extract → move → cleanup"
- Link to pre-refactor checkpoint tag

*Created: 2026-02-19T23:16:20.721Z*

### [LeazpNB4ZHNE4qdtocjwer] (architecture)

[PROJECT] STEPWISE VERIFICATION + ROLLBACK CHECKPOINTS (2026-02-19):

## Pre-Refactor Checkpoint
- `jj describe -m "checkpoint: pre-refactor snapshot"` OR `git tag pre-refactor-YYYYMMDD`
- Run full test suite, capture baseline

## Incremental Verification Pattern
1. **Characterization tests** (for untested code): Capture actual behavior, not expected
2. **Run after each extraction**: `cargo test` / `bun test`
3. **Gate with CI**: PR must pass before merge

## Rollback Strategy
- **JJ**: `jj undo` (single command, instant)
- **Git**: `git revert --no-commit` then review
- **Feature flags**: Disable new path, re-enable old path (no redeployment)

## Git Bisect Hygiene
- Every commit must build AND pass tests (no "WIP" commits)
- Refactor-only commits: no behavior changes
- If refactor reveals bug, fix in separate commit

## Checkpoint Cadence
- After every major extraction
- Before touching shared/util code
- Before modifying public APIs

*Created: 2026-02-19T23:16:11.368Z*

### [xPm5Cx3Ks2cQfR6mT5H4VJ] (architecture)

[PROJECT] AGGRESSIVE DEAD-CODE ELIMINATION BEST PRACTICES (2026-02-19):

## RUST Toolchain
- `cargo clippy --all-targets -- -D warnings -W dead_code` (CI enforcement)
- `cargo +nightly udeps` (unused dependencies in Cargo.toml)
- `cargo machete` (fast but imprecise dep check, use --with-metadata for accuracy)
- `#[allow(dead_code)]` for false positives (e.g., FFI entry points, macro-generated callers)

## TypeScript/Svelte Toolchain
- **knip** (recommended): `knip --fix` auto-removes unused exports/dependencies. Config: `ignoreFiles`, `ignoreDependencies`, `ignoreExportsUsedInFile: { interface: true, type: true }`
- **ts-prune**: `npx ts-prune --error` for CI gating. Use `.ts-prunerc` to ignore entrypoints.
- **ts-morph**: `sourceFile.fixUnusedIdentifiers()` for programmatic cleanup (iterate 2-3x for cascading removals)

## Svelte-Specific
- Tree-shaking works on ESM imports only (no require)
- Svelte 5 runes ($state, $derived) are tree-shakeable; transition code requires component usage to avoid pruning
- Vite's production build applies tree-shaking automatically

## False Positive Guards
- Dynamic imports: `await import()` not detected by static analysis
- Exported types used in .svelte files (knip's `ignoreExportsUsedInFile`)
- Reflection patterns (emit/decorators)
- Test utilities with indirect usage

*Created: 2026-02-19T23:16:11.190Z*

### [FAqi2HuL4M1rWQJxrnx7ZJ] (architecture)

[PROJECT] GOD-FILE REDUCTION STRATEGY (2026-02-19):

## Detection Heuristics (God Class/Module)
- >500 lines OR >20 public methods OR >15 dependencies
- Cohesion score <0.3 (methods access disjoint field subsets)
- Change frequency across unrelated bug fixes

## Staged Extraction (Martin Fowler)
1. **Identify code regions** (file I/O, validation, rendering, etc.)
2. **Create covering tests** for region before extraction
3. **Extract Method** for region-specific logic
4. **Move Method** to new class with minimal interface
5. **Replace caller** with new class delegation
6. **Iterate** until god-file is thin orchestration layer

## Risk Mitigation
- One region per commit (atomic + revertible)
- Keep original methods as deprecated wrappers temporarily
- Feature flag for new class path if high traffic
- Never extract >3 methods in single PR

## Prioritization
- Most-changed regions first (highest ROI)
- Stateless utilities (easiest to move)
- Dependencies with clear boundaries

*Created: 2026-02-19T23:16:11.161Z*

### [rXQ9966wMgzyEjwDRriv7t] (architecture)

[PROJECT] REFACTORING RESEARCH REQUEST (2026-02-19): User requested best practices research on: (1) Aggressive dead-code elimination in Rust + TypeScript/Svelte repos, (2) Staged god-file reduction strategy, (3) Stepwise verification and rollback checkpoints, (4) Excluding generated/third-party/binary artifacts from refactors, (5) Atomic commit sequencing for refactor-only changes. Research in progress.

*Created: 2026-02-19T23:13:14.929Z*

### [D3SdA3CLzLo3kFwutyi4eB] (conversation)

[PROJECT] Plan recovery expectation: The active implementation plan should be recovered from past session history (after crash recovery), not assumed from the only file currently under `.sisyphus/plans/`.

*Created: 2026-02-18T20:37:34.742Z*

### [iSyJMMwWH9PfFkYbVb2oUq] (architecture)

[PROJECT] UNSAFE ELIMINATION (2026-02-17): All std::mem::transmute calls in header.rs, movegen.rs, gen.rs replaced with safe piece_from_u8(), rotation_from_u8(), spin_from_u8() functions. Two MaybeUninit usages remain in movegen.rs:31 and :102 (MoveBuffer) - should be audited for soundness.

*Created: 2026-02-17T19:45:46.231Z*

### [GnKtbfhAQJuhTXWuWnGKUs] (architecture)

[PROJECT] MOSAIC-FUSION-TESTING TECH STACK (2026-02-17): Svelte 5.49.1, Vite 7.3.1, Tailwind 4.1.18, PIXI.js 8.15.0, Vitest 4.0.18. Uses Svelte 5 runes ($state, $derived, $effect). WASM loaded via Web Worker. Triangle engine at src/lib/triangle/ for replay simulation.

*Created: 2026-02-17T19:45:43.450Z*

### [r4ePgd4D4g2vJmc6bs3aUN] (architecture)

[PROJECT] BUG FIXES APPLIED (2026-02-17): Bug 1 (impossible placements) - evaluate_position validates best move against post-lock board via obstructed_move(), suppresses suggestions blocked by garbage. Bug 3 (P2 analysis) - ReplayViewer iterates ALL players in moveflagsPerPlayer, auto-switches selectedPlayer. Bug 4 (eval calibration) - thresholds widened from 0.5/1.5/3.0 to 5/15/30 to match board eval scale where swings reach 20-50 points.

*Created: 2026-02-17T19:45:39.849Z*

### [Wx24ye7zLovPsCe76Kgjef] (architecture)

[PROJECT] FUSION V2 WASM DEPLOYMENT: wasm-pack builds to pkg/ with package name direct-cobra-copy. Deploy by copying direct_cobra_copy_bg.wasm + .js + .d.ts to mosaic-fusion-testing/src/lib/fusion/wasm/. App imports via fusion_wasm.js which internally loads direct_cobra_copy_bg.wasm. wasm-pack 0.14.0 changed --out-dir to --artifact-dir (requires nightly); use default pkg/ output instead.

*Created: 2026-02-17T19:45:35.493Z*

### [BH3HbFhwK22YNScYv91qC7] (architecture)

[PROJECT] FUSION V2 CURRENT STATE (2026-02-17): 18 source files, 126 tests (112 unit + 14 integration), package name direct-cobra-copy, attack-aware beam search with evaluate_position() for board-comparison analysis. Eval thresholds: <5.0=None, <15.0=Inaccuracy, <30.0=Mistake, >=30.0=Blunder. All unsafe transmutes replaced with safe conversion functions. Stack-allocated SearchNode paths and RemainingPieces.

*Created: 2026-02-17T19:45:31.489Z*

### [8mQUxfxDPk63hm6bdxQewQ] (error-solution)

[PROJECT] EVAL POPUP PRE-LOCK TIMING FIX (2026-02-17): ReplayViewer.svelte now indexes moveflags by Math.max(1, preLockFrame - 1) instead of resultFrame (lock frame). This makes the eval popup pause playback 1-2 frames BEFORE the piece visually locks. Removed the old jumpTo(rewindTo) rewind logic at lines 661-668 — no more jarring frame jumps. The preLockFrame comes from the worker's ANALYSIS_RESULT message (spawn.frame captured before engine.tick()).

*Created: 2026-02-17T16:16:15.293Z*

### [ds9zN7xPydwkZfXYaDjzyM] (architecture)

[PROJECT] ATTACK SCORING INTEGRATION (2026-02-17): Integrated real S2 attack mechanics into beam search scoring. Changes in src/search.rs: SearchNode now tracks b2b(i16), combo(i16), attack_total(f32) per node. SearchConfig got attack_scale(0.3 default) and attack_cap(10.0 default). Score = evaluate(board, weights) + attack_scale * min(attack_total, cap). gen_and_eval_root and expand_node compute spin type from Move.spin(), call calculate_attack() with proper b2b/combo propagation. Root nodes inherit b2b/combo from GameState. 112 tests pass (109 existing + 3 new attack tests). WASM deploy note: pkg/ outputs direct_cobra_copy_bg.wasm (crate name), must copy to mosaic-fusion-testing/src/lib/fusion/wasm/ with that exact filename since fusion_wasm.js references it at line 608.

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

[PROJECT] PRESIM IMPLEMENTATION COMPLETE (2026-02-16): All 7 presim optimization tasks implemented and verified. (1) Search defaults: beam_width 800→300, depth 6→12. (2) New transposition.rs: ZobristKeys (400 keys, deterministic splitmix64), TranspositionTable (64K entries, depth-preferred replacement), probe-before-eval. (3) Futility pruning: futility_delta=3.0 default, skips children >Δ below best. (4) Iterative widening (CABS): time_budget_ms config, widths 100→200→400→beam_width, Instant on native, iteration-count fallback on WASM. (5) New bag.rs: BagTracker (7-bag tracking), extend_queue (conservative predict when ≤2 remain). (6) Integration: search.rs uses TT (opt-in use_tt:false default) + bag (opt-in extend_queue_7bag:true default). wasm.rs uses time_budget_ms:Some(50). analysis.rs unchanged (..default() spread). (7) Final: 109 tests pass, 0 fail. Native build clean. WASM build clean. New files: src/transposition.rs, src/bag.rs. Modified: src/search.rs, src/wasm.rs, src/lib.rs.

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

### [ioknRHiYZF9U7LwhLabdqe] (architecture)

[PROJECT] SRS+ REFERENCE DOCUMENT: Ingested from official sources. Authoritative for kick tables and rotation states. Y-axis convention note: all engines use Y-UP despite some external docs claiming Y-down.

*Created: 2026-02-10T05:59:28.414Z*

### [95QWjiWjAnXPrxHSqGqEjB] (learned-pattern)

[PROJECT] MINDSET: Node count is not the end goal -- coaching engine prioritizes eval quality over raw NPS. Validated by V2 architecture: beam search with attack-aware scoring, board-comparison analysis (evaluate_position), and calibrated severity thresholds.

*Created: 2026-02-10T05:50:11.296Z*

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

### [RhdTfhGxbaPCmQB38EgLQs] (architecture)

[PROJECT] USER RESEARCH REPORT: Ingested pre-V2 rewrite. Historical reference for coaching feature prioritization. User research predates the V2 ground-up rewrite but core insights about what players want from coaching remain valid.

*Created: 2026-02-09T22:22:46.647Z*

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
