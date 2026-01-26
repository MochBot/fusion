# Fusion Integration Guide (Mosaic + Triangle.js)

Fusion is an analysis backend. Triangle.js remains the simulation engine. Mosaic orchestrates both.

## Responsibilities

| Component | Owns | Does Not Own |
| --- | --- | --- |
| Triangle.js | tick(), queue, garbage, replay playback | analysis, search, eval |
| Fusion WASM | attack calc, board eval, search, misdrop detection | simulation loop |
| Mosaic | UI, replay ingestion, data flow | engine internals |

## Data Flow (Recommended)

1. Mosaic loads a replay and builds Triangle.js state for simulation.
2. Mosaic extracts ruleset options from replay metadata.
3. Mosaic creates a Fusion `JsAttackConfig` from those options.
4. Mosaic calls Fusion for analysis on demand (not every tick).

## Minimal Integration Steps

```ts
import init, { JsAttackConfig, calculateAttack, evaluate_board, find_best_move } from 'fusion-wasm';

await init();

const config = new JsAttackConfig(
  pcGarbage,
  pcB2B,
  b2bChaining,
  b2bChargingBase,
  comboTable,
  garbageMultiplier
);

const attack = calculateAttack(lines, spin, b2b, combo, config, isPc);
const score = evaluate_board(board);
const best = find_best_move(board, piece);
```

## Ruleset Mapping

Use replay options to build config dynamically:

| Option | Fusion Field |
| --- | --- |
| allclear_garbage | pcGarbage |
| allclear_b2b | pcB2B |
| b2bchaining | b2bChaining |
| b2b_charging_base | b2bChargingBase |
| combotable | comboTable |
| garbagemultiplier | garbageMultiplier |

Combo table mapping:
- `0` = Multiplier
- `1` = Classic
- `2` = Modern
- `3` = None

Recommended: keep a small adapter in Mosaic (like `createAttackConfig`) that
maps TTRM replay options to `JsAttackConfig` so presets stay test-only.

## When to Call Fusion

- Attack calculation (fast, safe per move)
- Board evaluation (when ranking moves)
- Best move search (per piece or per decision point)
- Misdrop detection (post-move analysis)

Avoid calling Fusion every simulation tick unless profiling proves it is safe.

## Anti-Patterns

- Replacing Triangle.js simulation with Fusion
- Hardcoding `tetraLeague()`/`quickPlay()` in production
- Running search on every frame

## Deferred (Non-MVP)

- Full replay analysis pipeline
- Advanced coaching heuristics
- Deep search tuning beyond 3-ply

## References

- `ARCHITECTURE.md` for formulas and crate details
- [Triangle.js](https://github.com/halp1/triangle) reference implementation (Mosaic uses it for simulation)
