//! Gravity configuration for TETR.IO game modes.
//!
//! Provides presets for different gravity levels. Most bots assume gravity off.

use serde::{Deserialize, Serialize};

/// Gravity configuration preset.
/// Gravity value is in cells per frame at 60fps (G).
/// Higher G = faster fall. G=0 means no gravity (for bots).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GravityConfig {
    /// Cells per frame at 60fps (0 = no gravity)
    pub gravity: f32,
    /// Soft drop multiplier (typically 20x)
    pub soft_drop_factor: f32,
    /// Lock delay in milliseconds
    pub lock_delay_ms: u32,
    /// Maximum lock delay resets allowed
    pub lock_delay_resets: u8,
}

impl GravityConfig {
    /// No gravity - for bot analysis where pieces don't fall.
    pub fn gravity_off() -> Self {
        Self {
            gravity: 0.0,
            soft_drop_factor: 20.0,
            lock_delay_ms: 500,
            lock_delay_resets: 15,
        }
    }

    /// TETR.IO Tetra League level 1 (G ≈ 0.0167, 1.0 sec/row)
    pub fn tetra_league_level_1() -> Self {
        Self {
            gravity: 0.0167,
            soft_drop_factor: 20.0,
            lock_delay_ms: 500,
            lock_delay_resets: 15,
        }
    }

    /// TETR.IO level 5 (G ≈ 0.069, 0.242 sec/row)
    pub fn tetra_league_level_5() -> Self {
        Self {
            gravity: 0.069,
            soft_drop_factor: 20.0,
            lock_delay_ms: 500,
            lock_delay_resets: 15,
        }
    }

    /// TETR.IO level 10 (G ≈ 0.167, 0.1 sec/row)
    pub fn tetra_league_level_10() -> Self {
        Self {
            gravity: 0.167,
            soft_drop_factor: 20.0,
            lock_delay_ms: 500,
            lock_delay_resets: 15,
        }
    }

    /// TETR.IO level 15 (G ≈ 0.333, 0.05 sec/row)
    pub fn tetra_league_level_15() -> Self {
        Self {
            gravity: 0.333,
            soft_drop_factor: 20.0,
            lock_delay_ms: 500,
            lock_delay_resets: 15,
        }
    }

    /// TETR.IO level 20 (G ≈ 0.833, 0.02 sec/row)
    pub fn tetra_league_level_20() -> Self {
        Self {
            gravity: 0.833,
            soft_drop_factor: 20.0,
            lock_delay_ms: 500,
            lock_delay_resets: 15,
        }
    }

    /// Quick Play default gravity (level 1 equivalent)
    pub fn quick_play() -> Self {
        Self::tetra_league_level_1()
    }
}

impl Default for GravityConfig {
    fn default() -> Self {
        Self::gravity_off()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gravity_off() {
        let config = GravityConfig::gravity_off();
        assert_eq!(config.gravity, 0.0);
        assert_eq!(config.lock_delay_ms, 500);
    }

    #[test]
    fn test_gravity_levels_increase() {
        let l1 = GravityConfig::tetra_league_level_1();
        let l5 = GravityConfig::tetra_league_level_5();
        let l10 = GravityConfig::tetra_league_level_10();
        let l15 = GravityConfig::tetra_league_level_15();
        let l20 = GravityConfig::tetra_league_level_20();

        assert!(l1.gravity < l5.gravity);
        assert!(l5.gravity < l10.gravity);
        assert!(l10.gravity < l15.gravity);
        assert!(l15.gravity < l20.gravity);
    }

    #[test]
    fn test_default_is_gravity_off() {
        assert_eq!(GravityConfig::default(), GravityConfig::gravity_off());
    }
}
