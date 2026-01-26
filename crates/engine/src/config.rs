use crate::b2b::ChargingConfig;

#[derive(Clone, Debug, PartialEq)]
pub struct AttackConfig {
    pub pc_garbage: u8,
    pub pc_b2b: u8,
    pub b2b_chaining: bool,
    pub b2b_charging: Option<ChargingConfig>,
    pub combo_table: ComboTable,
    pub garbage_multiplier: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComboTable {
    Multiplier,
    Classic,
    Modern,
    None,
}

impl AttackConfig {
    pub fn tetra_league() -> Self {
        Self {
            pc_garbage: 5,
            pc_b2b: 2,
            b2b_chaining: true,
            b2b_charging: Some(ChargingConfig::tetra_league()),
            combo_table: ComboTable::Multiplier,
            garbage_multiplier: 1.0,
        }
    }

    pub fn quick_play() -> Self {
        Self {
            pc_garbage: 3,
            pc_b2b: 2,
            b2b_chaining: false,
            b2b_charging: Some(ChargingConfig::quick_play()),
            combo_table: ComboTable::Multiplier,
            garbage_multiplier: 1.0,
        }
    }
}
