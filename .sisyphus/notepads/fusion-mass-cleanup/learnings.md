## 2026-02-20 Task 1: Baseline Snapshot
- Commit: f749dea8 "pre-mass-cleanup: snapshot with plan and previous cleanup changes"
- Tests: 145 pass, 0 fail, 4 ignored
- Metrics: 0 #[allow], 19 unsafe, 15 transmute, 8721 LOC
- Clippy: 55 errors (2 UB + 53 style)

## 2026-02-20 Task 2: MaybeUninit UB Fix
- MoveBuffer: `[Move::none(); MAX_MOVES]` replaces `MaybeUninit::uninit().assume_init()`
  - Move is Copy, Move::none() is const fn → zero-cost safe init
- spin_set: unconditional `[[[0u64; SPIN_NB]; ROTATION_NB]; COL_NB]` replaces conditional UB path
  - Compiler eliminates dead stores when CHECK_SPIN=false anyway
- UB lint count: 0 (was 2)
- Remaining clippy: 51 style errors (separate tasks)
- All 3 enums (Piece, Rotation, SpinType) have #[repr(u8)] — T3 can use match-based conversion
- Tests: 145 pass, 0 fail, 4 ignored (unchanged)
