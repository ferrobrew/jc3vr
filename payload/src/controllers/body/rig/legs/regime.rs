//! Moving or standing: the handoff between the walk and the standing steps. Pure.
//!
//! The regime switches from the measured speed with hysteresis: it starts moving above the moving
//! speed and stands again below the standing speed, so a speed hovering between them does not
//! switch every few ticks. A continuous weight between the two speeds blends a step's duration and
//! height, so a slow shuffle's steps are between a standing step's and a walk's. Feet are never
//! blended in position, so nothing slides.

/// The regime's state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Regime {
    moving: bool,
    weight: f32,
}

impl Regime {
    pub const fn new() -> Self {
        Self {
            moving: false,
            weight: 0.0,
        }
    }

    /// Whether the feet walk.
    pub fn moving(&self) -> bool {
        self.moving
    }

    /// How far between the standing and the moving speed the speed is, `0..=1`.
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// Update for a measured `speed`; `allowed` is whether the walk may run at all. Returns whether
    /// the feet walk.
    pub fn update(
        &mut self,
        speed: f32,
        allowed: bool,
        moving_mps: f32,
        standing_mps: f32,
    ) -> bool {
        let speed = if speed.is_finite() { speed } else { 0.0 };
        let stand = standing_mps.max(0.0);
        let start = moving_mps.max(stand + 1e-3);
        self.moving = allowed
            && if self.moving {
                speed >= stand
            } else {
                speed > start
            };
        let t = ((speed - stand) / (start - stand)).clamp(0.0, 1.0);
        self.weight = t * t * (3.0 - 2.0 * t);
        self.moving
    }
}

/// `a` blended toward `b` by `weight`.
pub fn blend(a: f32, b: f32, weight: f32) -> f32 {
    a + (b - a) * weight.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_regime_has_hysteresis() {
        let mut regime = Regime::new();
        assert!(!regime.update(0.3, true, 0.35, 0.15));
        assert!(regime.update(0.4, true, 0.35, 0.15));
        assert!(regime.update(0.2, true, 0.35, 0.15));
        assert!(!regime.update(0.1, true, 0.35, 0.15));
        assert!(!regime.update(2.0, false, 0.35, 0.15));
    }

    #[test]
    fn the_weight_runs_between_the_speeds() {
        let mut regime = Regime::new();
        regime.update(0.15, true, 0.35, 0.15);
        assert_eq!(regime.weight(), 0.0);
        regime.update(0.25, true, 0.35, 0.15);
        assert!((regime.weight() - 0.5).abs() < 1e-5);
        regime.update(1.0, true, 0.35, 0.15);
        assert_eq!(regime.weight(), 1.0);
        assert!((blend(0.25, 0.5, 0.5) - 0.375).abs() < 1e-6);
    }
}
