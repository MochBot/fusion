# Phase 1 Supervision Contract

Last updated: 2026-03-09 (UTC)

## Purpose

Phase 1 replaces legacy proxy teacher supervision with two explicit supervision lanes:

- **Phase 1a — search-aligned control lane** for tactical move-quality and value strength
- **Phase 1b — player-context lane** for elite-player next-action and trajectory intent

The core contract is that these lanes stay semantically separate. Search truth and player-intent truth must never be collapsed into one unlabeled target.

## Lane Definitions

### Phase 1a — Search-Aligned Control Lane

Phase 1a uses the current search engine as the first oracle.

- **Policy target source:** `root_scores` from `SearchResultFull`
- **Value target source:** the best search continuation score from `SearchResultFull.best.score`

This is the strength-first default for immediate move quality. Replay behavior and human-likeness are not the normative target in this lane.

### Phase 1b — Player-Context Lane

Phase 1b uses elite-player replay decisions as the primary policy/trajectory signal.

- **Primary policy target source:** the player's actual next action / placement decision
- **Context source:** recent trajectory and future continuation slices available from the replay sequence
- **Auxiliary search source:** optional search policy/value targets kept in parallel fields for comparison or auxiliary losses

This lane exists because isolated board states do not encode how the position was reached or what continuation the player was steering toward.

## Target Artifact Shape

Phase 1 introduces parallel sidecar artifacts next to the Phase 0 canonical example dataset.

### Search-aligned sidecar

For each canonical example, store:

- example identity (`schema_version`, `replay_id`, `round_id`, `player_id`, `frame_id`, `group_id`)
- `best_move_raw`
- `best_value`
- `position_complexity`
- `root_scores`: list of `(move_raw, raw_score)` pairs from search
- `policy_probs`: normalized distribution derived from `root_scores`

### Player-context sidecar

For each canonical example, store a separate player-context record containing:

- example identity (`schema_version`, `replay_id`, `round_id`, `player_id`, `frame_id`, `group_id`)
- turn-start piece context (`spawn_piece`, `hold_piece`, visible queue snapshot)
- actual human placement target (`actual_piece`, `actual_x`, `actual_y`, `actual_rotation`, `actual_hold_used`, `actual_lines_cleared`)
- executed turn input sequence (`input_keys`)
- recent placement context (`recent_piece_sequence`, `recent_hold_usage`)
- future continuation context (`future_piece_sequence`, `future_hold_usage`)

The player-context sidecar is the primary source for human intent. It is not a replacement name for search labels.

## Move ID Contract

Search-policy labels are keyed by `Move.raw()` from `src/header.rs`. This remains the stable root-move identifier for the search-oracle control lane.

The player-context lane does **not** require `Move.raw()` as its primary contract. Human action is preserved as explicit placement/context fields so it can remain semantically separate from search output.

## Policy Normalization Rule

The artifact stores both raw root scores and normalized policy probabilities.

- raw root scores preserve direct engine output for debugging and future renormalization
- normalized probabilities are derived by temperature softmax over the root scores that survive search

The default Phase 1 temperature is `1.0` unless overridden in generation metadata.

The canonical initial oracle profile is **stronger offline oracle**, not the interactive/default search config.

## Policy Head Shape

Phase 1 uses a **candidate-ranking policy head**.

- the model scores only the legal/root candidate moves present in each example
- policy supervision is stored over the surviving `root_scores` candidates from search
- Phase 1 does **not** introduce a fixed global `Move.raw()` output head

This avoids a sparse 16-bit action head and matches the current search-oracle artifact shape.

## Phase 1 Metadata Sidecar

Each Phase 1 artifact ships with metadata containing:

- `schema_version`
- source contract version
- generation mode (`search_oracle` or `player_context`)
- policy temperature
- sample count
- move ID contract (`Move.raw`) for search-aligned artifacts only

Player-context artifacts additionally record recent/future horizon lengths.

## Scope Boundaries

Phase 1 does **not** yet require:

- replacing the runtime model
- changing Rust search integration
- replacing coaching severity/explanation logic

Phase 1 **does** require:

- defining trustworthy search-aligned targets for the control lane
- defining a durable artifact format for player-context targets
- keeping human and search supervision in separate fields/sidecars
- building generation scaffolding and verification tests for both lanes

## Exit Condition

Phase 1 is complete when the repo can generate, validate, and train against both:

- search-aligned policy/value targets as a preserved control lane, and
- player-context supervision artifacts as a separate lane for elite-human intent,

without blending their semantics or regressing the Phase 0 canonical state contract.
