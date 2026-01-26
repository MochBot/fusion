//! fusion-engine - TETR.IO game logic and simulation engine.
//!
//! Provides SRS+ rotation, kicks, movement, and move generation.

pub mod attack;
pub mod b2b;
pub mod collision;
pub mod combo;
pub mod config;
pub mod garbage;
pub mod kicks;
pub mod movegen;
pub mod movement;

pub use attack::calculate_attack;
pub use b2b::{B2BResult, B2BTracker, ChargingConfig};
pub use collision::{can_place, collides, hard_drop_y};
pub use combo::{apply_combo_multiplier, COMBO_BONUS};
pub use config::{AttackConfig, ComboTable};
pub use garbage::IncreaseTracker;
pub use kicks::get_kicks;
pub use movegen::{generate_moves, generate_moves_with_hold};
pub use movement::{try_drop, try_move, try_rotate, try_rotate_180, RotationResult};
