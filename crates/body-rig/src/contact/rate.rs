//! The steps per minute, over the last minute. Pure.

use std::collections::VecDeque;

/// Counts the steps over a trailing window of the model's own time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StepRate {
    /// The model's time, seconds.
    clock: f32,
    /// When each step in the window was taken.
    steps: VecDeque<f32>,
}

impl StepRate {
    pub const fn new() -> Self {
        Self {
            clock: 0.0,
            steps: VecDeque::new(),
        }
    }

    /// Advance by `dt` seconds, with `stepped` steps taken in them.
    pub fn update(&mut self, dt: f32, stepped: usize) {
        if dt.is_finite() && dt > 0.0 {
            self.clock += dt;
        }
        for _ in 0..stepped.min(MAX_STEPS) {
            self.steps.push_back(self.clock);
        }
        while self
            .steps
            .front()
            .is_some_and(|at| self.clock - at > WINDOW_S)
            || self.steps.len() > MAX_STEPS
        {
            self.steps.pop_front();
        }
    }

    /// The steps per minute over the window, or over the time counted so far while it is shorter.
    pub fn per_minute(&self) -> f32 {
        let span = self.clock.min(WINDOW_S);
        if span < MIN_SPAN_S {
            return 0.0;
        }
        self.steps.len() as f32 * 60.0 / span
    }
}

/// The window the rate counts over, seconds.
const WINDOW_S: f32 = 60.0;

/// The most steps the window holds: far more than a minute of standing takes.
const MAX_STEPS: usize = 256;

/// The shortest span a rate is given over, seconds: a step in the first instant is not hundreds a
/// minute.
const MIN_SPAN_S: f32 = 5.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_older_than_a_minute_fall_out() {
        let mut rate = StepRate::new();
        rate.update(1.0, 0);
        assert_eq!(rate.per_minute(), 0.0);
        for _ in 0..60 {
            rate.update(1.0, 0);
        }
        rate.update(1.0, 6);
        assert!((rate.per_minute() - 6.0).abs() < 1e-4);
        for _ in 0..30 {
            rate.update(1.0, 0);
        }
        assert!((rate.per_minute() - 6.0).abs() < 1e-4);
        for _ in 0..31 {
            rate.update(1.0, 0);
        }
        assert_eq!(rate.per_minute(), 0.0);
    }

    #[test]
    fn a_short_session_counts_over_its_own_span() {
        let mut rate = StepRate::new();
        for _ in 0..10 {
            rate.update(1.0, 1);
        }
        assert!((rate.per_minute() - 60.0).abs() < 1e-3);
        rate.update(f32::NAN, 0);
        assert!(rate.per_minute().is_finite());
    }
}
