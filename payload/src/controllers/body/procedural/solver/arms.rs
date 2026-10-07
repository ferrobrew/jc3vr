//! The arms against the procedural torso: the torso's keep-out for the hands and elbows, and the
//! elbow hints. Pure, in Rico's model space.
//!
//! The torso is two capsules along the spine: the chest from `Spine2` to the neck and the abdomen
//! from the hips to `Spine2`. A wrist or elbow target inside either, widened by a margin for the
//! hand's own thickness, is pushed straight out of it, so a hand held against the belly rests on
//! it rather than in it, and an arm folded across the body goes around it.
//!
//! An elbow hint is where an arm's elbow naturally sits for a wrist target: at the middle of the
//! shoulder-to-wrist line, out along a pole that points down, out to the arm's side, and a little
//! back, as far as the arm's length bends it.

use glam::{Mat4, Vec3};

use crate::controllers::body::procedural::{rig::RigBone, solver::pose::RigPose};

/// A capsule: the points within `radius` of the segment from `a` to `b`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
}

impl Capsule {
    /// The closest point of the capsule's segment to `point`.
    fn closest(&self, point: Vec3) -> Vec3 {
        let axis = self.b - self.a;
        let length = axis.length_squared();
        if length < 1e-10 {
            return self.a;
        }
        let t = ((point - self.a).dot(axis) / length).clamp(0.0, 1.0);
        self.a + axis * t
    }
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
                let closest = capsule.closest(point);
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

/// Which side of the body an arm is on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// The arm's bones on this side: the clavicle (a rig bone), and the names of the upper arm, the
    /// forearm, and the hand under it.
    pub const fn arm_bones(self) -> (RigBone, [&'static str; 3]) {
        match self {
            Self::Left => (
                RigBone::LeftShoulder,
                ["LeftArm", "LeftForeArm", "LeftHand"],
            ),
            Self::Right => (
                RigBone::RightShoulder,
                ["RightArm", "RightForeArm", "RightHand"],
            ),
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }

    pub const BOTH: [Self; 2] = [Self::Left, Self::Right];
}

/// The elbow hint for an arm of `length` (upper arm and forearm together) from `shoulder` to the
/// wrist target `wrist`, on `side`.
pub fn elbow_hint(shoulder: Vec3, wrist: Vec3, length: f32, side: Side) -> Option<Vec3> {
    let reach = wrist - shoulder;
    let distance = reach.length();
    if !(distance > 1e-4 && length > 1e-4 && reach.is_finite()) {
        return None;
    }
    let along = reach / distance;
    let out = match side {
        Side::Left => Vec3::NEG_X,
        Side::Right => Vec3::X,
    };
    let pole = Vec3::NEG_Y * POLE_DOWN + out * POLE_OUT + Vec3::Z * POLE_BACK;
    let pole = (pole - along * pole.dot(along))
        .try_normalize()
        .unwrap_or(out);
    let half = length * 0.5;
    let bend = (half * half - (distance * 0.5).powi(2)).max(0.0).sqrt();
    Some(shoulder + reach * 0.5 + pole * bend)
}

/// The elbow pole's lean: mostly down, partly out to the arm's side, and a little back.
const POLE_DOWN: f32 = 1.0;
const POLE_OUT: f32 = 0.5;
const POLE_BACK: f32 = 0.3;

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::body::procedural::rig::{Rig, tests::skeleton};

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
        let hint = elbow_hint(shoulder, wrist, 0.6, Side::Right).unwrap();
        assert!(hint.y < (shoulder.y + wrist.y) * 0.5);
        assert!(hint.x > 0.2);
        assert!(((hint - shoulder).length() - 0.3).abs() < 1e-3);
        assert!(((hint - wrist).length() - 0.3).abs() < 1e-3);
        let left = elbow_hint(
            shoulder * Vec3::new(-1.0, 1.0, 1.0),
            wrist * Vec3::new(-1.0, 1.0, 1.0),
            0.6,
            Side::Left,
        )
        .unwrap();
        assert!(left.x < -0.2);
        assert!(elbow_hint(shoulder, shoulder, 0.6, Side::Left).is_none());
    }

    /// The horizontal distance of a point from the test torso's axis, which is vertical at the
    /// origin.
    fn horizontal(point: Vec3) -> f32 {
        Vec3::new(point.x, 0.0, point.z).length()
    }
}
