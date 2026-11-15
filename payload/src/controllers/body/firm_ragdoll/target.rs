//! The firm ragdoll's muscle target: the pose its keyframed upper body is held to and its motors
//! drive the legs toward, composed from the last tick's final pose and the IK body's legs. Pure.
//!
//! The ragdoll's pre-physics step maps a pose of the animated skeleton onto the ragdoll's and hands
//! it to its controllers: the keyframed bodies are moved to it outright, the motor-driven ones are
//! driven toward its relative rotations. By default that pose is the raw animation, before the
//! mod's HumanIK solve, so the keyframed upper body and the legs chase the clip rather than the
//! drawn body. The target composed here instead keeps every bone above the hips where the last
//! tick's final pose drew it (the solve included), and takes the hips' relation to the spine and
//! the legs' rotations from sources that do not contain the physics' own last answer: the last
//! final pose already holds the blended physics hips and legs, and motors driven toward it would
//! chase themselves and lose their restoring pull. The hips hang from the drawn spine as the clip
//! relates them, and each leg is the clip's leg aimed along the IK body's thigh and shin, its knee
//! turned into the IK leg's plane, or the clip's leg where the IK body gives none.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    rig::{Rig, RigBone},
    skeleton::Transform,
};

/// The bones the target composes, as indices into the animated skeleton.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TargetBones {
    pub hips: usize,
    pub pelvis: usize,
    /// The first spine bone, the hips' child the upper body hangs from.
    pub spine: usize,
    /// Each leg's upper leg, lower leg, and foot, left then right.
    pub legs: [[usize; 3]; 2],
}

impl TargetBones {
    pub(crate) fn of(rig: &Rig) -> Self {
        let at = |bone| rig.skeleton_index(bone);
        Self {
            hips: at(RigBone::Hips),
            pelvis: at(RigBone::Pelvis),
            spine: at(RigBone::Spine),
            legs: [
                [
                    at(RigBone::LeftUpLeg),
                    at(RigBone::LeftLeg),
                    at(RigBone::LeftFoot),
                ],
                [
                    at(RigBone::RightUpLeg),
                    at(RigBone::RightLeg),
                    at(RigBone::RightFoot),
                ],
            ],
        }
    }

    fn all(&self) -> impl Iterator<Item = usize> + '_ {
        [self.hips, self.pelvis, self.spine]
            .into_iter()
            .chain(self.legs.iter().flatten().copied())
    }
}

/// The IK body's legs, Rico's model space: each hip, knee, and ankle, left then right. Only the
/// thighs' and the shins' directions are read.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct IkLegs {
    pub hips: [Vec3; 2],
    pub knees: [Vec3; 2],
    pub ankles: [Vec3; 2],
}

/// What the target is composed from.
#[derive(Debug, Copy, Clone)]
pub(crate) struct TargetInput<'a> {
    /// Each bone's parent index, `-1` for a root; parents before children.
    pub parents: &'a [i16],
    /// The last tick's final pose, local transforms.
    pub last_final: &'a [Transform],
    /// This tick's animated pose, local transforms.
    pub animated: &'a [Transform],
    pub bones: TargetBones,
    pub legs: Option<IkLegs>,
}

/// The muscle target, local transforms, or `None` for poses that do not match the skeleton or are
/// not finite.
pub(crate) fn muscle_target(input: &TargetInput<'_>) -> Option<Vec<Transform>> {
    let count = input.parents.len();
    if input.last_final.len() != count
        || input.animated.len() != count
        || input.bones.all().any(|bone| bone >= count)
        || !parents_first(input.parents)
    {
        return None;
    }
    let bones = input.bones;
    let drawn = model_space(input.parents, input.last_final);
    let mut local = input.last_final.to_vec();
    let lower = |bone: usize| {
        descends(input.parents, bone, bones.hips) && !descends(input.parents, bone, bones.spine)
    };
    for (bone, (out, animated)) in local.iter_mut().zip(input.animated).enumerate() {
        if lower(bone) && bone != bones.hips {
            *out = *animated;
        }
    }
    // The hips hang from the drawn spine as the clip relates the two.
    let hips_model = drawn[bones.spine].compose(&inverse(&input.animated[bones.spine]));
    let parent_model = match parent(input.parents, bones.hips) {
        Some(p) => drawn[p],
        None => IDENTITY,
    };
    local[bones.hips] = relative(&parent_model, &hips_model);
    local[bones.spine] = input.animated[bones.spine];
    if let Some(legs) = input.legs {
        let mut model = model_space(input.parents, &local);
        for (side, chain) in bones.legs.iter().enumerate() {
            aim_leg(input.parents, &mut model, *chain, &legs, side);
            for bone in [chain[0], chain[1]] {
                let p = parent(input.parents, bone).map_or(IDENTITY, |p| model[p]);
                local[bone] = relative(&p, &model[bone]);
            }
        }
    }
    local.iter().all(is_finite).then_some(local)
}

/// The model-space transforms of `local` on the skeleton `parents`.
pub(crate) fn model_space(parents: &[i16], local: &[Transform]) -> Vec<Transform> {
    let mut model: Vec<Transform> = Vec::with_capacity(local.len());
    for (bone, transform) in local.iter().enumerate() {
        let composed = match parent(parents, bone) {
            Some(p) if p < bone => model[p].compose(transform),
            _ => *transform,
        };
        model.push(composed);
    }
    model
}

/// The local transforms of the model-space pose `model` on the skeleton `parents`.
pub(crate) fn local_space(parents: &[i16], model: &[Transform]) -> Vec<Transform> {
    model
        .iter()
        .enumerate()
        .map(|(bone, transform)| match parent(parents, bone) {
            Some(p) if p < bone => relative(&model[p], transform),
            _ => *transform,
        })
        .collect()
}

const IDENTITY: Transform = Transform {
    translation: Vec3::ZERO,
    rotation: Quat::IDENTITY,
    scale: Vec3::ONE,
};

/// The parent of `bone`, if it has one.
fn parent(parents: &[i16], bone: usize) -> Option<usize> {
    usize::try_from(*parents.get(bone)?).ok()
}

/// Whether every bone's parent comes before it.
fn parents_first(parents: &[i16]) -> bool {
    parents
        .iter()
        .enumerate()
        .all(|(bone, p)| *p < 0 || (*p as usize) < bone)
}

/// Whether `bone` is `ancestor` or hangs from it.
fn descends(parents: &[i16], bone: usize, ancestor: usize) -> bool {
    let mut at = Some(bone);
    while let Some(b) = at {
        if b == ancestor {
            return true;
        }
        at = parent(parents, b);
    }
    false
}

/// The inverse of `t`, as Havok inverts a `hkQsTransform`.
fn inverse(t: &Transform) -> Transform {
    let rotation = t.rotation.inverse();
    let scale = t.scale.recip();
    Transform {
        translation: rotation * (-t.translation) * scale,
        rotation,
        scale,
    }
}

/// `model` relative to `parent`: the local transform that composes onto `parent` to give it.
fn relative(parent: &Transform, model: &Transform) -> Transform {
    let rotation = parent.rotation.inverse();
    Transform {
        translation: (rotation * (model.translation - parent.translation)) / parent.scale,
        rotation: (rotation * model.rotation).normalize(),
        scale: model.scale / parent.scale,
    }
}

/// Aim the leg `chain` of `model` along the IK body's leg on `side`: the thigh along its thigh, the
/// knee's bend into its leg's plane, and the shin along its shin, each turn carrying the bones
/// under it.
fn aim_leg(
    parents: &[i16],
    model: &mut [Transform],
    chain: [usize; 3],
    legs: &IkLegs,
    side: usize,
) {
    let [up, knee, foot] = chain;
    let (Some(thigh_dir), Some(shin_dir)) = (
        (legs.knees[side] - legs.hips[side]).try_normalize(),
        (legs.ankles[side] - legs.knees[side]).try_normalize(),
    ) else {
        return;
    };
    let thigh = model[knee].translation - model[up].translation;
    let Some(thigh_now) = thigh.try_normalize() else {
        return;
    };
    let pivot = model[up].translation;
    turn_subtree(
        parents,
        model,
        up,
        Quat::from_rotation_arc(thigh_now, thigh_dir),
        pivot,
    );
    // Turn the thigh about itself so the knee bends in the IK leg's plane.
    let shin = model[foot].translation - model[knee].translation;
    let bend_now = thigh_dir.cross(shin);
    let bend_ik = thigh_dir.cross(shin_dir);
    if bend_now.length() > BENT && bend_ik.length() > BENT {
        let (a, b) = (bend_now.normalize(), bend_ik.normalize());
        let angle = a.cross(b).dot(thigh_dir).atan2(a.dot(b));
        turn_subtree(
            parents,
            model,
            up,
            Quat::from_axis_angle(thigh_dir, angle),
            pivot,
        );
    }
    let shin = model[foot].translation - model[knee].translation;
    if let Some(shin_now) = shin.try_normalize() {
        let pivot = model[knee].translation;
        turn_subtree(
            parents,
            model,
            knee,
            Quat::from_rotation_arc(shin_now, shin_dir),
            pivot,
        );
    }
}

/// How far from straight a leg must bend, as the sine of its knee's angle, before its plane counts.
const BENT: f32 = 0.05;

/// Turn `bone` and every bone under it by `rotation` about `pivot`.
fn turn_subtree(
    parents: &[i16],
    model: &mut [Transform],
    bone: usize,
    rotation: Quat,
    pivot: Vec3,
) {
    for (b, t) in model.iter_mut().enumerate().skip(bone) {
        if descends(parents, b, bone) {
            t.translation = pivot + rotation * (t.translation - pivot);
            t.rotation = (rotation * t.rotation).normalize();
        }
    }
}

fn is_finite(t: &Transform) -> bool {
    t.translation.is_finite() && t.rotation.is_finite() && t.scale.is_finite()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small skeleton: a root, the hips, a spine with a chest on it, and a pelvis with two
    /// three-bone legs.
    pub(crate) const PARENTS: [i16; 11] = [-1, 0, 1, 2, 1, 4, 5, 6, 4, 8, 9];
    pub(crate) const BONES: TargetBones = TargetBones {
        hips: 1,
        pelvis: 4,
        spine: 2,
        legs: [[5, 6, 7], [8, 9, 10]],
    };

    fn at(translation: Vec3, rotation: Quat) -> Transform {
        Transform {
            translation,
            rotation,
            scale: Vec3::ONE,
        }
    }

    /// A standing pose, local transforms: the legs straight down, turned by `lean` at the hips.
    pub(crate) fn standing(lean: Quat) -> Vec<Transform> {
        vec![
            at(Vec3::ZERO, Quat::IDENTITY),
            at(Vec3::new(0.0, 1.0, 0.0), lean),
            at(Vec3::new(0.0, 0.1, 0.0), Quat::IDENTITY),
            at(Vec3::new(0.0, 0.4, 0.0), Quat::IDENTITY),
            at(Vec3::new(0.0, -0.05, 0.0), Quat::IDENTITY),
            at(Vec3::new(-0.1, 0.0, 0.0), Quat::IDENTITY),
            at(Vec3::new(0.0, -0.45, 0.0), Quat::IDENTITY),
            at(Vec3::new(0.0, -0.45, 0.0), Quat::IDENTITY),
            at(Vec3::new(0.1, 0.0, 0.0), Quat::IDENTITY),
            at(Vec3::new(0.0, -0.45, 0.0), Quat::IDENTITY),
            at(Vec3::new(0.0, -0.45, 0.0), Quat::IDENTITY),
        ]
    }

    #[test]
    fn the_upper_body_stays_where_it_was_drawn_and_the_hips_and_legs_are_the_clips() {
        // The drawn pose leans 30 degrees and its legs swing (the physics' answer); the clip stands.
        let mut drawn = standing(Quat::from_rotation_x(0.5));
        drawn[5].rotation = Quat::from_rotation_x(0.8);
        let animated = standing(Quat::IDENTITY);
        let target = muscle_target(&TargetInput {
            parents: &PARENTS,
            last_final: &drawn,
            animated: &animated,
            bones: BONES,
            legs: None,
        })
        .unwrap();
        let before = model_space(&PARENTS, &drawn);
        let after = model_space(&PARENTS, &target);
        for bone in [2, 3] {
            assert!(
                after[bone]
                    .translation
                    .abs_diff_eq(before[bone].translation, 1e-5)
            );
            assert!(
                after[bone]
                    .rotation
                    .abs_diff_eq(before[bone].rotation, 1e-5)
            );
        }
        // The legs' local rotations are the clip's, not the drawn swing.
        for bone in 4..11 {
            assert_eq!(
                target[bone].rotation, animated[bone].rotation,
                "bone {bone}"
            );
        }
        // The hips hang from the drawn spine as the clip relates them.
        assert!(
            after[1]
                .rotation
                .abs_diff_eq(before[2].rotation * animated[2].rotation.inverse(), 1e-5)
        );
    }

    #[test]
    fn the_legs_aim_along_the_ik_legs_and_keep_their_lengths() {
        let pose = standing(Quat::IDENTITY);
        // The IK legs: the left knee forward and up, the shin back down; the right hanging.
        let legs = IkLegs {
            hips: [Vec3::new(-0.1, 0.95, 0.0), Vec3::new(0.1, 0.95, 0.0)],
            knees: [Vec3::new(-0.1, 0.6, -0.3), Vec3::new(0.1, 0.5, 0.0)],
            ankles: [Vec3::new(-0.1, 0.2, -0.15), Vec3::new(0.1, 0.05, 0.0)],
        };
        let target = muscle_target(&TargetInput {
            parents: &PARENTS,
            last_final: &pose,
            animated: &pose,
            bones: BONES,
            legs: Some(legs),
        })
        .unwrap();
        let model = model_space(&PARENTS, &target);
        let thigh = model[6].translation - model[5].translation;
        let shin = model[7].translation - model[6].translation;
        assert!((thigh.length() - 0.45).abs() < 1e-4);
        assert!((shin.length() - 0.45).abs() < 1e-4);
        let want_thigh = (legs.knees[0] - legs.hips[0]).normalize();
        let want_shin = (legs.ankles[0] - legs.knees[0]).normalize();
        assert!(thigh.normalize().abs_diff_eq(want_thigh, 1e-4), "{thigh}");
        assert!(shin.normalize().abs_diff_eq(want_shin, 1e-4), "{shin}");
        // The straight right leg hangs straight down, as before.
        let right = model[10].translation - model[8].translation;
        assert!(right.normalize().abs_diff_eq(Vec3::NEG_Y, 1e-4));
    }

    #[test]
    fn a_bent_clip_leg_turns_its_knee_into_the_ik_legs_plane() {
        let mut pose = standing(Quat::IDENTITY);
        // The clip's left knee bends out to the side.
        pose[6].rotation = Quat::from_rotation_z(0.6);
        let legs = IkLegs {
            hips: [Vec3::new(-0.1, 0.95, 0.0), Vec3::new(0.1, 0.95, 0.0)],
            knees: [Vec3::new(-0.1, 0.55, -0.2), Vec3::new(0.1, 0.5, 0.0)],
            ankles: [Vec3::new(-0.1, 0.15, 0.0), Vec3::new(0.1, 0.05, 0.0)],
        };
        let target = muscle_target(&TargetInput {
            parents: &PARENTS,
            last_final: &pose,
            animated: &pose,
            bones: BONES,
            legs: Some(legs),
        })
        .unwrap();
        let model = model_space(&PARENTS, &target);
        // The knee bends forward in the sagittal plane: the ankle stays at the hip's x.
        assert!((model[7].translation.x - model[5].translation.x).abs() < 1e-3);
        assert!(model[6].translation.z < model[5].translation.z - 0.1);
    }

    #[test]
    fn a_pose_that_does_not_fit_the_skeleton_is_refused() {
        let pose = standing(Quat::IDENTITY);
        let short = &pose[..5];
        assert!(
            muscle_target(&TargetInput {
                parents: &PARENTS,
                last_final: short,
                animated: &pose,
                bones: BONES,
                legs: None,
            })
            .is_none()
        );
        let out_of_order = [-1, 2, 0, 2, 1, 4, 5, 6, 4, 8, 9];
        assert!(
            muscle_target(&TargetInput {
                parents: &out_of_order,
                last_final: &pose,
                animated: &pose,
                bones: BONES,
                legs: None,
            })
            .is_none()
        );
    }
}
