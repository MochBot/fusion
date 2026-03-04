# Policy+Value V2 Contract Plan

Last updated: 2026-03-12 (UTC)

## Scope Boundary

This plan covers the next implementation batch only:

1. define the V2 contract
2. split shared causal inputs from replay-only auxiliary inputs
3. fix player-target semantics
4. align export/runtime with the shared-core contract
5. add offline coaching-grade evaluation metrics

Hard stop after step 5. Do **not** regenerate artifacts, rerun label generation, or retrain until the V2 verification gate passes.

## V2 Goal

The near-term product target is replay-grounded coaching and grading backed by strong search truth. V2 therefore separates:

- **shared-core search/value inputs**: causal, runtime-valid tensors used by search/value heads and export/runtime
- **auxiliary replay-intent inputs**: replay-only tensors used for player-intent supervision and coaching metrics, but not for the shared search/value backbone

## V2 Contract

### Shared-core inputs

- `features (B, 854)`
- `candidate_move_features (B, C, 14)`
- `candidate_mask (B, C)`

These are the only inputs allowed into the shared search/value path and the only inputs exported to ONNX/runtime.

### Auxiliary replay-intent inputs

- `player_aux_context_features (B, 56)` from recent replay trajectory
- `player_aux_future_piece_ids (B, 14)` retained as replay-only context
- `player_aux_future_hold_usage (B, 14)` retained as replay-only context

These tensors are training/coaching-only. They must never become required runtime inputs.

### Heads and loss boundaries

- **search policy head**: shared-core only
- **value head**: shared-core only
- **player policy head**: shared-core + replay-only auxiliary context

Search/value remain the strength-first core. Player-intent supervision remains a separate lane and must not contaminate the shared search/value representation.

### Player-target semantics

V2 player-target matching is based on exact preprocessed `actual_move_raw`, not tuple reconstruction. The replay extractor already knows the executed placement at extraction time, so the sidecar should preserve that exact move identity and the dataset should recover the player-policy index by exact candidate `Move.raw` match.

## Verification Gate Before Any Regeneration

All of the following must be true before the next artifact regeneration or retraining run:

- metadata encodes the V2 shared-core / auxiliary split
- search/value forward path has no dependency on replay-only tensors
- player-policy target recovery uses `actual_move_raw`
- exporter metadata matches native runtime expectations
- offline metrics expose at least:
  - player target availability rate
  - player top-1 / top-3 accuracy on available rows
  - player mean rank on available rows
  - search top-1 / top-3 accuracy on the oracle best move
  - search mean rank of the oracle best move

## Step Order

1. land this V2 contract and metadata
2. refactor dataset/model split
3. switch player-target recovery to `actual_move_raw`
4. update export/runtime manifest and parity tests
5. add offline coaching-grade metrics

Only after those five steps verify cleanly should artifact generation resume.
