//! The walk's tuning for a speed, and the neutral stance the feet stand and walk around. Pure.
//!
//! The stride, the duty factor, and the lift are set at a walking and a running speed and eased
//! between them, so a faster walk takes longer strides at a quicker cadence and spends less of each
//! cycle on the ground, until the run has a moment with both feet in the air.
//!
//! A crouch shortens the stride and lowers the lift by how far the hips have dropped, and keeps
//! both feet down for more of the cycle ([`CrouchGait`]): short, quick, low steps. At the same
//! speed a shorter stride is a quicker cadence, so the feet keep pace without lunging.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig, solver::feet::GaitTuning,
};

/// The walk's tuning at `speed` metres per second, or `None` below the speed the feet walk at. A
/// walk under way (`walking`) goes on down to [`STOP_SHARE`] of that speed, so a speed hovering
/// at the threshold does not switch between walking and standing every few ticks. `crouch` is how
/// far the hips have dropped toward their lowest, `0..=1` ([`crouch_depth`]), which the crouch
/// gait scales the walk by.
///
/// [`crouch_depth`]: crate::controllers::body::procedural::solver::hips::crouch_depth
pub fn tuning(
    speed: f32,
    walking: bool,
    crouch: f32,
    config: &ProceduralBodyConfig,
) -> Option<GaitTuning> {
    let threshold = if walking {
        config.gait_speed_mps * STOP_SHARE
    } else {
        config.gait_speed_mps
    };
    if !(config.gait && speed.is_finite() && speed >= threshold) {
        return None;
    }
    let span = (config.run_speed_mps - config.walk_speed_mps).max(1e-3);
    let t = ((speed - config.walk_speed_mps) / span).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    // Below the walking speed, the stride shortens with the speed, so a slow walk does not drag
    // the cadence down to a crawl.
    let slow = (speed / config.walk_speed_mps.max(1e-3)).clamp(MIN_SLOW_SHARE, 1.0);
    let upright = GaitTuning {
        stride_m: lerp(config.walk_stride_m, config.run_stride_m) * slow.sqrt(),
        duty: lerp(config.walk_duty, config.run_duty),
        height_m: lerp(config.walk_step_height_m, config.run_step_height_m),
    };
    Some(CrouchGait::of(config).scale(upright, crouch))
}

/// How a crouch changes the walk: at the hips' lowest, the stride and the lift are these shares of
/// the upright walk's, and each foot spends at least `duty` of the cycle on the ground.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CrouchGait {
    pub stride_share: f32,
    pub lift_share: f32,
    pub duty: f32,
}

impl CrouchGait {
    /// The crouch gait the settings ask for: no change with `crouch_gait` off.
    pub fn of(config: &ProceduralBodyConfig) -> Self {
        if config.crouch_gait {
            Self {
                stride_share: config.crouch_stride_share,
                lift_share: config.crouch_lift_share,
                duty: config.crouch_duty,
            }
        } else {
            Self {
                stride_share: 1.0,
                lift_share: 1.0,
                duty: 0.0,
            }
        }
    }

    /// `upright` scaled for a crouch `crouch` of the way to the hips' lowest, eased in so that a
    /// shallow dip of the head barely changes the walk.
    pub fn scale(&self, upright: GaitTuning, crouch: f32) -> GaitTuning {
        let c = if crouch.is_finite() {
            crouch.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let c = c * c * (3.0 - 2.0 * c);
        let lerp = |a: f32, b: f32| a + (b - a) * c;
        GaitTuning {
            stride_m: upright.stride_m * lerp(1.0, self.stride_share.clamp(0.1, 1.0)),
            duty: lerp(upright.duty, upright.duty.max(self.duty.clamp(0.0, 0.9))),
            height_m: upright.height_m * lerp(1.0, self.lift_share.clamp(0.0, 1.0)),
        }
    }

    /// The share of a standing step's lift a crouch `crouch` deep leaves.
    pub fn step_lift(&self, crouch: f32) -> f32 {
        let unit = GaitTuning {
            stride_m: 1.0,
            duty: 0.5,
            height_m: 1.0,
        };
        self.scale(unit, crouch).height_m
    }
}

/// A foot's place under the root when standing, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StanceFoot {
    pub position: Vec3,
    pub rotation: Quat,
}

/// The neutral stance: where the animation stands the feet when Rico has been still for a moment,
/// measured once and made symmetric, so that the feet walk and stand around the same place
/// whatever the animation's idle does with them afterwards. The feet's turns are kept only as a
/// mirrored toe-out about the root against the reference pose's feet: an idle's own twist of one
/// foot, and any tilt of the ground, are left out.
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

    /// Fold in a tick of the animated feet: measured once they have stood still on level ground
    /// for [`SETTLE_S`]. `reference` is the reference pose's feet's rotations, left then right.
    /// `still` must leave out aiming, whose stance is the weapon's. Returns the stance when this
    /// tick measured it.
    pub fn observe(
        &mut self,
        feet: [StanceFoot; 2],
        reference: [Quat; 2],
        still: bool,
        dt: f32,
    ) -> Option<[StanceFoot; 2]> {
        if self.stance.is_some() {
            return None;
        }
        let level = (feet[0].position.y - feet[1].position.y).abs() <= MAX_LEVEL_DIFFERENCE_M;
        if !still
            || !level
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
        // The toe-out, outward positive: the left foot's yaw is outward when positive about +Y
        // (toward -X when facing -Z), the right's when negative.
        let toe_out =
            (yaw_from(left.rotation, reference[0]) - yaw_from(right.rotation, reference[1])) * 0.5;
        let toe_out = if toe_out.is_finite() {
            toe_out.clamp(-MAX_TOE_OUT, MAX_TOE_OUT)
        } else {
            0.0
        };
        let stance = [
            StanceFoot {
                position: Vec3::new(-half_width, height, depth),
                rotation: (Quat::from_rotation_y(toe_out) * reference[0]).normalize(),
            },
            StanceFoot {
                position: Vec3::new(half_width, height, depth),
                rotation: (Quat::from_rotation_y(-toe_out) * reference[1]).normalize(),
            },
        ];
        self.stance = Some(stance);
        Some(stance)
    }
}

/// The share of the walking threshold a walk under way goes on down to.
pub const STOP_SHARE: f32 = 0.7;

/// How long the feet stand still before the stance is measured, seconds.
pub const SETTLE_S: f32 = 0.5;

/// The most the feet's heights may differ for the ground under them to count as level, metres.
const MAX_LEVEL_DIFFERENCE_M: f32 = 0.03;

/// The most toe-out the stance keeps, radians: about thirty degrees.
const MAX_TOE_OUT: f32 = 0.52;

/// How far `rotation` is turned about +Y from `reference`, radians: the yaw of the reference
/// forward (-Z) carried by the turn between them.
fn yaw_from(rotation: Quat, reference: Quat) -> f32 {
    let forward = (rotation * reference.inverse()) * Vec3::NEG_Z;
    if forward.x.abs() + forward.z.abs() < 1e-6 {
        return 0.0;
    }
    (-forward.x).atan2(-forward.z)
}

/// The shortest a slow walk's stride gets, as a share of the walking stride's square.
const MIN_SLOW_SHARE: f32 = 0.25;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faster_walks_take_longer_strides_and_less_ground() {
        let config = ProceduralBodyConfig::new();
        assert!(tuning(config.gait_speed_mps * 0.5, false, 0.0, &config).is_none());
        let walk = tuning(config.walk_speed_mps, false, 0.0, &config).unwrap();
        let run = tuning(config.run_speed_mps, false, 0.0, &config).unwrap();
        let sprint = tuning(config.run_speed_mps * 2.0, false, 0.0, &config).unwrap();
        assert!((walk.stride_m - config.walk_stride_m).abs() < 1e-4);
        assert!(run.stride_m > walk.stride_m);
        assert!(run.duty < 0.5 && walk.duty > 0.5);
        assert_eq!(run, sprint);
        // The cadence rises with the speed.
        let cadence = |speed: f32| speed / tuning(speed, false, 0.0, &config).unwrap().stride_m;
        let mut last = 0.0;
        for i in 1..=40 {
            let speed = config.gait_speed_mps + i as f32 * 0.15;
            let c = cadence(speed);
            assert!(c >= last - 1e-4, "cadence fell at {speed}");
            last = c;
        }
    }

    #[test]
    fn a_crouch_takes_short_quick_low_steps() {
        let config = ProceduralBodyConfig::new();
        let speed = config.walk_speed_mps;
        let upright = tuning(speed, false, 0.0, &config).unwrap();
        let half = tuning(speed, false, 0.5, &config).unwrap();
        let low = tuning(speed, false, 1.0, &config).unwrap();
        // Standing, the crouch gait changes nothing.
        assert!((upright.stride_m - config.walk_stride_m).abs() < 1e-4);
        // At the hips' lowest, the shares of the upright walk.
        assert!((low.stride_m - upright.stride_m * config.crouch_stride_share).abs() < 1e-4);
        assert!((low.height_m - upright.height_m * config.crouch_lift_share).abs() < 1e-4);
        assert!(low.duty >= config.crouch_duty - 1e-4);
        // Halfway down, between the two.
        assert!(half.stride_m < upright.stride_m && half.stride_m > low.stride_m);
        assert!(half.height_m < upright.height_m && half.height_m > low.height_m);
        // The same speed over a shorter stride is a quicker cadence.
        assert!(speed / low.stride_m > speed / upright.stride_m * 1.5);
        // Deeper than the lowest is the lowest, and a depth that is not a number is upright.
        assert_eq!(tuning(speed, false, 3.0, &config), Some(low));
        assert_eq!(tuning(speed, false, f32::NAN, &config), Some(upright));
        // Switched off, a crouch walks as upright.
        let off = ProceduralBodyConfig {
            crouch_gait: false,
            ..config
        };
        assert_eq!(tuning(speed, false, 1.0, &off), Some(upright));
        assert_eq!(CrouchGait::of(&off).step_lift(1.0), 1.0);
        assert!((CrouchGait::of(&config).step_lift(1.0) - config.crouch_lift_share).abs() < 1e-4);
    }

    #[test]
    fn a_walk_under_way_goes_on_a_little_below_the_speed_it_starts_at() {
        let config = ProceduralBodyConfig::new();
        let between = config.gait_speed_mps * (1.0 + STOP_SHARE) * 0.5;
        assert!(tuning(between, false, 0.0, &config).is_none());
        assert!(tuning(between, true, 0.0, &config).is_some());
        assert!(tuning(config.gait_speed_mps * STOP_SHARE * 0.9, true, 0.0, &config).is_none());
    }

    #[test]
    fn the_stance_is_measured_once_after_settling_and_symmetric() {
        let mut stance = NeutralStance::new();
        let foot = |x: f32, z: f32| StanceFoot {
            position: Vec3::new(x, 0.1, z),
            rotation: Quat::IDENTITY,
        };
        let feet = [foot(-0.15, 0.02), foot(0.11, -0.04)];
        let reference = [Quat::IDENTITY; 2];
        assert!(stance.observe(feet, reference, true, 0.3).is_none());
        assert!(stance.observe(feet, reference, false, 0.3).is_none());
        assert!(stance.observe(feet, reference, true, 0.3).is_none());
        let measured = stance.observe(feet, reference, true, 0.3).unwrap();
        assert!((measured[0].position.x + 0.13).abs() < 1e-5);
        assert!((measured[1].position.x - 0.13).abs() < 1e-5);
        assert!((measured[0].position.z + 0.01).abs() < 1e-5);
        assert!(stance.observe(feet, reference, true, 0.3).is_none());
        assert_eq!(stance.get(), Some(measured));
    }

    #[test]
    fn the_stance_mirrors_the_toe_out_and_drops_the_idles_twist_and_tilt() {
        let mut stance = NeutralStance::new();
        let reference = [Quat::IDENTITY; 2];
        // The left foot turned out by 0.3 and tilted, the right turned in by 0.1.
        let feet = [
            StanceFoot {
                position: Vec3::new(-0.12, 0.1, 0.0),
                rotation: Quat::from_rotation_y(0.3) * Quat::from_rotation_x(0.2),
            },
            StanceFoot {
                position: Vec3::new(0.12, 0.1, 0.0),
                rotation: Quat::from_rotation_y(0.1),
            },
        ];
        stance.observe(feet, reference, true, 0.3);
        let measured = stance.observe(feet, reference, true, 0.3).unwrap();
        let left = measured[0].rotation * Vec3::NEG_Z;
        let right = measured[1].rotation * Vec3::NEG_Z;
        assert!(left.y.abs() < 1e-5 && right.y.abs() < 1e-5);
        assert!((left.x + right.x).abs() < 1e-5, "{left} {right}");
        assert!(left.x < 0.0, "the left foot does not turn out: {left}");
    }

    #[test]
    fn uneven_ground_is_not_measured() {
        let mut stance = NeutralStance::new();
        let foot = |x: f32, y: f32| StanceFoot {
            position: Vec3::new(x, y, 0.0),
            rotation: Quat::IDENTITY,
        };
        let feet = [foot(-0.12, 0.1), foot(0.12, 0.25)];
        for _ in 0..10 {
            assert!(
                stance
                    .observe(feet, [Quat::IDENTITY; 2], true, 0.3)
                    .is_none()
            );
        }
    }
}
