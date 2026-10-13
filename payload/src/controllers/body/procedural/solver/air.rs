//! The legs in the air and the landing. Pure, in Rico's model space.
//!
//! Off the ground within the jump's grace (a jump, a short fall), the feet hang under the hips
//! rather than standing on the ground: tucked up while Rico rises, and let down toward a landing as
//! he falls ([`air_feet`]). [`AirFollower`] eases the legs between the ground's feet and the air's,
//! quickly back down so a landing meets the ground. On landing, the hips compress and recover
//! ([`LandingDip`]): a critically damped spring kicked by the speed of the fall.

use glam::{Quat, Vec3};

use crate::controllers::body::{
    procedural::rig::{Rig, RigBone},
    rig::legs::swing::FootPose,
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

/// How the legs reach for a landing.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReachTuning {
    /// How long before touching down the legs reach fully for the ground, seconds; they start
    /// reaching twice as long before.
    pub time_s: f32,
}

/// The feet in the air (`air`, model space) reaching for the ground below them as Rico falls:
/// each foot with ground below it (`ground`, its height in model space) goes down toward it by how
/// soon the fall at `vertical_mps` (up positive) reaches it, all the way when it is
/// `tuning.time_s` or less away, and as far as its leg reaches straight from the hips at
/// `hips_height`; its toes come level as it does. `stance` gives the ankle's height over the
/// ground. Rising, or with no ground below, a foot hangs as it was. Returns the feet, and how far
/// the further-reaching one has gone toward the ground, `0..=1`.
pub fn reach_for_landing(
    rig: &Rig,
    air: [FootPose; 2],
    stance: [FootPose; 2],
    hips_height: f32,
    ground: [Option<f32>; 2],
    vertical_mps: f32,
    tuning: &ReachTuning,
) -> ([FootPose; 2], f32) {
    let fall = -vertical_mps;
    if !(fall.is_finite() && fall > 0.0 && tuning.time_s > 0.0 && hips_height.is_finite()) {
        return (air, 0.0);
    }
    let hips = rig.reference(RigBone::Hips).translation.y;
    let legs = [
        (RigBone::LeftUpLeg, RigBone::LeftFoot),
        (RigBone::RightUpLeg, RigBone::RightFoot),
    ];
    let mut reached: f32 = 0.0;
    let feet = [0, 1].map(|i| {
        let Some(ground) = ground[i].filter(|g| g.is_finite()) else {
            return air[i];
        };
        let (up, foot) = legs[i];
        let hip_joint = hips_height - (hips - rig.reference(up).translation.y);
        let leg = rig.reference(up).translation.y - rig.reference(foot).translation.y;
        let lowest = hip_joint - leg * REACH_EXTENSION;
        let touch = ground + stance[i].position.y;
        let hanging = air[i].position.y;
        let until = (hanging - touch).max(0.0) / fall;
        let w = (1.0 - (until - tuning.time_s) / tuning.time_s).clamp(0.0, 1.0);
        let w = w * w * (3.0 - 2.0 * w);
        if w <= 0.0 {
            return air[i];
        }
        // A foot already below its landing (the ground came up under it) waits for the landing
        // rather than being pulled up through the tuck.
        let target = touch.max(lowest).min(hanging);
        reached = reached.max(w);
        FootPose {
            position: Vec3::new(
                air[i].position.x,
                hanging + (target - hanging) * w,
                air[i].position.z,
            ),
            rotation: air[i].rotation.slerp(stance[i].rotation, w).normalize(),
        }
    });
    (feet, reached)
}

/// The share of the leg's length a foot reaching for a landing stretches to: a leg never locks
/// quite straight.
const REACH_EXTENSION: f32 = 0.97;

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
    fn falling_legs_reach_for_the_ground_as_it_nears() {
        let (rig, stance, tuning) = setup();
        let reach = ReachTuning { time_s: 0.25 };
        // Hips a metre and a half up, falling at four metres a second: the hanging ankles are
        // about 0.7 m up.
        let hips = 1.5;
        let air = air_feet(&rig, stance, hips, -4.0, &tuning);
        let ankle = stance[0].position.y;
        // Far above the ground, the legs hang.
        let (far, w) = reach_for_landing(&rig, air, stance, hips, [Some(-5.0); 2], -4.0, &reach);
        assert_eq!(far, air);
        assert_eq!(w, 0.0);
        // Within the reach time, the feet go down toward it, at most as far as the legs reach.
        let (near, w) = reach_for_landing(&rig, air, stance, hips, [Some(0.0); 2], -4.0, &reach);
        assert_eq!(w, 1.0);
        let hip_joint = hips - 0.08;
        let lowest = hip_joint - 0.84 * REACH_EXTENSION;
        for foot in near {
            assert!(
                (foot.position.y - lowest.max(ankle)).abs() < 1e-4,
                "{foot:?}"
            );
        }
        // The toes come level for the landing.
        assert!(near[0].rotation.angle_between(stance[0].rotation) < 1e-3);
        // Between, part of the way, more of it nearer.
        let between =
            |g: f32| reach_for_landing(&rig, air, stance, hips, [Some(g); 2], -4.0, &reach).1;
        let (a, b) = (between(-1.2), between(-0.9));
        assert!(a > 0.0 && a < b && b < 1.0, "{a} {b}");
        // Rising, or without ground under one foot, that foot hangs.
        assert_eq!(
            reach_for_landing(&rig, air, stance, hips, [Some(0.0); 2], 2.0, &reach).0,
            air
        );
        let (one, _) = reach_for_landing(&rig, air, stance, hips, [None, Some(0.0)], -4.0, &reach);
        assert_eq!(one[0], air[0]);
        assert_ne!(one[1], air[1]);
        // Ground above the hanging foot (the tuck reaches the ledge) does not pull it up.
        let (high, _) = reach_for_landing(&rig, air, stance, hips, [Some(1.2); 2], -4.0, &reach);
        assert!(high[0].position.y <= air[0].position.y + 1e-6);
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
