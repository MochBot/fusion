# V6 Eval Rework — Coaching-First Architecture

**Created**: 2026-02-23
**Status**: Planning — not yet started
**Snapshot**: V5 commit `tuyotupz` (cc523761)

---

## The Coaching Problem (Not an Engineering Problem)

### What Coaching Actually Is

Coaching isn't "find the optimal move and measure distance from it." That's an engine.

Coaching works when:
1. **You show consequences** — "if you'd played here instead, this 12-line spike happens at piece 7"
2. **You match the player's level** — a D-rank needs "stop covering garbage holes" not "you missed a TST donation setup"
3. **You explain in terms of intent** — "you chose safety when you had a kill opportunity" not "score was 2.3 below optimal"
4. **You identify patterns** — "60% of your blunders happen when your stack exceeds row 12" not one-off red dots

### What Winning Looks Like in Competitive Tetris

It's NOT "make the flattest board." It's:

| Concept | What It Means | Current Engine Awareness |
|---------|---------------|------------------------|
| **Damage windows** | Recognize opponent vulnerability → attack NOW | ❌ Zero — board-shape-only scoring |
| **Resource management** | I-pieces for Tetrises, T-pieces for spins — don't waste on flat fills | ❌ Zero — I-piece flat fill scores same as Tetris |
| **Tempo (B2B/Combo)** | Maintain chains because they multiply damage logarithmically | ❌ Zero — B2B tracked as metadata, never scored |
| **Survival timing** | Sacrifice attack potential to not die | ❌ Partially — height penalty exists but not context-aware |
| **Setup investment** | Accept "messy" for 2-3 pieces to unlock massive damage at piece 4-5 | ❌ Actively penalized — messy = bad eval = pruned |
| **Downstack quality** | Clear lines that expose next garbage hole, don't cover them | ❌ Zero — no garbage hole awareness |
| **Bag efficiency** | Plan sequences around known bag contents (7-bag system) | ❌ Queue extended but pieces treated independently |

### The Current Pipeline Failure

```
Player makes move → Search finds "best" 14-move sequence → Compare

But "best" means "flattest board in 14 moves"
NOT "most damage" or "cleanest survival" or "best setup"

Result: Engine says L-piece is worse than J-piece
        because J makes a flatter board
        even though L funnels into a T-spin double next turn
```

The search is 14 moves deep with 800-wide beam. That's enormous compute. **But the objective function (what it optimizes) is wrong.** The search amplifies eval quality — it can't fix an eval that's blind to strategy.

---

## Research Findings: How Good Coaching Engines Work

### Chess.com Depth Discrepancy ("Brilliant" Detection)
Shallow search (depth 10) hates the move. Deep search (depth 20+) loves it. The move requires **calculation beyond intuition** — it "looks bad" but resolves into a winning position.

**Tetris mapping**: T-spin setup looks bad at depth 2 (creates overhang = holes + bumpiness penalty). At depth 14, the overhang resolves into 6 lines of garbage sent. Current engine prunes this at depth 3 because `evaluate()` sees "messy board."

### Lc0 Policy vs Search ("Intuition Trap")
High prior (policy network says the move is natural) + low posterior (search reveals it's bad). This is the most valuable coaching signal: **"this move FEELS right but IS wrong."**

**Tetris mapping**: Placing a piece that flattens the stack (visually clean) but covers a dependency needed 3 moves later. "Natural mistake: looks clean but starves your queue."

### Maia Chess Behavioral Cloning (Rank-Appropriate Coaching)
9 models per 100-Elo band, trained to PREDICT what a player at that level would play. Surprise = -log(P(move|model)). Instead of "you should have played the engine move," it's "a player at your level wouldn't have done that."

**Tetris mapping**: Recommending X+-level TST donation setups to a B-rank player is mathematically optimal but pedagogically useless. The coaching engine needs rank-aware expectations. We partially have this (PlayerSkill → sigmoid_c calibration), but it only affects severity thresholds — it doesn't change what the search considers "good."

### MochBot "No Eval, Just Presim"
Trivial static eval, depth 12-20. Deep search **naturally discovers** penalties because bad moves constrain the board 12+ moves later. The consequences become visible as actual failed placements / forced misdrops / death.

**Key insight**: Show CONSEQUENCES not HEURISTIC VIOLATIONS. "14 moves later, you can't place the I-piece anywhere" is more compelling than "bumpiness score increased by 2.3."

### Deliberate Practice Psychology
- Passive game review (red/green dots) ≠ deliberate practice
- Effective coaching: aggregate across games, identify the ONE thing they do wrong most often
- Load specific scenarios from past games for targeted practice
- Pattern recognition: "you always panic-downstack by covering holes when stack > row 12"

---

## Rank-Specific Coaching Priorities

What the engine needs to detect and flag, by rank:

### D-C Rank (<1.0 PPS, <20 APM)
- **Primary**: Stop covering holes during downstack (BLUNDER-level)
- **Secondary**: Reduce bumpiness, avoid I-piece dependencies for basic clears
- **Don't penalize**: B2B breaks, inefficient piece usage, slow play
- **Engine requirement**: Garbage hole visibility in eval — "this move covers the garbage hole you need to expose"

### B-A Rank (1.0-1.4 PPS, 20-40 APM)
- **Primary**: Recognize and execute basic T-spin doubles
- **Secondary**: 9-0 / 6-3 stacking structure, one opener
- **Penalize**: Missing obvious TSD setups when T-piece available
- **Engine requirement**: Spin setup recognition — search must see TSD resolving at depth 3-5

### S-SS Rank (1.5-2.0 PPS, 50-80 APM)
- **Primary**: B2B chain maintenance — never break B2B for unnecessary singles ("skimming")
- **Secondary**: Opener proficiency, queue reading, attack timing
- **Strongly penalize**: Dropping B2B for panic clears that don't address survival
- **Engine requirement**: B2B value in scoring — breaking B2B must have measurable cost

### U-X+ Rank (2.5-3.5+ PPS, 120-170+ APM)
- **Primary**: Timing (attack into vulnerable opponent, not into ready opponent)
- **Secondary**: Cheese survival, efficiency optimization, spike opportunities
- **Penalize**: Attacking into ready opponent ("tanking"), missing spike windows
- **Engine requirement**: Opponent context in scoring — this is the opponent-as-environment layer

---

## What V5 Got Right (Keep)

### 1. Classification Pipeline ✅
Sigmoid win-prob-drop → Severity enum. Rank-aware sigmoid C calibration. Coaching state multipliers on ΔP. `classify_major_first` override for lethal negligence. **Keep entirely.** Inputs to it change; logic stays.

### 2. Coaching State Machine ✅
`CoachingState` with Fatality/Obligation/Surge/Phase tracking via `TransitionObservation`. **Keep and PROMOTE** from metadata-only to scoring-participant.

### 3. Root Move Forcing ✅
`find_best_move_with_scores_forced` ensures player's actual move survives beam pruning. **Keep.**

### 4. Beam Search Structure ✅
800-wide, 14-deep with futility pruning, TT, CABS. The structure is correct. **Keep. Change the objective function.**

### 5. WASM Pipeline ✅
Plumbing between Rust and frontend. **Keep. Expand output fields.**

### 6. Position Complexity ✅
Variance of top-10 root_scores. **Keep. Becomes more meaningful with better scoring.**

### 7. Attack Infrastructure ✅
`calculate_attack()` in attack.rs — pure arithmetic, no board access, cheap. Already computes exact TETR.IO S2 damage with B2B chaining, combo multipliers, PC bonus, spin bonuses, surge release, garbage clear boost. **Currently unused by search. Wire it in.**

---

## What V5 Gets Wrong (Must Change)

### Core Problem: Search Objective Function

**Current**: `SearchNode.score = evaluate(board)` — 9-feature board shape heuristic.

The beam search optimizes for "flattest board in 14 moves." It is **blind to**:

1. **Attack output** — T-spin triples, Tetrises, combos generate zero score
2. **Combo chains** — 10-combo that sends 20 garbage lines = same score as flat fill
3. **B2B maintenance** — keeping B2B alive for damage multipliers is invisible
4. **Spin setup quality** — T-spin slot scores WORSE than flat fill (overhang = holes + bumpiness)
5. **Bag efficiency** — wasting I/T pieces on flat fills is indistinguishable from using them for attacks
6. **Piece dependencies** — "this placement only works if I get S next" is not modeled
7. **Downstack quality** — covering vs exposing garbage holes during cheese clearing

### Secondary Problem: `check_spin=false`

`search_expand.rs` hardcodes `check_spin=false` in `generate()`. No Full T-spin detection during search. Can't even identify spin setups, let alone score them. Mini spins detected via different path but Full T-spin is the bread-and-butter of competitive play.

### Consequence

A T-spin triple setup gets **pruned at depth 3** because it looks "messy" to `evaluate()`. The search never discovers that the mess resolves into 6+ lines of garbage. The 14-move depth is wasted because the objective function can't see what matters.

---

## V6 Design: Path-Aware Scoring

### Principle: Context and Time Flow is King

A move's value depends on the ENTIRE PATH through the search tree:
- What came before (board state, b2b/combo chain, coaching state)
- What the move accomplishes (attack, setup, survival)
- What it enables afterward (remaining bag pieces, chain continuation)

Pieces exist in sequences — bags, not isolation. Scoring must reflect this.

### New Scoring Formula

```
node_score = α × board_eval(leaf_board)           // survival (shape quality)
           + β × path_attack_total                 // damage (cumulative attack along path)
           + γ × chain_value(b2b, combo)           // momentum (chain maintenance reward)
           + δ × context_modifier(coaching_state)  // situational (surge/fatality/obligation)
```

| Component | What It Measures | Source |
|-----------|-----------------|--------|
| `board_eval` | Board shape quality at leaf (survival) | Existing `evaluate()` — unchanged |
| `path_attack_total` | Total garbage lines sent along entire search path | `calculate_attack()` per expansion step — already exists, just unwired |
| `chain_value` | B2B streak length + combo chain length at leaf | `SearchNode.b2b` and `.combo` — already tracked |
| `context_modifier` | Situation-appropriate adjustment | `CoachingState` — already tracked as metadata |

### What This Fixes

| Scenario | V5 (board-only) | V6 (path-aware) |
|----------|-----------------|-----------------|
| T-spin triple setup | Pruned at depth 3 (messy board) | Survives — 6+ attack along path |
| 10-combo chain | Same score as flat fill | Massively higher — cumulative 15+ attack |
| B2B Tetris → T-spin | No preference | Rewarded — chain_value for maintained B2B |
| Downstack during Fatal | No urgency | Context modifier rewards clearing |
| I-piece Tetris vs flat fill | Indistinguishable | Tetris sends 4 lines → attack bonus |
| Panic singles breaking B2B | Same as efficient clears | Penalized — chain_value drops to zero |
| T-spin setup (2-piece investment) | Penalized (messy intermediate) | Rewarded — depth 5 shows 4+ attack payoff |

### How Coaching Changes

**V5 coaching**: "Your move scored 0.15 win-prob below optimal" (meaningless to player)

**V6 coaching**: The search now finds sequences that maximize damage. When the player's move diverges from the best sequence, the coaching engine can show:
- "Best line: T-spin double at piece 3 → Tetris at piece 5 → 10 lines sent"
- "Your line: flat fill → flat fill → single clear → 0 lines sent"
- "Lost opportunity: 10 lines of attack damage over 5 pieces"

The difference isn't just a number — it's a **narrative** the player can understand and learn from.

---

## Implementation Phases

### Phase 1: Wire Attack into Search (Minimum Viable)
**Goal**: SearchNode.score includes cumulative attack along path.

1. Enable `check_spin=true` for T-pieces in `generate()` calls within `search_expand.rs`
2. In `expand_node`: call `calculate_attack(lines, spin, b2b, combo, config, is_pc)` after `do_move`
3. Add `path_attack: f32` to `SearchNode`
4. Score = `evaluate(board) + 0.3 × path_attack`
5. Root_scores now reflect attack potential, not just board shape

**Files**: `search_expand.rs`, `search_config.rs`
**Risk**: check_spin increases T-piece branching (~2× for T positions). calculate_attack is cheap (arithmetic only).
**Verification**: Beam search still runs within time budget. T-spin sequences survive pruning.

### Phase 2: Chain Value
**Goal**: Reward maintaining B2B and combo chains.

1. Add `chain_value: f32` to `SearchNode`
2. After expansion: `chain_value = b2b_bonus(b2b) + combo_bonus(combo)` where bonus increases non-linearly
3. Score = `board_eval + 0.3 × path_attack + 0.2 × chain_value`

**Files**: `search_expand.rs`, `search_config.rs`
**Risk**: Over-weighting chains causes bot to sacrifice board state for marginal B2B maintenance. Start conservative.

### Phase 3: Context Modifier (Coaching State in Search)
**Goal**: Search behavior adapts to game situation.

1. During Surge (Active): increase attack weight (β → 0.5) — press the advantage
2. During Fatal: increase survival weight (α → 1.5), decrease attack weight (β → 0.1) — survive first
3. During Obligation (MustDownstack): reward moves that reduce height, penalize moves that attack without clearing
4. During Build (opponent clean): reward setup investment, dampen attack urgency

**Files**: `search.rs` (weight modulation), `search_expand.rs` (scoring adjustment)
**Risk**: Complex interaction between coaching state and search scoring. Test each modifier independently.

### Phase 4: Recalibration
**Goal**: All classification thresholds work with new scoring distribution.

1. Root_scores now have different scale (board_eval ≈ -15 to 0, path_attack ≈ 0 to 20+, chain_value ≈ 0 to 5)
2. Retune SIGMOID_K, SIGMOID_C_BASE, classification thresholds
3. Re-run corpus calibration (D/S/X+ severity distributions)
4. Verify: X+ ≥60% None ≤5% Blunder, D ≥15% M+B, monotonic worsening

**Files**: `analysis.rs`, `tests/presim_validation.rs`
**Risk**: Highest risk phase. Wrong calibration makes everything worse. Keep V5 calibration tests as regression baseline.

### Phase 5: Enhanced Coaching Output (Stretch)
**Goal**: Frontend can show attack narratives, not just severity.

1. Add `path_attack_total: f32` to `MoveEvalResultJson` — how much damage the best sequence produces
2. Add `best_path_attacks: Vec<(usize, f32)>` — at which depths the best line generates attack
3. Add `actual_path_attacks: Vec<(usize, f32)>` — same for the player's actual line
4. Frontend renders: "Best line sends 10 lines over 5 moves. You sent 0."

**Files**: `wasm.rs`, `search_config.rs`
**Risk**: Adds data to WASM output but no structural risk.

---

## Files Affected

| File | Change | Phase |
|------|--------|-------|
| `src/search_expand.rs` | Enable check_spin for T, call calculate_attack, accumulate path_attack | 1 |
| `src/search_config.rs` | Add path_attack, chain_value to SearchNode | 1-2 |
| `src/search.rs` | Blended scoring in sort/truncate/root_scores, context weight modulation | 1-3 |
| `src/eval.rs` | **Unchanged** — board eval stays as survival component | — |
| `src/attack.rs` | **Unchanged** — already exists, just called during search now | — |
| `src/state.rs` | Add scoring modifier methods to CoachingState | 3 |
| `src/analysis.rs` | Recalibrate thresholds, update sigmoid constants | 4 |
| `src/wasm.rs` | New output fields for path attack data | 5 |
| `tests/presim_validation.rs` | New calibration tests, regression tests for path scoring | 4 |

---

## Risk Assessment

1. **Performance**: `calculate_attack()` per expansion step. With 800 × 14 × ~35 branching = ~400K nodes, this adds ~400K arithmetic calls. `calculate_attack` is ~20 ops (no branching, no allocation). Cost: negligible (<1ms total).

2. **T-spin branching**: `check_spin=true` for T-pieces increases move count per T-piece position (spin + non-spin variants of same physical position). Branching factor for T-piece ply increases ~1.5-2×. With 1/7 chance of T-piece per depth, overall beam search cost increases ~7-14%. Acceptable.

3. **Calibration chaos**: All classification thresholds tuned for board-only scores. New scoring has fundamentally different distribution. **Must retune everything.** Keep V5 calibration infrastructure.

4. **Weight balancing**: α/β/γ/δ are new hyperparameters. Over-weighting attack (β too high) → bot sacrifices survival for marginal damage. Over-weighting chains (γ too high) → bot extends worthless B2B via singles. **Start conservative, iterate with corpus data.**

5. **Coaching state in search (Phase 3)**: Most complex — coaching state modulates search weights, which changes which moves survive, which changes coaching state transitions. Potential feedback loop. **Test each modifier in isolation.**

---

## What V6 Does NOT Address (Deferred to V7+)

- **NNUE / neural net eval**: Replacing `evaluate()` with trained network. Requires training infrastructure + replay data pipeline. The path-scoring framework is compatible — just swap `board_eval` component.
- **Opponent modeling**: Search scoring based on opponent's board state. Currently opponent data is environment context only. Would require dual-board search or opponent prediction model.
- **Behavioral cloning (Maia-style)**: Rank-specific prediction models trained on TTRM replays. Requires ML pipeline. Would enable "a player at your level wouldn't do that" coaching.
- **CMA-ES weight optimization**: Automated tuning of α/β/γ/δ. Requires eval harness + runtime infrastructure.
- **Piece dependency eval**: Explicitly scoring whether current placement creates or blocks dependencies for upcoming bag pieces. Complex — requires bag-state-aware lookahead beyond queue extension.
- **Garbage hole awareness in eval**: Adding "does this placement cover/expose a garbage hole" as an eval feature. Important for downstack coaching but requires garbage layer tracking in Board.
