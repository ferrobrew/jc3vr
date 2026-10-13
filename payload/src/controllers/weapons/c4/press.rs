//! The wrist detonator's press detector, and where the button and the fingertip are. Pure.
//!
//! The button is a disc on the inside of the off-hand's wrist, fixed to the off-hand's grip pose,
//! with a face normal pointing out of the wrist. The dominant hand's index fingertip presses it.
//! The caller gives both in one frame, the frame the button is drawn in, so that the fingertip the
//! player sees on the button is the one that presses it ([`carry`] brings a point read from the
//! controllers into it).
//!
//! A press is a touch of the fingertip on the button from any direction in front of the wrist, so
//! that a poke from above, from the side, or straight in all work, while a brush past it does not:
//!
//! - the fingertip touches while it is within `touch_radius_m` of the face's centre and no more
//!   than `max_behind_m` behind the face's plane. A fingertip that far behind the plane has come
//!   through the wrist, which a real finger cannot, so it is a tracking error or the estimate of a
//!   finger resting elsewhere, and it is blocked;
//! - a touch presses at once when the fingertip was closing on the centre at
//!   `min_entry_speed_mps` or more as it came in, and otherwise once it has stayed touching for
//!   `dwell_s`;
//! - once pressed, the button presses again only after the fingertip has moved away past
//!   `release_radius_m`, which is more than `touch_radius_m`, so a fingertip resting on the button
//!   does not chatter;
//! - a fingertip first seen touching (tracking just found it there), or one that reached the
//!   button from behind the wrist, is blocked until it leaves the touch sphere, so that regaining
//!   tracking never presses, while a finger that slipped past the wrist's side can come straight
//!   back in.
//!
//! A hand that loses tracking resets the detector without a press.

use glam::Vec3;

use crate::controllers::{
    frame::{HAND_JOINT_COUNT, Hand, HandJoint, Pose},
    weapons::config::WristDetonatorConfig,
};

/// The button's face in the caller's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ButtonFace {
    pub centre: Vec3,
    /// Unit length, out of the wrist.
    pub normal: Vec3,
}

/// The button's face on the `off` hand whose grip is at `grip`: the configured left-hand placement,
/// mirrored for the right hand. `None` for a degenerate normal.
pub fn button_face(off: Hand, grip: Pose, tuning: &WristDetonatorConfig) -> Option<ButtonFace> {
    let local_normal = mirror_from_left(off, tuning.normal).try_normalize()?;
    let local_centre = mirror_from_left(off, tuning.offset_m);
    let orientation = grip.orientation.normalize();
    let face = ButtonFace {
        centre: grip.position + orientation * local_centre,
        normal: (orientation * local_normal).normalize(),
    };
    (face.centre.is_finite() && face.normal.is_finite()).then_some(face)
}

/// A point on a hand whose pose is `from`, carried with the hand to where its pose is `to`: the
/// same point in the hand's own frame. The controllers report a fingertip relative to the
/// controller they track; carried from the controller's grip to the grip the hand is drawn at, it
/// lands on the drawn hand.
pub fn carry(point: Vec3, from: Pose, to: Pose) -> Vec3 {
    let local = from.orientation.normalize().inverse() * (point - from.position);
    to.position + to.orientation.normalize() * local
}

/// What the fingertip is read from on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FingertipSource<'a> {
    /// The tracked hand skeleton, when the runtime reports one.
    pub joints: Option<&'a [HandJoint; HAND_JOINT_COUNT]>,
    /// The controller's aim pose.
    pub aim: Pose,
    /// Whether the index finger touches the trigger.
    pub trigger_touch: bool,
}

/// The dominant hand's index fingertip in the frame its `source` is given in: the tracked
/// skeleton's index tip when it is valid, and otherwise an estimate in the controller's aim frame,
/// ahead of the controller while the finger points and at the trigger while it rests there.
pub fn fingertip(hand: Hand, source: FingertipSource<'_>, tuning: &WristDetonatorConfig) -> Vec3 {
    if let Some(tip) = source
        .joints
        .map(|joints| joints[INDEX_TIP_JOINT])
        .filter(|joint| joint.valid && joint.pose.position.is_finite())
    {
        return tip.pose.position;
    }
    let offset = if source.trigger_touch {
        tuning.fingertip_resting_m
    } else {
        tuning.fingertip_pointing_m
    };
    let offset = match hand {
        Hand::Right => offset,
        Hand::Left => Vec3::new(-offset.x, offset.y, offset.z),
    };
    source.aim.position + source.aim.orientation.normalize() * offset
}

/// Where the detector is.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum PressPhase {
    /// No fingertip or no button: a hand is untracked or the detonator is off.
    #[default]
    Lost,
    /// The fingertip is away from the button: free to press.
    Armed {
        /// The fastest the fingertip has closed on the button's centre while near it, metres per
        /// second.
        approach_mps: f32,
    },
    /// The fingertip touches the button.
    Touching {
        /// Whether it came in fast enough to press at once.
        fast: bool,
        /// How long it has touched, seconds.
        dwell_s: f32,
    },
    /// The button pressed; it waits for the fingertip to move away.
    Pressed,
    /// The fingertip was first seen touching, or came from behind the wrist; it waits for the
    /// fingertip to leave the touch sphere.
    Blocked,
}

impl PressPhase {
    pub const fn label(self) -> &'static str {
        match self {
            PressPhase::Lost => "no fingertip or button",
            PressPhase::Armed { .. } => "armed",
            PressPhase::Touching { fast: true, .. } => "touching",
            PressPhase::Touching { fast: false, .. } => "touching slowly (dwell)",
            PressPhase::Pressed => "pressed, waiting for the finger to move away",
            PressPhase::Blocked => "blocked, waiting for the finger to leave the button",
        }
    }

    /// How far a slow touch is through its dwell, `0..=1`; zero when not touching.
    pub fn dwell_progress(self, tuning: &WristDetonatorConfig) -> f32 {
        match self {
            PressPhase::Touching { dwell_s, .. } if tuning.dwell_s > 0.0 => {
                (dwell_s / tuning.dwell_s).clamp(0.0, 1.0)
            }
            PressPhase::Touching { .. } => 1.0,
            _ => 0.0,
        }
    }
}

/// One step's reading.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PressReading {
    /// The button pressed on this step.
    pub pressed: bool,
    /// How far the fingertip is from the face's centre, metres.
    pub distance_m: Option<f32>,
    /// How far the fingertip is in front of the face's plane, metres (negative behind it).
    pub height_m: Option<f32>,
    pub phase: PressPhase,
}

/// The press detector's state across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PressDetector {
    phase: PressPhase,
    /// The fingertip's distance from the face's centre on the previous step.
    last_distance_m: Option<f32>,
}

impl PressDetector {
    pub const fn new() -> Self {
        Self {
            phase: PressPhase::Lost,
            last_distance_m: None,
        }
    }

    /// Advance by `dt` seconds with the fingertip at `tip` and the button's `face`, either `None`
    /// while untracked.
    pub fn step(
        &mut self,
        tip: Option<Vec3>,
        face: Option<ButtonFace>,
        dt: f32,
        tuning: &WristDetonatorConfig,
    ) -> PressReading {
        let (Some(tip), Some(face)) = (tip.filter(|t| t.is_finite()), face) else {
            *self = Self::new();
            return PressReading::default();
        };
        let relative = tip - face.centre;
        let distance = relative.length();
        let height = relative.dot(face.normal);
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        // Positive while closing on the centre.
        let approach = match self.last_distance_m {
            Some(last) if dt > 0.0 => (last - distance) / dt,
            _ => 0.0,
        };
        self.last_distance_m = Some(distance);

        let within = distance <= tuning.touch_radius_m.max(0.0);
        let behind = height < -tuning.max_behind_m.max(0.0);
        let touching = within && !behind;
        let away = distance > tuning.release_radius_m.max(tuning.touch_radius_m);
        let mut pressed = false;
        self.phase = match self.phase {
            PressPhase::Lost if within => PressPhase::Blocked,
            PressPhase::Lost => PressPhase::Armed { approach_mps: 0.0 },
            PressPhase::Armed { approach_mps } => {
                let approach_mps = if away {
                    0.0
                } else {
                    approach_mps.max(approach)
                };
                if touching {
                    let fast = approach_mps >= tuning.min_entry_speed_mps;
                    Self::touch(fast, 0.0, tuning, &mut pressed)
                } else if within {
                    PressPhase::Blocked
                } else {
                    PressPhase::Armed { approach_mps }
                }
            }
            PressPhase::Touching { fast, dwell_s } => {
                if touching {
                    Self::touch(fast, dwell_s + dt, tuning, &mut pressed)
                } else if within {
                    PressPhase::Blocked
                } else {
                    PressPhase::Armed { approach_mps: 0.0 }
                }
            }
            PressPhase::Pressed if away => PressPhase::Armed { approach_mps: 0.0 },
            PressPhase::Blocked if !within => PressPhase::Armed { approach_mps: 0.0 },
            phase @ (PressPhase::Pressed | PressPhase::Blocked) => phase,
        };
        PressReading {
            pressed,
            distance_m: Some(distance),
            height_m: Some(height),
            phase: self.phase,
        }
    }

    /// A touch that has lasted `dwell_s`: it presses at once when it came in fast, and otherwise
    /// once the dwell is reached.
    fn touch(
        fast: bool,
        dwell_s: f32,
        tuning: &WristDetonatorConfig,
        pressed: &mut bool,
    ) -> PressPhase {
        if fast || dwell_s >= tuning.dwell_s.max(0.0) {
            *pressed = true;
            PressPhase::Pressed
        } else {
            PressPhase::Touching { fast, dwell_s }
        }
    }
}

/// The `XR_EXT_hand_tracking` index of the index finger's tip (`XR_HAND_JOINT_INDEX_TIP_EXT`).
pub(super) const INDEX_TIP_JOINT: usize = 10;

/// A vector given for the left hand, mirrored across the controller's X for `hand`.
fn mirror_from_left(hand: Hand, v: Vec3) -> Vec3 {
    match hand {
        Hand::Left => v,
        Hand::Right => Vec3::new(-v.x, v.y, v.z),
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    const DT: f32 = 1.0 / 90.0;

    fn tuning() -> WristDetonatorConfig {
        WristDetonatorConfig::new()
    }

    /// A button at the origin facing +X.
    const FACE: ButtonFace = ButtonFace {
        centre: Vec3::ZERO,
        normal: Vec3::X,
    };

    /// Step the detector through `path`, returning how many presses happened.
    fn run(d: &mut PressDetector, path: impl IntoIterator<Item = Vec3>) -> usize {
        let t = tuning();
        path.into_iter()
            .filter(|tip| d.step(Some(*tip), Some(FACE), DT, &t).pressed)
            .count()
    }

    /// A straight move from `from` to `to` at `speed` metres per second.
    fn moving(from: Vec3, to: Vec3, speed: f32) -> Vec<Vec3> {
        let count = (from.distance(to) / (speed * DT)).ceil().max(1.0) as usize;
        (0..=count)
            .map(|i| from.lerp(to, i as f32 / count as f32))
            .collect()
    }

    fn held(at: Vec3, seconds: f32) -> impl Iterator<Item = Vec3> {
        std::iter::repeat_n(at, (seconds / DT).ceil() as usize + 1)
    }

    /// A point in front of the face at `distance` along `direction` (which points away from the
    /// face, out of the wrist or across it).
    fn from_direction(direction: Vec3, distance: f32) -> Vec3 {
        direction.normalize() * distance
    }

    #[test]
    fn a_quick_poke_presses_once() {
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, moving(Vec3::X * 0.1, Vec3::ZERO, 0.5)), 1);
        // Resting on the button does not press again.
        assert_eq!(run(&mut d, held(Vec3::ZERO, 1.0)), 0);
        assert_eq!(d.phase, PressPhase::Pressed);
    }

    #[test]
    fn a_poke_from_any_direction_in_front_presses() {
        // Straight in, from above, from either side along the forearm, and at an angle.
        let directions = [
            Vec3::X,
            Vec3::new(0.3, 1.0, 0.0),
            Vec3::new(0.2, 0.0, 1.0),
            Vec3::new(0.2, 0.0, -1.0),
            Vec3::new(1.0, -1.0, 1.0),
        ];
        for direction in directions {
            let mut d = PressDetector::new();
            let start = from_direction(direction, 0.12);
            assert_eq!(
                run(&mut d, moving(start, Vec3::ZERO, 0.5)),
                1,
                "{direction}"
            );
        }
    }

    #[test]
    fn the_press_needs_the_finger_to_move_away_past_the_release_radius() {
        let t = tuning();
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, moving(Vec3::X * 0.1, Vec3::ZERO, 0.5)), 1);
        // Jitter between touching and just inside the release radius.
        let jitter = (0..40).map(|i| {
            Vec3::X
                * if i % 2 == 0 {
                    t.touch_radius_m * 0.5
                } else {
                    t.release_radius_m * 0.95
                }
        });
        assert_eq!(run(&mut d, jitter), 0);
        // Away, and in again: a second press.
        assert_eq!(run(&mut d, moving(Vec3::ZERO, Vec3::X * 0.1, 0.5)), 0);
        assert_eq!(run(&mut d, moving(Vec3::X * 0.1, Vec3::ZERO, 0.5)), 1);
    }

    #[test]
    fn a_slow_touch_presses_only_after_the_dwell() {
        let t = tuning();
        let mut d = PressDetector::new();
        let slow = t.min_entry_speed_mps * 0.3;
        // Just inside the touch radius, so that the move in takes little of the dwell.
        let rest = Vec3::X * t.touch_radius_m * 0.9;
        assert_eq!(run(&mut d, moving(Vec3::X * 0.1, rest, slow)), 0);
        assert!(matches!(d.phase, PressPhase::Touching { fast: false, .. }));
        assert_eq!(run(&mut d, held(rest, t.dwell_s)), 1);
        // A slow touch that leaves before the dwell does nothing.
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, moving(Vec3::X * 0.1, rest, slow)), 0);
        assert_eq!(run(&mut d, held(rest, t.dwell_s * 0.3)), 0);
        assert_eq!(run(&mut d, moving(rest, Vec3::X * 0.1, 0.5)), 0);
        assert!(matches!(d.phase, PressPhase::Armed { .. }));
    }

    #[test]
    fn a_fast_pass_beside_the_button_does_not_press() {
        let t = tuning();
        let mut d = PressDetector::new();
        // Sweeping along the forearm just outside the touch radius, fast.
        let offset = t.touch_radius_m * 1.3;
        let sweep = moving(
            Vec3::new(offset, 0.0, 0.15),
            Vec3::new(offset, 0.0, -0.15),
            2.0,
        );
        assert_eq!(run(&mut d, sweep), 0);
    }

    #[test]
    fn a_fingertip_behind_the_wrist_does_not_press() {
        let t = tuning();
        let mut d = PressDetector::new();
        // Coming up through the arm, from behind the face's plane.
        let below = Vec3::new(-(t.max_behind_m + 0.01), 0.0, 0.0);
        assert_eq!(
            run(&mut d, moving(Vec3::new(-0.1, 0.0, 0.0), below, 0.5)),
            0
        );
        assert_eq!(run(&mut d, held(below, t.dwell_s * 2.0)), 0);
        assert_eq!(d.phase, PressPhase::Blocked);
        // Away, and a poke from in front presses.
        assert_eq!(run(&mut d, moving(below, Vec3::X * 0.1, 0.5)), 0);
        assert_eq!(run(&mut d, moving(Vec3::X * 0.1, Vec3::ZERO, 0.5)), 1);
    }

    #[test]
    fn a_blocked_fingertip_is_free_once_it_leaves_the_button() {
        let t = tuning();
        let mut d = PressDetector::new();
        let below = Vec3::new(-(t.max_behind_m + 0.005), 0.0, 0.0);
        assert_eq!(
            run(&mut d, moving(Vec3::new(-0.1, 0.0, 0.0), below, 0.5)),
            0
        );
        assert_eq!(d.phase, PressPhase::Blocked);
        // Just outside the touch sphere, well inside the release radius, frees it.
        let outside = Vec3::Y * (t.touch_radius_m + 0.002);
        assert!(outside.length() < t.release_radius_m);
        assert_eq!(run(&mut d, moving(below, outside, 0.5)), 0);
        assert!(matches!(d.phase, PressPhase::Armed { .. }));
        assert_eq!(run(&mut d, moving(outside, Vec3::X * 0.005, 0.5)), 1);
    }

    #[test]
    fn losing_tracking_resets_without_a_press() {
        let t = tuning();
        let mut d = PressDetector::new();
        let near = Vec3::X * (t.touch_radius_m + 0.005);
        assert_eq!(run(&mut d, moving(Vec3::X * 0.1, near, 0.5)), 0);
        let lost = d.step(None, Some(FACE), DT, &t);
        assert!(!lost.pressed);
        assert_eq!(lost.phase, PressPhase::Lost);
        // Reappearing on the button is not a press.
        assert!(!d.step(Some(Vec3::ZERO), Some(FACE), DT, &t).pressed);
        assert_eq!(d.phase, PressPhase::Blocked);
        assert_eq!(run(&mut d, held(Vec3::ZERO, t.dwell_s * 2.0)), 0);
    }

    #[test]
    fn the_dwell_progress_fills_while_touching() {
        let t = tuning();
        assert_eq!(
            PressPhase::Armed { approach_mps: 1.0 }.dwell_progress(&t),
            0.0
        );
        let half = PressPhase::Touching {
            fast: false,
            dwell_s: t.dwell_s * 0.5,
        };
        assert!((half.dwell_progress(&t) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_point_is_carried_with_its_hand() {
        let from = Pose {
            position: Vec3::new(1.0, 0.0, 0.0),
            orientation: Quat::IDENTITY,
        };
        let to = Pose {
            position: Vec3::new(0.0, 2.0, 0.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        // A point 0.1 m ahead (-Z) of the first pose is 0.1 m ahead of the second, which a quarter
        // turn about +Y points along -X.
        let carried = carry(Vec3::new(1.0, 0.0, -0.1), from, to);
        assert!(
            carried.distance(Vec3::new(-0.1, 2.0, 0.0)) < 1e-5,
            "{carried}"
        );
        assert_eq!(carry(Vec3::ONE, Pose::IDENTITY, Pose::IDENTITY), Vec3::ONE);
    }

    #[test]
    fn the_button_mirrors_onto_the_right_wrist() {
        let t = tuning();
        let left = button_face(Hand::Left, Pose::IDENTITY, &t).unwrap();
        let right = button_face(Hand::Right, Pose::IDENTITY, &t).unwrap();
        assert_eq!(left.centre, t.offset_m);
        assert_eq!(
            right.centre,
            Vec3::new(-t.offset_m.x, t.offset_m.y, t.offset_m.z)
        );
        assert_eq!(left.normal, Vec3::X);
        assert_eq!(right.normal, Vec3::NEG_X);
        // The button turns with the grip.
        let turned = Pose {
            position: Vec3::new(1.0, 0.0, 0.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let face = button_face(Hand::Left, turned, &t).unwrap();
        assert!(face.normal.distance(Vec3::NEG_Z) < 1e-5, "{face:?}");
        let degenerate = WristDetonatorConfig {
            normal: Vec3::ZERO,
            ..t
        };
        assert_eq!(button_face(Hand::Left, Pose::IDENTITY, &degenerate), None);
    }

    #[test]
    fn the_fingertip_prefers_the_tracked_skeleton() {
        let t = tuning();
        let aim = Pose::IDENTITY;
        let mut joints = [HandJoint::default(); HAND_JOINT_COUNT];
        let estimated = fingertip(
            Hand::Right,
            FingertipSource {
                joints: Some(&joints),
                aim,
                trigger_touch: false,
            },
            &t,
        );
        assert_eq!(estimated, t.fingertip_pointing_m);
        joints[INDEX_TIP_JOINT] = HandJoint {
            pose: Pose {
                position: Vec3::new(0.1, 0.2, 0.3),
                orientation: Quat::IDENTITY,
            },
            radius: 0.01,
            valid: true,
            tracked: true,
        };
        let tracked = fingertip(
            Hand::Right,
            FingertipSource {
                joints: Some(&joints),
                aim,
                trigger_touch: false,
            },
            &t,
        );
        assert_eq!(tracked, Vec3::new(0.1, 0.2, 0.3));
        // On the trigger, and mirrored for the left hand.
        let resting = fingertip(
            Hand::Left,
            FingertipSource {
                joints: None,
                aim,
                trigger_touch: true,
            },
            &t,
        );
        let r = t.fingertip_resting_m;
        assert_eq!(resting, Vec3::new(-r.x, r.y, r.z));
    }
}
