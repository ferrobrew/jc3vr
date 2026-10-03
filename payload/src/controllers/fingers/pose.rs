//! The finger bones' bend, pure: from a hand's curls to per-joint angles, and from the angles to
//! the model-space joints written over the animated ones.
//!
//! The angles are absolute: each is a joint's bend away from a straight finger, not a bend added to
//! the animated one, so a hand whose animation already holds the fingers bent opens fully and
//! closes no further than a fist. The straight finger is built from the animated palm, which stays
//! as animated: a finger's open direction is the one from the wrist to its knuckle, and the
//! thumb's lies in the palm's plane, turned from the fingers' direction toward the thumb side by a
//! spread angle. A finger bends toward the palm about the axis square to its open direction and to
//! the palm's normal, and turns sideways about the axis square to its open direction and to the
//! thumb side; the thumb bends across the palm, toward the little finger's side.
//!
//! The bones' rest frames are not known, so each bone's own axis along its length is read from the
//! animated pose: the proximal and middle bones' from the direction to the next joint in the
//! bone's frame, and the distal bone's, which has no joint beyond it, as the middle bone's. Each
//! bone then turns by the shortest arc from its animated direction to its target direction, which
//! keeps the bone's animated roll about its length.

use glam::{Quat, Vec3};

use crate::controllers::{
    fingers::{
        config::FingersConfig,
        curl::{Finger, FingerCurls},
    },
    frame::Hand,
    hands::pose as hand_pose,
};

/// One bone's model-space joint.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BoneJoint {
    pub position: Vec3,
    pub orientation: Quat,
}

/// One hand's target finger pose: each joint's bend from straight, radians, indexed by
/// [`Finger::index`] and then from the finger's root joint outward.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandBend {
    pub bend: [[f32; 3]; 5],
    /// The knuckle's sideways turn toward the thumb side.
    pub splay: [f32; 5],
    /// Which fingers are driven; the others keep the animated pose.
    pub driven: [bool; 5],
    /// The open thumb's turn from the fingers' direction toward the thumb side, radians.
    pub thumb_spread: f32,
    /// How far the driven fingers go from the animated pose to the target, from 0 to 1.
    pub weight: f32,
}

/// The largest bend any joint is given, degrees, whatever the configuration asks for.
pub const MAX_FLEXION_DEG: f32 = 110.0;
/// The largest bend back past straight any joint is given, degrees.
pub const MAX_HYPEREXTENSION_DEG: f32 = 30.0;

/// The target pose for a hand's `curls`, reached by the hand's tier `weight`. A hand that holds a
/// weapon drives only its trigger finger unless the configuration drives every finger.
pub fn hand_bend(
    curls: &FingerCurls,
    config: &FingersConfig,
    weight: f32,
    holding: bool,
) -> HandBend {
    let max_splay = config.max_splay_deg.max(0.0).to_radians();
    let mut out = HandBend {
        thumb_spread: config.thumb_spread_deg.to_radians(),
        weight: weight.clamp(0.0, 1.0),
        ..HandBend::default()
    };
    for finger in Finger::ALL {
        let i = finger.index();
        let driven = !holding || config.drive_all_when_holding || finger == Finger::Index;
        if !driven {
            continue;
        }
        out.driven[i] = true;
        let share = config.curl_ranges[i].at(curls.curl[i]);
        let full = match finger {
            Finger::Thumb => config.thumb_bend_deg,
            _ => config.finger_bend_deg,
        };
        out.bend[i] = std::array::from_fn(|k| {
            (config.rest_offset_deg[i][k] + full[k] * share)
                .clamp(-MAX_HYPEREXTENSION_DEG, MAX_FLEXION_DEG)
                .to_radians()
        });
        out.splay[i] = (curls.splay[i] * config.splay_scale).clamp(-max_splay, max_splay);
    }
    out
}

/// The hand's frame for bending, from the animated wrist and knuckles (model space).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PalmFrame {
    pub wrist: Vec3,
    /// Out of the palm (the opposite of the back of the hand).
    pub palm: Vec3,
    /// From the wrist toward the middle knuckle, in the palm's plane.
    pub forward: Vec3,
    /// Across the knuckles toward the thumb side, in the palm's plane.
    pub toward_thumb: Vec3,
}

impl PalmFrame {
    /// The frame from the wrist and the index, middle, and little fingers' knuckles, or `None` for
    /// a degenerate hand.
    pub fn from_bones(
        hand: Hand,
        wrist: Vec3,
        index: Vec3,
        middle: Vec3,
        little: Vec3,
    ) -> Option<Self> {
        let frame = hand_pose::wrist_frame_from_bones(hand, wrist, index, middle)?;
        let back = frame * Vec3::Y;
        let forward = frame * Vec3::NEG_Z;
        let across = index - little;
        let toward_thumb =
            (across - back * across.dot(back) - forward * across.dot(forward)).try_normalize()?;
        Some(PalmFrame {
            wrist,
            palm: -back,
            forward,
            toward_thumb,
        })
    }
}

/// A finger's three joints (root outward) posed with `angles` bent from straight and `splay`
/// sideways, reached by `weight` from the animated `chain`, each joint carrying the ones beyond it.
pub fn bend_finger(
    chain: [BoneJoint; 3],
    palm: &PalmFrame,
    angles: [f32; 3],
    splay: f32,
    weight: f32,
) -> [BoneJoint; 3] {
    let Some(axes) = bone_axes(&chain) else {
        return chain;
    };
    let Some(open) = (chain[0].position - palm.wrist).try_normalize() else {
        return chain;
    };
    let (Some(flex_axis), Some(splay_axis)) = (
        open.cross(palm.palm).try_normalize(),
        open.cross(palm.toward_thumb).try_normalize(),
    ) else {
        return chain;
    };
    let turn = Quat::from_axis_angle(splay_axis, splay);
    let targets = bent_directions(turn * open, turn * flex_axis, angles);
    pose_chain(chain, axes, targets, weight)
}

/// The thumb's three joints (root outward) posed with `angles` bent across the palm from the open
/// thumb, which lies in the palm's plane turned `spread` radians from the fingers toward the thumb
/// side, reached by `weight` from the animated `chain`.
pub fn bend_thumb(
    chain: [BoneJoint; 3],
    palm: &PalmFrame,
    angles: [f32; 3],
    spread: f32,
    weight: f32,
) -> [BoneJoint; 3] {
    let Some(axes) = bone_axes(&chain) else {
        return chain;
    };
    let open = palm.forward * spread.cos() + palm.toward_thumb * spread.sin();
    let toward = (palm.palm - palm.toward_thumb).normalize_or_zero();
    let Some(axis) = open.cross(toward).try_normalize() else {
        return chain;
    };
    let targets = bent_directions(open, axis, angles);
    pose_chain(chain, axes, targets, weight)
}

/// Each bone's axis along its length in its own frame, or `None` for a chain with coincident joints.
fn bone_axes(chain: &[BoneJoint; 3]) -> Option<[Vec3; 3]> {
    let local = |k: usize| {
        (chain[k].orientation.inverse() * (chain[k + 1].position - chain[k].position))
            .try_normalize()
    };
    let proximal = local(0)?;
    let middle = local(1)?;
    Some([proximal, middle, middle])
}

/// The bones' target directions: the first is `open` bent by the root angle about `axis`, and each
/// further one the previous bent by its own.
fn bent_directions(open: Vec3, axis: Vec3, angles: [f32; 3]) -> [Vec3; 3] {
    let mut direction = open;
    angles.map(|angle| {
        direction = Quat::from_axis_angle(axis, angle) * direction;
        direction
    })
}

/// Turn each bone, root outward, by `weight` of the shortest arc from its direction to its target.
fn pose_chain(
    chain: [BoneJoint; 3],
    axes: [Vec3; 3],
    targets: [Vec3; 3],
    weight: f32,
) -> [BoneJoint; 3] {
    let mut chain = chain;
    for (k, (axis, target)) in axes.into_iter().zip(targets).enumerate() {
        let current = (chain[k].orientation * axis).normalize();
        let arc = Quat::from_rotation_arc(current, target.normalize());
        rotate_from(
            &mut chain,
            k,
            Quat::IDENTITY.slerp(arc, weight.clamp(0.0, 1.0)),
        );
    }
    chain
}

/// Rotate joint `from` and every joint beyond it by `rotation` about joint `from`'s position.
fn rotate_from(chain: &mut [BoneJoint; 3], from: usize, rotation: Quat) {
    let pivot = chain[from].position;
    for joint in &mut chain[from..] {
        joint.position = pivot + rotation * (joint.position - pivot);
        joint.orientation = (rotation * joint.orientation).normalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::fingers::config::CurlRange;

    const EPS: f32 = 1e-4;

    /// A right hand in the wrist frame's convention: fingers along -Z, the back of the hand up
    /// (+Y), and the little finger at +X.
    const RIGHT_WRIST: Vec3 = Vec3::ZERO;
    const RIGHT_INDEX: Vec3 = Vec3::new(-0.03, 0.0, -0.09);
    const RIGHT_MIDDLE: Vec3 = Vec3::new(0.0, 0.0, -0.09);
    const RIGHT_LITTLE: Vec3 = Vec3::new(0.05, 0.0, -0.09);
    const RIGHT_THUMB_BASE: Vec3 = Vec3::new(-0.025, -0.01, -0.02);

    fn palm(hand: Hand) -> PalmFrame {
        let side = |v: Vec3| match hand {
            Hand::Right => v,
            Hand::Left => mirror(v),
        };
        PalmFrame::from_bones(
            hand,
            side(RIGHT_WRIST),
            side(RIGHT_INDEX),
            side(RIGHT_MIDDLE),
            side(RIGHT_LITTLE),
        )
        .unwrap()
    }

    /// The reflection that carries a right hand onto a left one.
    fn mirror(v: Vec3) -> Vec3 {
        Vec3::new(-v.x, v.y, v.z)
    }

    fn mirror_joint(j: BoneJoint) -> BoneJoint {
        let q = j.orientation;
        BoneJoint {
            position: mirror(j.position),
            orientation: Quat::from_xyzw(q.x, -q.y, -q.z, q.w),
        }
    }

    /// A chain from `start` whose bones point along `directions`, with each bone's length along its
    /// own +X and rolled about it by `roll`, as a rig with unknown bone frames might hold it.
    fn chain_along(start: Vec3, directions: [Vec3; 3], roll: f32) -> [BoneJoint; 3] {
        let lengths = [0.045, 0.03];
        let mut position = start;
        std::array::from_fn(|k| {
            let joint = BoneJoint {
                position,
                orientation: Quat::from_rotation_arc(Vec3::X, directions[k].normalize())
                    * Quat::from_rotation_x(roll),
            };
            if k < 2 {
                position += directions[k].normalize() * lengths[k];
            }
            joint
        })
    }

    /// A right-hand finger rooted at `knuckle`, each joint already bent toward the palm by `flex`
    /// radians, as an animation holding the fingers curled would leave it.
    fn animated_finger(knuckle: Vec3, flex: f32) -> [BoneJoint; 3] {
        let open = (knuckle - RIGHT_WRIST).normalize();
        let axis = open.cross(Vec3::NEG_Y).normalize();
        let directions = [1.0, 2.0, 3.0].map(|n| Quat::from_axis_angle(axis, n * flex) * open);
        chain_along(knuckle, directions, 0.7)
    }

    fn direction(chain: &[BoneJoint; 3], k: usize) -> Vec3 {
        match k {
            0 | 1 => (chain[k + 1].position - chain[k].position).normalize(),
            // The distal bone's length runs along +X in `chain_along`.
            _ => chain[2].orientation * Vec3::X,
        }
    }

    fn full_curl() -> FingerCurls {
        FingerCurls {
            curl: [1.0; 5],
            splay: [0.0; 5],
        }
    }

    #[test]
    fn the_palm_frame_faces_down_with_the_thumb_to_the_left() {
        let palm = palm(Hand::Right);
        assert!(palm.palm.abs_diff_eq(Vec3::NEG_Y, 1e-5), "{palm:?}");
        assert!(palm.forward.abs_diff_eq(Vec3::NEG_Z, 1e-5), "{palm:?}");
        assert!(palm.toward_thumb.abs_diff_eq(Vec3::NEG_X, 1e-5), "{palm:?}");
    }

    #[test]
    fn an_open_hand_straightens_a_finger_the_animation_holds_curled() {
        let palm = palm(Hand::Right);
        let chain = animated_finger(RIGHT_MIDDLE, 0.9);
        let open = RIGHT_MIDDLE.normalize();
        let bend = hand_bend(&FingerCurls::default(), &FingersConfig::new(), 1.0, false);
        let posed = bend_finger(chain, &palm, bend.bend[Finger::Middle.index()], 0.0, 1.0);
        assert_eq!(posed[0].position, chain[0].position, "the knuckle stays");
        for k in 0..3 {
            assert!(
                direction(&posed, k).abs_diff_eq(open, EPS),
                "bone {k} is straight: {:?}",
                direction(&posed, k)
            );
        }
        for k in 0..2 {
            let before = chain[k + 1].position.distance(chain[k].position);
            let after = posed[k + 1].position.distance(posed[k].position);
            assert!((before - after).abs() < 1e-5, "bone lengths are kept");
        }
    }

    #[test]
    fn the_pose_does_not_depend_on_the_animated_curl() {
        let palm = palm(Hand::Right);
        let bend = hand_bend(&full_curl(), &FingersConfig::new(), 1.0, false);
        let angles = bend.bend[Finger::Index.index()];
        let from_straight = bend_finger(animated_finger(RIGHT_INDEX, 0.0), &palm, angles, 0.0, 1.0);
        let from_curled = bend_finger(animated_finger(RIGHT_INDEX, 1.2), &palm, angles, 0.0, 1.0);
        for k in 0..3 {
            assert!(
                from_straight[k]
                    .position
                    .abs_diff_eq(from_curled[k].position, EPS)
            );
            assert!(direction(&from_straight, k).abs_diff_eq(direction(&from_curled, k), EPS));
        }
    }

    #[test]
    fn a_full_curl_stays_within_the_joint_range() {
        let palm = palm(Hand::Right);
        let mut config = FingersConfig::new();
        let check = |config: &FingersConfig| {
            let bend = hand_bend(&full_curl(), config, 1.0, false);
            for finger in Finger::ALL {
                for angle in bend.bend[finger.index()] {
                    assert!(angle.to_degrees() <= MAX_FLEXION_DEG + EPS, "{finger:?}");
                }
            }
            let angles = bend.bend[Finger::Middle.index()];
            let posed = bend_finger(animated_finger(RIGHT_MIDDLE, 1.0), &palm, angles, 0.0, 1.0);
            let mut previous = RIGHT_MIDDLE.normalize();
            for k in 0..3 {
                let d = direction(&posed, k);
                let turned = previous.angle_between(d).to_degrees();
                assert!(turned <= MAX_FLEXION_DEG + 0.1, "joint {k} turns {turned}°");
                assert!(turned > 45.0, "joint {k} closes: {turned}°");
                previous = d;
            }
            assert!(
                posed[2].position.y < 0.0,
                "the fist closes on the palm side"
            );
        };
        check(&config);
        config.finger_bend_deg = [300.0; 3];
        config.rest_offset_deg = [[40.0; 3]; 5];
        check(&config);
    }

    #[test]
    fn the_left_hand_mirrors_the_right() {
        let config = FingersConfig::new();
        let curls = FingerCurls {
            curl: [0.6, 0.4, 0.7, 0.8, 0.9],
            splay: [0.0, 0.15, 0.0, -0.1, -0.2],
        };
        let bend = hand_bend(&curls, &config, 1.0, false);
        let (right, left) = (palm(Hand::Right), palm(Hand::Left));
        let finger = animated_finger(RIGHT_LITTLE, 0.5);
        let i = Finger::Little.index();
        let posed_right = bend_finger(finger, &right, bend.bend[i], bend.splay[i], 1.0);
        let posed_left = bend_finger(
            finger.map(mirror_joint),
            &left,
            bend.bend[i],
            bend.splay[i],
            1.0,
        );
        let thumb = chain_along(
            RIGHT_THUMB_BASE,
            [
                Vec3::new(-0.3, -0.4, -1.0),
                Vec3::new(0.2, -0.6, -1.0),
                Vec3::new(0.5, -0.6, -1.0),
            ],
            -0.4,
        );
        let t = Finger::Thumb.index();
        let thumb_right = bend_thumb(thumb, &right, bend.bend[t], bend.thumb_spread, 1.0);
        let thumb_left = bend_thumb(
            thumb.map(mirror_joint),
            &left,
            bend.bend[t],
            bend.thumb_spread,
            1.0,
        );
        for k in 0..3 {
            assert!(
                posed_left[k]
                    .position
                    .abs_diff_eq(mirror(posed_right[k].position), EPS)
            );
            assert!(
                thumb_left[k]
                    .position
                    .abs_diff_eq(mirror(thumb_right[k].position), EPS)
            );
        }
    }

    #[test]
    fn a_splay_turns_the_finger_toward_the_thumb() {
        let palm = palm(Hand::Right);
        let chain = animated_finger(RIGHT_MIDDLE, 0.6);
        let posed = bend_finger(chain, &palm, [0.0; 3], 0.2, 1.0);
        assert!(posed[2].position.x < -0.01);
        assert!(
            posed[2].position.y.abs() < 1e-5,
            "a splay stays in the palm's plane"
        );
    }

    #[test]
    fn the_open_thumb_lies_in_the_palm_and_closes_across_it() {
        let palm = palm(Hand::Right);
        let thumb = chain_along(
            RIGHT_THUMB_BASE,
            [
                Vec3::new(0.1, -0.5, -1.0),
                Vec3::new(0.4, -0.8, -1.0),
                Vec3::new(0.6, -0.9, -0.8),
            ],
            0.3,
        );
        let spread = 40f32.to_radians();
        let open = bend_thumb(thumb, &palm, [0.0; 3], spread, 1.0);
        let expected = Vec3::new(-spread.sin(), 0.0, -spread.cos());
        for k in 0..3 {
            assert!(direction(&open, k).abs_diff_eq(expected, EPS), "bone {k}");
        }
        let bend = hand_bend(&full_curl(), &FingersConfig::new(), 1.0, false);
        let closed = bend_thumb(
            thumb,
            &palm,
            bend.bend[Finger::Thumb.index()],
            bend.thumb_spread,
            1.0,
        );
        assert!(
            closed[2].position.x > open[2].position.x,
            "toward the little finger"
        );
        assert!(closed[2].position.y < open[2].position.y, "into the palm");
    }

    #[test]
    fn the_weight_blends_from_the_animated_pose() {
        let palm = palm(Hand::Right);
        let chain = animated_finger(RIGHT_INDEX, 0.8);
        let still = bend_finger(chain, &palm, [0.0; 3], 0.0, 0.0);
        for k in 0..3 {
            assert!(still[k].position.abs_diff_eq(chain[k].position, 1e-6));
        }
        let open = RIGHT_INDEX.normalize();
        let half = bend_finger(chain, &palm, [0.0; 3], 0.0, 0.5);
        let animated = open.angle_between(direction(&chain, 0));
        let halfway = open.angle_between(direction(&half, 0));
        assert!((halfway - animated * 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_holding_hand_drives_only_the_trigger_finger() {
        let mut config = FingersConfig::new();
        config.curl_ranges = [CurlRange {
            open: 0.0,
            closed: 1.0,
        }; 5];
        let held = hand_bend(&full_curl(), &config, 1.0, true);
        assert_eq!(held.driven, [false, true, false, false, false]);
        assert_eq!(held.bend[Finger::Middle.index()], [0.0; 3]);
        assert!(
            (held.bend[Finger::Index.index()][0] - config.finger_bend_deg[0].to_radians()).abs()
                < 1e-6
        );
        config.drive_all_when_holding = true;
        assert_eq!(
            hand_bend(&full_curl(), &config, 1.0, true).driven,
            [true; 5]
        );
        let half = hand_bend(&full_curl(), &config, 0.5, false);
        assert_eq!(half.weight, 0.5);
        assert!(
            (half.bend[Finger::Thumb.index()][2] - config.thumb_bend_deg[2].to_radians()).abs()
                < 1e-6,
            "the weight blends the pose, not the angle"
        );
    }

    #[test]
    fn the_rest_offset_shifts_the_open_pose() {
        let mut config = FingersConfig::new();
        config.rest_offset_deg[Finger::Ring.index()] = [10.0, -5.0, 0.0];
        let bend = hand_bend(&FingerCurls::default(), &config, 1.0, false);
        let ring = bend.bend[Finger::Ring.index()].map(f32::to_degrees);
        assert!((ring[0] - 10.0).abs() < 1e-4 && (ring[1] + 5.0).abs() < 1e-4);
        assert_eq!(bend.bend[Finger::Middle.index()], [0.0; 3]);
    }

    #[test]
    fn the_splay_is_clamped() {
        let config = FingersConfig::new();
        let curls = FingerCurls {
            curl: [0.0; 5],
            splay: [1.0; 5],
        };
        let bend = hand_bend(&curls, &config, 1.0, false);
        assert!(
            (bend.splay[Finger::Ring.index()] - config.max_splay_deg.to_radians()).abs() < 1e-6
        );
    }
}
