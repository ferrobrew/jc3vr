//! The clavicles reaching with the hands. Pure, in Rico's model space.
//!
//! A hand raised above the shoulder, or reaching toward the end of the arm, takes the shoulder
//! with it: the clavicle lifts for a hand overhead, drops a little for a hand far down, and swings
//! forward for a hand reaching far ahead or across the body. Each turn is limited, and grows with
//! how far the hand is along the arm's length, so a hand held near the body leaves the shoulder
//! alone and a hand on top of the head lifts it only a little. The turn moves the shoulder joint
//! toward the hand, which leaves the arm less to reach, so a far reach does not stretch it.
//!
//! [`wanted`] measures the turn for a hand target, [`ClavicleFollower`] eases it over time, and
//! [`apply`] turns the clavicle (and the arm under it) about its own joint.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    rig::{Rig, RigBone},
    solver::{arms::Side, pose::RigPose},
};

/// A clavicle's turn from where the torso carries it, radians.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ClavicleTurn {
    /// Up toward the head, positive; down toward the hips, negative.
    pub elevation: f32,
    /// Forward, positive.
    pub protraction: f32,
}

impl ClavicleTurn {
    pub const ZERO: Self = Self {
        elevation: 0.0,
        protraction: 0.0,
    };

    fn is_finite(&self) -> bool {
        self.elevation.is_finite() && self.protraction.is_finite()
    }
}

/// The clavicles' limits and where their reach starts.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ClavicleTuning {
    /// The furthest a clavicle lifts, radians.
    pub elevation: f32,
    /// The furthest a clavicle drops, radians.
    pub depression: f32,
    /// The furthest a clavicle swings forward, radians.
    pub protraction: f32,
    /// The share of the arm's length at which a hand ahead or below starts to take the shoulder.
    pub reach_start: f32,
}

/// The turn the clavicle on `side` takes toward the wrist target `target`, scaled by `weight`
/// (the hand's own weight against the animation), in `pose`.
pub fn wanted(
    pose: &RigPose,
    rig: &Rig,
    side: Side,
    target: Vec3,
    weight: f32,
    tuning: &ClavicleTuning,
) -> ClavicleTurn {
    let arm = rig.arm(side);
    let shoulder = shoulder_joint(pose, rig, side);
    let reach = target - shoulder;
    let distance = reach.length();
    let weight = if weight.is_finite() {
        weight.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if !(distance > 1e-4 && arm.length > 1e-4 && reach.is_finite()) || weight <= 0.0 {
        return ClavicleTurn::ZERO;
    }
    let direction = reach / distance;
    let extension = distance / arm.length;
    let Some(forward) = chest_forward(pose) else {
        return ClavicleTurn::ZERO;
    };
    let outward = (shoulder - pose.get(side.arm_bones().0).translation)
        .with_y(0.0)
        .normalize_or_zero();

    let reach_start = tuning.reach_start.clamp(0.0, REACH_FULL - 0.01);
    let far = smoothstep(reach_start, REACH_FULL, extension);
    // A hand overhead lifts the shoulder by how straight the arm is, from about half its length.
    let lift = smoothstep(LIFT_START, 1.0, direction.y) * smoothstep(0.5, REACH_FULL, extension);
    let lower = smoothstep(LOWER_START, 1.0, -direction.y) * far;
    // Ahead, and across the body (half as much, since the chest is in the way).
    let ahead = (direction.dot(forward) - ACROSS_SHARE * direction.dot(outward)).clamp(0.0, 1.0);

    let turn = ClavicleTurn {
        elevation: (tuning.elevation.max(0.0) * lift - tuning.depression.max(0.0) * lower) * weight,
        protraction: tuning.protraction.max(0.0) * far * ahead * weight,
    };
    if turn.is_finite() {
        turn
    } else {
        ClavicleTurn::ZERO
    }
}

/// Turn the clavicle on `side` by `turn` about its own joint, carrying the arm under it.
pub fn apply(pose: &mut RigPose, rig: &Rig, side: Side, turn: ClavicleTurn) {
    if !turn.is_finite() || turn == ClavicleTurn::ZERO {
        return;
    }
    let (clavicle, _) = side.arm_bones();
    let pivot = pose.position(clavicle);
    let out = shoulder_joint(pose, rig, side) - pivot;
    let Some(forward) = chest_forward(pose) else {
        return;
    };
    // Turning about `out × up` carries `out` toward up; about `out × forward`, toward forward.
    let flat = out.with_y(0.0);
    let (Some(lift_axis), Some(swing_axis)) = (
        flat.cross(Vec3::Y).try_normalize(),
        flat.cross(forward).try_normalize(),
    ) else {
        return;
    };
    let rotation = Quat::from_axis_angle(swing_axis, turn.protraction)
        * Quat::from_axis_angle(lift_axis, turn.elevation);
    pose.rotate_about(clavicle, rotation, pivot);
}

/// Eases both clavicles' turns toward the wanted ones.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ClavicleFollower {
    turns: [ClavicleTurn; 2],
}

impl ClavicleFollower {
    pub const fn new() -> Self {
        Self {
            turns: [ClavicleTurn::ZERO; 2],
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Move toward `wanted` (left then right) over `dt` seconds with the time constant `time_s`,
    /// and return the turns.
    pub fn step(&mut self, wanted: [ClavicleTurn; 2], dt: f32, time_s: f32) -> [ClavicleTurn; 2] {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let k = if time_s > 0.0 {
            1.0 - (-dt / time_s).exp()
        } else {
            1.0
        };
        for (turn, wanted) in self.turns.iter_mut().zip(wanted) {
            let wanted = if wanted.is_finite() {
                wanted
            } else {
                ClavicleTurn::ZERO
            };
            turn.elevation += (wanted.elevation - turn.elevation) * k;
            turn.protraction += (wanted.protraction - turn.protraction) * k;
        }
        self.turns
    }
}

/// Where the shoulder joint on `side` is in `pose`: carried by its clavicle.
pub fn shoulder_joint(pose: &RigPose, rig: &Rig, side: Side) -> Vec3 {
    let clavicle = pose.get(side.arm_bones().0);
    clavicle.translation + clavicle.rotation * rig.arm(side).shoulder_local
}

/// The extension (the hand's distance from the shoulder over the arm's length) at which a reach
/// takes the clavicle fully.
const REACH_FULL: f32 = 1.0;

/// How steeply up the hand must be from the shoulder before it lifts it: the sine of the angle
/// above the horizontal.
const LIFT_START: f32 = 0.25;

/// How steeply down the hand must be from the shoulder before it lowers it.
const LOWER_START: f32 = 0.5;

/// How much a reach across the body counts against a reach ahead.
const ACROSS_SHARE: f32 = 0.5;

/// The chest's horizontal forward: square to the line between the clavicles.
fn chest_forward(pose: &RigPose) -> Option<Vec3> {
    let line =
        (pose.position(RigBone::RightShoulder) - pose.position(RigBone::LeftShoulder)).with_y(0.0);
    Vec3::Y.cross(line).try_normalize()
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::rig::tests::skeleton;

    fn setup() -> (Rig, RigPose, ClavicleTuning) {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let pose = RigPose {
            bones: rig.reference,
        };
        let tuning = ClavicleTuning {
            elevation: 30f32.to_radians(),
            depression: 10f32.to_radians(),
            protraction: 25f32.to_radians(),
            reach_start: 0.8,
        };
        (rig, pose, tuning)
    }

    #[test]
    fn a_hand_near_the_body_leaves_the_shoulder_alone() {
        let (rig, pose, tuning) = setup();
        let shoulder = shoulder_joint(&pose, &rig, Side::Right);
        let turn = wanted(
            &pose,
            &rig,
            Side::Right,
            shoulder + Vec3::new(0.0, -0.1, -0.25),
            1.0,
            &tuning,
        );
        assert_eq!(turn, ClavicleTurn::ZERO);
    }

    #[test]
    fn a_hand_overhead_lifts_the_shoulder_within_its_limit() {
        let (rig, mut pose, tuning) = setup();
        let shoulder = shoulder_joint(&pose, &rig, Side::Left);
        let target = shoulder + Vec3::Y * 0.6;
        let turn = wanted(&pose, &rig, Side::Left, target, 1.0, &tuning);
        assert!((turn.elevation - tuning.elevation).abs() < 1e-4, "{turn:?}");
        let before = target.distance(shoulder);
        apply(&mut pose, &rig, Side::Left, turn);
        let lifted = shoulder_joint(&pose, &rig, Side::Left);
        assert!(lifted.y > shoulder.y + 0.05, "{lifted:?}");
        assert!(target.distance(lifted) < before);
        // The other side and the rest of the torso stay.
        assert_eq!(
            pose.get(RigBone::RightShoulder),
            &rig.reference[RigBone::RightShoulder.index()]
        );
        assert_eq!(
            pose.get(RigBone::Neck),
            &rig.reference[RigBone::Neck.index()]
        );
    }

    #[test]
    fn a_far_reach_ahead_swings_the_shoulder_forward() {
        let (rig, mut pose, tuning) = setup();
        let shoulder = shoulder_joint(&pose, &rig, Side::Right);
        let target = shoulder + Vec3::NEG_Z * 0.62;
        let turn = wanted(&pose, &rig, Side::Right, target, 1.0, &tuning);
        assert!(turn.protraction > tuning.protraction * 0.9, "{turn:?}");
        assert!(turn.elevation.abs() < 1e-4);
        apply(&mut pose, &rig, Side::Right, turn);
        let moved = shoulder_joint(&pose, &rig, Side::Right);
        assert!(moved.z < shoulder.z - 0.03, "{moved:?}");
        // The left clavicle swings forward too, about the mirrored axis.
        let (rig, mut pose, _) = setup();
        let left = shoulder_joint(&pose, &rig, Side::Left);
        let turn = wanted(
            &pose,
            &rig,
            Side::Left,
            left + Vec3::NEG_Z * 0.62,
            1.0,
            &tuning,
        );
        apply(&mut pose, &rig, Side::Left, turn);
        assert!(shoulder_joint(&pose, &rig, Side::Left).z < left.z - 0.03);
    }

    #[test]
    fn the_weight_scales_the_turn_and_the_follower_eases_toward_it() {
        let (rig, pose, tuning) = setup();
        let shoulder = shoulder_joint(&pose, &rig, Side::Left);
        let target = shoulder + Vec3::Y * 0.6;
        let full = wanted(&pose, &rig, Side::Left, target, 1.0, &tuning);
        let half = wanted(&pose, &rig, Side::Left, target, 0.5, &tuning);
        assert!((half.elevation - full.elevation * 0.5).abs() < 1e-5);
        let mut follower = ClavicleFollower::new();
        let first = follower.step([full, ClavicleTurn::ZERO], 1.0 / 60.0, 0.1)[0];
        assert!(first.elevation > 0.0 && first.elevation < full.elevation);
        let mut last = first;
        for _ in 0..120 {
            last = follower.step([full, ClavicleTurn::ZERO], 1.0 / 60.0, 0.1)[0];
        }
        assert!((last.elevation - full.elevation).abs() < 1e-4);
        let nan = ClavicleTurn {
            elevation: f32::NAN,
            protraction: 0.0,
        };
        assert!(follower.step([nan; 2], 1.0 / 60.0, 0.1)[0].is_finite());
    }
}
