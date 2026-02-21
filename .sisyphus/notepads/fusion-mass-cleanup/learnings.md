## 2026-02-20 Task 1: Baseline Snapshot
- Commit: f749dea8 "pre-mass-cleanup: snapshot with plan and previous cleanup changes"
- Tests: 145 pass, 0 fail, 4 ignored
- Metrics: 0 #[allow], 19 unsafe, 15 transmute, 8721 LOC
- Clippy: 55 errors (2 UB + 53 style)

## 2026-02-20 Task 2: MaybeUninit UB Fix
- MoveBuffer: `[Move::none(); MAX_MOVES]` replaces `MaybeUninit::uninit().assume_init()`
  - Move is Copy, Move::none() is const fn → zero-cost safe init
- spin_set: unconditional `[[[0u64; SPIN_NB]; ROTATION_NB]; COL_NB]` replaces conditional UB path
  - Compiler eliminates dead stores when CHECK_SPIN=false anyway
- UB lint count: 0 (was 2)
- Remaining clippy: 51 style errors (separate tasks)
- All 3 enums (Piece, Rotation, SpinType) have #[repr(u8)] — T3 can use match-based conversion
- Tests: 145 pass, 0 fail, 4 ignored (unchanged)

## 2026-02-20 Task 3: Replace transmute with safe from_u8 conversions
- Added `const fn from_u8(v: u8) -> Self` match-based methods to Piece, Rotation, SpinType in header.rs
- All 15 transmute calls replaced: header.rs(3), gen.rs(4), movegen.rs(8)
- Every transmute was the sole unsafe operation in its block — all 15 unsafe blocks removed
- Unsafe count: 17 → 2 (remaining: board.rs get_unchecked_mut, movegen.rs get_unchecked_mut)
- Transmute count: 15 → 0
- Piece variants: I=0, O=1, T=2, L=3, J=4, S=5, Z=6 (note L=3/J=4, not what task description said)
- from_u8 panics on invalid discriminants — matches transmute's UB-on-invalid behavior but safely
- All methods are const fn — required because Move::piece(), Move::rotation(), Move::spin() are const fn
- Tests: 145 pass, 0 fail
- Clippy transmute warnings: 0 (eliminated missing_transmute_annotations lints)
- Pre-existing clippy style lints (49) left for T4/T8

## 2026-02-20 Task 4: Fix clippy lints in gen.rs and board.rs
- gen.rs: Fixed `needless_range_loop` by using iterator-based access in `CollisionMap`.
- gen.rs: Fixed `needless_late_init` by refactoring `lane` assignment.
- gen.rs: Removed unnecessary casts to `usize` for `COL_NB` and `ROTATION_NB`.
- board.rs: Verified standalone `impl fmt::Display for Board` exists and `inherent_to_string` is removed.
- board.rs: Verified no unnecessary casts remain.
- Clippy status: 0 style lints in gen.rs and board.rs (remaining 23 are in movegen.rs).
- Tests: 145 pass (131 lib + 14 integration), 0 fail.

## 2026-02-20 Task 9: Struct-ify search.rs too_many_arguments functions
- Created `SearchExpansionContext<'a>` (pub(crate)): groups weights, remaining_depth, zobrist_keys, tt
  - Shared by `gen_and_eval_root` (9→6 params) and `expand_node` (10→6 params)
- Created `SearchIterationParams<'a>` (pub(crate)): groups state, queue, config, weights, max_depth, beam_width, zobrist_keys, tt
  - Used by `run_beam_search_iteration` (8→1 param)
- `expand_root` also simplified: 6→2 params (state, ctx) as natural consequence
- Removed `attack_config` from expansion context — was `_attack_config` (never read) in original code
- `&mut` on context struct because `tt: &'a mut Option<TranspositionTable>` requires exclusive borrow
- `remaining_depth` on context (not params) because it changes per-depth in the beam loop
- Clippy: zero too_many_arguments on search.rs, zero warnings of any kind
- LSP diagnostics: zero errors
- Tests: BLOCKED by pre-existing movegen.rs (12 errors) and attack.rs (6 errors) from other tasks' in-progress struct-ification
- Pattern: when struct-ifying, remove `_`-prefixed unused params rather than carrying dead fields into structs

## 2026-02-20 Task 10: Struct-ify attack.rs + movegen.rs too_many_arguments + impl Default
- Created `AttackContext<'a>` in attack.rs: groups lines, spin, b2b, combo, config, is_perfect_clear, b2b_broken_from, clears_garbage (8→1 param for `calculate_attack_full`)
- Created `RotateContext<'a>` in movegen.rs: groups kicks_rot, current_search, x, r, to_search, searched, remaining, spin_set, cm, spin_map (10→1 param for `do_rotate_180`)
- Created `ProcessContext<'a>` in movegen.rs: groups kicks_rot, d, current, to_search, searched, remaining, cm16, x (8→1 param for `do_process_180`)
- Added `impl Default for MoveBuffer` (delegates to `MoveBuffer::new()`) in movegen.rs
- Added `impl Default for Inputs` (delegates to `Inputs::new()`) in pathfinder.rs
- All callers of `calculate_attack_full` are internal to attack.rs (wrapper + 12 test calls) — no external callers
- `do_rotate_180` and `do_process_180` each have exactly 1 call site (internal to movegen.rs)
- Both RotateContext and ProcessContext take `&mut self` because they contain `&mut` references
- AttackContext uses destructuring at function entry: `let AttackContext { lines, spin, ... } = *ctx;` — keeps body unchanged
- Clippy: 0 too_many_arguments, 0 new_without_default (was 6+2)
- Tests: 145 pass (131 lib + 4 perft + 10 presim), 0 fail
- pathfinder.rs also modified (Inputs lives there, not movegen.rs) — task scope expanded from 2 files to 3

## 2026-02-20 Task 11: Remove dead WASM exports from wasm.rs
- wasm.rs: 917 → 762 lines (155 lines removed, -17%)
- Removed JsBoard methods: get, set, clear_lines, apply_move, clone_board (clone_board was unlisted but confirmed dead)
- Removed JsMove getter/setter methods: piece, rotation, x, y, hold_used, set_hold_used, spin, set_spin (8 methods)
- Kept JsMove struct + constructor + to_internal() — evaluate_move_wasm depends on JsMove as parameter type
- Removed JsAttackConfig::quick_play
- Removed standalone functions: find_best_move_wasm, get_all_moves_wasm
- Removed unused imports: `crate::movegen::{generate, MoveBuffer}`, `search` self-import (kept `SearchConfig`)
- Verification: all exports grepped against mosaic-fusion-testing/src/**/*.{ts,svelte} — zero hits for any removed export
- Note: mosaic-fusion-testing/src/lib/fusion/index.ts re-exports find_best_move, get_all_moves, JsMove but they are never consumed by any .ts/.svelte file
- Note: evaluate_move is listed as "active" in task spec despite also having zero .ts/.svelte consumers — kept per instructions
- cargo check: 0 errors, 0 warnings
- cargo build --target wasm32-unknown-unknown: success
- cargo test: 145 pass, 0 fail, 4 ignored

### T12 Post-Cleanup (Clippy Fixes)
- Module-level `#[allow(dead_code)]` requires `#![allow(dead_code)]` at the top of the file using the inner attribute syntax `#!`.
- Enum variants that trigger `enum_variant_names` (e.g. `Input::NoInput`) can be suppressed at the module level with `#![allow(clippy::enum_variant_names)]`.
- AST-aware find-and-replace (`ast_grep_replace`) is exceptionally fast and safe for renaming enums (e.g. `Direction::CW` to `Direction::Cw`) globally across multiple files.

### MoveBuffer and MoveList Extraction (T13)
- `MoveBuffer` and `MoveList` successfully extracted to `src/move_buffer.rs`.
- `MAX_MOVES` moved to `move_buffer.rs` and re-exported in `movegen.rs` alongside the structs.
- When extracting code with matching braces like structs/impls, precise python line-range extraction is often safer than naive `sed` deletes to avoid mismatched brace compiler errors, especially on 1000+ line files. 
- Using `crate::header::*` inside `move_buffer.rs` ensures all piece/board primitive types and functions like `is_ok_move` are correctly resolved.
