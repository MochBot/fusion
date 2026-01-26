use crate::combo::apply_combo_multiplier;
use crate::config::{AttackConfig, ComboTable};
use fusion_core::SpinType;

pub const SINGLE: u8 = 0;
pub const DOUBLE: u8 = 1;
pub const TRIPLE: u8 = 2;
pub const QUAD: u8 = 4;
pub const PENTA: u8 = 5;

pub const TSPIN_MINI: u8 = 0;
pub const TSPIN: u8 = 0;
pub const TSPIN_MINI_SINGLE: u8 = 0;
pub const TSPIN_SINGLE: u8 = 2;
pub const TSPIN_MINI_DOUBLE: u8 = 1;
pub const TSPIN_DOUBLE: u8 = 4;
pub const TSPIN_MINI_TRIPLE: u8 = 2;
pub const TSPIN_TRIPLE: u8 = 6;
pub const TSPIN_QUAD: u8 = 10;
pub const TSPIN_PENTA: u8 = 12;

pub const BACK_TO_BACK_BONUS: u8 = 1;
/// Perfect Clear attack values (S2 - changed from 10 in S1)
// S2 PC values: tetrio.wiki.gg/wiki/TETRA_LEAGUE (Aug 2024)
pub const ALL_CLEAR_TL: u8 = 5;
pub const ALL_CLEAR_QP: u8 = 3;
pub const ALL_CLEAR_B2B_QP: u8 = 2;

const CLASSIC_COMBO_TABLE: [u8; 11] = [0, 1, 1, 2, 2, 3, 3, 4, 4, 4, 5];
const MODERN_COMBO_TABLE: [u8; 13] = [0, 1, 1, 2, 2, 2, 3, 3, 3, 3, 3, 3, 4];

pub fn perfect_clear_attack(config: &AttackConfig) -> u8 {
    config.pc_garbage
}

pub fn perfect_clear_b2b_bonus(config: &AttackConfig) -> u8 {
    config.pc_b2b
}

fn base_attack(lines: u8, spin: SpinType) -> f32 {
    let is_mini = spin == SpinType::Mini;
    let is_spin = spin != SpinType::None;

    match lines {
        0 => {
            if is_mini {
                TSPIN_MINI as f32
            } else if is_spin {
                TSPIN as f32
            } else {
                0.0
            }
        }
        1 => {
            if is_mini {
                TSPIN_MINI_SINGLE as f32
            } else if is_spin {
                TSPIN_SINGLE as f32
            } else {
                SINGLE as f32
            }
        }
        2 => {
            if is_mini {
                TSPIN_MINI_DOUBLE as f32
            } else if is_spin {
                TSPIN_DOUBLE as f32
            } else {
                DOUBLE as f32
            }
        }
        3 => {
            if is_mini {
                TSPIN_MINI_TRIPLE as f32
            } else if is_spin {
                TSPIN_TRIPLE as f32
            } else {
                TRIPLE as f32
            }
        }
        4 => {
            if is_spin {
                TSPIN_QUAD as f32
            } else {
                QUAD as f32
            }
        }
        5 => {
            if is_spin {
                TSPIN_PENTA as f32
            } else {
                PENTA as f32
            }
        }
        _ => {
            let extra = lines.saturating_sub(5) as u16;
            if is_spin {
                (TSPIN_PENTA as u16 + 2 * extra) as f32
            } else {
                (PENTA as u16 + extra) as f32
            }
        }
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
    let mut garbage = base_attack(lines, spin);

    if is_perfect_clear {
        garbage += perfect_clear_attack(config) as f32;
    }

    if lines > 0 && b2b > 0 {
        let bonus = if is_perfect_clear {
            perfect_clear_b2b_bonus(config)
        } else {
            BACK_TO_BACK_BONUS
        };
        garbage += bonus as f32;
    }

    let garbage = apply_combo_table(garbage, combo, config.combo_table);
    garbage * config.garbage_multiplier
}

fn apply_combo_table(base_garbage: f32, combo: u8, table: ComboTable) -> f32 {
    match table {
        ComboTable::Multiplier => apply_combo_multiplier(base_garbage, combo),
        ComboTable::Classic => base_garbage + combo_table_bonus(combo, &CLASSIC_COMBO_TABLE),
        ComboTable::Modern => base_garbage + combo_table_bonus(combo, &MODERN_COMBO_TABLE),
        ComboTable::None => base_garbage,
    }
}

fn combo_table_bonus(combo: u8, table: &[u8]) -> f32 {
    if table.is_empty() {
        return 0.0;
    }

    let index = combo as usize;
    let value = if index < table.len() {
        table[index]
    } else {
        table[table.len() - 1]
    };
    value as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quad_attack() {
        let config = AttackConfig::tetra_league();
        assert_eq!(
            calculate_attack(4, SpinType::None, 0, 0, &config, false),
            4.0
        );
    }

    #[test]
    fn test_tspin_double_attack() {
        let config = AttackConfig::tetra_league();
        assert_eq!(
            calculate_attack(2, SpinType::Full, 0, 0, &config, false),
            4.0
        );
    }

    #[test]
    fn test_combo_multiplier_applied() {
        let config = AttackConfig::tetra_league();
        let attack = calculate_attack(2, SpinType::None, 0, 4, &config, false);
        assert!((attack - 2.0).abs() < 0.0001);
    }

    #[test]
    fn test_b2b_bonus_applied() {
        let config = AttackConfig::tetra_league();
        let attack = calculate_attack(4, SpinType::None, 1, 0, &config, false);
        assert!((attack - 5.0).abs() < 0.0001);
    }

    #[test]
    fn test_perfect_clear_tetra_league() {
        let config = AttackConfig::tetra_league();
        let attack = calculate_attack(4, SpinType::None, 0, 0, &config, true);
        assert!((attack - 9.0).abs() < 0.0001);
    }

    #[test]
    fn test_perfect_clear_quick_play() {
        let config = AttackConfig::quick_play();
        let attack = calculate_attack(4, SpinType::None, 0, 0, &config, true);
        assert!((attack - 7.0).abs() < 0.0001);
    }

    #[test]
    fn test_perfect_clear_bonus_values() {
        let tetra_config = AttackConfig::tetra_league();
        let quick_config = AttackConfig::quick_play();

        assert_eq!(perfect_clear_attack(&tetra_config), ALL_CLEAR_TL);
        assert_eq!(perfect_clear_attack(&quick_config), ALL_CLEAR_QP);
        assert_eq!(perfect_clear_b2b_bonus(&quick_config), ALL_CLEAR_B2B_QP);
    }
}
