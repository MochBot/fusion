# Fusion Architecture Guide

TETR.IO Season 2 replay analysis engine in Rust/WASM.

See also:
- `README.md`
- `docs/INTEGRATION.md`
- `docs/DEVELOPMENT.md`

## Crate Structure

```
fusion/
├── crates/
│   ├── core/       # Fundamental types (Board, Piece, Move, GameState)
│   ├── engine/     # Game logic (SRS+, kicks, attack calculation)
│   ├── eval/       # Board evaluation heuristics
│   ├── search/     # Beam search, lookahead
│   ├── analysis/   # Misdrop detection, coaching
│   └── wasm/       # JavaScript bindings
```

## Core Types (`fusion-core`)

| Type | Purpose |
|------|---------|
| `Board` | 10x40 grid using `[u64; 40]` bitfield |
| `Piece` | I, O, T, S, Z, J, L tetrominos |
| `Rotation` | North, East, South, West |
| `Move` | Placement with position, rotation, spin type |
| `SpinType` | None, Mini, Full (All-Mini+ detection) |
| `GameState` | Board + queue + hold + B2B + combo |

## Attack Calculation (`fusion-engine`)

### AttackConfig

Runtime configuration mirroring Triangle.js `EngineInitializeParams`:

```rust
use fusion_engine::{AttackConfig, calculate_attack};
use fusion_core::SpinType;

// Presets (testing only)
let tl = AttackConfig::tetra_league();  // pc=5, surge_base=4
let qp = AttackConfig::quick_play();    // pc=3, surge_base=1

// Production: use replay ruleset options
let config = AttackConfig {
    pc_garbage,
    pc_b2b,
    b2b_chaining,
    b2b_charging: Some(ChargingConfig::new(4, b2b_charging_base)),
    combo_table,
    garbage_multiplier,
};

// Calculate attack
let garbage = calculate_attack(
    lines,      // Lines cleared (0-4)
    spin,       // SpinType::None/Mini/Full
    b2b,        // B2B counter
    combo,      // Combo counter
    &config,    // AttackConfig reference
    is_pc,      // Perfect clear flag
);
```

### S2 Formulas

| Clear Type | Base Attack |
|------------|-------------|
| Single | 0 |
| Double | 1 |
| Triple | 2 |
| Quad | 4 |
| T-Spin Single | 2 |
| T-Spin Double | 4 |
| T-Spin Triple | 6 |
| T-Spin Mini 0-1 | 0 |
| T-Spin Mini 2 | 1 |

**B2B Bonus**: Flat +1 when `b2b > 0` (S2 simple mode)
**PC B2B Bonus**: Uses `pc_b2b` when `is_pc` and `b2b > 0`

**Combo**: `base * (1 + 0.25 * combo)` with soft cap `max(garbage, ln(1 + 1.25 * combo))`

**Perfect Clear**: `config.pc_garbage` (5 TL, 3 QP)

### B2B Surge

```rust
use fusion_engine::{B2BTracker, ChargingConfig};

let mut tracker = B2BTracker::new(false, Some(ChargingConfig::tetra_league()));
let result = tracker.register_clear(lines, is_difficult);

if let Some(surge) = result.surge {
    let [a, b, c] = surge;  // 3-way split
}
```

**Surge Formula**: `floor((btb - at + base + 1) * multiplier)`

**3-way Split**: `[round(s/3), round(s/3), s - 2*round(s/3)]`

## Extending Fusion

### Adding a New Spin Bonus

1. Edit `crates/engine/src/attack.rs`:
```rust
fn base_attack(lines: u8, spin: SpinType) -> f32 {
    match (lines, spin) {
        // Add new spin type here
        (2, SpinType::Full) => 4.0,  // Existing TSD
        _ => // ...
    }
}
```

2. Add test to `crates/engine/tests/parity_matrix.rs`

### Adding a New Combo Table

1. Edit `crates/engine/src/config.rs`:
```rust
pub enum ComboTable {
    Multiplier,  // S2: base * (1 + 0.25 * combo)
    Classic,     // Fixed table
    Modern,      // Fixed table
    None,
    Custom(Vec<u8>),  // Add custom variant
}
```

2. Update `crates/engine/src/combo.rs` to handle new variant

### Adding WASM Bindings

1. Edit `crates/wasm/src/lib.rs`:
```rust
#[wasm_bindgen]
pub fn my_new_function(/* args */) -> JsValue {
    // Implementation
}
```

2. Run `cargo build -p fusion-wasm`

## WASM Usage (JavaScript)

```javascript
import init, { 
    JsAttackConfig, 
    calculateAttack,
    JsBoard,
    findBestMove 
} from 'fusion-wasm';

await init();

// Attack calculation
// Use ruleset options in production
const config = new JsAttackConfig(pcGarbage, pcB2B, b2bChaining, b2bChargingBase, comboTable, garbageMultiplier);
const garbage = calculateAttack(4, 0, 1, 2, config, false);
// 4 lines, no spin, b2b=1, combo=2, not PC

// Board operations
const board = new JsBoard();
board.set(0, 0, true);
const linesCleared = board.clearLines();

// Move search
const best = findBestMove(board, 2);  // T piece
```

## Test Verification

```bash
# Full suite (125 tests)
cargo test --workspace

# Parity matrix only (56 scenarios)
cargo test --test parity_matrix

# Engine tests (38 tests)
cargo test -p fusion-engine
```

## References

- `crates/engine/tests/parity_matrix.rs` - S2 attack parity table used to validate formulas
- [Triangle.js](https://github.com/halp1/triangle) - Reference implementation
