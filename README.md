# Fusion

Rust/WASM analysis backend for TETR.IO Season 2 replays. Fusion complements Mosaic and Triangle.js by providing fast analysis primitives (attack calc, evaluation, search, misdrop detection) while Triangle.js remains the simulation engine.

## Relationship to Mosaic and Triangle.js

| Component | Responsibilities | Not Responsible For |
| --- | --- | --- |
| Triangle.js | Simulation: tick, queue, garbage, replay playback | Analysis, eval, search | 
| Fusion (this repo) | Analysis: attack calc, board eval, search, misdrop detection | Simulation or game loop | 
| Mosaic | UI/visualization, replay ingestion, orchestration | Engine internals | 

Fusion is intentionally **analysis-only**. It is designed to be called from Mosaic alongside Triangle.js, not to replace it.

## Quick Start

```bash
# Build all crates
cargo build --workspace

# Run Rust tests (125)
cargo test --workspace

# WASM tests (14, requires headless Chrome)
wasm-pack test --headless --chrome crates/wasm
```

## Workspace Layout

| Crate | Purpose |
| --- | --- |
| `fusion-core` | Board, Piece, Move, GameState (bitfields) |
| `fusion-engine` | Attack calc, B2B, combo, movement, SRS+ kicks |
| `fusion-eval` | Heuristics (holes, height, wells, bumpiness) |
| `fusion-search` | Beam search + lookahead |
| `fusion-analysis` | Misdrop detection + replay pipeline (advanced) |
| `fusion-wasm` | JS/WASM bindings for browser use |

## WASM API (Core)

```ts
// Initialization
await init();

// Analysis APIs
calculateAttack(lines, spin, b2b, combo, config, isPc): number
evaluate_board(board): number
find_best_move(board, piece): JsMoveResult | null
detect_misdrop(board, piece, playerMove, frame): JsMisdrop | null
```

## Configuration (Production)

Presets are test-only. In production, build config from replay ruleset options:

```ts
const config = new JsAttackConfig(
  pcGarbage,
  pcB2B,
  b2bChaining,
  b2bChargingBase,
  comboTable,
  garbageMultiplier
);
```

Combo table mapping:
- `0` = Multiplier
- `1` = Classic
- `2` = Modern
- `3` = None

## Documentation

- `ARCHITECTURE.md` - Detailed architecture and formulas
- `docs/INTEGRATION.md` - Mosaic/Triangle.js integration guide
- `docs/DEVELOPMENT.md` - Dev setup and workflow

## Non-Goals (Deferred)

- Replacing Triangle.js simulation
- Advanced coaching heuristics and UI features
- Deep search tuning beyond 3-ply

## License

MIT
