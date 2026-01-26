use fusion_core::SpinType;

const BACK_TO_BACK_BONUS: f32 = 1.0;
const BACK_TO_BACK_BONUS_LOG: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargingConfig {
    pub at: u8,
    pub base: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct B2BResult {
    pub bonus: f32,
    pub surge: Option<[u8; 3]>,
}

pub struct B2BTracker {
    pub level: u8,
    pub chaining: bool,
    pub charging: Option<ChargingConfig>,
}

impl ChargingConfig {
    pub fn new(at: u8, base: u8) -> Self {
        Self { at, base }
    }

    /// QUICK PLAY default: surge starts at 1 line
    // B2B Charging: tetris.wiki/TETR.IO - Beta 1.0.0 (Jul 26, 2024)
    pub fn quick_play() -> Self {
        Self { at: 4, base: 1 }
    }

    /// TETRA LEAGUE / Custom default: surge starts at 4 lines
    // B2B Charging: tetris.wiki/TETR.IO - Beta 1.0.0 (Jul 26, 2024)
    pub fn tetra_league() -> Self {
        Self { at: 4, base: 4 }
    }
}

impl B2BTracker {
    pub fn new(chaining: bool, charging: Option<ChargingConfig>) -> Self {
        Self {
            level: 0,
            chaining,
            charging,
        }
    }

    pub fn reset(&mut self) {
        self.level = 0;
    }

    pub fn register_clear(&mut self, lines: u8, spin: SpinType) -> B2BResult {
        if lines == 0 {
            return B2BResult {
                bonus: 0.0,
                surge: None,
            };
        }

        if qualifies_b2b(lines, spin) {
            self.level = self.level.saturating_add(1);
            let bonus = if spin == SpinType::Mini {
                0.0
            } else {
                self.bonus_for_level(self.level)
            };
            return B2BResult { bonus, surge: None };
        }

        let surge = self.surge_on_break();
        self.level = 0;
        B2BResult { bonus: 0.0, surge }
    }

    fn bonus_for_level(&self, level: u8) -> f32 {
        if level == 0 {
            return 0.0;
        }

        if self.chaining {
            chaining_bonus(level)
        } else {
            BACK_TO_BACK_BONUS
        }
    }

    fn surge_on_break(&self) -> Option<[u8; 3]> {
        let config = self.charging?;
        if self.level == 0 {
            return None;
        }

        if self.level.saturating_add(1) <= config.at {
            return None;
        }

        let total = self.level as i16 - config.at as i16 + config.base as i16 + 1;
        if total <= 0 {
            return None;
        }

        Some(split_surge(total as u8))
    }
}

fn qualifies_b2b(lines: u8, spin: SpinType) -> bool {
    lines > 0 && (lines >= 4 || spin != SpinType::None)
}

fn chaining_bonus(level: u8) -> f32 {
    if level == 0 {
        return 0.0;
    }

    let log_value = (level as f32 * BACK_TO_BACK_BONUS_LOG).ln_1p();
    let base = (1.0 + log_value).floor();
    let fraction = if level == 1 {
        0.0
    } else {
        (1.0 + (log_value % 1.0)) / 3.0
    };
    BACK_TO_BACK_BONUS * (base + fraction)
}

fn split_surge(total: u8) -> [u8; 3] {
    let chunk = (total as f32 / 3.0).round() as u8;
    let first = chunk;
    let second = chunk;
    let third = total.saturating_sub(first.saturating_mul(2));
    [first, second, third]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_bonus_levels() {
        let mut tracker = B2BTracker::new(false, None);
        for level in 1..=3 {
            let result = tracker.register_clear(4, SpinType::None);
            assert!((result.bonus - 1.0).abs() < 0.0001);
            assert_eq!(tracker.level, level);
        }
    }

    #[test]
    fn test_surge_on_break_tetra_league() {
        let mut tracker = B2BTracker::new(false, Some(ChargingConfig::tetra_league()));
        for _ in 0..4 {
            tracker.register_clear(4, SpinType::None);
        }
        let result = tracker.register_clear(1, SpinType::None);
        assert_eq!(result.surge, Some([2, 2, 1]));
        assert_eq!(tracker.level, 0);
    }

    #[test]
    fn test_surge_on_break_quick_play() {
        let mut tracker = B2BTracker::new(false, Some(ChargingConfig::quick_play()));
        for _ in 0..4 {
            tracker.register_clear(4, SpinType::None);
        }
        let result = tracker.register_clear(1, SpinType::None);
        assert_eq!(result.surge, Some([1, 1, 0]));
        assert_eq!(tracker.level, 0);
    }

    #[test]
    fn test_mini_spin_keeps_chain_no_bonus() {
        let mut tracker = B2BTracker::new(false, None);
        tracker.register_clear(4, SpinType::None);
        let result = tracker.register_clear(1, SpinType::Mini);
        assert!((result.bonus - 0.0).abs() < 0.0001);
        assert_eq!(tracker.level, 2);
    }

    #[test]
    fn test_chaining_bonus_formula() {
        let mut tracker = B2BTracker::new(true, None);
        tracker.register_clear(4, SpinType::None);
        let result = tracker.register_clear(4, SpinType::None);
        let expected = chaining_bonus(2);
        assert!((result.bonus - expected).abs() < 0.0001);
    }
}
