//! The walk's tuning for a speed, and the neutral stance the feet stand and walk around. Pure.
//!
//! The stride, the duty factor, and the lift are set at a walking and a running speed and eased
//! between them, so a faster walk takes longer strides at a quicker cadence and spends less of each
//! cycle on the ground, until the run has a moment with both feet in the air.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig, solver::feet::GaitTuning,
};

/// The walk's tuning at `speed` metres per second, or `None` below the speed the feet walk at.
pub fn tuning(speed: f32, config: &ProceduralBodyConfig) -> Option<GaitTuning> {
    if !(config.gait && speed.is_finite() && speed >= config.gait_speed_mps) {
        return None;
    }
    let span = (config.run_speed_mps - config.walk_speed_mps).max(1e-3);
    let t = ((speed - config.walk_speed_mps) / span).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    // Below the walking speed, the stride shortens with the speed, so a slow walk does not drag
    // the cadence down to a crawl.
    let slow = (speed / config.walk_speed_mps.max(1e-3)).clamp(MIN_SLOW_SHARE, 1.0);
    Some(GaitTuning {
        stride_m: lerp(config.walk_stride_m, config.run_stride_m) * slow.sqrt(),
        duty: lerp(config.walk_duty, config.run_duty),
        height_m: lerp(config.walk_step_height_m, config.run_step_height_m),
    })
}

/// A foot's place under the root when standing, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StanceFoot {
    pub position: Vec3,
    pub rotation: Quat,
}

/// The neutral stance: where the animation stands the feet when Rico has been still for a moment,
/// measured once and made symmetric, so that the feet walk and stand around the same place
/// whatever the animation's idle does with them afterwards.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct NeutralStance {
    still_s: f32,
    stance: Option<[StanceFoot; 2]>,
}

impl NeutralStance {
    pub const fn new() -> Self {
        Self {
            still_s: 0.0,
            stance: None,
        }
    }

    /// The measured stance, left then right.
    pub fn get(&self) -> Option<[StanceFoot; 2]> {
        self.stance
    }

    /// Fold in a tick of the animated feet: measured once they have stood still for
    /// [`SETTLE_S`]. Returns the stance when this tick measured it.
    pub fn observe(
        &mut self,
        feet: [StanceFoot; 2],
        still: bool,
        dt: f32,
    ) -> Option<[StanceFoot; 2]> {
        if self.stance.is_some() {
            return None;
        }
        if !still
            || !feet
                .iter()
                .all(|f| f.position.is_finite() && f.rotation.is_finite())
        {
            self.still_s = 0.0;
            return None;
        }
        self.still_s += dt.max(0.0);
        if self.still_s < SETTLE_S {
            return None;
        }
        let [left, right] = feet;
        let half_width = ((right.position.x - left.position.x) * 0.5).abs();
        let height = (left.position.y + right.position.y) * 0.5;
        let depth = (left.position.z + right.position.z) * 0.5;
        let stance = [
            StanceFoot {
                position: Vec3::new(-half_width, height, depth),
                rotation: left.rotation,
            },
            StanceFoot {
                position: Vec3::new(half_width, height, depth),
                rotation: right.rotation,
            },
        ];
        self.stance = Some(stance);
        Some(stance)
    }
}

/// How long the feet stand still before the stance is measured, seconds.
pub const SETTLE_S: f32 = 0.5;

/// The shortest a slow walk's stride gets, as a share of the walking stride's square.
const MIN_SLOW_SHARE: f32 = 0.25;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faster_walks_take_longer_strides_and_less_ground() {
        let config = ProceduralBodyConfig::new();
        assert!(tuning(config.gait_speed_mps * 0.5, &config).is_none());
        let walk = tuning(config.walk_speed_mps, &config).unwrap();
        let run = tuning(config.run_speed_mps, &config).unwrap();
        let sprint = tuning(config.run_speed_mps * 2.0, &config).unwrap();
        assert!((walk.stride_m - config.walk_stride_m).abs() < 1e-4);
        assert!(run.stride_m > walk.stride_m);
        assert!(run.duty < 0.5 && walk.duty > 0.5);
        assert_eq!(run, sprint);
        // The cadence rises with the speed.
        let cadence = |speed: f32| speed / tuning(speed, &config).unwrap().stride_m;
        let mut last = 0.0;
        for i in 1..=40 {
            let speed = config.gait_speed_mps + i as f32 * 0.15;
            let c = cadence(speed);
            assert!(c >= last - 1e-4, "cadence fell at {speed}");
            last = c;
        }
    }

    #[test]
    fn the_stance_is_measured_once_after_settling_and_symmetric() {
        let mut stance = NeutralStance::new();
        let foot = |x: f32, z: f32| StanceFoot {
            position: Vec3::new(x, 0.1, z),
            rotation: Quat::IDENTITY,
        };
        let feet = [foot(-0.15, 0.02), foot(0.11, -0.04)];
        assert!(stance.observe(feet, true, 0.3).is_none());
        assert!(stance.observe(feet, false, 0.3).is_none());
        assert!(stance.observe(feet, true, 0.3).is_none());
        let measured = stance.observe(feet, true, 0.3).unwrap();
        assert!((measured[0].position.x + 0.13).abs() < 1e-5);
        assert!((measured[1].position.x - 0.13).abs() < 1e-5);
        assert!((measured[0].position.z + 0.01).abs() < 1e-5);
        assert!(stance.observe(feet, true, 0.3).is_none());
        assert_eq!(stance.get(), Some(measured));
    }
}
