# DATA INVENTORY — fusion-engine policy/value pipeline

**Purpose:** single source of truth for every training corpus and replay collection, so nobody
re-discovers the hard way which data is canonical, which is broken, and which names are load-bearing.

**Last verified:** 2026-05-29 (replay-ID set comparison + full code-reference map + post-cleanup).

---

## TL;DR

- **Canonical training corpus = `training/training_data_triangle_v1.bin`** (23.6M samples, accurate, bun/Triangle). This is what the baseline (player_top1 69.1% / search_top1 56.0%) was trained on.
- **Baseline replay source = `data/replays-x-xplus/`** (X/X+ high-tier). 100% of triangle_v1's replays live here.
- **`data/replays-legacy/` (renamed from `data/replays/` on 2026-05-29) is the OLDER, GENERAL scraper+teammate corpus** (2025 season, ~top players), later X/X+-filtered. It is **disjoint** from triangle_v1 (shares zero replays) and is **superseded** as a training source by `data/replays-x-xplus/`. (It is NOT "z-tier"; z-tier was just one analysis facet — see `_meta/z_tier_replay_counts.json`.)
- The broken Python supplement was **deleted** 2026-05-29; legacy/superseded corpora were **archived** to `training/_archive/`.

---

## Replay collections (raw `.ttrm`)

| Path                    | Files / unique                     | What it is                                                                                                                                                       | Load-bearing?                                                                              |
| ----------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| **`data/replays-x-xplus/`** | 10,457 ttrm (x: 8,363, x+: 2,094)      | **SOURCE OF THE BASELINE** (X/X+ high-tier). All 9,421 triangle_v1 replays are here; ~1,036 newer ones not yet labeled. Per-player rank metadata in `_meta/`.            | **YES** — `collect_x_xplus_replays.py` (absolute `ROOT`), `eval_ood_player.py:50`, `lookup_player_tiers.py:94`, `collect_x_xplus_incremental.py` |
| `data/replays-legacy/`  | 5,969 ttrm (3,638 yield positions) | **Legacy general corpus** (2025 scraper + teammate archive, ~top players), later X/X+-filtered (`_meta/x_xplus_filtered.txt`). **Disjoint** from triangle_v1 & from replays-x-xplus. Superseded as a training source. Source of `supplement_full`. Renamed from `data/replays/` 2026-05-29. | partial — `extract_training_data.ts:9`, `scripts/modal_app.py` `replay_dir` default (both updated) |

> ⚠️ History: this dir was named `data/replays/` (a confusingly generic name) until 2026-05-29; it is the
> **old general** corpus. The current training replays are in `data/replays-x-xplus/`.

---

## Labeled training corpora (`training/`)

| Corpus                                     | Samples    | Replays | Date   | Provenance              | Distribution | Status                          | Load-bearing?                                              |
| ------------------------------------------ | ---------- | ------- | ------ | ----------------------- | ------------ | ------------------------------- | --------------------------------------------------------- |
| **`training_data_triangle_v1.bin`**            | 23,585,629 | 9,421   | May 27 | **bun/Triangle ✓ accurate** | X/X+         | **CANONICAL baseline** (current)    | **YES** — Modal volume `triangle-v1`, `modal_app.py` `BIN_NAME`/`REQUIRED_CORPUS_FILES` |
| `fresh_x_xplus_clean.bin`                  | 143,863    | 60      | May 29 | bun ✓                   | X/X+         | clean eval corpus — **shares replay IDs with triangle_v1** (eval-leakage caveat) | no (docs/generated metadata only)                         |
| ~~`training_data_triangle_v1_supplement.bin`~~ | 5,184,695  | —       | May 28 | Python ✗ BROKEN RNG     | —            | **DELETED 2026-05-29** (~26GB; wrong board states). `combine_canonical_supplement.py` now refuses this basename. | — |
| ~~`training_data_elite_v1.bin`~~               | 907,158    | —       | May 26 | pre-Triangle            | —            | **ARCHIVED** → `training/_archive/` (superseded) | no |
| ~~`training_data.bin`~~                        | 350,307    | —       | Mar 11 | legacy phase-0          | —            | **ARCHIVED** → `training/_archive/` (285k-param era) | no |
| ~~`training_data_bench10pct.bin`~~             | 44,018     | —       | Mar 11 | legacy                  | —            | **ARCHIVED** → `training/_archive/` | no |

---

## Preprocessed but NOT yet labeled

| Artifact            | Positions | Replays | Source                | Provenance | Status                                                                                                  |
| ------------------- | --------- | ------- | --------------------- | ---------- | ------------------------------------------------------------------------------------------------------ |
| `supplement_full.*` | 9,195,089 | 3,638   | `data/replays-legacy/` (legacy general) | bun ✓      | requests+player_context aligned; labeling DEFERRED. **Different distribution** from the X/X+ baseline (a shift, not an in-distribution top-up). 0 overlap with triangle_v1. |

---

## Distribution & overlap facts (verified by replay-ID sets)

- `triangle_v1` ∩ `data/replays-x-xplus` = **9,421 (100% of triangle_v1)** → X/X+ folder is the baseline source.
- `triangle_v1` ∩ `data/replays` = **0** ; `data/replays-x-xplus` ∩ `data/replays` = **0** → the two replay folders are disjoint collections.
- `data/replays-x-xplus` has 10,457 replays; triangle_v1 used 9,421 → **~1,036 X/X+ replays are unlabeled** (the in-distribution expansion candidate).
- `fresh_x_xplus` (60 replays) ⊂ `triangle_v1` → baseline eval on `fresh_x_xplus_clean` was partly on its own training replays.

## Names that MUST NOT be renamed (hardcoded)

- `training_data_triangle_v1.bin` (+ sidecars) — `modal_app.py`, `triangle-v1` Modal volume, `combine_canonical_supplement.py`, `scripts/modal_app.py`.
- `data/replays-x-xplus/` (+ `_meta/{replay_index,players,ranks}.jsonl`) — `collect_x_xplus_replays.py` (absolute `ROOT`), `eval_ood_player.py:50`, `lookup_player_tiers.py:94`, `collect_x_xplus_incremental.py`.

## Footgun (neutralized)

- `training/scripts/triangle_label_gen/combine_canonical_supplement.py` used to default to combining canonical + the **broken** Python supplement. As of 2026-05-29 it **refuses** the `training_data_triangle_v1_supplement.bin` basename with a fatal error.

## Recommended expansion (if data growth is pursued)

Label the **~1,036 new X/X+ replays** in `data/replays-x-xplus/` (in-distribution, small) rather than the
9.2M legacy-general `supplement_full` (distribution shift). Either way, value is unproven — run a cheap
train-with-vs-without A/B before committing labeling + GPU budget.

## Archive

`training/_archive/` (gitignored) holds superseded labeled corpora kept for reference:
`training_data.bin` (phase-0 350K), `training_data_bench10pct.bin` (44K), `training_data_elite_v1.bin` (907K).
Safe to delete for ~5.6GB if space is needed.
