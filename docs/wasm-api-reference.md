# Fusion WASM API reference

This reference documents the generated WASM boundary used by Mosaic. Source of truth is `fusion-engine/src/wasm.rs`; generated TypeScript declarations live in `mosaic-fusion-testing/src/lib/fusion/wasm/fusion_wasm.d.ts`; Mosaic re-exports a subset from `mosaic-fusion-testing/src/lib/fusion/index.ts`.

## Board and attack classes

### `JsBoard`

- `new()` creates an empty board.
- `JsBoard.from_rows(rows: BigUint64Array): JsBoard` creates a board from 40 row masks.
- `to_rows(): BigUint64Array` returns 40 row masks.
- `free()` releases WASM memory. Always call it for manually created boards.

### `JsAttackConfig`

- `new(pc_garbage, pc_b2b, b2b_chaining, b2b_charging_base, combo_table, garbage_multiplier)` creates a config.
- `JsAttackConfig.quickPlay()` returns the Quick Play ruleset.
- `JsAttackConfig.tetraLeague()` returns the Tetra League ruleset.
- `pcGarbage` and `garbageMultiplier` expose the relevant public fields.

### `calculateAttack(...)`

`calculateAttack(lines: number, spin: number, b2b: number, combo: number, config: JsAttackConfig, is_pc: boolean) -> number` is the legacy/general JavaScript-facing attack helper. Current coaching-gap logic uses the S2/Tetra League line-search exports below instead.

## Evaluation exports

- `evaluate_board(board: JsBoard) -> number`
- `evaluate_position(pre_board, post_board, piece, frame) -> any`
- `find_best_move(board, piece, frame) -> any`
- `extract_features_for_position(board, piece, frame, opponent_board?) -> any`

These are board-evaluation and diagnostic helpers. Pass piece IDs in the JS/Triangle external order used by `mosaic-fusion-testing/src/lib/fusion/mapping.ts` unless the specific caller documents otherwise.

## Move expansion exports

- `get_all_moves(board: JsBoard, piece: number) -> any`
- `expand_all(board: JsBoard, piece: number, b2b: number, combo: number, pending_garbage: number) -> Float64Array`
- `expand_all_gm(board: JsBoard, piece: number, b2b: number, combo: number, pending_garbage: number, garbage_rows: BigUint64Array, garbage_multiplier: number) -> Float64Array`

`expand_all_gm` is the current S2-aware expansion primitive for fallback/shortlist coaching paths. Records contain attack, lines, post-B2B, post-combo, spin metadata, and resulting rows; `s2-line.ts` decodes these records and aligns them with `get_all_moves` to recover placements when it is not using the Rust line-emitting beam.
Move generation routes through the smear-core kernel: `generate_with_request` calls `smear_core::generate_smear` (strict reachability, engine-exact Full/Mini spin strata) for every piece, height, and force mode. The scalar `generate_engine` path remains for parity and diagnostic checks only.

## Beam search exports

- `beam_best_gm(start_board: Uint32Array, start_gmask: Uint32Array, pieces: Uint8Array, b2b: number, combo: number, pending: number, keep_line: Uint32Array, beam_width: number, garbage_multiplier: number) -> number`
- `beam_best_gm_surge(start_board: Uint32Array, start_gmask: Uint32Array, pieces: Uint8Array, b2b: number, combo: number, pending: number, keep_line: Uint32Array, beam_width: number, garbage_multiplier: number) -> number`
- `beam_best_gm_line(start_board: Uint32Array, start_gmask: Uint32Array, pieces: Uint8Array, b2b: number, combo: number, pending: number, beam_width: number, garbage_multiplier: number, hole_w: number, height_w: number, height_grace: number) -> any`
- `beam_best_gm_gi(start_board: Uint32Array, start_gmask: Uint32Array, pieces: Uint8Array, b2b: number, combo: number, pending: number, keep_line: Uint32Array, beam_width: number, multipliers: Float64Array, garbage_rows: Uint32Array, garbage_counts: Uint32Array, max_gi: number) -> number`

Use `beam_best_gm` as the raw S2 attack anchor, `beam_best_gm_surge` as the surge-shaped anchor, and `beam_best_gm_line` when JavaScript needs the actual line plus board-health-aware selection metadata. The live worker normally uses `beam_best_gm_line`; the TypeScript `expand_all_gm`/`get_all_moves` beam remains for fallback and shortlist cases. `beam_best_gm_gi` is the garbage-injecting variant used by offline relabeling when the player's real mid-window garbage rows must be replayed into the beam.

## Coaching sequence export

- `simulate_coaching_sequence(board: JsBoard, path: any) -> any`

This returns step-by-step clear events for a supplied path. Offline reports use it to keep per-step attack and the aggregate player/best labels on the same Rust attack model.

## JavaScript re-export surface

`$lib/fusion` intentionally re-exports a smaller set than the generated module: `initFusion`, `calculateAttack`, `JsAttackConfig`, `evaluate_board`, `JsBoard`, `find_best_move`, `extract_features_for_position`, `evaluate_position`, `simulate_coaching_sequence`, `beam_best_gm`, `beam_best_gm_surge`, `beam_best_gm_line`, `expand_all_gm`, and `get_all_moves`. Import raw generated exports only when a script or test needs a boundary that the app surface does not expose.
