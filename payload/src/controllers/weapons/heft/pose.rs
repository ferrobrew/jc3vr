//! The weighted pose a gun's weight puts on the holding hand, and the damped spring the shown gun
//! follows it on.

use std::f32::consts::TAU;

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    frame::Hand,
    hands::CockpitHand,
    weapons::{
        config::{BracedTuning, HeftConfig, HeftTuning},
        foregrip,
        heft::{HeftClass, lerp, shortest, smoothstep},
    },
};

/// The weighted pose of a gun held at `grip`, before the spring.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WeightedPose {
    /// The sagged and swayed grip orientation.
    pub orientation: Quat,
    /// The sag applied, radians.
    pub sag: f32,
    /// The hand's reach, `0..=1`.
    pub reach: f32,
    /// The sway asked for at full share, pitch and yaw, radians.
    pub sway: Vec2,
    /// The share of the sway the reach lets through, `0..=1`.
    pub sway_reach: f32,
}

/// What a gun's weight asks of its pose: its one-handed and its braced tunings blended by how
/// braced it is.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Weight {
    /// The sag close in, degrees.
    pub sag_deg: f32,
    /// The further sag at full reach, degrees.
    pub reach_sag_deg: f32,
    /// The sway's amplitude, degrees.
    pub sway_deg: f32,
    /// How much of the sway shows, `0..=1`.
    pub sway_share: f32,
    /// The spring's stiffness, per second squared; zero follows rigidly.
    pub stiffness: f32,
    pub damping_ratio: f32,
    /// The furthest the gun may turn from where the hands turn it, degrees.
    pub max_angle_deg: f32,
}

impl Weight {
    /// The weight of a gun of `class` with the unbraced `share` (one in one hand, zero braced):
    /// each of the one-handed and the braced weights, where its switch in `config` is on, blended
    /// by the share. The sag and the limits blend linearly; the spring blends by its natural
    /// frequency, a rigid side counting as a very stiff spring, so that the lag shrinks smoothly as
    /// the foregrip engages.
    pub fn of(class: HeftClass, config: &HeftConfig, share: f32) -> Self {
        let one = if config.enabled {
            class.tuning(config)
        } else {
            HeftTuning::NONE
        };
        let braced = if config.braced.enabled {
            class
                .braced(&config.braced)
                .at_strength(config.braced.strength)
        } else {
            BracedTuning::NONE
        };
        Self::blend(&one, &braced, share, config)
    }

    /// The blend of the one-handed `one` and the braced `braced` by the unbraced `share`.
    pub fn blend(one: &HeftTuning, braced: &BracedTuning, share: f32, config: &HeftConfig) -> Self {
        let share = if share.is_finite() {
            share.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let (one_k, braced_k) = (one.lag_stiffness.max(0.0), braced.stiffness.max(0.0));
        let stiffness = if share >= 1.0 {
            one_k
        } else if share <= 0.0 {
            braced_k
        } else if one_k == 0.0 && braced_k == 0.0 {
            0.0
        } else {
            let omega = |k: f32| if k > 0.0 { k.sqrt() } else { RIGID_OMEGA };
            lerp(omega(braced_k), omega(one_k), share).powi(2)
        };
        Self {
            sag_deg: lerp(braced.sag_deg, one.sag_deg, share),
            reach_sag_deg: one.reach_sag_deg * share,
            sway_deg: one.sway_deg,
            sway_share: share,
            stiffness,
            damping_ratio: lerp(1.0, one.lag_damping_ratio, share),
            max_angle_deg: lerp(config.braced.max_angle_deg, config.max_angle_deg, share),
        }
    }
}

/// Sag and sway the grip orientation of a gun held in `hand` with the head at `head`, by its
/// `weight`. The barrel is the wrist frame's fingers axis (its -Z), along which the hand holds the
/// gun; the controller's grip -Z runs up the handle instead. The sag turns the barrel down about
/// the horizontal axis across it, by the sag plus the reach sag as the hand moves out, scaled by
/// how level the barrel is; `sway` (pitch and yaw, radians, at full share) pitches it about that
/// same horizontal axis and yaws it about the vertical, so that neither rolls the gun, half of it
/// close in and all of it at full reach.
pub fn weighted_pose(
    hand: &CockpitHand,
    head: Vec3,
    weight: &Weight,
    config: &HeftConfig,
    sway: Vec2,
) -> WeightedPose {
    let grip = hand.grip;
    let reach = smoothstep(
        config.reach_near_m,
        config.reach_far_m,
        grip.position.distance(head),
    );
    let barrel = hand.wrist.orientation * Vec3::NEG_Z;
    let level = Vec2::new(barrel.x, barrel.z).length().min(1.0);
    let sag = (weight.sag_deg + weight.reach_sag_deg * reach).to_radians() * level;
    let across = Vec3::Y.cross(barrel).try_normalize();
    let droop = across.map_or(Quat::IDENTITY, |axis| Quat::from_axis_angle(axis, sag));
    // A vertical barrel has no horizontal axis across it; the palm axis is square to the barrel,
    // and any axis square to it serves for a sway that small.
    let pitch_axis = across
        .or_else(|| {
            (grip.orientation * Vec3::X)
                .reject_from(barrel)
                .try_normalize()
        })
        .unwrap_or(Vec3::X);
    let sway_reach = 0.5 + 0.5 * reach;
    let sway_scaled = sway * weight.sway_share * sway_reach;
    let wobble =
        Quat::from_rotation_y(sway_scaled.y) * Quat::from_axis_angle(pitch_axis, sway_scaled.x);
    WeightedPose {
        orientation: (wobble * droop * grip.orientation).normalize(),
        sag: if across.is_some() { sag } else { 0.0 },
        reach,
        sway,
        sway_reach,
    }
}

/// The sway at time `t` seconds: a pitch and a yaw of amplitude `amplitude` (radians) at
/// `hz` and at an incommensurate multiple of it, so the path does not repeat.
pub fn sway(t: f32, hz: f32, amplitude: f32) -> Vec2 {
    let phase = TAU * hz.max(0.0) * t;
    Vec2::new(
        phase.sin() * amplitude,
        (phase * YAW_RATIO + YAW_PHASE).sin() * amplitude * YAW_SHARE,
    )
}

/// The share of the weight left by a two-hand `grip` on `hand`'s gun: one in one hand, zero with
/// the foregrip fully engaged.
pub fn unbraced_share(grip: Option<foregrip::TwoHandGrip>, hand: Hand) -> f32 {
    1.0 - grip
        .filter(|g| g.holder == hand)
        .map_or(0.0, |g| g.weight.clamp(0.0, 1.0))
}

/// A damped spring on an orientation, pulled toward a target orientation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RotSpring {
    pub orientation: Quat,
    /// The angular velocity, radians per second, as a rotation vector.
    pub velocity: Vec3,
}

impl RotSpring {
    /// At rest at `orientation`.
    pub fn at(orientation: Quat) -> Self {
        Self {
            orientation,
            velocity: Vec3::ZERO,
        }
    }

    /// Advance by `dt` seconds toward `target` with the given stiffness (per second squared) and
    /// damping ratio, in sub-steps short enough to stay stable at any stiffness the settings allow.
    pub fn step(&mut self, target: Quat, dt: f32, stiffness: f32, damping_ratio: f32) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        let stiffness = stiffness.max(0.0);
        let damping = 2.0 * damping_ratio.max(0.0) * stiffness.sqrt();
        let steps = (dt / MAX_SUBSTEP).ceil().max(1.0) as u32;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let error = shortest(target * self.orientation.inverse()).to_scaled_axis();
            self.velocity += (stiffness * error - damping * self.velocity) * h;
            self.orientation =
                (Quat::from_scaled_axis(self.velocity * h) * self.orientation).normalize();
        }
        if !(self.orientation.is_finite() && self.velocity.is_finite()) {
            *self = Self::at(target);
        }
    }
}

/// Limit a rotation to `max` radians, keeping its axis.
pub fn clamp_angle(rotation: Quat, max: f32) -> Quat {
    let rotation = shortest(rotation);
    let (axis, angle) = rotation.to_axis_angle();
    if angle <= max.max(0.0) || !axis.is_finite() {
        rotation
    } else {
        Quat::from_axis_angle(axis, max.max(0.0))
    }
}

/// The natural frequency a rigid side of a blend counts as, radians per second: stiff enough that
/// its lag cannot be seen, soft enough for the spring's sub-steps.
const RIGID_OMEGA: f32 = 120.0;
/// The longest sub-step the spring integrates in one go, seconds.
const MAX_SUBSTEP: f32 = 1.0 / 480.0;
/// The yaw sway's frequency relative to the pitch sway's, its phase offset, and its share of the
/// amplitude.
const YAW_RATIO: f32 = 0.618;
const YAW_PHASE: f32 = 1.3;
const YAW_SHARE: f32 = 0.8;
