# Project AGENTS.md — Fusion Engine

**Last Updated:** 2026-02-24

---

## SUBAGENT DELEGATION GOTCHAS

### JJ Working Copy Diff ≠ Subagent Changes

When a `task()` delegation returns, the file change list in the output shows the **entire JJ working copy diff** (all uncommitted changes across all waves), NOT changes made by that specific subagent. This is because JJ snapshots the full working copy state.

**Before assuming scope creep:**
1. `read` the actual source files to verify whether the subagent made any edits
2. `jj diff --stat` shows cumulative uncommitted changes, not per-task deltas
3. A subagent returning with "No assistant response found" means it didn't finish — the file list is misleading

**Recovery:** If a subagent truly went rogue, use `jj op restore <op-id>` to revert to pre-delegation state. Use `jj op log` to find the right operation.

---

## BUILD & TEST

| Command | Notes |
|---------|-------|
| `CLOUD_EXEC_SKIP=1 cargo check` | Compile check (cloud-exec interceptor requires prefix) |
| `CLOUD_EXEC_SKIP=1 cargo test --lib` | Unit tests only |
| `CLOUD_EXEC_SKIP=1 cargo clippy -- -D warnings` | Lint gate |
| `CLOUD_EXEC_SKIP=1 cargo test --test presim_validation` | Integration tests |
| `wasm-pack build --target web ...` | WASM build (see below) |

**Cloud-exec intercepts** `cargo test/bench/run --release` — always prefix with `CLOUD_EXEC_SKIP=1` or use `pty_spawn`.

**WASM rebuild:**
```bash
wasm-pack build --target web --out-dir mosaic-fusion-testing/src/lib/fusion/wasm --out-name fusion_wasm --features wasm --no-default-features
```

**WASM cache gotcha:** `cargo clean -p direct-cobra-copy` only cleans native targets. For stale WASM, `rm -rf target/wasm32-unknown-unknown` before rebuild.

---

## ARCHITECTURE QUICK REFERENCE

### Search Pipeline
```
find_best_move_with_scores_forced()
  → run_beam_search_iteration()
    → expand_root() → gen_and_eval_root()    [src/search_expand.rs]
    → loop { expand_node() → prune → sort }  [src/search_expand.rs]
    → extract best → SearchResultFull         [src/search.rs]
```

### Key Files
| File | Purpose |
|------|---------|
| `src/search_expand.rs` | gen_and_eval_root, expand_node, evaluate_with_tt |
| `src/search.rs` | Beam search loop, futility pruning, result extraction |
| `src/search_config.rs` | SearchConfig, SearchNode, SearchResultFull, SearchExpansionContext |
| `src/analysis.rs` | shape_chain_value, shape_context_modifier, assemble_composite, InsightTag |
| `src/attack.rs` | calculate_attack, AttackConfig, AttackContext |
| `src/eval.rs` | Board evaluation (9 features) |
| `src/wasm.rs` | WASM bridge, MoveEvalResultJson |
| `src/gen.rs` | Move generation (generate with check_spin param) |
| `src/state.rs` | GameState, CoachingState, TransitionObservation |

### Scoring Architecture
- **TT stores board_eval ONLY** — never composite scores (path-dependent terms composed outside TT)
- **Composite score** = `assemble_composite(board_eval, attack_val, chain_val, context_mod, config)`
- **SearchConfig weights**: attack_weight=0.25, chain_weight=0.15, context_weight=0.10, board_weight=1.0
- **Futility pruning** uses `node.score` — composite scoring automatically flows through pruning

### Board Conventions
- Row bitmasks: `u16` per row, 10 columns
- Y-up: row 0 = bottom
- `board_from_bottom_rows` helper in presim tests

---

## VCS

JJ-colocated repo. Use `jj` commands, never raw `git`.

- `jj status` / `jj diff --stat` — working copy state
- `jj op log` → `jj op restore <op-id>` — undo anything
- `jj describe -m "msg"` — amend commit message

---

## PLAN MANAGEMENT

- Plans: `.sisyphus/plans/*.md` (READ ONLY for subagents)
- Notepads: `.sisyphus/notepads/{plan-name}/{learnings,issues,decisions,problems}.md`
- Evidence: `.sisyphus/evidence/`
- Subagents APPEND to notepad files, never overwrite
