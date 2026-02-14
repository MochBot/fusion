//! fusion-engine - TETR.IO game logic and simulation engine.
//!
//! Provides SRS+ rotation, kicks, movement, and move generation.

pub mod apply;
pub mod attack;
pub mod b2b;
pub mod bag;
pub mod collision;
pub mod collision_map;
pub mod collision_specialized;
pub mod combo;
pub mod config;
pub mod garbage;
pub mod gravity;
pub mod kicks;
pub mod move_list;
pub mod movegen_bitboard;
pub mod movegen_ssa;
pub mod movement;
pub mod perft;
pub mod row_board;
pub mod tt;
pub mod validity_mask;

pub use apply::apply_move;
pub use attack::calculate_attack;
pub use b2b::{B2BResult, B2BTracker, ChargingConfig};
pub use collision::{can_place, collides, hard_drop_y};
pub use combo::{apply_combo_multiplier, COMBO_BONUS};
pub use config::{AttackConfig, ComboTable};
pub use garbage::IncreaseTracker;
pub use gravity::GravityConfig;
pub use kicks::get_kicks;
pub use move_list::MoveList;
pub use movegen_ssa::{count_moves_ssa, generate_moves_ssa};

// Backward-compatible aliases for search crate
pub use movegen_ssa::generate_moves_ssa as generate_moves;

/// Generate moves considering hold piece swap
/// Returns moves for current piece, plus moves after swapping with hold
pub fn generate_moves_with_hold(
    board: &fusion_core::Board,
    current: fusion_core::Piece,
    hold: Option<fusion_core::Piece>,
    queue: &[fusion_core::Piece],
) -> Vec<fusion_core::Move> {
    let mut moves = generate_moves_ssa(board, current);

    // If we can use hold, also generate moves for the held/swapped piece
    if let Some(hold_piece) = hold {
        // Swap with existing hold piece
        let mut hold_moves = generate_moves_ssa(board, hold_piece);
        for mv in &mut hold_moves {
            mv.hold_used = true;
        }
        moves.extend(hold_moves);
    } else if let Some(&first_queue) = queue.first() {
        // No hold piece yet - hold current, play from queue
        let mut queue_moves = generate_moves_ssa(board, first_queue);
        for mv in &mut queue_moves {
            mv.hold_used = true;
        }
        moves.extend(queue_moves);
    }

    moves
}
pub use movement::{try_drop, try_move, try_rotate, try_rotate_180, RotationResult};
