//! The legs in the air and the landing. Pure, in Rico's model space.
//!
//! Off the ground within the jump's grace (a jump, a short fall), the feet hang under the hips
//! rather than standing on the ground: tucked up while Rico rises, and let down toward a landing as
//! he falls ([`air_feet`]). [`AirFollower`] eases the legs between the ground's feet and the air's,
//! quickly back down so a landing meets the ground. On landing, the hips compress and recover
//! ([`LandingDip`]): a critically damped spring kicked by the speed of the fall.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    rig::{Rig, RigBone},
    solver::feet::FootPose,
};

/// How the legs hang in the air.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct AirTuning {
    /// The hip-to-ankle height while rising, as a share of the standing leg's: the tuck.
    pub tuck: f32,
    /// The hip-to-ankle height while falling at `fall_speed_mps` or faster: the legs let down.
    pub hang: f32,
    /// The falling speed at which the legs hang fully, metres per second.
    pub fall_speed_mps: f32,
    /// How far the toes point down with the legs let down, radians.
    pub toe_drop: f32,
}

/// Where the feet hang in the air: under `stance` (the neutral stance, model space), with the
/// ankles below the hips at `hips_height` by the tuck or the hang for the vertical speed
/// `vertical_mps` (up positive).
pub fn air_feet(
    rig: &Rig,
    stance: [FootPose; 2],
    hips_height: f32,
    vertical_mps: f32,
    tuning: &AirTuning,
) -> [FootPose; 2] {
    let falling = if tuning.fall_speed_mps > 0.0 && vertical_mps.is_finite() {
        (-vertical_mps / tuning.fall_speed_mps).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let share = tuning.tuck + (tuning.hang - tuning.tuck) * falling;
    let hips = rig.reference(RigBone::Hips).translation.y;
    let toe_drop = Quat::from_rotation_x(-tuning.toe_drop * falling);
    let legs = [
        (RigBone::LeftUpLeg, RigBone::LeftFoot),
        (RigBone::RightUpLeg, RigBone::RightFoot),
    ];
    [0, 1].map(|i| {
        let (up, foot) = legs[i];
        let hip_joint = hips - rig.reference(up).translation.y;
        let leg = rig.reference(up).translation.y - rig.reference(foot).translation.y;
        FootPose {
            position: Vec3::new(
                stance[i].position.x,
                hips_height - hip_joint - leg * share.clamp(0.0, 1.0),
                stance[i].position.z,
            ),
            rotation: (toe_drop * stance[i].rotation).normalize(),
        }
    })
}

/// The feet mixed from `ground` toward `air` by `weight`.
pub fn mix_feet(ground: [FootPose; 2], air: [FootPose; 2], weight: f32) -> [FootPose; 2] {
    let w = weight.clamp(0.0, 1.0);
    [0, 1].map(|i| FootPose {
        position: ground[i].position.lerp(air[i].position, w),
        rotation: ground[i].rotation.slerp(air[i].rotation, w).normalize(),
    })
}

/// Eases the legs between the ground's feet (zero) and the air's (one).
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AirFollower {
    weight: f32,
}

impl AirFollower {
    pub const fn new() -> Self {
        Self { weight: 0.0 }
    }

    pub fn reset(&mut self) {
        self.weight = 0.0;
    }

    /// Move toward the air when `airborne` and back to the ground otherwise, over `dt` seconds,
    /// and return the weight.
    pub fn step(&mut self, airborne: bool, dt: f32) -> f32 {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let (target, time) = if airborne {
            (1.0, AIR_IN_S)
        } else {
            (0.0, AIR_OUT_S)
        };
        let k = 1.0 - (-dt / time).exp();
        self.weight += (target - self.weight) * k;
        if (self.weight - target).abs() < SETTLE {
            self.weight = target;
        }
        self.weight
    }
}

/// The landing's dip of the hips.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DipTuning {
    /// How deep the dip goes per metre per second of falling speed past `min_speed_mps`, metres.
    pub per_mps: f32,
    /// The deepest dip, metres.
    pub max_m: f32,
    /// The falling speed below which a landing does not dip, metres per second.
    pub min_speed_mps: f32,
    /// How long after the landing the dip is deepest, seconds.
    pub time_s: f32,
}

/// The hips' landing dip: a critically damped spring on the drop, at rest at zero, kicked
/// downward by a landing.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LandingDip {
    /// The drop, metres down.
    drop: f32,
    /// The drop's rate, metres per second down.
    rate: f32,
}

impl LandingDip {
    pub const fn new() -> Self {
        Self {
            drop: 0.0,
            rate: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Kick the spring for a landing at `fall_mps` (downward positive), so the dip is deepest
    /// `tuning.time_s` later. Returns the depth it will reach from rest.
    pub fn land(&mut self, fall_mps: f32, tuning: &DipTuning) -> f32 {
        let depth = if fall_mps.is_finite() {
            ((fall_mps - tuning.min_speed_mps).max(0.0) * tuning.per_mps.max(0.0))
                .min(tuning.max_m.max(0.0))
        } else {
            0.0
        };
        if depth > 0.0 && tuning.time_s > 0.0 {
            // From rest, `x(t) = v t e^(-t/T)` peaks at `t = T` with `v T / e`.
            self.rate += depth * std::f32::consts::E / tuning.time_s;
        }
        depth
    }

    /// Advance by `dt` seconds with the spring's time `time_s`, and return the drop.
    pub fn step(&mut self, dt: f32, time_s: f32) -> f32 {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if time_s <= 0.0 {
            self.reset();
            return 0.0;
        }
        // The exact step of a critically damped spring toward zero.
        let omega = 1.0 / time_s;
        let decay = (-omega * dt).exp();
        let carry = (self.rate + omega * self.drop) * dt;
        self.drop = (self.drop + carry) * decay;
        self.rate = (self.rate - omega * carry) * decay;
        if self.drop.abs() < SETTLE * 1e-2 && self.rate.abs() < SETTLE {
            self.reset();
        }
        if !(self.drop.is_finite() && self.rate.is_finite()) {
            self.reset();
        }
        self.drop.max(0.0)
    }
}

/// How quickly the legs go to the air, seconds: the follower's time constant.
const AIR_IN_S: f32 = 0.12;

/// How quickly the legs come back to the ground, seconds: quicker, so a landing meets it.
const AIR_OUT_S: f32 = 0.05;

const SETTLE: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::rig::tests::skeleton;

    fn setup() -> (Rig, [FootPose; 2], AirTuning) {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let stance = [RigBone::LeftFoot, RigBone::RightFoot].map(|bone| FootPose {
            position: rig.reference(bone).translation,
            rotation: rig.reference(bone).rotation,
        });
        let tuning = AirTuning {
            tuck: 0.75,
            hang: 0.95,
            fall_speed_mps: 4.0,
            toe_drop: 0.3,
        };
        (rig, stance, tuning)
    }

    #[test]
    fn rising_tucks_the_legs_and_falling_lets_them_down() {
        let (rig, stance, tuning) = setup();
        let rising = air_feet(&rig, stance, 1.0, 3.0, &tuning);
        let falling = air_feet(&rig, stance, 1.0, -6.0, &tuning);
        // The test leg is 0.84 from the hip joint to the ankle, the hip joint 0.08 below the hips.
        assert!((rising[0].position.y - (1.0 - 0.08 - 0.84 * 0.75)).abs() < 1e-4);
        assert!((falling[1].position.y - (1.0 - 0.08 - 0.84 * 0.95)).abs() < 1e-4);
        assert_eq!(rising[0].position.x, stance[0].position.x);
        assert_eq!(rising[0].rotation, stance[0].rotation);
        assert!(falling[0].rotation.angle_between(stance[0].rotation) > 0.29);
        // Lower hips carry the feet down with them.
        let lower = air_feet(&rig, stance, 0.8, 3.0, &tuning);
        assert!((rising[0].position.y - lower[0].position.y - 0.2).abs() < 1e-4);
    }

    #[test]
    fn the_air_follower_goes_up_smoothly_and_comes_down_quicker() {
        let mut follower = AirFollower::new();
        let first = follower.step(true, 1.0 / 60.0);
        assert!(first > 0.0 && first < 0.5);
        let up = (1..120)
            .position(|_| follower.step(true, 1.0 / 60.0) >= 0.9)
            .unwrap();
        for _ in 0..120 {
            follower.step(true, 1.0 / 60.0);
        }
        assert_eq!(follower.step(true, 1.0 / 60.0), 1.0);
        let down = (0..120)
            .position(|_| follower.step(false, 1.0 / 60.0) <= 0.1)
            .unwrap();
        assert!(down < up, "{down} >= {up}");
    }

    #[test]
    fn a_landing_dips_by_its_speed_and_recovers() {
        let tuning = DipTuning {
            per_mps: 0.02,
            max_m: 0.15,
            min_speed_mps: 1.5,
            time_s: 0.12,
        };
        let mut dip = LandingDip::new();
        assert_eq!(dip.land(1.0, &tuning), 0.0);
        assert_eq!(dip.step(1.0 / 60.0, tuning.time_s), 0.0);
        let depth = dip.land(6.5, &tuning);
        assert!((depth - 0.1).abs() < 1e-5);
        let drops: Vec<f32> = (0..120)
            .map(|_| dip.step(1.0 / 120.0, tuning.time_s))
            .collect();
        let (peak_at, peak) = drops
            .iter()
            .copied()
            .enumerate()
            .fold((0, 0.0f32), |a, (i, d)| if d > a.1 { (i, d) } else { a });
        assert!((peak - depth).abs() < 2e-3, "{peak}");
        assert!(((peak_at + 1) as f32 / 120.0 - tuning.time_s).abs() < 0.02);
        assert!(*drops.last().unwrap() < 0.01);
        assert!(drops.iter().all(|d| *d >= 0.0));
        // A hard fall is capped.
        let mut dip = LandingDip::new();
        assert_eq!(dip.land(40.0, &tuning), tuning.max_m);
        assert_eq!(dip.land(f32::NAN, &tuning), 0.0);
    }

    #[test]
    fn feet_mix_between_the_ground_and_the_air() {
        let (rig, stance, tuning) = setup();
        let air = air_feet(&rig, stance, 1.0, 3.0, &tuning);
        assert_eq!(mix_feet(stance, air, 0.0), stance);
        let half = mix_feet(stance, air, 0.5);
        assert!(
            half[0]
                .position
                .abs_diff_eq((stance[0].position + air[0].position) * 0.5, 1e-5)
        );
    }
}
