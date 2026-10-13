//! The player's torso, estimated from the tracked head and hands. Pure.
//!
//! Nothing tracks the torso, so its heading and its joints are inferred from what is tracked, the
//! standard way for a head-and-hands VR rig:
//!
//! - **The head's heading, through a deadzone.** Turning the head inside the deadzone leaves the
//!   torso where it is, so looking around does not spin the body. Past the deadzone the torso
//!   follows by the excess, eased, and the neck limit is a hard bound on how far the head may be
//!   turned from the torso. The heading is read from the head's forward axis, blended with its up
//!   axis as the head pitches, so that looking straight down at the belt still has a heading.
//! - **The hands.** With both hands tracked and apart, the line between them and their midpoint's
//!   direction from the neck say where the chest faces, much better than the head does when the
//!   player looks over a shoulder. They pull the heading toward theirs by a weight scaled by how
//!   much they can be trusted: how far apart they are, and whether they are in front of the body.
//! - **Recent head motion.** A player walking in the room faces where they walk, so a steady
//!   forward walk pulls the heading toward its direction.
//!
//! The joints hang below the head by the body dimensions. The neck pivot sits behind and below the
//! eyes in the head's frame, so nodding moves the eyes and not the shoulders; the shoulders sit
//! below the pivot along the torso's heading; and the pelvis sits below the shoulders along the
//! spine, which leans when the shoulders move away from a pelvis that follows them slowly, as when
//! the player leans over without stepping. Where the feet are is the rig's
//! ([`crate::controllers::body::rig::feet`]).
//!
//! All positions and headings are in the cockpit frame: +X right, +Y up, -Z forward, metres. A
//! heading is a yaw about +Y, positive turning -Z toward -X, the convention of
//! [`crate::headpose::sim::body_yaw_of`].

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::controllers::{
    body::{self, BodyDimensions},
    weapons::volumes::HIP_HEIGHT_FRACTION,
};

/// How the torso estimate behaves. The defaults are starting points for in-headset tuning.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorsoConfig {
    /// How far the head may turn from the torso before the torso starts to follow, degrees.
    pub deadzone_deg: f32,
    /// The furthest the head may be turned from the torso, degrees: past it the torso is dragged
    /// along at once.
    pub neck_limit_deg: f32,
    /// How quickly the torso eases toward where the head, the hands, and the walk put it, seconds
    /// (a time constant).
    pub follow_s: f32,
    /// How strongly a confident pair of hands pulls the heading toward theirs, `0..=1`.
    pub hands_weight: f32,
    /// The distance between the hands below which they say nothing about the heading, metres.
    pub hands_min_spread_m: f32,
    /// How strongly a steady forward walk pulls the heading toward its direction, `0..=1`.
    pub walk_weight: f32,
    /// The head's horizontal speed at which a walk counts fully, metres per second. Below a third
    /// of it the walk counts for nothing.
    pub walk_speed_mps: f32,
    /// How quickly the pelvis follows the shoulders horizontally, seconds (a time constant): a lean
    /// shorter than this tilts the spine, a step longer than this carries the pelvis along.
    pub pelvis_follow_s: f32,
    /// The furthest the spine may lean from upright, degrees.
    pub max_lean_deg: f32,
}

impl TorsoConfig {
    pub const fn new() -> Self {
        Self {
            deadzone_deg: 25.0,
            neck_limit_deg: 65.0,
            follow_s: 0.3,
            hands_weight: 0.5,
            hands_min_spread_m: 0.2,
            walk_weight: 0.6,
            walk_speed_mps: 0.45,
            pelvis_follow_s: 0.8,
            max_lean_deg: 50.0,
        }
    }
}

impl Default for TorsoConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// One update's inputs, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TorsoInputs {
    /// The head (the eye midpoint).
    pub head_position: Vec3,
    pub head_orientation: Quat,
    /// The tracked hands' grip positions, left then right, `None` for a hand that is not tracked.
    pub hands: [Option<Vec3>; 2],
}

/// The estimated torso, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TorsoEstimate {
    /// The torso's heading, radians.
    pub yaw: f32,
    /// The head's heading, radians.
    pub head_yaw: f32,
    /// How far the head is turned from the torso, radians, positive turning left.
    pub neck_twist: f32,
    /// The hands' heading, when they were trusted at all.
    pub hands_yaw: Option<f32>,
    /// How much the hands were trusted, `0..=1`.
    pub hands_confidence: f32,
    /// How much the walk counted, `0..=1`.
    pub walk_confidence: f32,
    /// The neck pivot at the base of the skull.
    pub neck: Vec3,
    /// The shoulder joints, left then right.
    pub shoulders: [Vec3; 2],
    /// The middle of the shoulder joints.
    pub chest: Vec3,
    /// The middle of the hip joints.
    pub pelvis: Vec3,
    /// The hip joints, left then right.
    pub hips: [Vec3; 2],
    /// The spine's direction from the pelvis to the chest, a unit vector.
    pub spine: Vec3,
}

impl TorsoEstimate {
    /// The rotation about +Y by the torso's heading.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }
}

/// The estimator's state: the heading it holds and the slow pelvis.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TorsoEstimator {
    state: Option<State>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct State {
    yaw: f32,
    /// The pelvis's horizontal position, following the shoulders.
    pelvis_xz: Vec2,
    /// The last head position and the smoothed horizontal head velocity.
    head: Vec3,
    velocity: Vec2,
}

impl TorsoEstimator {
    pub const fn new() -> Self {
        Self { state: None }
    }

    /// Forget the held heading, so the next update starts from the head.
    pub fn reset(&mut self) {
        self.state = None;
    }

    /// Turn the held state by `turn` about +Y, for a cockpit frame that turned by it (new frame
    /// from old), so that the torso stays where it is in the room.
    pub fn rotate(&mut self, turn: Quat) {
        if let Some(state) = self.state.as_mut() {
            let yaw = yaw_of(turn * yaw_forward(state.yaw));
            state.yaw = yaw.unwrap_or(state.yaw);
            let pelvis = turn * Vec3::new(state.pelvis_xz.x, 0.0, state.pelvis_xz.y);
            state.pelvis_xz = Vec2::new(pelvis.x, pelvis.z);
            state.head = turn * state.head;
            let velocity = turn * Vec3::new(state.velocity.x, 0.0, state.velocity.y);
            state.velocity = Vec2::new(velocity.x, velocity.z);
        }
    }

    /// Advance by `dt` seconds and return the estimate, or `None` when the inputs carry no
    /// heading (a non-finite head). `dims` are the player's body dimensions.
    pub fn update(
        &mut self,
        inputs: &TorsoInputs,
        dt: f32,
        dims: &BodyDimensions,
        config: &TorsoConfig,
    ) -> Option<TorsoEstimate> {
        let head = inputs.head_position;
        let orientation = inputs.head_orientation.normalize();
        if !head.is_finite() || !orientation.is_finite() {
            return None;
        }
        let head_yaw = head_heading(orientation)?;
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.25)
        } else {
            0.0
        };
        let neck = head + orientation * EYES_TO_NECK_PIVOT;

        let state = self.state.get_or_insert(State {
            yaw: head_yaw,
            pelvis_xz: Vec2::new(neck.x, neck.z),
            head,
            velocity: Vec2::ZERO,
        });

        // The head's horizontal velocity, smoothed so a single noisy frame is not a walk.
        if dt > 0.0 {
            let step = (head - state.head) / dt;
            let alpha = ease(dt, VELOCITY_SMOOTHING_S);
            state.velocity = state.velocity.lerp(Vec2::new(step.x, step.z), alpha);
        }
        state.head = head;

        // The head-driven heading: the held heading, moved by however far the head is turned past
        // the deadzone.
        let deadzone = config.deadzone_deg.max(0.0).to_radians();
        let off = wrap(head_yaw - state.yaw);
        let excess = (off.abs() - deadzone).max(0.0);
        let from_head = wrap(state.yaw + off.signum() * excess);

        let (hands_yaw, hands_confidence) =
            hands_heading(inputs.hands, neck, state.yaw, config).unwrap_or((from_head, 0.0));
        let hands_weight = (config.hands_weight.clamp(0.0, 1.0) * hands_confidence).clamp(0.0, 1.0);
        let mut target = lerp_angle(from_head, hands_yaw, hands_weight);

        let walk_confidence = walk_confidence(state.velocity, head_yaw, config);
        if walk_confidence > 0.0
            && let Some(walk_yaw) = yaw_of(Vec3::new(state.velocity.x, 0.0, state.velocity.y))
        {
            let weight = (config.walk_weight.clamp(0.0, 1.0) * walk_confidence).clamp(0.0, 1.0);
            target = lerp_angle(target, walk_yaw, weight);
        }

        let alpha = ease(dt, config.follow_s);
        let mut yaw = wrap(state.yaw + wrap(target - state.yaw) * alpha);
        // The neck limit is anatomy, not a preference: it holds at once, whatever the easing.
        let limit = config.neck_limit_deg.max(0.0).to_radians();
        let twist = wrap(head_yaw - yaw);
        if twist.abs() > limit {
            yaw = wrap(head_yaw - twist.signum() * limit);
        }
        state.yaw = yaw;

        let torso = Quat::from_rotation_y(yaw);
        let back = torso * Vec3::Z;
        let right = torso * Vec3::X;
        let chest = neck
            + Vec3::Y * (EYES_TO_NECK_PIVOT_DROP - (dims.eye_height - dims.shoulder_height))
            + back * (body::EYES_FORWARD_OF_SHOULDERS - EYES_TO_NECK_PIVOT_BACK);
        let half_shoulders = right * (dims.shoulder_width * 0.5);
        let shoulders = [chest - half_shoulders, chest + half_shoulders];

        // The pelvis trails the chest horizontally; the spine leans from it to the chest, by at
        // most the lean limit, and keeps its length.
        let spine_length = (dims.shoulder_height - dims.height * HIP_HEIGHT_FRACTION).max(0.1);
        let pelvis_alpha = ease(dt, config.pelvis_follow_s);
        state.pelvis_xz = state
            .pelvis_xz
            .lerp(Vec2::new(chest.x, chest.z), pelvis_alpha);
        let max_lean = config.max_lean_deg.clamp(0.0, 89.0).to_radians();
        let max_reach = spine_length * max_lean.sin();
        let mut lean = Vec2::new(chest.x, chest.z) - state.pelvis_xz;
        if lean.length() > max_reach {
            lean = lean.clamp_length_max(max_reach);
            state.pelvis_xz = Vec2::new(chest.x, chest.z) - lean;
        }
        let rise = (spine_length * spine_length - lean.length_squared())
            .max(0.0)
            .sqrt();
        let spine = Vec3::new(lean.x, rise, lean.y).normalize_or(Vec3::Y);
        let pelvis = chest - spine * spine_length;
        let half_hips = right * (dims.shoulder_width * HIP_SPACING_FRACTION * 0.5);
        let hips = [pelvis - half_hips, pelvis + half_hips];

        Some(TorsoEstimate {
            yaw,
            head_yaw,
            neck_twist: wrap(head_yaw - yaw),
            hands_yaw: (hands_confidence > 0.0).then_some(hands_yaw),
            hands_confidence,
            walk_confidence,
            neck,
            shoulders,
            chest,
            pelvis,
            hips,
            spine,
        })
    }
}

/// The heading of a flat direction, or `None` for a direction with no horizontal extent.
pub fn yaw_of(direction: Vec3) -> Option<f32> {
    let flat = Vec2::new(direction.x, direction.z);
    (flat.length_squared() > 1.0e-10 && flat.is_finite()).then(|| (-flat.x).atan2(-flat.y))
}

/// The forward direction of a heading.
pub fn yaw_forward(yaw: f32) -> Vec3 {
    Quat::from_rotation_y(yaw) * Vec3::NEG_Z
}

/// `angle` wrapped to `-π..=π`.
pub fn wrap(angle: f32) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }
    let tau = std::f32::consts::TAU;
    let wrapped = (angle + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    if wrapped <= -std::f32::consts::PI {
        wrapped + tau
    } else {
        wrapped
    }
}

/// The head's heading: its forward axis on the ground, blended with its up axis as it pitches, so
/// that the heading stays defined looking straight down or up. Looking down, the up axis points
/// the way the face does; looking up, it points back.
pub fn head_heading(orientation: Quat) -> Option<f32> {
    let forward = orientation * Vec3::NEG_Z;
    let up = orientation * Vec3::Y;
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    yaw_of(flat(forward) - flat(up) * forward.y)
}

/// The neck pivot, at the base of the skull, from the eyes in the head's frame: below and behind
/// them, so that nodding turns the eyes about it.
pub const EYES_TO_NECK_PIVOT: Vec3 =
    Vec3::new(0.0, -EYES_TO_NECK_PIVOT_DROP, EYES_TO_NECK_PIVOT_BACK);

/// How far the neck pivot sits below the eyes, metres. A generic anthropometric figure.
const EYES_TO_NECK_PIVOT_DROP: f32 = 0.075;

/// How far the neck pivot sits behind the eyes, metres. A generic anthropometric figure.
const EYES_TO_NECK_PIVOT_BACK: f32 = 0.085;

/// The distance between the hip joints as a fraction of the distance between the shoulder joints.
/// A generic anthropometric figure.
pub const HIP_SPACING_FRACTION: f32 = 0.45;

/// The time constant of the head-velocity smoothing, seconds.
const VELOCITY_SMOOTHING_S: f32 = 0.2;

/// How far the hands' midpoint must be from the neck, horizontally, to say which way it faces,
/// metres.
const HANDS_MIN_REACH_M: f32 = 0.1;

/// The furthest the hands' heading may be from the held torso's and still count, radians: hands
/// crossed over or held behind the back point a line that is not the chest's.
const HANDS_MAX_DISAGREEMENT: f32 = 100.0 * std::f32::consts::PI / 180.0;

/// The furthest a walk's direction may be from the head's heading and still count, radians: a
/// step back or to the side does not turn the body.
const WALK_MAX_ANGLE: f32 = 60.0 * std::f32::consts::PI / 180.0;

/// The heading the hands give and how much to trust it: the normal to the line from the left hand
/// to the right, toward the side their midpoint is on from the neck. `None` without both hands, or
/// when they are too close together to have a line.
fn hands_heading(
    hands: [Option<Vec3>; 2],
    neck: Vec3,
    held_yaw: f32,
    config: &TorsoConfig,
) -> Option<(f32, f32)> {
    let [Some(left), Some(right)] = hands else {
        return None;
    };
    if !left.is_finite() || !right.is_finite() {
        return None;
    }
    let lateral = Vec2::new(right.x - left.x, right.z - left.z);
    let spread = lateral.length();
    let min_spread = config.hands_min_spread_m.max(0.01);
    if spread < min_spread {
        return None;
    }
    // Perpendicular to the line, toward -Z for a left hand on -X: the forward of a body holding
    // its hands out to its sides.
    let normal = Vec3::new(lateral.y, 0.0, -lateral.x) / spread;
    let middle = (left + right) * 0.5 - neck;
    let reach = Vec2::new(middle.x, middle.z);
    let forward = if reach.length() > HANDS_MIN_REACH_M {
        normal + Vec3::new(reach.x, 0.0, reach.y).normalize() * 0.5
    } else {
        normal
    };
    let yaw = yaw_of(forward)?;
    if wrap(yaw - held_yaw).abs() > HANDS_MAX_DISAGREEMENT {
        return None;
    }
    let spread_trust = ((spread - min_spread) / HANDS_FULL_SPREAD_M).clamp(0.0, 1.0);
    let in_front = Vec3::new(reach.x, 0.0, reach.y).dot(yaw_forward(held_yaw));
    let front_trust = if in_front > 0.0 { 1.0 } else { 0.5 };
    let confidence = smoothstep(spread_trust) * front_trust;
    (confidence > 0.0).then_some((yaw, confidence))
}

/// The spread beyond the minimum over which the hands' trust ramps to full, metres.
const HANDS_FULL_SPREAD_M: f32 = 0.2;

/// How much a walk at `velocity` counts: nothing below a third of the walking speed or off the
/// head's heading by more than [`WALK_MAX_ANGLE`], fully at the walking speed straight ahead.
fn walk_confidence(velocity: Vec2, head_yaw: f32, config: &TorsoConfig) -> f32 {
    let full = config.walk_speed_mps;
    if !full.is_finite() || full <= 0.0 || !velocity.is_finite() {
        return 0.0;
    }
    let speed = velocity.length();
    let Some(yaw) = yaw_of(Vec3::new(velocity.x, 0.0, velocity.y)) else {
        return 0.0;
    };
    if wrap(yaw - head_yaw).abs() > WALK_MAX_ANGLE {
        return 0.0;
    }
    smoothstep(((speed - full / 3.0) / (full * 2.0 / 3.0)).clamp(0.0, 1.0))
}

/// The shorter-way blend from `from` to `to` by `t`.
fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    wrap(from + wrap(to - from) * t.clamp(0.0, 1.0))
}

/// The fraction of the way an exponential ease with time constant `tau` covers in `dt`.
fn ease(dt: f32, tau: f32) -> f32 {
    if dt.is_nan() || dt <= 0.0 {
        return 0.0;
    }
    if !(tau.is_finite() && tau > 1.0e-4) {
        return 1.0;
    }
    1.0 - (-dt / tau).exp()
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIMS: BodyDimensions = BodyDimensions::GENERIC_ADULT;
    const DT: f32 = 1.0 / 90.0;

    fn deg(d: f32) -> f32 {
        d.to_radians()
    }

    fn looking(yaw: f32, pitch: f32) -> Quat {
        Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, 0.0)
    }

    fn head(yaw: f32) -> TorsoInputs {
        TorsoInputs {
            head_position: Vec3::ZERO,
            head_orientation: looking(yaw, 0.0),
            hands: [None; 2],
        }
    }

    /// Run `seconds` of updates with the same inputs and return the last estimate.
    fn settle(
        estimator: &mut TorsoEstimator,
        inputs: &TorsoInputs,
        config: &TorsoConfig,
        seconds: f32,
    ) -> TorsoEstimate {
        let mut last = None;
        for _ in 0..(seconds / DT).round() as usize {
            last = estimator.update(inputs, DT, &DIMS, config);
        }
        last.unwrap()
    }

    #[test]
    fn the_heading_conventions_agree() {
        for yaw in [0.0, 0.7, -2.0, 3.0] {
            assert!((yaw_of(yaw_forward(yaw)).unwrap() - yaw).abs() < 1e-5);
            assert!((head_heading(looking(yaw, 0.0)).unwrap() - yaw).abs() < 1e-5);
        }
        // Turning -Z toward -X is positive, as the body yaw is.
        assert!(yaw_of(Vec3::NEG_X).unwrap() > 0.0);
        assert!((crate::headpose::sim::body_yaw_of(Quat::from_rotation_y(0.4)) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_head_keeps_its_heading_looking_straight_down_or_up() {
        for yaw in [0.0, 1.0, -2.5] {
            for pitch in [deg(-89.9), deg(-60.0), deg(60.0), deg(89.9)] {
                let heading = head_heading(looking(yaw, pitch)).unwrap();
                assert!(wrap(heading - yaw).abs() < 1e-3, "{yaw} {pitch}: {heading}");
            }
        }
    }

    #[test]
    fn wrapping_stays_in_a_half_turn_each_way() {
        assert!((wrap(3.0 * std::f32::consts::PI) - std::f32::consts::PI).abs() < 1e-5);
        assert!((wrap(-0.5) + 0.5).abs() < 1e-6);
        assert!((wrap(deg(350.0)) - deg(-10.0)).abs() < 1e-5);
        assert_eq!(wrap(f32::NAN), 0.0);
    }

    #[test]
    fn looking_around_inside_the_deadzone_does_not_turn_the_torso() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 0.5);
        let estimate = settle(&mut estimator, &head(deg(20.0)), &config, 2.0);
        assert!(estimate.yaw.abs() < 1e-4, "{}", estimate.yaw);
        assert!((estimate.neck_twist - deg(20.0)).abs() < 1e-4);
    }

    #[test]
    fn turning_past_the_deadzone_brings_the_torso_to_its_edge() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 0.5);
        let estimate = settle(&mut estimator, &head(deg(-50.0)), &config, 3.0);
        assert!(
            (estimate.yaw - deg(-25.0)).abs() < deg(0.5),
            "{}",
            estimate.yaw.to_degrees()
        );
        assert!((estimate.neck_twist - deg(-25.0)).abs() < deg(0.5));
    }

    #[test]
    fn the_neck_limit_holds_at_once() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 0.5);
        let estimate = estimator
            .update(&head(deg(120.0)), DT, &DIMS, &config)
            .unwrap();
        assert!(
            (estimate.neck_twist - deg(65.0)).abs() < 1e-4,
            "{}",
            estimate.neck_twist.to_degrees()
        );
    }

    #[test]
    fn turning_all_the_way_round_carries_the_torso_round() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        let mut yaw = 0.0;
        for _ in 0..(4.0 / DT) as usize {
            yaw = wrap(yaw + deg(90.0) * DT);
            estimator.update(&head(yaw), DT, &DIMS, &config);
        }
        let estimate = settle(&mut estimator, &head(yaw), &config, 0.0 + DT);
        assert!(estimate.neck_twist.abs() <= deg(65.0) + 1e-4);
        assert!(estimate.neck_twist > 0.0, "the torso trails the turn");
    }

    fn hands_at(left: Vec3, right: Vec3) -> [Option<Vec3>; 2] {
        [Some(left), Some(right)]
    }

    #[test]
    fn hands_out_in_front_turn_the_torso_toward_them_while_the_head_looks_away() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 0.5);
        // The chest turned 30 degrees left, the hands out in front of it, the head looking back to
        // the right. The head alone keeps the torso near zero; the hands pull it toward theirs.
        let chest = Quat::from_rotation_y(deg(30.0));
        let left = chest * Vec3::new(-0.25, -0.4, -0.35);
        let right = chest * Vec3::new(0.25, -0.4, -0.35);
        let inputs = TorsoInputs {
            head_position: Vec3::ZERO,
            head_orientation: looking(deg(5.0), 0.0),
            hands: hands_at(left, right),
        };
        let estimate = settle(&mut estimator, &inputs, &config, 3.0);
        assert!(estimate.hands_confidence > 0.9);
        assert!(
            (estimate.hands_yaw.unwrap() - deg(30.0)).abs() < deg(8.0),
            "{:?}",
            estimate.hands_yaw.map(f32::to_degrees)
        );
        assert!(
            estimate.yaw > deg(10.0) && estimate.yaw < deg(30.0),
            "{}",
            estimate.yaw.to_degrees()
        );
    }

    #[test]
    fn hands_close_together_or_crossed_say_nothing() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        let close = TorsoInputs {
            hands: hands_at(Vec3::new(-0.05, -0.4, -0.3), Vec3::new(0.05, -0.4, -0.3)),
            ..head(0.0)
        };
        let estimate = settle(&mut estimator, &close, &config, 0.5);
        assert_eq!(estimate.hands_confidence, 0.0);
        assert_eq!(estimate.hands_yaw, None);
        // Crossed: the right hand on the left, which points the line's normal backward.
        let crossed = TorsoInputs {
            hands: hands_at(Vec3::new(0.3, -0.4, -0.3), Vec3::new(-0.3, -0.4, -0.3)),
            ..head(0.0)
        };
        let estimate = settle(&mut estimator, &crossed, &config, 0.5);
        assert_eq!(estimate.hands_confidence, 0.0);
        assert!(estimate.yaw.abs() < 1e-3);
    }

    #[test]
    fn hands_at_the_sides_keep_the_heading_but_count_less_behind_the_body() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        let inputs = TorsoInputs {
            hands: hands_at(Vec3::new(-0.3, -0.7, 0.15), Vec3::new(0.3, -0.7, 0.15)),
            ..head(0.0)
        };
        let estimate = settle(&mut estimator, &inputs, &config, 1.0);
        assert!(estimate.hands_confidence > 0.0 && estimate.hands_confidence <= 0.5);
        assert!(estimate.yaw.abs() < deg(1.0));
    }

    #[test]
    fn walking_forward_turns_the_torso_to_the_walk_but_stepping_sideways_does_not() {
        let config = TorsoConfig::new();
        // The head looks 40 degrees left of the torso and walks that way.
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 0.5);
        let mut position = Vec3::ZERO;
        let mut last = None;
        for _ in 0..(2.0 / DT) as usize {
            position += yaw_forward(deg(40.0)) * 0.8 * DT;
            let inputs = TorsoInputs {
                head_position: position,
                ..head(deg(40.0))
            };
            last = estimator.update(&inputs, DT, &DIMS, &config);
        }
        let walked = last.unwrap();
        assert!(walked.walk_confidence > 0.9);
        assert!(walked.yaw > deg(30.0), "{}", walked.yaw.to_degrees());

        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 0.5);
        let mut position = Vec3::ZERO;
        let mut last = None;
        for _ in 0..(2.0 / DT) as usize {
            position += Vec3::X * 0.8 * DT;
            let inputs = TorsoInputs {
                head_position: position,
                ..head(0.0)
            };
            last = estimator.update(&inputs, DT, &DIMS, &config);
        }
        let stepped = last.unwrap();
        assert_eq!(stepped.walk_confidence, 0.0);
        assert!(stepped.yaw.abs() < 1e-3);
    }

    #[test]
    fn an_upright_body_hangs_its_joints_below_the_head_by_the_dimensions() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        let estimate = settle(&mut estimator, &head(0.0), &config, 2.0);
        // The shoulders as the shared estimate has them.
        let expected = body::shoulder_positions(Vec3::ZERO, Quat::IDENTITY, &DIMS);
        for (estimated, expected) in estimate.shoulders.iter().zip(expected) {
            assert!(estimated.distance(expected) < 1e-4);
        }
        let hip_height = DIMS.height * HIP_HEIGHT_FRACTION;
        assert!((estimate.pelvis.y - (hip_height - DIMS.eye_height)).abs() < 1e-3);
        assert!((estimate.pelvis.x).abs() < 1e-3);
        assert!((estimate.pelvis.z - body::EYES_FORWARD_OF_SHOULDERS).abs() < 1e-3);
        assert!(estimate.spine.distance(Vec3::Y) < 1e-3);
        assert!(estimate.hips[0].x < 0.0 && estimate.hips[1].x > 0.0);
        assert!(
            ((estimate.hips[1].x - estimate.hips[0].x)
                - DIMS.shoulder_width * HIP_SPACING_FRACTION)
                .abs()
                < 1e-4
        );
    }

    #[test]
    fn nodding_moves_the_eyes_and_not_the_shoulders() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        let level = settle(&mut estimator, &head(0.0), &config, 1.0);
        // Pitching about the neck pivot moves the eyes; express the same pivot from them.
        let nod = looking(0.0, deg(-50.0));
        let pivot = level.neck;
        let eyes = pivot - nod * EYES_TO_NECK_PIVOT;
        let inputs = TorsoInputs {
            head_position: eyes,
            head_orientation: nod,
            hands: [None; 2],
        };
        let nodded = estimator.update(&inputs, DT, &DIMS, &config).unwrap();
        assert!(nodded.chest.distance(level.chest) < 1e-4);
    }

    #[test]
    fn a_quick_lean_tilts_the_spine_and_a_step_carries_the_pelvis() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 1.0);
        let leaned = TorsoInputs {
            head_position: Vec3::new(0.25, -0.05, 0.0),
            ..head(0.0)
        };
        let quick = estimator.update(&leaned, DT, &DIMS, &config).unwrap();
        assert!(quick.spine.x > 0.3, "{:?}", quick.spine);
        assert!(quick.pelvis.x < 0.05);
        let stepped = settle(&mut estimator, &leaned, &config, 6.0);
        assert!(stepped.spine.distance(Vec3::Y) < 1e-2);
        assert!((stepped.pelvis.x - 0.25).abs() < 1e-2);
        // The spine keeps its length either way.
        let length = DIMS.shoulder_height - DIMS.height * HIP_HEIGHT_FRACTION;
        assert!((quick.chest.distance(quick.pelvis) - length).abs() < 1e-4);
    }

    #[test]
    fn the_lean_is_bounded() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 1.0);
        let far = TorsoInputs {
            head_position: Vec3::new(2.0, 0.0, 0.0),
            ..head(0.0)
        };
        let estimate = estimator.update(&far, DT, &DIMS, &config).unwrap();
        let lean = estimate.spine.angle_between(Vec3::Y);
        assert!(lean <= deg(50.0) + 1e-3, "{}", lean.to_degrees());
    }

    #[test]
    fn rotating_the_frame_keeps_the_torso_in_the_room() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), &config, 0.5);
        settle(&mut estimator, &head(deg(20.0)), &config, 0.5);
        // The frame turns so that the head's heading becomes zero.
        let turn = Quat::from_rotation_y(deg(-20.0));
        estimator.rotate(turn);
        let estimate = estimator.update(&head(0.0), DT, &DIMS, &config).unwrap();
        assert!(
            (estimate.yaw - deg(-20.0)).abs() < 1e-3,
            "{}",
            estimate.yaw.to_degrees()
        );
    }

    #[test]
    fn a_bad_head_yields_nothing_and_a_bad_step_holds() {
        let config = TorsoConfig::new();
        let mut estimator = TorsoEstimator::new();
        let bad = TorsoInputs {
            head_position: Vec3::NAN,
            ..head(0.0)
        };
        assert_eq!(estimator.update(&bad, DT, &DIMS, &config), None);
        settle(&mut estimator, &head(0.0), &config, 0.5);
        let held = estimator
            .update(&head(deg(40.0)), f32::NAN, &DIMS, &config)
            .unwrap();
        assert!(held.yaw.abs() < 1e-5);
    }
}
