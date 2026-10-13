//! The pelvis's horizontal place: over the stance the root carries, hinged back by a share of the
//! lean the chase leaves alone, leaning into the body's motion, and followed by a stiff spring.
//! Pure, in Rico's model space, `(x, z)`.
//!
//! The head is tracked, so leaning the body into its motion is done from below: the pelvis moves
//! back along the motion by the spine's length times the sine of the lean, and the spine solve
//! leans the chest forward to the fixed neck. The lean grows with the measured speed and with the
//! forward acceleration (a start leans in, a stop leans back), and a sideways acceleration rolls
//! the body into a turn. The stick's commanded velocity never feeds it: only how the root actually
//! moved.
//!
//! The spring is critically damped and short, so the pelvis is held under the neck pivot while a
//! quick lean still bends the spine first.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::{
    procedural::{
        config::ProceduralBodyConfig,
        rig::{Rig, RigBone},
        solver::{SolveInput, hips_height, hips_yaw},
    },
    rig::config::PelvisConfig,
};

/// Where the pelvis rests before the hinge and the motion: over the middle of the stance the root
/// carries, drawn [`PLANTED_BIAS`] of the way toward the one foot on the ground while the other
/// swings, and sent back over the heels by [`SQUAT_BACK`] of the hips' drop in a squat.
pub fn rest(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> Vec2 {
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let support = Vec2::new(input.support.x, input.support.z);
    let yaw = hips_yaw(input, config);
    let bias = match input.planted {
        [true, false] => Some(0),
        [false, true] => Some(1),
        _ => None,
    }
    .map_or(Vec2::ZERO, |i| {
        let foot = input.feet[i];
        Vec2::new(foot.x - support.x, 0.0) * PLANTED_BIAS
    });
    let squat = (reference_hips.y - hips_height(rig, input.hips_head(), config)).max(0.0);
    let back = Quat::from_rotation_y(yaw) * Vec3::Z * (squat * SQUAT_BACK);
    let place = support + bias + Vec2::new(back.x, back.z);
    if place.is_finite() { place } else { support }
}

/// The pelvis's target: the rest place, moved back by the configured share of the `lean` the chase
/// leaves alone (the hip hinge) and by the motion lean's `motion` offset.
pub fn target(rest: Vec2, lean: Vec2, motion: Vec2, config: &PelvisConfig) -> Vec2 {
    let lean = if lean.is_finite() { lean } else { Vec2::ZERO };
    let motion = if motion.is_finite() {
        motion
    } else {
        Vec2::ZERO
    };
    rest - lean * config.hinge_share.clamp(0.0, 1.0) - motion
}

/// The spine's length the motion lean swings over, metres: from the hips to the neck in the
/// reference pose.
pub fn spine_length(rig: &Rig) -> f32 {
    (rig.reference(RigBone::Neck).translation - rig.reference(RigBone::Hips).translation).length()
}

/// The body's lean into its motion on one tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct MotionLeanOut {
    /// How far the pelvis moves, `(x, z)`, along the motion and toward the outside of a turn.
    pub offset: Vec2,
    /// The forward lean, radians.
    pub pitch: f32,
    /// The roll into a turn, radians.
    pub roll: f32,
}

/// The measured velocity and acceleration, smoothed.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct MotionLean {
    velocity: Option<Vec2>,
    acceleration: Vec2,
}

impl MotionLean {
    pub const fn new() -> Self {
        Self {
            velocity: None,
            acceleration: Vec2::ZERO,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advance with the root's measured `velocity` over `dt` seconds, `(x, z)` metres per second,
    /// for a spine `spine_m` long.
    pub fn step(
        &mut self,
        velocity: Vec2,
        dt: f32,
        spine_m: f32,
        config: &PelvisConfig,
    ) -> MotionLeanOut {
        let velocity = if velocity.is_finite() {
            velocity
        } else {
            Vec2::ZERO
        };
        let alpha = ease(dt, config.motion_smoothing_s);
        let previous = self.velocity.unwrap_or(velocity);
        let smoothed = previous.lerp(velocity, alpha);
        if dt > 0.0 && dt.is_finite() {
            let raw = (smoothed - previous) / dt;
            self.acceleration = self.acceleration.lerp(raw, alpha);
        }
        self.velocity = Some(smoothed);
        lean(smoothed, self.acceleration, spine_m, config)
    }
}

/// The lean for a smoothed `velocity` and `acceleration`.
pub fn lean(
    velocity: Vec2,
    acceleration: Vec2,
    spine_m: f32,
    config: &PelvisConfig,
) -> MotionLeanOut {
    let speed = velocity.length();
    let forward = if speed > MIN_LEAN_SPEED_MPS {
        velocity / speed
    } else {
        acceleration.normalize_or_zero()
    };
    let right = Vec2::new(-forward.y, forward.x);
    let a_forward = acceleration.dot(forward);
    let a_side = acceleration.dot(right);
    let pitch = (config.speed_lean * speed).clamp(0.0, config.max_speed_lean)
        + (config.accel_lean * a_forward).clamp(-config.max_accel_lean, config.max_accel_lean);
    let roll = (config.accel_lean * a_side).clamp(-config.max_roll, config.max_roll);
    let spine = if spine_m.is_finite() {
        spine_m.max(0.0)
    } else {
        0.0
    };
    let offset = forward * (spine * pitch.sin()) + right * (spine * roll.sin());
    MotionLeanOut {
        offset: if offset.is_finite() {
            offset
        } else {
            Vec2::ZERO
        },
        pitch,
        roll,
    }
}

/// The pelvis's critically damped spring.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PelvisSpring {
    place: Option<Vec2>,
    velocity: Vec2,
}

impl PelvisSpring {
    pub const fn new() -> Self {
        Self {
            place: None,
            velocity: Vec2::ZERO,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Start from `place`, at rest: the pelvis starts where something else left it.
    pub fn seed(&mut self, place: Vec2) {
        self.place = place.is_finite().then_some(place);
        self.velocity = Vec2::ZERO;
    }

    /// Move toward `target` over `dt` seconds with the time constant `tau`, and return the place.
    /// The first step takes `target` as it is.
    pub fn step(&mut self, target: Vec2, dt: f32, tau: f32) -> Vec2 {
        if !target.is_finite() {
            return self.place.unwrap_or(Vec2::ZERO);
        }
        let Some(mut place) = self.place else {
            self.place = Some(target);
            return target;
        };
        if !(tau.is_finite() && tau > 1e-4) || !dt.is_finite() {
            self.place = Some(target);
            self.velocity = Vec2::ZERO;
            return target;
        }
        let omega = 1.0 / tau;
        let steps = (dt.max(0.0) / SPRING_STEP_S).ceil().max(1.0) as usize;
        let h = dt.max(0.0) / steps as f32;
        for _ in 0..steps {
            let accel = (target - place) * (omega * omega) - self.velocity * (2.0 * omega);
            self.velocity += accel * h;
            place += self.velocity * h;
        }
        self.place = Some(place);
        place
    }
}

/// How far back the hips go per metre they drop below standing.
pub const SQUAT_BACK: f32 = 0.3;

/// How far toward the one foot on the ground the hips are drawn while the other swings, as a share
/// of that foot's sideways offset from the stance's middle: a walk's small sway over the stance
/// foot.
pub const PLANTED_BIAS: f32 = 0.15;

/// The speed below which the lean's forward is the acceleration's, metres per second.
const MIN_LEAN_SPEED_MPS: f32 = 0.05;

/// The longest step the spring integrates at once, seconds.
const SPRING_STEP_S: f32 = 0.005;

fn ease(dt: f32, tau: f32) -> f32 {
    if !dt.is_finite() || dt <= 0.0 {
        return 0.0;
    }
    if !(tau.is_finite() && tau > 1e-4) {
        return 1.0;
    }
    1.0 - (-dt / tau).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: PelvisConfig = PelvisConfig::new();
    const SPINE: f32 = 0.55;

    #[test]
    fn the_lean_grows_with_the_speed_and_the_acceleration() {
        let mut last = -1.0;
        for speed in [0.0, 0.5, 1.0, 2.0, 3.0, 5.0, 8.0] {
            let out = lean(Vec2::new(0.0, -speed), Vec2::ZERO, SPINE, &CONFIG);
            assert!(out.pitch >= last, "{speed}: {out:?}");
            last = out.pitch;
        }
        assert!((last - CONFIG.max_speed_lean).abs() < 1e-6);
        let mut last = -1.0;
        for accel in [-10.0, -3.0, 0.0, 2.0, 4.0, 10.0] {
            let out = lean(Vec2::new(0.0, -1.0), Vec2::new(0.0, -accel), SPINE, &CONFIG);
            assert!(out.pitch >= last, "{accel}: {out:?}");
            last = out.pitch;
        }
    }

    #[test]
    fn a_forward_lean_moves_the_pelvis_along_the_motion_and_a_turn_rolls_it() {
        let out = lean(Vec2::new(0.0, -2.0), Vec2::ZERO, SPINE, &CONFIG);
        assert!(out.offset.y < 0.0 && out.offset.x.abs() < 1e-6, "{out:?}");
        // Turning left while moving forward: the acceleration points left.
        let turning = lean(Vec2::new(0.0, -2.0), Vec2::new(-3.0, 0.0), SPINE, &CONFIG);
        assert!(turning.roll.abs() > 0.0 && turning.roll.abs() <= CONFIG.max_roll + 1e-6);
        assert!(turning.offset.x < 0.0, "{turning:?}");
        assert_eq!(
            lean(Vec2::ZERO, Vec2::ZERO, SPINE, &CONFIG).offset,
            Vec2::ZERO
        );
    }

    #[test]
    fn the_measured_motion_is_smoothed() {
        let mut motion = MotionLean::new();
        let dt = 1.0 / 30.0;
        motion.step(Vec2::ZERO, dt, SPINE, &CONFIG);
        let start = motion.step(Vec2::new(0.0, -2.0), dt, SPINE, &CONFIG);
        assert!(start.pitch > 0.0, "a start leans in: {start:?}");
        let mut out = start;
        for _ in 0..60 {
            out = motion.step(Vec2::new(0.0, -2.0), dt, SPINE, &CONFIG);
        }
        assert!(
            (out.pitch - 2.0 * CONFIG.speed_lean).abs() < 1e-3,
            "{out:?}"
        );
    }

    #[test]
    fn the_hinge_moves_the_target_back_from_the_lean() {
        let target = target(Vec2::ZERO, Vec2::new(0.0, -0.3), Vec2::ZERO, &CONFIG);
        assert!((target - Vec2::new(0.0, 0.09)).length() < 1e-6);
    }

    #[test]
    fn the_spring_settles_without_overshooting() {
        let mut spring = PelvisSpring::new();
        spring.seed(Vec2::ZERO);
        let mut furthest: f32 = 0.0;
        for _ in 0..90 {
            let place = spring.step(Vec2::new(0.2, 0.0), 1.0 / 90.0, CONFIG.follow_s);
            furthest = furthest.max(place.x);
        }
        assert!(furthest <= 0.2 + 1e-4);
        assert!(
            (spring
                .step(Vec2::new(0.2, 0.0), 1.0 / 90.0, CONFIG.follow_s)
                .x
                - 0.2)
                .abs()
                < 1e-3
        );
    }
}
