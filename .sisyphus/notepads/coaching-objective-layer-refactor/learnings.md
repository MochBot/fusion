## Baseline Findings (Task 1)
- Current search configuration: beam_width=800, depth=14, futility_delta=5.5.
- Current coaching sigmoid parameters: SIGMOID_K=0.15, SIGMOID_C_BASE=-13.5.
- Codebase is healthy: 132/132 tests passing, clippy clean (native + wasm).
- Baseline total line count for core files: 2466.
## MoveEvalResultJson Extension (T6)
- Extended `MoveEvalResultJson` struct in `src/wasm.rs` with composite score placeholder fields:
  - `board_score: f32`
  - `attack_score: f32`
  - `chain_score: f32`
  - `context_score: f32`
  - `insight_tags: Vec<String>`
- All new fields are initialized with safe defaults (0.0 or empty vec) in `evaluate_position_wasm`.
- Struct uses `#[derive(serde::Serialize, serde::Deserialize)]` for automatic JSON serialization to the JS consumer.
- Verified that `wasm32-unknown-unknown` target clippy passes with these changes.
## Board Construction Patterns
- Board uses row bitmasks (u16 per row, 10 columns).
- Y-up convention: row 0 is the bottom.
- Helper `board_from_bottom_rows` in `tests/presim_validation.rs` provides a clean way to setup deterministic states.
- Piece piece queues are passed as `Vec<Piece>` to `GameState::new`.

## Composite Scoring Helpers (T4)
- Added in `src/analysis.rs`:
  - `pub fn shape_chain_value(raw_chain: f32) -> f32`
  - `pub fn shape_context_modifier(raw_modifier: f32) -> f32`
  - `pub fn assemble_composite(board: f32, attack: f32, chain: f32, context: f32, config: &SearchConfig) -> f32`
  - `pub enum InsightTag { AttackWindowMiss, ChainBreak, DownstackEfficiencyMiss }`
  - `pub struct InsightResult { pub tag: InsightTag, pub severity: f32, pub delta: f32 }`
- Chain shaping design: bounded concave transform `1 - exp(-0.25 * raw_chain)` clamped to `[0.0, 1.0]` to enforce diminishing returns and prevent runaway chain dominance.
- Context shaping design: direct clamp to `[-1.0, 1.0]` so situational modifiers cannot explode composite magnitude.
- Composite assembly formula implemented as weighted sum over board/attack/chain/context channels; weight access is currently isolated through helper functions in `analysis.rs` pending T2 weight-field availability in `SearchConfig`.

## T2 - SearchConfig Surface Addition
- **Fields Added**: attack_weight (0.25), chain_weight (0.15), context_weight (0.10), board_weight (1.0).
- **Construction Pattern**: All identified initialization sites for `SearchConfig` (`src/search.rs`, `src/wasm.rs`, `tests/presim_validation.rs`) use functional update syntax `..SearchConfig::default()`.
- **Verification**: `cargo check` and `clippy` pass without requiring manual updates to construction sites, confirming the safety of the additive change.
- **Role**: These fields will serve as the single source of truth for composite scoring coefficients in tasks T7-T11.

## Search Struct Extension (T3)
- SearchNode gained: board_score, attack_score, chain_score, context_score, path_attack
- SearchResultFull gained: board_score, attack_score, chain_score, context_score
- All fields initialized to 0.0 at construction sites in search.rs and search_expand.rs
- Design preserves TT-safety by separating board_score from path terms.
## Search Struct Extension (T3)
- SearchNode gained: board_score, attack_score, chain_score, context_score, path_attack
- SearchResultFull gained: board_score, attack_score, chain_score, context_score
- All fields initialized to 0.0 at construction sites in search.rs and search_expand.rs
- Design preserves TT-safety by separating board_score from path terms.
### Composite Scoring Rollout
- Enabled `check_spin: true` in `generate()` for both root and depth expansion to support spin-aware composite scoring.
- Added `is_empty` method to `Board` for perfect clear detection during search.
- Added `config: &'a SearchConfig` to `SearchExpansionContext` to support coefficient-driven scoring and attack configuration.
- Successfully implemented composite scoring in `SearchNode` replacing raw board_eval:
  - `board_eval` continues to be TT-cached exclusively
  - `attack_val`, `chain_val`, and `context_mod` are added to the search `score` outside the TT
- `path_attack` accumulates `parent.path_attack + attack_val` in `expand_node`
- Explicit type casting: `next_combo` comes from `u32` but `calculate_attack` requires `u8`, resolved via `next_combo as u8` casting just for attack calc.
- `CoachingState` does not expose `context_modifier()` directly, so `0.0` is used for context channel fallback per spec.
