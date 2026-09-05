//! Move-encoding FFI - single source of truth for Move.raw u16 across languages.
//!
//! Problem this module solves:
//!
//!   Python's `policy_value_schema.encode_move_raw` and Rust's
//!   `header::Move::new(...).raw()` independently pack a 16-bit field. The
//!   formulas agree, but the input *semantics* diverged historically:
//!     - Python preprocess passes (x, y) as the BOUNDING BOX origin in
//!       matrix coordinates with row-increasing-down.
//!     - Rust expects (x, y) as the PIVOT coordinate in game-engine
//!       coordinates with y-increasing-up.
//!
//!   When the two disagreed, the dataloader silently failed to match
//!   `actual_move_raw` against any search-oracle candidate (0% match across
//!   50k samples) and `player_policy_loss` collapsed to zero in every
//!   training run. Both production checkpoints were therefore trained on
//!   search supervision only, not human imitation.
//!
//! Fix:
//!
//!   Make Rust the authoritative encoder. This module exports a C ABI
//!   function callable from Python via `ctypes`, so Python preprocess (and
//!   any other consumer) can stop maintaining its own copy of the bit
//!   packing.
//!
//!   Input convention: PIVOT (x, y) in Rust/game-engine coordinates. Any
//!   converter that produces bbox-origin coordinates must transform them
//!   to pivot coordinates BEFORE calling this function. The bbox→pivot
//!   table is derived empirically against this oracle (see
//!   `training/tests/test_move_encoding_parity.py`).
//!
//! Symbol exposed:
//!   `fusion_encode_move_raw(piece_id, rotation, x, y, spin) -> i32`
//!   where piece_id ∈ {0..=6} (Rust Piece enum order: I O T L J S Z),
//!   rotation ∈ {0..=3} (N E S W), x,y are i32 pivot coords, spin ∈ {0,1}.
//!   Returns the u16 raw widened to i32 on success, or -1 on invalid input.
//!   The signed return is the sentinel mechanism: every valid u16 fits in
//!   the non-negative range of i32, so a negative result unambiguously
//!   signals an error. (An earlier u16-returning design hit a collision:
//!   `Move::new(I, West, 15, 63, spin=true)` encodes to 0xFFFF, which is
//!   the same bit pattern as `u16::MAX` - the original sentinel.)

use crate::header::{Move, Piece, Rotation};

/// Pure-Rust encoder. Use from Rust tests and from internal call sites.
///
/// Returns `Err(EncodeMoveRawError)` if any input is out of range. The
/// validation matches `Move::new`'s implicit assumptions (Piece in 0..7,
/// Rotation in 0..4, x fits in 4 bits, y fits in 6 bits, spin in {0,1}).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeMoveRawError {
    InvalidPiece(u8),
    InvalidRotation(u8),
    InvalidSpin(u8),
    XOutOfRange(i32),
    YOutOfRange(i32),
}

pub fn encode_move_raw(
    piece_id: u8,
    rotation: u8,
    x: i32,
    y: i32,
    spin: u8,
) -> Result<u16, EncodeMoveRawError> {
    if piece_id > 6 {
        return Err(EncodeMoveRawError::InvalidPiece(piece_id));
    }
    if rotation > 3 {
        return Err(EncodeMoveRawError::InvalidRotation(rotation));
    }
    if spin > 1 {
        return Err(EncodeMoveRawError::InvalidSpin(spin));
    }
    // Move::new masks x with 0xF and y with 0x3F. Reject inputs that would
    // be silently truncated so callers see encoding bugs instead of
    // collisions. (Pivot coords always fit on a 10×40 board.)
    if !(0..=0x0F).contains(&x) {
        return Err(EncodeMoveRawError::XOutOfRange(x));
    }
    if !(0..=0x3F).contains(&y) {
        return Err(EncodeMoveRawError::YOutOfRange(y));
    }

    let piece = Piece::from_u8(piece_id);
    let rot = Rotation::from_u8(rotation);
    let fullspin = spin != 0;
    Ok(Move::new(piece, rot, x, y, fullspin).raw())
}

/// C ABI wrapper for FFI consumers (Python via ctypes).
///
/// Returns the u16 raw value widened to i32 on success, or
/// `FUSION_ENCODE_MOVE_RAW_ERROR` (-1) on invalid input. Negative is an
/// unambiguous sentinel because every valid raw fits in 0..=0xFFFF, the
/// non-negative range of i32.
///
/// # Safety
///
/// Safe to call from any thread. No raw pointers cross the boundary.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn fusion_encode_move_raw(
    piece_id: u8,
    rotation: u8,
    x: i32,
    y: i32,
    spin: u8,
) -> i32 {
    match encode_move_raw(piece_id, rotation, x, y, spin) {
        Ok(raw) => raw as i32,
        Err(_) => FUSION_ENCODE_MOVE_RAW_ERROR,
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub static FUSION_ENCODE_MOVE_RAW_ERROR: i32 = -1;

#[cfg(test)]
mod tests {
    use super::*;

    /// Direct cross-check: every valid (piece, rotation, x, y, spin)
    /// encoded via the FFI wrapper matches Move::new(...).raw() directly.
    /// This is the property-based parity test.
    #[test]
    fn ffi_matches_move_new_for_every_valid_input() {
        for piece_id in 0u8..=6 {
            for rotation in 0u8..=3 {
                for x in 0i32..=0x0F {
                    for y in 0i32..=0x3F {
                        for spin in 0u8..=1 {
                            let ffi = fusion_encode_move_raw(piece_id, rotation, x, y, spin);
                            let direct = Move::new(
                                Piece::from_u8(piece_id),
                                Rotation::from_u8(rotation),
                                x,
                                y,
                                spin != 0,
                            )
                            .raw();
                            assert!(ffi >= 0, "FFI returned sentinel for valid input");
                            assert_eq!(
                                ffi as u16, direct,
                                "FFI/direct mismatch at piece={} rot={} x={} y={} spin={}: \
                                 ffi=0x{:04x} direct=0x{:04x}",
                                piece_id, rotation, x, y, spin, ffi, direct
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn ffi_returns_sentinel_for_high_corner_case() {
        // Move::new(I, West, 15, 63, fullspin=true) encodes to 0xFFFF.
        // Confirm the new i32 sentinel does NOT collide with this value.
        let ffi = fusion_encode_move_raw(Piece::I as u8, Rotation::West as u8, 15, 63, 1);
        assert_eq!(ffi, 0xFFFF);
        let direct = Move::new(Piece::I, Rotation::West, 15, 63, true).raw();
        assert_eq!(ffi as u16, direct);
        assert_ne!(ffi, FUSION_ENCODE_MOVE_RAW_ERROR);
    }

    #[test]
    fn ffi_rejects_invalid_piece() {
        assert_eq!(
            fusion_encode_move_raw(7, 0, 0, 0, 0),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
        assert_eq!(
            fusion_encode_move_raw(255, 0, 0, 0, 0),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
    }

    #[test]
    fn ffi_rejects_invalid_rotation() {
        assert_eq!(
            fusion_encode_move_raw(0, 4, 0, 0, 0),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
    }

    #[test]
    fn ffi_rejects_out_of_range_coords() {
        assert_eq!(
            fusion_encode_move_raw(0, 0, -1, 0, 0),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
        assert_eq!(
            fusion_encode_move_raw(0, 0, 16, 0, 0),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
        assert_eq!(
            fusion_encode_move_raw(0, 0, 0, -1, 0),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
        assert_eq!(
            fusion_encode_move_raw(0, 0, 0, 64, 0),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
    }

    #[test]
    fn ffi_rejects_invalid_spin() {
        assert_eq!(
            fusion_encode_move_raw(0, 0, 0, 0, 2),
            FUSION_ENCODE_MOVE_RAW_ERROR
        );
    }

    #[test]
    fn known_values_round_trip_through_move() {
        let cases = [
            (Piece::T, Rotation::East, 5i32, 10i32, false),
            (Piece::I, Rotation::North, 0, 0, false),
            (Piece::Z, Rotation::West, 9, 39, false),
            (Piece::T, Rotation::South, 4, 18, true),
        ];
        for (piece, rotation, x, y, fullspin) in cases {
            let raw = fusion_encode_move_raw(piece as u8, rotation as u8, x, y, fullspin as u8);
            assert!(raw >= 0, "sentinel returned for valid input");
            let m = Move::new(piece, rotation, x, y, fullspin);
            assert_eq!(raw as u16, m.raw());
            if !fullspin {
                assert_eq!(m.piece(), piece);
            }
            assert_eq!(m.rotation(), rotation);
        }
    }
}
