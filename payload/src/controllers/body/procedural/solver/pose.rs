//! The rig's bones in model space, moved the way `AnimationController::SetJoint` moves them: a
//! change to a bone carries every bone under it along, keeping their transforms relative to it.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{rig::RigBone, skeleton::Transform};

/// The rig's model-space transforms, indexed by [`RigBone::index`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigPose {
    pub bones: [Transform; RigBone::COUNT],
}

impl RigPose {
    pub fn get(&self, bone: RigBone) -> &Transform {
        &self.bones[bone.index()]
    }

    pub fn position(&self, bone: RigBone) -> Vec3 {
        self.bones[bone.index()].translation
    }

    /// Turn `bone` and everything under it by `rotation` about `pivot`.
    pub fn rotate_about(&mut self, bone: RigBone, rotation: Quat, pivot: Vec3) {
        for b in RigBone::ALL {
            if b.descends_from(bone) {
                let t = &mut self.bones[b.index()];
                t.translation = pivot + rotation * (t.translation - pivot);
                t.rotation = (rotation * t.rotation).normalize();
            }
        }
    }

    /// `self` mixed toward `other` by `weight`, bone by bone.
    pub fn blend(&self, other: &Self, weight: f32) -> Self {
        let w = weight.clamp(0.0, 1.0);
        let mut out = *self;
        for (o, (a, b)) in out
            .bones
            .iter_mut()
            .zip(self.bones.iter().zip(&other.bones))
        {
            o.translation = a.translation.lerp(b.translation, w);
            o.rotation = a.rotation.slerp(b.rotation, w).normalize();
            o.scale = a.scale.lerp(b.scale, w);
        }
        out
    }

    /// `self` (the animated pose) mixed toward `solved` by `torso` above the hips: the pelvis and
    /// the legs stay the animation's, which keeps the hips in every solve.
    pub fn blend_torso(&self, solved: &Self, torso: f32) -> Self {
        let mut out = self.blend(solved, torso);
        for bone in RigBone::ALL {
            if bone == RigBone::Hips || bone.descends_from(RigBone::Pelvis) {
                out.bones[bone.index()] = self.bones[bone.index()];
            }
        }
        out
    }

    pub fn is_finite(&self) -> bool {
        self.bones
            .iter()
            .all(|t| t.translation.is_finite() && t.rotation.is_finite() && t.scale.is_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose() -> RigPose {
        let mut bones = [Transform {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }; RigBone::COUNT];
        for (i, b) in bones.iter_mut().enumerate() {
            b.translation = Vec3::new(0.0, i as f32 * 0.1, 0.0);
        }
        RigPose { bones }
    }

    #[test]
    fn a_turn_carries_the_children_and_spares_the_rest() {
        let mut p = pose();
        let before = p;
        let pivot = p.position(RigBone::Spine);
        p.rotate_about(RigBone::Spine, Quat::from_rotation_z(0.5), pivot);
        assert_eq!(p.get(RigBone::Pelvis), before.get(RigBone::Pelvis));
        assert_eq!(p.position(RigBone::Spine), before.position(RigBone::Spine));
        let arm = before.position(RigBone::Head) - pivot;
        let moved = p.position(RigBone::Head) - pivot;
        assert!((arm.length() - moved.length()).abs() < 1e-5);
        assert!(moved.angle_between(arm) > 0.49);
    }

    #[test]
    fn the_torso_blend_leaves_the_hips_and_the_legs_to_the_animation() {
        let animated = pose();
        let mut solved = pose();
        let pivot = solved.position(RigBone::Spine);
        solved.rotate_about(RigBone::Spine, Quat::from_rotation_x(0.4), pivot);
        solved.rotate_about(
            RigBone::LeftUpLeg,
            Quat::from_rotation_x(0.4),
            solved.position(RigBone::LeftUpLeg),
        );
        let full = animated.blend_torso(&solved, 1.0);
        assert!(
            full.position(RigBone::Head)
                .abs_diff_eq(solved.position(RigBone::Head), 1e-5)
        );
        for bone in [
            RigBone::Hips,
            RigBone::Pelvis,
            RigBone::LeftUpLeg,
            RigBone::LeftFoot,
        ] {
            assert_eq!(full.get(bone), animated.get(bone));
        }
        let none = animated.blend_torso(&solved, 0.0);
        for bone in RigBone::ALL {
            assert!(
                none.position(bone)
                    .abs_diff_eq(animated.position(bone), 1e-5)
            );
        }
    }
}
