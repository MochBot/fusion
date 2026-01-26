pub struct IncreaseTracker {
    value: f32,
    pub base: f32,
    pub increase: f32,
    pub margin: u32,
    frame: u32,
}

impl IncreaseTracker {
    pub fn new(base: f32, increase: f32, margin: u32) -> Self {
        Self {
            value: base,
            base,
            increase,
            margin,
            frame: 0,
        }
    }

    pub fn reset(&mut self) {
        self.value = self.base;
        self.frame = 0;
    }

    pub fn tick(&mut self) -> f32 {
        self.frame += 1;
        if self.frame > self.margin {
            self.value += self.increase / 60.0;
        }
        self.value
    }

    pub fn get(&self) -> f32 {
        self.value
    }

    pub fn set(&mut self, value: f32) {
        self.value = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_increases_after_margin() {
        let mut tracker = IncreaseTracker::new(1.0, 6.0, 2);
        assert!((tracker.tick() - 1.0).abs() < 0.0001);
        assert!((tracker.tick() - 1.0).abs() < 0.0001);
        assert!((tracker.tick() - 1.1).abs() < 0.0001);
    }

    #[test]
    fn test_reset_restores_base() {
        let mut tracker = IncreaseTracker::new(2.0, 6.0, 0);
        tracker.tick();
        tracker.set(5.0);
        tracker.reset();
        assert!((tracker.get() - 2.0).abs() < 0.0001);
    }

    #[test]
    fn test_set_get() {
        let mut tracker = IncreaseTracker::new(1.0, 1.0, 0);
        tracker.set(3.5);
        assert!((tracker.get() - 3.5).abs() < 0.0001);
    }
}
