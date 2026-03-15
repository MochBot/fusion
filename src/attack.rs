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

fn base_attack(lines: u8, spin: SpinType) -> f32 {
    match spin {
        SpinType::NoSpin => match lines {
            0 => 0.0,
            1 => SINGLE as f32,
            2 => DOUBLE as f32,
            3 => TRIPLE as f32,
            4 => QUAD as f32,
            5 => PENTA as f32,
            _ => PENTA as f32 + (lines - 5) as f32,
        },
        SpinType::Mini => match lines {
            0 => SPIN_MINI as f32,
            1 => SPIN_MINI_SINGLE as f32,
            2 => SPIN_MINI_DOUBLE as f32,
            3 => SPIN_MINI_TRIPLE as f32,
            4 => SPIN_QUAD as f32,
            _ => SPIN_QUAD as f32 + 2.0 * (lines - 4) as f32,
        },
        SpinType::Full => match lines {
            0 => SPIN as f32,
            1 => SPIN_SINGLE as f32,
            2 => SPIN_DOUBLE as f32,
            3 => SPIN_TRIPLE as f32,
            4 => SPIN_QUAD as f32,
            5 => SPIN_PENTA as f32,
            _ => SPIN_PENTA as f32 + 2.0 * (lines - 5) as f32,
        },
    }
}

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

fn apply_combo(base: f32, combo: u8, table: ComboTable) -> f32 {
    if combo == 0 {
        return base;
    }

    match table {
        ComboTable::Multiplier => {
            let multiplied = base * (1.0 + COMBO_BONUS * combo as f32);
            // for combo > 1, log floor is a MINIMUM guarantee (matches Triangle.js)
            if combo > 1 {
                let log_floor = (1.0 + combo as f32 * COMBO_FLOOR_SCALE).ln();
                f32::max(multiplied, log_floor)
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

pub fn calculate_attack(
    lines: u8,
    spin: SpinType,
    b2b: u8,
    combo: u8,
    config: &AttackConfig,
    is_perfect_clear: bool,
) -> f32 {
    calculate_attack_full(&AttackContext {
        lines,
        spin,
        b2b,
        combo,
        config,
        is_perfect_clear,
        b2b_broken_from: None,
        clears_garbage: false,
    })
}

pub struct AttackContext<'a> {
    pub lines: u8,
    pub spin: SpinType,
    pub b2b: u8,
    pub combo: u8,
    pub config: &'a AttackConfig,
    pub is_perfect_clear: bool,
    pub b2b_broken_from: Option<u8>,
    pub clears_garbage: bool,
}

pub fn calculate_attack_full(ctx: &AttackContext<'_>) -> f32 {
    let AttackContext {
        lines,
        spin,
        b2b,
        combo,
        config,
        is_perfect_clear,
        b2b_broken_from,
        clears_garbage,
    } = *ctx;
    if lines == 0 {
        return 0.0;
    }

    let mut attack = base_attack(lines, spin);

    // perfect clear bonus
    if is_perfect_clear {
        attack += config.pc_garbage as f32;
    }

    let is_b2b_eligible = spin != SpinType::NoSpin || lines >= 4;

    if b2b > 0 {
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

    // surge release: non-difficult clear breaks a long B2B chain
    if let Some(prev_b2b) = b2b_broken_from {
        if prev_b2b >= 4 && !is_b2b_eligible {
            attack += 4.0 + (prev_b2b - 4) as f32;
        }
    }

    // garbage clear boost: difficult clear that also clears garbage
    if clears_garbage && is_b2b_eligible {
        attack += 1.0;
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
    fn test_b2b_applied_when_caller_passes_positive() {
        let dmg_no_b2b = calculate_attack(1, SpinType::NoSpin, 0, 0, &tl(), false);
        let dmg_with_b2b = calculate_attack(1, SpinType::NoSpin, 5, 0, &tl(), false);
        assert!(
            dmg_with_b2b > dmg_no_b2b,
            "b2b>0 must always apply bonus (caller enforces eligibility)"
        );
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

    #[test]
    fn test_nospin_6_lines() {
        // 5 + (6-5) = 6
        let dmg = calculate_attack(6, SpinType::NoSpin, 0, 0, &tl(), false);
        assert_eq!(dmg, 6.0);
    }

    #[test]
    fn test_nospin_8_lines() {
        // 5 + (8-5) = 8
        let dmg = calculate_attack(8, SpinType::NoSpin, 0, 0, &tl(), false);
        assert_eq!(dmg, 8.0);
    }

    #[test]
    fn test_full_spin_6_lines() {
        // 12 + 2*(6-5) = 14
        let dmg = calculate_attack(6, SpinType::Full, 0, 0, &tl(), false);
        assert_eq!(dmg, 14.0);
    }

    #[test]
    fn test_mini_spin_5_lines() {
        // SPIN_QUAD(10) + 2*(5-4) = 12
        let dmg = calculate_attack(5, SpinType::Mini, 0, 0, &tl(), false);
        assert_eq!(dmg, 12.0);
    }

    #[test]
    fn test_full_spin_7_lines() {
        // 12 + 2*(7-5) = 16
        let dmg = calculate_attack(7, SpinType::Full, 0, 0, &tl(), false);
        assert_eq!(dmg, 16.0);
    }

    #[test]
    fn test_combo_multiplier_max_semantics() {
        // combo=2, base=4 (quad, no b2b)
        // multiplied = 4*(1+0.25*2) = 6.0
        // log_floor = floor(ln(1+2*1.25)) = floor(ln(3.5)) = floor(1.25) = 1
        // result = max(6.0, 4.0+1.0) = 6.0 (multiplier wins)
        let dmg = calculate_attack(4, SpinType::NoSpin, 0, 2, &tl(), false);
        assert_eq!(dmg, 6.0);
    }

    #[test]
    fn test_combo_log_floor_kicks_in_low_base() {
        // combo=4, base=1 (double, no b2b)
        // multiplied = 1*(1+0.25*4) = 2.0
        // log_floor = floor(ln(1+4*1.25)) = floor(ln(6)) = floor(1.79) = 1
        // result = max(2.0, 1.0+1.0) = 2.0 (multiplier still wins here)
        let dmg = calculate_attack(2, SpinType::NoSpin, 0, 4, &tl(), false);
        assert_eq!(dmg, 2.0);
    }

    #[test]
    fn test_combo_high_combo_log_floor_as_minimum() {
        // combo=8, base=0 (single=0 base, no b2b)
        // multiplied = 0*(1+0.25*8) = 0.0
        // log_floor = ln(1+8*1.25) = ln(11) ≈ 2.397 (matches Triangle.js: no floor())
        // result = max(0.0, 2.397) ≈ 2.397
        let dmg = calculate_attack(1, SpinType::NoSpin, 0, 8, &tl(), false);
        let expected = (1.0_f32 + 8.0 * COMBO_FLOOR_SCALE).ln();
        assert!(
            (dmg - expected).abs() < 0.001,
            "expected ~{expected}, got {dmg}"
        );
    }

    #[test]
    fn test_surge_release_b2b4_broken() {
        // single clear (non-difficult) breaks b2b=4 chain
        // base=0, surge = 4 + (4-4) = 4
        let dmg = calculate_attack_full(&AttackContext {
            lines: 1,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: Some(4),
            clears_garbage: false,
        });
        assert_eq!(dmg, 4.0);
    }

    #[test]
    fn test_surge_release_b2b7_broken() {
        // double clear (non-difficult) breaks b2b=7 chain
        // base=1, surge = 4 + (7-4) = 7, total = 8
        let dmg = calculate_attack_full(&AttackContext {
            lines: 2,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: Some(7),
            clears_garbage: false,
        });
        assert_eq!(dmg, 8.0);
    }

    #[test]
    fn test_surge_release_not_triggered_by_difficult_clear() {
        // quad (b2b-eligible) should NOT trigger surge release even if b2b_broken_from
        let with_surge = calculate_attack_full(&AttackContext {
            lines: 4,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: Some(6),
            clears_garbage: false,
        });
        let without_surge = calculate_attack_full(&AttackContext {
            lines: 4,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: false,
        });
        assert_eq!(with_surge, without_surge);
    }

    #[test]
    fn test_surge_release_not_triggered_below_4() {
        // b2b_broken_from=3, below threshold — no surge
        let dmg = calculate_attack_full(&AttackContext {
            lines: 1,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: Some(3),
            clears_garbage: false,
        });
        assert_eq!(dmg, 0.0);
    }

    #[test]
    fn test_old_api_unchanged() {
        // old 6-arg API passes defaults (no surge, no garbage boost)
        let old = calculate_attack(4, SpinType::NoSpin, 0, 0, &tl(), false);
        let new = calculate_attack_full(&AttackContext {
            lines: 4,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: false,
        });
        assert_eq!(old, new);
    }

    #[test]
    fn test_garbage_clear_boost_on_quad() {
        // quad (b2b-eligible) + clears garbage = +1
        let without = calculate_attack_full(&AttackContext {
            lines: 4,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: false,
        });
        let with = calculate_attack_full(&AttackContext {
            lines: 4,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: true,
        });
        assert_eq!(with - without, 1.0);
    }

    #[test]
    fn test_garbage_clear_boost_on_spin() {
        // spin single (b2b-eligible) + clears garbage = +1
        let without = calculate_attack_full(&AttackContext {
            lines: 1,
            spin: SpinType::Full,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: false,
        });
        let with = calculate_attack_full(&AttackContext {
            lines: 1,
            spin: SpinType::Full,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: true,
        });
        assert_eq!(with - without, 1.0);
    }

    #[test]
    fn test_garbage_clear_boost_not_on_non_difficult() {
        // double clear (not b2b-eligible, no spin) — no boost
        let without = calculate_attack_full(&AttackContext {
            lines: 2,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: false,
        });
        let with = calculate_attack_full(&AttackContext {
            lines: 2,
            spin: SpinType::NoSpin,
            b2b: 0,
            combo: 0,
            config: &tl(),
            is_perfect_clear: false,
            b2b_broken_from: None,
            clears_garbage: true,
        });
        assert_eq!(without, with);
    }
}
