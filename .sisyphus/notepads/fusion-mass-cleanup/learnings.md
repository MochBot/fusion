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

## 2026-02-20 Task 3: Replace transmute with safe from_u8 conversions
- Added `const fn from_u8(v: u8) -> Self` match-based methods to Piece, Rotation, SpinType in header.rs
- All 15 transmute calls replaced: header.rs(3), gen.rs(4), movegen.rs(8)
- Every transmute was the sole unsafe operation in its block — all 15 unsafe blocks removed
- Unsafe count: 17 → 2 (remaining: board.rs get_unchecked_mut, movegen.rs get_unchecked_mut)
- Transmute count: 15 → 0
- Piece variants: I=0, O=1, T=2, L=3, J=4, S=5, Z=6 (note L=3/J=4, not what task description said)
- from_u8 panics on invalid discriminants — matches transmute's UB-on-invalid behavior but safely
- All methods are const fn — required because Move::piece(), Move::rotation(), Move::spin() are const fn
- Tests: 145 pass, 0 fail
- Clippy transmute warnings: 0 (eliminated missing_transmute_annotations lints)
- Pre-existing clippy style lints (49) left for T4/T8

## 2026-02-20 Task 4: Fix clippy lints in gen.rs and board.rs
- gen.rs: Fixed `needless_range_loop` by using iterator-based access in `CollisionMap`.
- gen.rs: Fixed `needless_late_init` by refactoring `lane` assignment.
- gen.rs: Removed unnecessary casts to `usize` for `COL_NB` and `ROTATION_NB`.
- board.rs: Verified standalone `impl fmt::Display for Board` exists and `inherent_to_string` is removed.
- board.rs: Verified no unnecessary casts remain.
- Clippy status: 0 style lints in gen.rs and board.rs (remaining 23 are in movegen.rs).
- Tests: 145 pass (131 lib + 14 integration), 0 fail.
