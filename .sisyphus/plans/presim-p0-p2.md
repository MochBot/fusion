# Presim plan snapshot (copied)

Source: `ses_390222b95ffe7xY8h6o6le12lb` from tool-output file `tool_c717a2643001gy34m2RWel14PP`, lines 1193-1211.

---

All background tasks are complete.  
For **immediate, deterministic gains** (no training/data pipeline), these are the best moves:

- **1) Replace scalar beam ranking with staged ranking (highest ROI).**  
  In `src/search.rs` at the two beam sort sites (~195, ~270) and futility pruning (~284), rank by: **survival first**, then tactical value, then structure quality (instead of only `score`). This directly fixes bad “high attack but dead board” picks.

- **2) Upgrade attack term from “capped total” to attack-quality decomposition.**  
  In `src/search.rs` root/child score assembly (~409, ~470), stop relying on only `attack_total.min(attack_cap)`. Use deterministic components from `attack.rs` semantics: continuation value (B2B retention), surge-release timing value, and overkill control.

- **3) Add cheap mobility + downstack-access signals in search loop (not eval-only).**  
  Use existing `MoveBuffer`/`generate(...)` in expansion to derive low-cost optionality metrics (legal-move count proxy + hole access continuity proxy). This gives immediate anti-plonk improvement with low tuning sensitivity.

- **4) Keep eval static features, but add one structural accessibility feature.**  
  In `src/eval.rs`, add a deterministic buried-hole accessibility/path-cost term (small, normalized). This improves defense/downstack quality without needing pattern training.

- **5) Expose reason tags in coaching output and add strategic regression fixtures.**  
  In `src/analysis.rs` + `src/wasm.rs`, emit deterministic reasons (`surge_miss`, `defense_miss`, `opener_break`, `plonk`). In `tests/presim_validation.rs`, add intent-level cases so strategic regressions are caught.

If you want, I’ll produce the **exact minimal implementation sequence** (patch order + touched functions only) before any code edits.
