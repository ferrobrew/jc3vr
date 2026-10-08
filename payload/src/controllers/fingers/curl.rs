//! Per-finger curl and splay, pure: derived from the tracked hand joints, or estimated from the
//! inputs the fingers rest on.
//!
//! A curl runs from 0 (the finger straight) to 1 (the finger closed into a fist). A splay is the
//! finger's sideways turn at its knuckle toward the thumb side, in radians.

use glam::Quat;

use crate::controllers::{
    fingers::config::FingerSource,
    frame::{HAND_JOINT_COUNT, Hand, HandFrame, HandInputs, HandJoint, InteractionProfile},
};

/// A finger of the hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Finger {
    Thumb,
    Index,
    Middle,
    Ring,
    Little,
}

impl Finger {
    pub const ALL: [Finger; 5] = [
        Finger::Thumb,
        Finger::Index,
        Finger::Middle,
        Finger::Ring,
        Finger::Little,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Finger::Thumb => "thumb",
            Finger::Index => "index",
            Finger::Middle => "middle",
            Finger::Ring => "ring",
            Finger::Little => "little",
        }
    }
}

/// One hand's curls and splays, indexed by [`Finger::index`].
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct FingerCurls {
    pub curl: [f32; 5],
    /// Radians toward the thumb side; the thumb's is always zero.
    pub splay: [f32; 5],
}

/// Where a hand's curls came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CurlSource {
    Joints,
    Estimate,
}

impl CurlSource {
    pub const fn label(self) -> &'static str {
        match self {
            CurlSource::Joints => "joints",
            CurlSource::Estimate => "estimate",
        }
    }
}

/// A hand's curls from the configured source, or `None` when that source has nothing for it (the
/// joints alone are wanted and the runtime reports none, or the controller is absent).
pub fn select(
    setting: FingerSource,
    hand: Hand,
    frame: &HandFrame,
) -> Option<(CurlSource, FingerCurls)> {
    if !frame.active {
        return None;
    }
    let joints = || {
        frame
            .joints
            .as_ref()
            .and_then(|joints| from_joints(hand, joints))
            .map(|curls| (CurlSource::Joints, curls))
    };
    let estimated = || (CurlSource::Estimate, estimate(&frame.inputs, frame.profile));
    match setting {
        FingerSource::Auto => Some(joints().unwrap_or_else(estimated)),
        FingerSource::Joints => joints(),
        FingerSource::Estimate => Some(estimated()),
    }
}

/// The curls from a tracked hand skeleton, or `None` when a joint they read is not valid.
///
/// Every `XR_EXT_hand_tracking` joint has -Z along its bone toward the fingertip and +Y out of the
/// back of the hand, on both hands, so a joint's flexion is its rotation relative to its parent
/// joint about the parent's -X, and a knuckle's sideways turn is the rotation about +Y. A finger's
/// curl is the sum of its three flexions (at the knuckle, the middle, and the last joint) over
/// their sum at a full fist; the thumb's is the sum of its two outer flexions over theirs.
pub fn from_joints(hand: Hand, joints: &[HandJoint; HAND_JOINT_COUNT]) -> Option<FingerCurls> {
    let mut curls = FingerCurls::default();
    for finger in Finger::ALL {
        let chain = chain(finger);
        if chain.iter().any(|&j| !joints[j].valid) {
            return None;
        }
        let orientation = |k: usize| joints[chain[k]].pose.orientation;
        let flexion = |k: usize| flexion(orientation(k), orientation(k + 1));
        let (bent, full) = match finger {
            Finger::Thumb => (flexion(0) + flexion(1), THUMB_FULL_FLEX_RAD),
            _ => (flexion(0) + flexion(1) + flexion(2), FINGER_FULL_FLEX_RAD),
        };
        curls.curl[finger.index()] = (bent / full).clamp(0.0, 1.0);
        if finger != Finger::Thumb {
            // The spec's +Y turn swings -Z toward -X, the thumb side of a right hand and the
            // little finger's side of a left hand.
            let turn = twist(orientation(0).inverse() * orientation(1), TwistAxis::Y);
            curls.splay[finger.index()] = match hand {
                Hand::Right => turn,
                Hand::Left => -turn,
            };
        }
    }
    Some(curls)
}

/// The curls estimated from the inputs the fingers rest on.
///
/// - The thumb rests on the face: touching the thumbstick, the trackpad, or a face button bends it
///   onto the face, and a click or a trackpad press bends it further. Off the face it is straight.
/// - The index finger curls with the trigger's pull once it touches the trigger, starting from
///   the bend that rests it on the trigger; off the trigger it points.
/// - The middle, ring, and little fingers share the grip. On the Index controller the grip value
///   follows the fingers closing around the handle and its force carries them to a fist; on a
///   Touch controller the grip value is a pull, from fingers resting on the handle to a fist.
pub fn estimate(inputs: &HandInputs, profile: InteractionProfile) -> FingerCurls {
    let pressed = inputs.thumbstick_click
        || inputs.a_click
        || inputs.b_click
        || inputs.trackpad_force > TRACKPAD_PRESSED_FORCE;
    let resting = [
        (inputs.thumbstick_touch, THUMB_ON_STICK),
        (inputs.trackpad_touch, THUMB_ON_TRACKPAD),
        (inputs.a_touch || inputs.a_click, THUMB_ON_A),
        (inputs.b_touch || inputs.b_click, THUMB_ON_B),
    ]
    .into_iter()
    .filter(|&(touched, _)| touched)
    .map(|(_, curl)| curl)
    .fold(None, |most: Option<f32>, curl| {
        Some(most.map_or(curl, |m| m.max(curl)))
    });
    let thumb = match (resting, pressed) {
        (Some(curl), true) => curl + THUMB_PRESS_EXTRA,
        (Some(curl), false) => curl,
        (None, true) => THUMB_ON_A + THUMB_PRESS_EXTRA,
        (None, false) => 0.0,
    };

    let trigger = inputs.trigger.clamp(0.0, 1.0);
    let index = if inputs.trigger_touch || trigger > 0.0 {
        lerp(TRIGGER_REST, 1.0, trigger)
    } else {
        0.0
    };

    let squeeze = inputs.squeeze.clamp(0.0, 1.0);
    let grip = match profile {
        InteractionProfile::OculusTouch => lerp(TOUCH_GRIP_REST, 1.0, squeeze),
        _ => lerp(squeeze, 1.0, inputs.squeeze_force.clamp(0.0, 1.0)),
    };

    FingerCurls {
        curl: [thumb.clamp(0.0, 1.0), index, grip, grip, grip],
        splay: [0.0; 5],
    }
}

/// The sum of a finger's knuckle, middle, and last joint flexions at a full fist.
const FINGER_FULL_FLEX_RAD: f32 = (90.0 + 105.0 + 75.0) * std::f32::consts::PI / 180.0;
/// The sum of the thumb's two outer flexions at a full bend.
const THUMB_FULL_FLEX_RAD: f32 = (55.0 + 80.0) * std::f32::consts::PI / 180.0;

/// The estimated thumb curl resting on each surface of the face. The face buttons sit nearer the
/// palm than the thumbstick, so the thumb bends further to reach them.
const THUMB_ON_STICK: f32 = 0.45;
const THUMB_ON_TRACKPAD: f32 = 0.5;
const THUMB_ON_A: f32 = 0.6;
const THUMB_ON_B: f32 = 0.55;
/// The extra curl of a thumb pressing down.
const THUMB_PRESS_EXTRA: f32 = 0.15;
const TRACKPAD_PRESSED_FORCE: f32 = 0.2;
/// The index finger's curl resting on the trigger.
const TRIGGER_REST: f32 = 0.35;
/// The curl of fingers resting on a Touch controller's grip without pulling it.
const TOUCH_GRIP_REST: f32 = 0.4;

/// The `XR_EXT_hand_tracking` joints of a finger, from its root to the last joint before the tip:
/// the thumb's metacarpal, proximal, and distal, and a finger's metacarpal, proximal,
/// intermediate, and distal.
fn chain(finger: Finger) -> &'static [usize] {
    match finger {
        Finger::Thumb => &[2, 3, 4],
        Finger::Index => &[6, 7, 8, 9],
        Finger::Middle => &[11, 12, 13, 14],
        Finger::Ring => &[16, 17, 18, 19],
        Finger::Little => &[21, 22, 23, 24],
    }
}

/// The flexion of `child` relative to `parent`: its turn about the parent's -X, which swings -Z
/// toward the palm.
fn flexion(parent: Quat, child: Quat) -> f32 {
    -twist(parent.inverse() * child, TwistAxis::X)
}

#[derive(Copy, Clone)]
enum TwistAxis {
    X,
    Y,
}

/// The angle of `q`'s twist about `axis` (its swing-twist decomposition), in `-π..=π`.
fn twist(q: Quat, axis: TwistAxis) -> f32 {
    let component = match axis {
        TwistAxis::X => q.x,
        TwistAxis::Y => q.y,
    };
    let angle = 2.0 * component.atan2(q.w);
    if angle > std::f32::consts::PI {
        angle - std::f32::consts::TAU
    } else if angle < -std::f32::consts::PI {
        angle + std::f32::consts::TAU
    } else {
        angle
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::controllers::frame::Pose;

    /// A hand skeleton with every finger's joints bent by `flex` radians each, and each knuckle
    /// turned by `turn` radians about +Y.
    fn skeleton(flex: f32, turn: f32) -> [HandJoint; HAND_JOINT_COUNT] {
        let mut joints = [HandJoint {
            pose: Pose::default(),
            radius: 0.01,
            valid: true,
            tracked: true,
        }; HAND_JOINT_COUNT];
        for finger in Finger::ALL {
            let mut orientation = Quat::from_rotation_z(0.3);
            for (k, &j) in chain(finger).iter().enumerate() {
                if k == 1 && finger != Finger::Thumb {
                    orientation *= Quat::from_rotation_y(turn);
                }
                if k > 0 {
                    orientation *= Quat::from_rotation_x(-flex);
                }
                joints[j].pose = Pose {
                    position: Vec3::ZERO,
                    orientation,
                };
            }
        }
        joints
    }

    #[test]
    fn a_flat_hand_is_open_and_a_fist_is_closed() {
        let open = from_joints(Hand::Right, &skeleton(0.0, 0.0)).unwrap();
        assert!(open.curl.iter().all(|&c| c.abs() < 1e-5), "{open:?}");
        let fist = from_joints(Hand::Right, &skeleton(1.7, 0.0)).unwrap();
        assert!(fist.curl.iter().all(|&c| c > 0.99), "{fist:?}");
    }

    #[test]
    fn a_partial_bend_is_a_partial_curl() {
        let curls = from_joints(Hand::Left, &skeleton(0.5, 0.0)).unwrap();
        let finger = 1.5 / FINGER_FULL_FLEX_RAD;
        assert!((curls.curl[Finger::Index.index()] - finger).abs() < 1e-4);
        let thumb = 1.0 / THUMB_FULL_FLEX_RAD;
        assert!((curls.curl[Finger::Thumb.index()] - thumb).abs() < 1e-4);
    }

    #[test]
    fn hyperextension_reads_as_open() {
        let curls = from_joints(Hand::Right, &skeleton(-0.3, 0.0)).unwrap();
        assert!(curls.curl.iter().all(|&c| c == 0.0));
    }

    #[test]
    fn the_splay_sign_follows_the_side_of_the_thumb() {
        let right = from_joints(Hand::Right, &skeleton(0.0, 0.2)).unwrap();
        let left = from_joints(Hand::Left, &skeleton(0.0, 0.2)).unwrap();
        assert!((right.splay[Finger::Index.index()] - 0.2).abs() < 1e-5);
        assert!((left.splay[Finger::Index.index()] + 0.2).abs() < 1e-5);
        assert_eq!(right.splay[Finger::Thumb.index()], 0.0);
    }

    #[test]
    fn an_invalid_joint_gives_no_curls() {
        let mut joints = skeleton(0.2, 0.0);
        joints[13].valid = false;
        assert!(from_joints(Hand::Right, &joints).is_none());
    }

    #[test]
    fn the_index_finger_points_off_the_trigger_and_rests_on_it() {
        let mut inputs = HandInputs::default();
        let off = estimate(&inputs, InteractionProfile::ValveIndex);
        assert_eq!(off.curl[Finger::Index.index()], 0.0);
        inputs.trigger_touch = true;
        let resting = estimate(&inputs, InteractionProfile::ValveIndex);
        assert_eq!(resting.curl[Finger::Index.index()], TRIGGER_REST);
        inputs.trigger = 1.0;
        let pulled = estimate(&inputs, InteractionProfile::ValveIndex);
        assert_eq!(pulled.curl[Finger::Index.index()], 1.0);
    }

    #[test]
    fn the_thumb_bends_onto_the_face_and_further_when_pressing() {
        let mut inputs = HandInputs::default();
        assert_eq!(
            estimate(&inputs, InteractionProfile::ValveIndex).curl[0],
            0.0
        );
        inputs.thumbstick_touch = true;
        assert_eq!(
            estimate(&inputs, InteractionProfile::ValveIndex).curl[0],
            THUMB_ON_STICK
        );
        inputs.a_touch = true;
        assert_eq!(
            estimate(&inputs, InteractionProfile::ValveIndex).curl[0],
            THUMB_ON_A
        );
        inputs.a_click = true;
        let pressed = estimate(&inputs, InteractionProfile::ValveIndex).curl[0];
        assert!((pressed - (THUMB_ON_A + THUMB_PRESS_EXTRA)).abs() < 1e-6);
    }

    #[test]
    fn the_grip_fingers_follow_each_profile() {
        let mut inputs = HandInputs::default();
        let index_open = estimate(&inputs, InteractionProfile::ValveIndex);
        assert_eq!(index_open.curl[Finger::Middle.index()], 0.0);
        let touch_rest = estimate(&inputs, InteractionProfile::OculusTouch);
        assert_eq!(touch_rest.curl[Finger::Ring.index()], TOUCH_GRIP_REST);
        inputs.squeeze = 0.5;
        inputs.squeeze_force = 1.0;
        let forced = estimate(&inputs, InteractionProfile::ValveIndex);
        assert_eq!(forced.curl[Finger::Little.index()], 1.0);
        inputs.squeeze_force = 0.0;
        let half = estimate(&inputs, InteractionProfile::ValveIndex);
        assert_eq!(half.curl[Finger::Middle.index()], 0.5);
    }

    #[test]
    fn the_source_setting_selects_or_falls_back() {
        let mut frame = HandFrame {
            active: true,
            ..HandFrame::default()
        };
        let auto = select(FingerSource::Auto, Hand::Right, &frame).unwrap();
        assert_eq!(auto.0, CurlSource::Estimate);
        assert!(select(FingerSource::Joints, Hand::Right, &frame).is_none());
        frame.joints = Some(skeleton(0.4, 0.0));
        assert_eq!(
            select(FingerSource::Auto, Hand::Right, &frame).unwrap().0,
            CurlSource::Joints
        );
        assert_eq!(
            select(FingerSource::Estimate, Hand::Right, &frame)
                .unwrap()
                .0,
            CurlSource::Estimate
        );
        frame.active = false;
        assert!(select(FingerSource::Estimate, Hand::Right, &frame).is_none());
    }
}
