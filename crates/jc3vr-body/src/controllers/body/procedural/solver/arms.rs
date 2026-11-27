//! The arms against the procedural torso: the torso's keep-out for the hands and elbows, and the
//! elbow hints. Pure, in Rico's model space.
//!
//! The torso is two capsules along the spine: the chest from `Spine2` to the neck and the abdomen
//! from the hips to `Spine2`. A wrist or elbow target inside either, widened by a margin for the
//! hand's own thickness, is pushed straight out of it, so a hand held against the belly rests on
//! it rather than in it, and an arm folded across the body goes around it.
//!
//! An elbow hint is where an arm's elbow naturally sits for a wrist target: the middle joint of an
//! arm of two equal halves ([`middle_joint`]), bent along a pole that points down, out to the arm's
//! side, and a little back. The side and the back are the chest's, so a chest turned toward the
//! player's torso carries the elbows round with it. The pole is chosen from the reach: a hand
//! reaching up or across the body lifts the elbow out to the side, as a person's does, rather than
//! leaving it hanging under a raised arm. Where the reach runs along the pole (a hand reaching
//! down, out, and back toward a hip holster), the pole's part across the reach vanishes, so a
//! second pole, straight back, takes over smoothly as it does rather than at once.

use body_rig::math::{
    Side, geometry::closest_on_segment, smooth::smoothstep, two_bone::middle_joint,
};
use glam::{Mat4, Quat, Vec3};

use crate::controllers::body::procedural::{
    rig::RigBone, solver::pose::RigPose, tuning::PROCEDURAL,
};

/// A capsule: the points within `radius` of the segment from `a` to `b`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
}

/// The torso's keep-out: the chest and the abdomen.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TorsoKeepOut {
    pub capsules: [Capsule; 2],
    /// How far outside the capsules a pushed point lands, metres: the hand's half-thickness.
    pub margin: f32,
}

impl TorsoKeepOut {
    /// The keep-out around `pose`'s torso.
    pub fn around(pose: &RigPose, chest_radius: f32, abdomen_radius: f32, margin: f32) -> Self {
        Self {
            capsules: [
                Capsule {
                    a: pose.position(RigBone::Spine2),
                    b: pose.position(RigBone::Neck),
                    radius: chest_radius.max(0.0),
                },
                Capsule {
                    a: pose.position(RigBone::Hips),
                    b: pose.position(RigBone::Spine2),
                    radius: abdomen_radius.max(0.0),
                },
            ],
            margin: margin.max(0.0),
        }
    }

    /// This keep-out's capsules carried from Rico's model space into world space by `to_world`,
    /// the character's own world transform: the shared geometry other systems that work in world
    /// space, such as the physical hands' held-object keep-out, query it through. The transform is
    /// rigid (translation and rotation only), so the radii and the margin are carried unchanged.
    pub fn to_world(self, to_world: Mat4) -> Self {
        Self {
            capsules: self.capsules.map(|capsule| Capsule {
                a: to_world.transform_point3(capsule.a),
                b: to_world.transform_point3(capsule.b),
                radius: capsule.radius,
            }),
            margin: self.margin,
        }
    }

    /// `point` moved straight out of any capsule it is inside. A point on a capsule's axis, which
    /// has no outward direction of its own, goes forward.
    pub fn push_out(&self, point: Vec3) -> Vec3 {
        if !point.is_finite() {
            return point;
        }
        let mut point = point;
        // Pushing out of one capsule can push into the other where they meet; a second pass
        // settles it.
        for _ in 0..2 {
            for capsule in &self.capsules {
                let closest = closest_on_segment(point, capsule.a, capsule.b);
                let offset = point - closest;
                let clearance = capsule.radius + self.margin;
                if offset.length_squared() < clearance * clearance {
                    let out = offset.try_normalize().unwrap_or(Vec3::NEG_Z);
                    point = closest + out * clearance;
                }
            }
        }
        point
    }
}

/// The arm's bones on `side`: the clavicle (a rig bone), and the names of the upper arm, the
/// forearm, and the hand under it.
pub(crate) const fn arm_bones(side: Side) -> (RigBone, [&'static str; 3]) {
    match side {
        Side::Left => (
            RigBone::LeftShoulder,
            ["LeftArm", "LeftForeArm", "LeftHand"],
        ),
        Side::Right => (
            RigBone::RightShoulder,
            ["RightArm", "RightForeArm", "RightHand"],
        ),
    }
}

/// The elbow hint for an arm of `length` (upper arm and forearm together) from `shoulder` to the
/// wrist target `wrist`, on `side`, for a chest turned `chest_yaw` radians about +Y.
pub fn elbow_hint(
    shoulder: Vec3,
    wrist: Vec3,
    length: f32,
    side: Side,
    chest_yaw: f32,
) -> Option<Vec3> {
    let reach = wrist - shoulder;
    let distance = reach.length();
    if !(distance > 1e-4 && length > 1e-4 && reach.is_finite()) {
        return None;
    }
    let along = reach / distance;
    let out = Vec3::X * side.across();
    let chest = Quat::from_rotation_y(if chest_yaw.is_finite() {
        chest_yaw
    } else {
        0.0
    });
    let local = chest.inverse() * along;
    let tuning = &PROCEDURAL.get().elbow_hint;
    // Up, and across toward the other side, both lift the elbow out.
    let lift = (local.y.max(0.0) + (-local.dot(out)).max(0.0)).min(1.0);
    let pole = Vec3::NEG_Y * tuning.pole_down
        + chest
            * (out * (tuning.pole_out + tuning.pole_reach_out * lift) + Vec3::Z * tuning.pole_back);
    let across = |pole: Vec3| pole - along * pole.dot(along);
    let primary = across(pole.normalize_or_zero());
    let fallback = across(chest * Vec3::Z).normalize_or_zero();
    let keep = smoothstep(primary.length() / tuning.pole_fallback_below);
    let pole = (primary.normalize_or_zero() * keep + fallback * (1.0 - keep))
        .try_normalize()
        .unwrap_or(out);
    middle_joint(shoulder, wrist, length * 0.5, length * 0.5, pole)
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::body::procedural::rig::{Rig, test_support::skeleton};

    fn keep_out() -> TorsoKeepOut {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let pose = RigPose {
            bones: rig.reference,
        };
        TorsoKeepOut::around(&pose, 0.14, 0.13, 0.03)
    }

    #[test]
    fn a_hand_in_the_belly_rests_on_it_and_one_outside_is_left_alone() {
        let torso = keep_out();
        let inside = Vec3::new(0.02, 1.1, -0.05);
        let pushed = torso.push_out(inside);
        let distance = horizontal(pushed);
        assert!((distance - 0.16).abs() < 1e-4, "{pushed:?}");
        assert!(pushed.z < 0.0, "pushed backward: {pushed:?}");
        let outside = Vec3::new(0.0, 1.1, -0.4);
        assert_eq!(torso.push_out(outside), outside);
    }

    #[test]
    fn the_world_space_keep_out_pushes_out_the_same_point_wherever_it_is_carried() {
        let torso = keep_out();
        let to_world = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(1.0, 2.0, 3.0),
        );
        let world = torso.to_world(to_world);
        assert_eq!(world.margin, torso.margin);
        for capsule in world.capsules {
            assert!((capsule.b - capsule.a).length() > 0.0);
        }
        let point_model = Vec3::new(0.02, 1.1, -0.05);
        let pushed_model = torso.push_out(point_model);
        let point_world = to_world.transform_point3(point_model);
        let pushed_world = world.push_out(point_world);
        assert!(
            (pushed_world - to_world.transform_point3(pushed_model)).length() < 1e-4,
            "{pushed_world:?} vs {:?}",
            to_world.transform_point3(pushed_model)
        );
    }

    #[test]
    fn a_hand_on_the_spine_goes_forward() {
        let torso = keep_out();
        let pushed = torso.push_out(Vec3::new(0.0, 1.2, 0.0));
        assert!(pushed.z < -0.15);
    }

    #[test]
    fn the_elbow_hangs_down_and_out_and_keeps_the_arm_length() {
        let shoulder = Vec3::new(0.2, 1.45, 0.0);
        let wrist = Vec3::new(0.2, 1.3, -0.4);
        let hint = elbow_hint(shoulder, wrist, 0.6, Side::Right, 0.0).unwrap();
        assert!(hint.y < (shoulder.y + wrist.y) * 0.5);
        assert!(hint.x > 0.2);
        assert!(((hint - shoulder).length() - 0.3).abs() < 1e-3);
        assert!(((hint - wrist).length() - 0.3).abs() < 1e-3);
        let left = elbow_hint(
            shoulder * Vec3::new(-1.0, 1.0, 1.0),
            wrist * Vec3::new(-1.0, 1.0, 1.0),
            0.6,
            Side::Left,
            0.0,
        )
        .unwrap();
        assert!(left.x < -0.2);
        assert!(elbow_hint(shoulder, shoulder, 0.6, Side::Left, 0.0).is_none());
    }

    #[test]
    fn the_elbow_turns_with_the_chest() {
        let shoulder = Vec3::new(0.2, 1.45, 0.0);
        let wrist = Vec3::new(0.2, 1.0, -0.2);
        let square = elbow_hint(shoulder, wrist, 0.6, Side::Right, 0.0).unwrap();
        // The chest turned a quarter left (+Y): its right side faces -Z.
        let turned = elbow_hint(
            shoulder,
            wrist,
            0.6,
            Side::Right,
            std::f32::consts::FRAC_PI_2,
        )
        .unwrap();
        assert!(turned.z < square.z, "{square} -> {turned}");
    }

    #[test]
    fn a_hand_reaching_up_or_across_lifts_the_elbow_out() {
        let shoulder = Vec3::new(0.2, 1.45, 0.0);
        let down = elbow_hint(shoulder, Vec3::new(0.25, 1.0, -0.3), 0.6, Side::Right, 0.0).unwrap();
        let up = elbow_hint(shoulder, Vec3::new(0.25, 1.9, -0.25), 0.6, Side::Right, 0.0).unwrap();
        let across =
            elbow_hint(shoulder, Vec3::new(-0.2, 1.4, -0.35), 0.6, Side::Right, 0.0).unwrap();
        assert!(up.x - shoulder.x > down.x - shoulder.x, "{down} {up}");
        // Across the body, the elbow stands out to the arm's side of the reach's middle.
        let middle = (shoulder + Vec3::new(-0.2, 1.4, -0.35)) * 0.5;
        let out = (across - middle).normalize();
        assert!(out.x > 0.3, "{across} {out}");
    }

    #[test]
    fn the_elbow_moves_smoothly_through_a_reach_along_the_pole() {
        // Sweep the wrist through down, out, and back, where the reach runs along the pole.
        let shoulder = Vec3::new(0.2, 1.45, 0.0);
        let mut last: Option<Vec3> = None;
        for i in 0..=60 {
            let a = i as f32 / 60.0;
            let wrist = shoulder
                + Vec3::new(0.15 + 0.2 * a, -0.5 + 0.1 * a, 0.05 + 0.25 * a).normalize() * 0.5;
            let hint = elbow_hint(shoulder, wrist, 0.6, Side::Right, 0.0).unwrap();
            if let Some(last) = last {
                assert!(
                    (hint - last).length() < 0.03,
                    "a jump at {a}: {last} -> {hint}"
                );
            }
            last = Some(hint);
        }
    }

    /// The horizontal distance of a point from the test torso's axis, which is vertical at the
    /// origin.
    fn horizontal(point: Vec3) -> f32 {
        Vec3::new(point.x, 0.0, point.z).length()
    }
}
