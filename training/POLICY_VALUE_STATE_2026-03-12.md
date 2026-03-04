# Policy+Value State — 2026-03-12

Last updated: 2026-03-12 (UTC, post-Phase-1 comparison and initial Phase 2 slice)

## Current State

The V2 policy/value contract batch is landed on bookmark `v2-contract`, the regenerated artifact family is valid again, the immediate batch-0 NaN training blocker is fixed, real player-context training runs now complete cleanly, the cross-lane Phase 1 comparison is now closed in favor of `player_context_primary`, the first real Phase 2 runtime ownership slice is implemented, and the current L4 hardware boundary has been demonstrated with the dedicated saturation probe.

This file is the durable consolidation point for the current checkpoint.

## What Was Accomplished

### 1. V2 contract work landed and verified

The V2 contract batch described in `training/POLICY_VALUE_V2_CONTRACT_PLAN.md` was implemented and verified:

- shared-core search/value inputs were split from replay-only auxiliary inputs
- player-target semantics were switched to exact `actual_move_raw`
- export/runtime parity was updated to the shared-core manifest
- offline coaching-grade metrics were added

This completed the intended hard-stop batch before regeneration/retraining.

### 2. Artifact family was regenerated back to a valid pre-retrain checkpoint

For `training/training_data.bin`, the canonical policy/value artifact family is now valid again.

Validated facts:

- `dataset_ready=True`
- `labels_ready=True`
- `player_context_ready=True`
- `sample_count=350307`
- both `search_control` and `player_context_primary` readiness checks passed

The 70-shard Modal label path was rerun cleanly and canonical root files were recovered:

- `training_data.bin.policy_value.jsonl`
- `training_data.bin.policy_value.metadata.json`

### 3. The real epoch-0 NaN blocker was fixed

The actual root cause was not corrupt labels. It was padded-candidate KL divergence:

- `collate_policy_value_batch` zero-padded `search_policy_probs`
- `PolicyValueNet` masked padded logits to `-inf`
- `PolicyValueLitModule` computed dense `kl_div` across the full padded tensor

That produced the `0 * -inf` path and immediate `nan`.

The fix was to compute search-policy KL only on `candidate_mask == True` entries. After that:

- focused tests passed
- the manual reproduction showed old dense KL was `NaN` while masked KL was finite
- a real full-bundle remote smoke completed cleanly

### 4. Real player-context training now works end-to-end

An 8-run real Modal training grid completed successfully on the regenerated full bundle in `player_context_primary` mode.

Grid facts:

- profile: `l4`
- `sample_count=350307`
- `max_epochs=50`
- all 8 runs completed with exit code `0`
- no `nan`
- no traceback

Best checkpoint from that validated grid:

- `policy_value/pvc-real-r03/policy-value-epoch=49-val_total_loss=1186.6691.ckpt`

Other completed checkpoints from the same grid:

- `pvc-real-r01` → `val_total_loss=1289.7172`
- `pvc-real-r02` → `1467.7120`
- `pvc-real-r04` → `1372.0081`
- `pvc-real-r05` → `1679.9540`
- `pvc-real-r06` → `1909.6254`
- `pvc-real-r07` → `1717.6162`
- `pvc-real-r08` → `1915.7294`

Practical conclusion from the real grid:

- batch size `256` produced the best visible validation result in this run family
- larger real-training batch sizes improved runtime but worsened validation quality

### 5. Confirmation runs showed real L4 workload headroom

Follow-up confirmation runs were launched on the winning hyperparameter family.

Observed results:

- `batch_size=512` repeated cleanly but stayed worse than the original winner
- `batch_size=768` and `1024` trained cleanly and ran faster, but degraded validation to `1666.5892` and `1550.9996`
- much larger real-training batches (`1536` through full-dataset `350307`) were admitted into real GPU training without immediate OOM

This proved the earlier batch ceiling was conservative, but also showed that quality degraded before raw memory admission failed on the real policy/value training path.

### 6. The actual L4 hardware boundary was then captured explicitly

Because real training telemetry was too coarse to prove hardware saturation, the dedicated `training/scripts/gpu_saturation_probe.py` path was used to capture a real boundary.

Concrete L4 limit evidence:

- single-instance probe at `batch_size=65536` failed with real CUDA OOM
- packed concurrent probe at `4 x 32768` also failed with real CUDA OOM

That satisfied the loop end condition for "reach the current L4 limit".

### 7. Phase 1 comparison was closed and Phase 2 began

After the durable checkpoint above was written, the missing matched lane comparison was completed.

Matched comparison facts:

- rerun baseline `pvc-real-r09` (`player_context_primary`, batch 256, lr `3e-4`, weight decay `3e-5`) finished at `val_total_loss=1202.2172`
- matched `search_control` run `pvc-search-r01` with the same batch size / lr / weight decay finished at `val_total_loss=5036.4312`
- side high-batch run `pvc-vhb-r01` (`player_context_primary`, batch 1024) finished at `val_total_loss=1680.8580`

This closed the Phase 1 lane decision boundary:

- `player_context_primary` clearly outperformed `search_control`
- the promoted checkpoint remained `pvc-real-r03` at `1186.6691`
- larger-batch player-context runs remained worse on quality even when they were faster

With that boundary closed, the smallest real Phase 2 runtime ownership slice was then implemented.

Native runtime changes:

- `src/search_config.rs` now includes `policy_guided_expansion_cap` (default `32`) and threads `current_beam_width` through `SearchExpansionContext`
- `src/search.rs` now passes the live beam width into search expansion
- `src/search_expand.rs` now uses learned policy logits to gate child admission / expansion ordering when runtime inference succeeds, instead of only applying policy as a post-admission score bonus
- explicit handcrafted fallback remains intact when runtime inference is unavailable or fails

Focused verification for this Phase 2 slice passed via:

- `source "$HOME/.cargo/env" && CLOUD_EXEC_SKIP=1 cargo test search --lib`

Oracle reviewed that slice and verified that it counts as a real Phase 2 start rather than prep.

## What Is Currently True

### Quality baseline

The current best validated player-context checkpoint is still:

- `pvc-real-r03`
- `batch_size=256`
- `lr=3e-4`
- `weight_decay=3e-5`
- `val_total_loss=1186.6691`

This is the current promoted quality baseline.

### Efficiency finding

L4 has materially more workload headroom than the original real-training grid suggested.

However, on the actual policy/value objective, pushing batch size upward improved wall-clock speed while degrading validation quality. So the useful operating point for quality is not the same as the maximum admitted batch size.

### Hardware-limit finding

The current L4 limit has now been evidenced directly by the saturation probe, not just inferred from training speed.

## Clear Next Steps From The Plans

The plan documents are still specific enough to drive the next work.

### Immediate next step set

1. **Validate the initial Phase 2 slice behavior on live search quality**
   - The mechanics are implemented and unit-tested.
   - The next question is behavioral: does policy-guided child admission improve native search decisions enough to justify expanding runtime ownership further?

2. **Keep `pvc-real-r03` as the promoted quality baseline**
   - `pvc-real-r09` reproduced in the same quality band but did not beat it.
   - `search_control` lost badly in matched comparison.
   - high-batch `player_context_primary` remains an efficiency experiment, not the promoted quality setting.

3. **If live search behavior is good, extend Phase 2 deeper rather than reopening Phase 1**
   - The lane comparison is now good enough to stop re-litigating Phase 1.
   - The next structural work should stay inside Phase 2 runtime ownership, not go back to broad training exploration by default.

### Why this is the right boundary

`training/POLICY_VALUE_V2_CONTRACT_PLAN.md` is effectively satisfied for the landed batch, but the broader rebuild blueprint is larger than that batch. The rebuild blueprint still points to:

- Phase 1 completion by generating, validating, and training both lanes cleanly
- then Phase 2 runtime integration

So the next step is not blind more-batch experimentation and it is not more lane-comparison work by default. The justified next boundary is validating and then expanding the now-started Phase 2 runtime ownership path.

## If We Need Brainstorming After The Plan Steps

If the lane comparison stops being informative, the most promising path options are:

1. **Phase 2 live-search validation**
   - run native search comparisons with the new policy-guided expansion cap on/off
   - measure whether move choice / score quality improves before broadening ownership

2. **Phase 2 deeper runtime promotion**
   - integrate policy ranking into expansion ordering first
   - then expand learned ownership further only if live validation holds

3. **Optimization-quality tradeoff work**
   - keep `256` as quality baseline
   - spend faster larger-batch settings on confirmation, ablations, and hyperparameter search rather than assuming larger-batch training itself is better

## Current Summary In One Paragraph

The project is no longer blocked on artifact corruption or the epoch-0 NaN. The V2 contract batch landed, the regenerated bundle is valid at `350307` samples, real `player_context_primary` training works end-to-end, the current best validated checkpoint is `pvc-real-r03`, larger batches are faster but worse on validation, `search_control` lost clearly in the matched comparison, the current L4 limit has been proven explicitly by the saturation probe, and the first real Phase 2 runtime ownership slice is now implemented in native search expansion. The most concrete next move is to validate that Phase 2 slice on live search behavior and then expand runtime ownership further if it holds.
