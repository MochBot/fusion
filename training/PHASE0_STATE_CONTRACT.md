# Phase 0 State Contract

Last updated: 2026-03-09 (UTC)

## Purpose

Phase 0 defines one canonical contract for replay-derived decision points so training artifacts, dataset loading, and runtime-facing piece/state assumptions stop drifting independently.

## Example Identity

Each canonical example owns these identity fields before any flat export step:

- `schema_version`
- `replay_id`
- `round_id`
- `player_id`
- `frame_id`
- `group_id`

`group_id` is replay-round scoped. Both players from the same round share one group so later train/validation splitting can stay replay-disjoint.

## Replay Alignment Rule

A decision-point example is anchored to one player key event frame. The player state is extracted **after** applying the player’s hard drop at that frame. The opponent snapshot is the opponent board state from **before any opponent events at the same frame**. This avoids arbitrary same-frame tie ordering and makes alignment deterministic.

## Board Ownership Rule

- `player_board`: the acting player’s board after the placement event
- `opponent_board`: the aligned opponent board snapshot for the same decision point
- both boards use `column_major_binary` encoding over a `10 x 40` board

## Queue / Hold / Current Ownership

- `current_piece`: current piece after the placement-spawn transition
- `hold_piece`: current hold slot after the placement
- `queue`: next visible queue slice after the placement, truncated to 5 entries

## Scalar Field Definitions

Scalar order is canonical and versioned:

1. `combo`
2. `b2b`
3. `lines`
4. `garbage_pending`
5. `bag_number`

`bag_number` is defined as completed bags based on placed-piece count, not an ad hoc positional guess in downstream code.

## Label Field Definitions

Current Phase 0 label order remains:

1. `game_outcome`
2. `lines_sent`
3. `b2b_after`
4. `position_normalized`
5. `time_to_topout`

These labels are still legacy training labels. Phase 1 will replace primary supervision with policy/value targets.

## Legal Move Indexing Contract

Phase 0 does not yet introduce the final policy head, but the contract reserves canonical example identity and grouping so legal-move indexing can be layered on top without redefining replay ownership.

## Split-Group Metadata

Flat binary training artifacts must ship with:

- `training_data.bin.metadata.json` — schema/version/order sidecar
- `training_data.bin.groups.u64` — one stable u64 group hash per sample

The binary sample file is no longer trusted on shape alone.

## Runtime Boundary in Current Checkout

This checkout does not currently include a Rust-side feature encoder for the training vector. The runtime-facing contract that still matters in Phase 0 is the WASM piece ordering exposed in `src/wasm_types.rs`, which remains intentionally separate from the training piece order and must be explicitly documented rather than inferred.

## Known Legacy Violations Repaired by Phase 0

- preprocessing previously wrote anonymous flat rows with no schema sidecar
- preprocessing processed players separately, which risked opponent-snapshot timing drift
- dataset loading previously trusted raw shape and silently accepted ambiguous artifacts
- teacher target derivation previously accessed `bag_number` through the wrong scalar index
- train/validation splitting previously happened after mirror augmentation, leaking mirrored twins across splits
