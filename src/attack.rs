// attack.rs -- TETR.IO Season 2 damage formula
// piece-agnostic allspin: any piece with spin gets bonus, not just T

use crate::header::SpinType;

// base attack table — no spin
pub const SINGLE: u8 = 0;
pub const DOUBLE: u8 = 1;
pub const TRIPLE: u8 = 2;
pub const QUAD: u8 = 4;
pub const PENTA: u8 = 5;

// allspin attack (any piece with spin, not just T)
pub const SPIN_MINI: u8 = 0;
pub const SPIN: u8 = 0;
pub const SPIN_MINI_SINGLE: u8 = 0;
pub const SPIN_SINGLE: u8 = 2;
pub const SPIN_MINI_DOUBLE: u8 = 1;
pub const SPIN_DOUBLE: u8 = 4;
pub const SPIN_MINI_TRIPLE: u8 = 2;
pub const SPIN_TRIPLE: u8 = 6;
pub const SPIN_QUAD: u8 = 10;
pub const SPIN_PENTA: u8 = 12;

pub const BACK_TO_BACK_BONUS: u8 = 1;
const B2B_CHAINING_LOG: f32 = 0.8;
const COMBO_BONUS: f32 = 0.25;
const COMBO_FLOOR_SCALE: f32 = 1.25;

const CLASSIC_COMBO_TABLE: [u8; 11] = [0, 1, 1, 2, 2, 3, 3, 4, 4, 4, 5];
const MODERN_COMBO_TABLE: [u8; 13] = [0, 1, 1, 2, 2, 2, 3, 3, 3, 3, 3, 3, 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComboTable {
    Multiplier,
    Classic,
    Modern,
    None,
}

#[derive(Debug, Clone)]
pub struct AttackConfig {
    pub pc_garbage: u8,
    pub pc_b2b: u8,
    pub b2b_chaining: bool,
    pub combo_table: ComboTable,
    pub garbage_multiplier: f32,
}

impl AttackConfig {
    pub fn tetra_league() -> Self {
        Self {
            pc_garbage: 5,
            pc_b2b: 2,
            b2b_chaining: true,
            combo_table: ComboTable::Multiplier,
            garbage_multiplier: 1.0,
        }
    }

    pub fn quick_play() -> Self {
        Self {
            pc_garbage: 3,
            pc_b2b: 2,
            b2b_chaining: false,
            combo_table: ComboTable::Multiplier,
            garbage_multiplier: 1.0,
        }
    }
}

/// base garbage for a line clear + spin type (piece-agnostic)
fn base_attack(lines: u8, spin: SpinType) -> f32 {
    match spin {
        SpinType::NoSpin => match lines {
            0 => 0.0,
            1 => SINGLE as f32,
            2 => DOUBLE as f32,
            3 => TRIPLE as f32,
            4 => QUAD as f32,
            _ => PENTA as f32,
        },
        SpinType::Mini => match lines {
            0 => SPIN_MINI as f32,
            1 => SPIN_MINI_SINGLE as f32,
            2 => SPIN_MINI_DOUBLE as f32,
            3 => SPIN_MINI_TRIPLE as f32,
            _ => SPIN_QUAD as f32,
        },
        SpinType::Full => match lines {
            0 => SPIN as f32,
            1 => SPIN_SINGLE as f32,
            2 => SPIN_DOUBLE as f32,
            3 => SPIN_TRIPLE as f32,
            4 => SPIN_QUAD as f32,
            _ => SPIN_PENTA as f32,
        },
    }
}

/// logarithmic B2B chaining bonus (S2 surge mechanic)
fn b2b_chaining_bonus(b2b: u8) -> f32 {
    if b2b <= 1 {
        return BACK_TO_BACK_BONUS as f32;
    }
    // floor(1 + ln(1 + b2b * B2B_CHAINING_LOG)) with fractional third
    let log_part = (1.0 + b2b as f32 * B2B_CHAINING_LOG).ln();
    let floored = (1.0 + log_part).floor();
    // fractional third: the remainder after floor contributes a third
    let remainder = (1.0 + log_part) - floored;
    let third = if remainder > 0.0 {
        remainder / 3.0
    } else {
        0.0
    };
    floored + third
}

/// apply combo bonus based on combo table mode
fn apply_combo(base: f32, combo: u8, table: ComboTable) -> f32 {
    if combo == 0 {
        return base;
    }

    match table {
        ComboTable::Multiplier => {
            let multiplied = base * (1.0 + COMBO_BONUS * combo as f32);
            // for combo > 1, add a log floor component
            if combo > 1 {
                let log_floor = (1.0 + combo as f32 * COMBO_FLOOR_SCALE).ln().floor();
                multiplied + log_floor
            } else {
                multiplied
            }
        }
        ComboTable::Classic => {
            let idx = (combo as usize).min(CLASSIC_COMBO_TABLE.len() - 1);
            base + CLASSIC_COMBO_TABLE[idx] as f32
        }
        ComboTable::Modern => {
            let idx = (combo as usize).min(MODERN_COMBO_TABLE.len() - 1);
            base + MODERN_COMBO_TABLE[idx] as f32
        }
        ComboTable::None => base,
    }
}

/// TETR.IO S2 attack calculation
/// returns garbage lines sent as f32 (caller truncates as needed)
pub fn calculate_attack(
    lines: u8,
    spin: SpinType,
    b2b: u8,
    combo: u8,
    config: &AttackConfig,
    is_perfect_clear: bool,
) -> f32 {
    if lines == 0 {
        return 0.0;
    }

    let mut attack = base_attack(lines, spin);

    // perfect clear bonus
    if is_perfect_clear {
        attack += config.pc_garbage as f32;
    }

    // B2B bonus: only if b2b > 0 and this clear qualifies (spin or quad+)
    let is_b2b_eligible = spin != SpinType::NoSpin || lines >= 4;
    if b2b > 0 && is_b2b_eligible {
        if config.b2b_chaining {
            attack += b2b_chaining_bonus(b2b);
        } else {
            attack += BACK_TO_BACK_BONUS as f32;
        }
    }

    // perfect clear B2B bonus (separate from regular B2B)
    if is_perfect_clear && b2b > 0 {
        attack += config.pc_b2b as f32;
    }

    // combo
    attack = apply_combo(attack, combo, config.combo_table);

    // garbage multiplier
    attack *= config.garbage_multiplier;

    attack
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tl() -> AttackConfig {
        AttackConfig::tetra_league()
    }

    fn qp() -> AttackConfig {
        AttackConfig::quick_play()
    }

    #[test]
    fn test_no_clear_zero() {
        assert_eq!(
            calculate_attack(0, SpinType::NoSpin, 0, 0, &tl(), false),
            0.0
        );
    }

    #[test]
    fn test_single_zero_garbage() {
        // single clear = 0 lines sent
        let dmg = calculate_attack(1, SpinType::NoSpin, 0, 0, &tl(), false);
        assert_eq!(dmg, 0.0);
    }

    #[test]
    fn test_double_one_garbage() {
        let dmg = calculate_attack(2, SpinType::NoSpin, 0, 0, &tl(), false);
        assert_eq!(dmg, 1.0);
    }

    #[test]
    fn test_triple_two_garbage() {
        let dmg = calculate_attack(3, SpinType::NoSpin, 0, 0, &tl(), false);
        assert_eq!(dmg, 2.0);
    }

    #[test]
    fn test_quad_four_garbage() {
        let dmg = calculate_attack(4, SpinType::NoSpin, 0, 0, &tl(), false);
        assert_eq!(dmg, 4.0);
    }

    #[test]
    fn test_tspin_double_four_garbage() {
        let dmg = calculate_attack(2, SpinType::Full, 0, 0, &tl(), false);
        assert_eq!(dmg, 4.0);
    }

    #[test]
    fn test_tspin_triple_six_garbage() {
        let dmg = calculate_attack(3, SpinType::Full, 0, 0, &tl(), false);
        assert_eq!(dmg, 6.0);
    }

    #[test]
    fn test_tspin_single_two_garbage() {
        let dmg = calculate_attack(1, SpinType::Full, 0, 0, &tl(), false);
        assert_eq!(dmg, 2.0);
    }

    #[test]
    fn test_allspin_s_double() {
        // S-spin double = same as T-spin double = 4 garbage
        let dmg = calculate_attack(2, SpinType::Full, 0, 0, &tl(), false);
        assert_eq!(dmg, 4.0);
    }

    #[test]
    fn test_allspin_l_triple() {
        // L-spin triple = same as T-spin triple = 6 garbage
        let dmg = calculate_attack(3, SpinType::Full, 0, 0, &tl(), false);
        assert_eq!(dmg, 6.0);
    }

    #[test]
    fn test_mini_spin_double() {
        let dmg = calculate_attack(2, SpinType::Mini, 0, 0, &tl(), false);
        assert_eq!(dmg, SPIN_MINI_DOUBLE as f32);
    }

    #[test]
    fn test_b2b_flat_bonus() {
        // b2b=1, quad, no chaining (QP mode)
        let dmg = calculate_attack(4, SpinType::NoSpin, 1, 0, &qp(), false);
        assert_eq!(dmg, 4.0 + 1.0); // QUAD + flat B2B
    }

    #[test]
    fn test_b2b_chaining_grows() {
        // b2b=1, quad, chaining enabled (TL mode)
        let dmg_b2b1 = calculate_attack(4, SpinType::NoSpin, 1, 0, &tl(), false);
        let dmg_b2b5 = calculate_attack(4, SpinType::NoSpin, 5, 0, &tl(), false);
        assert!(
            dmg_b2b5 > dmg_b2b1,
            "higher b2b chain should give more damage: b2b1={}, b2b5={}",
            dmg_b2b1,
            dmg_b2b5
        );
    }

    #[test]
    fn test_b2b_not_applied_to_singles() {
        // single clear with b2b=5 should NOT get B2B bonus (not eligible)
        let dmg_no_b2b = calculate_attack(1, SpinType::NoSpin, 0, 0, &tl(), false);
        let dmg_with_b2b = calculate_attack(1, SpinType::NoSpin, 5, 0, &tl(), false);
        assert_eq!(dmg_no_b2b, dmg_with_b2b);
    }

    #[test]
    fn test_perfect_clear_tl() {
        let dmg = calculate_attack(4, SpinType::NoSpin, 0, 0, &tl(), true);
        assert_eq!(dmg, 4.0 + 5.0); // QUAD + pc_garbage
    }

    #[test]
    fn test_perfect_clear_qp() {
        let dmg = calculate_attack(4, SpinType::NoSpin, 0, 0, &qp(), true);
        assert_eq!(dmg, 4.0 + 3.0); // QUAD + pc_garbage
    }

    #[test]
    fn test_combo_multiplier() {
        let dmg_0 = calculate_attack(4, SpinType::NoSpin, 0, 0, &tl(), false);
        let dmg_2 = calculate_attack(4, SpinType::NoSpin, 0, 2, &tl(), false);
        assert!(dmg_2 > dmg_0, "combo should increase damage");
    }

    #[test]
    fn test_combo_classic_table() {
        let config = AttackConfig {
            combo_table: ComboTable::Classic,
            ..AttackConfig::tetra_league()
        };
        // combo=3, double clear
        let dmg = calculate_attack(2, SpinType::NoSpin, 0, 3, &config, false);
        // base=1, classic[3]=2
        assert_eq!(dmg, 3.0);
    }

    #[test]
    fn test_combo_modern_table() {
        let config = AttackConfig {
            combo_table: ComboTable::Modern,
            ..AttackConfig::tetra_league()
        };
        let dmg = calculate_attack(2, SpinType::NoSpin, 0, 3, &config, false);
        // base=1, modern[3]=2
        assert_eq!(dmg, 3.0);
    }

    #[test]
    fn test_combo_none_table() {
        let config = AttackConfig {
            combo_table: ComboTable::None,
            ..AttackConfig::tetra_league()
        };
        let dmg_0 = calculate_attack(2, SpinType::NoSpin, 0, 0, &config, false);
        let dmg_5 = calculate_attack(2, SpinType::NoSpin, 0, 5, &config, false);
        assert_eq!(dmg_0, dmg_5, "ComboTable::None should ignore combo");
    }

    #[test]
    fn test_garbage_multiplier() {
        let mut config = tl();
        config.garbage_multiplier = 2.0;
        let dmg = calculate_attack(4, SpinType::NoSpin, 0, 0, &config, false);
        assert_eq!(dmg, 8.0); // 4 * 2.0
    }

    #[test]
    fn test_full_stack_tsd_b2b_combo() {
        // TSD with b2b=3 and combo=2 in TL
        let dmg = calculate_attack(2, SpinType::Full, 3, 2, &tl(), false);
        // base=4, b2b chaining bonus for b2b=3, combo multiplier
        assert!(dmg > 4.0, "stacked bonuses should exceed base");
    }
}
