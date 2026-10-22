//! The player's torso, estimated from the tracked head and hands. Pure.
//!
//! Nothing tracks the torso, so its heading is inferred from what is tracked (after Parger et al.,
//! 2018): the head's heading, weighted by [`TorsoConfig::head_weight`], plus the horizontal
//! direction from the neck pivot to each tracked hand. Each hand's direction is normalized before
//! it is flattened, and the flat part is scaled by the square of its own length, so a hand held out
//! in front carries the chest while a hand hanging at the side says almost nothing. The hands carry
//! the chest when the player glances aside, and the head carries it when the hands hang. The
//! result is held within the neck limit of the head and follows through a critically damped
//! filter; there is no deadzone. The head's heading is read from its forward axis, blended with its
//! up axis as it pitches, so that looking straight down at the belt still has a heading.
//!
//! The joints hang below the head by the body dimensions. The neck pivot sits behind and below the
//! eyes in the head's frame, so nodding moves the eyes and not the shoulders; the shoulders sit
//! below the pivot along the torso's heading, and the pelvis a spine's length below the chest.
//! Where the feet stand is the legs' ([`crate::controllers::body::rig::legs`]).
//!
//! All positions and headings are in the cockpit frame: +X right, +Y up, -Z forward, metres. A
//! heading is a yaw about +Y, positive turning -Z toward -X, the convention of
//! [`crate::headpose::sim::body_yaw_of`].

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    body::{self, BodyDimensions, rig::config::TorsoConfig},
    weapons::volumes::HIP_HEIGHT_FRACTION,
};

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
    /// The hands' own heading, when they say anything.
    pub hands_yaw: Option<f32>,
    /// How much the hands counted against the head's weight: the sum of their horizontal shares
    /// cubed, `0..=2`.
    pub hands_weight: f32,
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
}

impl TorsoEstimate {
    /// The rotation about +Y by the torso's heading.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }
}

/// The estimator's state: the filtered heading and its rate.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TorsoEstimator {
    state: Option<State>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct State {
    yaw: f32,
    rate: f32,
}

impl TorsoEstimator {
    pub const fn new() -> Self {
        Self { state: None }
    }

    /// Forget the held heading, so the next update starts from the target.
    pub fn reset(&mut self) {
        self.state = None;
    }

    /// Turn the held heading by `turn` about +Y, for a cockpit frame that turned by it (new frame
    /// from old), so that the torso stays where it is in the room.
    pub fn rotate(&mut self, turn: Quat) {
        if let Some(state) = self.state.as_mut()
            && let Some(yaw) = yaw_of(turn * yaw_forward(state.yaw))
        {
            state.yaw = yaw;
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
        let limit = config.neck_limit_deg.clamp(0.0, 180.0).to_radians();

        let (hands, hands_weight) = hands_pull(inputs.hands, neck);
        let pull = flat(yaw_forward(head_yaw)) * config.head_weight.max(0.0) + hands;
        let target = yaw_of(Vec3::new(pull.x, 0.0, pull.y)).unwrap_or(head_yaw);
        let target = within_neck(target, head_yaw, limit);

        let state = self.state.get_or_insert(State {
            yaw: target,
            rate: 0.0,
        });
        follow(state, target, dt, config.follow_s);
        let twist = wrap(head_yaw - state.yaw);
        if twist.abs() > limit {
            // The neck limit is anatomy, not a preference: it holds at once, whatever the filter.
            state.yaw = wrap(head_yaw - twist.signum() * limit);
            state.rate = 0.0;
        }
        let yaw = state.yaw;

        let torso = Quat::from_rotation_y(yaw);
        let back = torso * Vec3::Z;
        let right = torso * Vec3::X;
        let chest = neck
            + Vec3::Y * (EYES_TO_NECK_PIVOT_DROP - (dims.eye_height - dims.shoulder_height))
            + back * (body::EYES_FORWARD_OF_SHOULDERS - EYES_TO_NECK_PIVOT_BACK);
        let half_shoulders = right * (dims.shoulder_width * 0.5);
        let shoulders = [chest - half_shoulders, chest + half_shoulders];
        let spine_length = (dims.shoulder_height - dims.height * HIP_HEIGHT_FRACTION).max(0.1);
        let pelvis = chest - Vec3::Y * spine_length;
        let half_hips = right * (dims.shoulder_width * HIP_SPACING_FRACTION * 0.5);
        let hips = [pelvis - half_hips, pelvis + half_hips];

        Some(TorsoEstimate {
            yaw,
            head_yaw,
            neck_twist: wrap(head_yaw - yaw),
            hands_yaw: (hands_weight > MIN_HANDS_WEIGHT)
                .then(|| yaw_of(Vec3::new(hands.x, 0.0, hands.y)))
                .flatten(),
            hands_weight,
            neck,
            shoulders,
            chest,
            pelvis,
            hips,
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

/// The hands' weight below which they have no heading of their own.
const MIN_HANDS_WEIGHT: f32 = 1e-3;

/// The longest step the heading's filter integrates at once, seconds, so that it stays stable at
/// any frame rate.
const FILTER_STEP_S: f32 = 0.005;

/// The hands' horizontal pull on the heading, `(x, z)`, and its total weight: each tracked hand's
/// direction from the neck, normalized, flattened, and scaled by the square of its horizontal
/// share, so a hand held out counts nearly in full and a hand hanging down nearly not at all.
fn hands_pull(hands: [Option<Vec3>; 2], neck: Vec3) -> (Vec2, f32) {
    let mut pull = Vec2::ZERO;
    let mut weight = 0.0;
    for hand in hands.into_iter().flatten() {
        let direction = (hand - neck).normalize_or_zero();
        let horizontal = flat(direction);
        if !horizontal.is_finite() {
            continue;
        }
        let share = horizontal.length();
        pull += horizontal * (share * share);
        weight += share * share * share;
    }
    (pull, weight)
}

/// `target` held within `limit` of the head's heading.
fn within_neck(target: f32, head_yaw: f32, limit: f32) -> f32 {
    let off = wrap(target - head_yaw);
    wrap(head_yaw + off.clamp(-limit, limit))
}

/// Step the critically damped filter toward `target` over `dt`, with the time constant `tau`.
fn follow(state: &mut State, target: f32, dt: f32, tau: f32) {
    if !(tau.is_finite() && tau > 1.0e-4) {
        state.yaw = target;
        state.rate = 0.0;
        return;
    }
    let omega = 1.0 / tau;
    let steps = (dt / FILTER_STEP_S).ceil().max(1.0) as usize;
    let h = dt / steps as f32;
    for _ in 0..steps {
        let error = wrap(target - state.yaw);
        state.rate += (omega * omega * error - 2.0 * omega * state.rate) * h;
        state.yaw = wrap(state.yaw + state.rate * h);
    }
}

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
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

    /// The head turned `yaw` about a neck pivot that stays where a level head's is, at the
    /// origin's pivot: the eyes swing about it.
    fn head(yaw: f32) -> TorsoInputs {
        let orientation = looking(yaw, 0.0);
        TorsoInputs {
            head_position: EYES_TO_NECK_PIVOT - orientation * EYES_TO_NECK_PIVOT,
            head_orientation: orientation,
            hands: [None; 2],
        }
    }

    /// Run `seconds` of updates with the same inputs and return the last estimate.
    fn settle(estimator: &mut TorsoEstimator, inputs: &TorsoInputs, seconds: f32) -> TorsoEstimate {
        let config = TorsoConfig::new();
        let mut last = None;
        for _ in 0..(seconds / DT).round().max(1.0) as usize {
            last = estimator.update(inputs, DT, &DIMS, &config);
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

    /// Both hands held out in front of a chest facing -Z, at about the height of the shoulders.
    fn hands_forward() -> [Option<Vec3>; 2] {
        [
            Some(Vec3::new(-0.2, -0.3, -0.45)),
            Some(Vec3::new(0.2, -0.3, -0.45)),
        ]
    }

    /// Both hands hanging at the sides.
    fn hands_down() -> [Option<Vec3>; 2] {
        [
            Some(Vec3::new(-0.22, -0.75, 0.0)),
            Some(Vec3::new(0.22, -0.75, 0.0)),
        ]
    }

    #[test]
    fn hands_held_forward_keep_the_chest_through_a_glance() {
        let mut estimator = TorsoEstimator::new();
        let inputs = TorsoInputs {
            hands: hands_forward(),
            ..head(deg(60.0))
        };
        let estimate = settle(&mut estimator, &inputs, 2.0);
        assert!(
            estimate.yaw.abs() < deg(10.0),
            "{}",
            estimate.yaw.to_degrees()
        );
        assert!(estimate.hands_weight > 1.0);
    }

    #[test]
    fn hands_hanging_down_let_the_head_lead() {
        let mut estimator = TorsoEstimator::new();
        let inputs = TorsoInputs {
            hands: hands_down(),
            ..head(deg(60.0))
        };
        let estimate = settle(&mut estimator, &inputs, 2.0);
        assert!(
            (estimate.yaw - deg(60.0)).abs() < deg(10.0),
            "{}",
            estimate.yaw.to_degrees()
        );
        // With no hands at all, the head leads entirely.
        let alone = settle(&mut TorsoEstimator::new(), &head(deg(60.0)), 2.0);
        assert!((alone.yaw - deg(60.0)).abs() < 1e-3);
    }

    #[test]
    fn the_neck_limit_holds_at_once() {
        let mut estimator = TorsoEstimator::new();
        let inputs = TorsoInputs {
            hands: hands_forward(),
            ..head(0.0)
        };
        settle(&mut estimator, &inputs, 1.0);
        // The head turns past the limit in one frame; the hands hold the chest, the neck does not.
        let turned = TorsoInputs {
            hands: hands_forward(),
            ..head(deg(120.0))
        };
        let estimate = estimator
            .update(&turned, DT, &DIMS, &TorsoConfig::new())
            .unwrap();
        assert!(
            (estimate.neck_twist.abs() - deg(65.0)).abs() < 1e-3,
            "{}",
            estimate.neck_twist.to_degrees()
        );
    }

    #[test]
    fn the_heading_follows_smoothly_without_overshooting() {
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), 0.5);
        let mut previous = 0.0;
        for _ in 0..90 {
            let estimate = estimator
                .update(&head(deg(40.0)), DT, &DIMS, &TorsoConfig::new())
                .unwrap();
            assert!(estimate.yaw >= previous - 1e-6);
            assert!(estimate.yaw <= deg(40.0) + 1e-4);
            previous = estimate.yaw;
        }
        assert!((previous - deg(40.0)).abs() < deg(0.5));
    }

    #[test]
    fn a_turned_frame_keeps_the_torso_in_the_room() {
        let mut estimator = TorsoEstimator::new();
        settle(&mut estimator, &head(0.0), 0.5);
        let turn = Quat::from_rotation_y(deg(30.0));
        estimator.rotate(turn);
        let estimate = estimator
            .update(&head(deg(30.0)), 0.0, &DIMS, &TorsoConfig::new())
            .unwrap();
        assert!((estimate.yaw - deg(30.0)).abs() < 1e-4);
    }

    #[test]
    fn the_joints_hang_under_the_neck_pivot() {
        let estimate = settle(&mut TorsoEstimator::new(), &head(0.0), 0.1);
        assert!((estimate.neck - EYES_TO_NECK_PIVOT).length() < 1e-6);
        assert!(estimate.chest.y < estimate.neck.y);
        assert!(estimate.pelvis.y < estimate.chest.y);
        assert!((estimate.shoulders[1] - estimate.shoulders[0]).x > 0.0);
        let nan = TorsoInputs {
            head_position: Vec3::NAN,
            ..head(0.0)
        };
        assert_eq!(
            TorsoEstimator::new().update(&nan, DT, &DIMS, &TorsoConfig::new()),
            None
        );
    }
}
