use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Tween {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl Tween {
    pub fn new(value: f32, milliseconds: u64) -> Self {
        Self {
            from: value,
            to: value,
            started: Instant::now(),
            duration: Duration::from_millis(milliseconds),
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return self.to;
        }
        let fraction = (now.saturating_duration_since(self.started).as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0.0, 1.0);
        let easing = 1.0 - (1.0 - fraction).powi(3);
        self.from + (self.to - self.from) * easing
    }

    pub fn set(&mut self, goal: f32, now: Instant, reduced: bool) {
        self.from = self.value(now);
        self.to = goal;
        self.started = now;
        if reduced {
            self.from = goal;
        }
    }

    pub fn active(&self, now: Instant) -> bool {
        (self.to - self.from).abs() > 0.0001
            && now.saturating_duration_since(self.started) < self.duration
    }

    pub fn snap(&mut self) {
        self.from = self.to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_is_bounded_continuous_and_settles() {
        let start = Instant::now();
        let mut tween = Tween::new(0.0, 200);
        tween.set(100.0, start, false);
        let middle = start + Duration::from_millis(80);
        let value = tween.value(middle);
        assert!((0.0..100.0).contains(&value));
        tween.set(75.0, middle, false);
        assert!((tween.value(middle) - value).abs() < 0.001);
        assert_eq!(tween.value(middle + Duration::from_millis(200)), 75.0);
        assert!(!tween.active(middle + Duration::from_millis(200)));
    }

    #[test]
    fn reduced_motion_is_immediate() {
        let now = Instant::now();
        let mut tween = Tween::new(0.0, 250);
        tween.set(1.0, now, true);
        assert_eq!(tween.value(now), 1.0);
        assert!(!tween.active(now));
    }
}
