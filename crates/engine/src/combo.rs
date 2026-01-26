pub const COMBO_BONUS: f32 = 0.25;

pub fn apply_combo_multiplier(base_garbage: f32, combo: u8) -> f32 {
    if combo > 0 {
        let multiplied = base_garbage * (1.0 + COMBO_BONUS * combo as f32);
        if combo > 1 {
            multiplied.max((1.0 + combo as f32 * 1.25).ln_1p())
        } else {
            multiplied
        }
    } else {
        base_garbage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combo_zero_no_change() {
        assert_eq!(apply_combo_multiplier(3.0, 0), 3.0);
    }

    #[test]
    fn test_combo_one_multiplier() {
        assert!((apply_combo_multiplier(4.0, 1) - 5.0).abs() < 0.0001);
    }

    #[test]
    fn test_combo_min_floor() {
        let result = apply_combo_multiplier(0.1, 4);
        let expected = (1.0_f32 + 4.0_f32 * 1.25_f32).ln_1p();
        assert!((result - expected).abs() < 0.0001);
    }
}
