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

    /// Move `bone` and everything under it by `offset`.
    pub fn translate(&mut self, bone: RigBone, offset: Vec3) {
        for b in RigBone::ALL {
            if b.descends_from(bone) {
                self.bones[b.index()].translation += offset;
            }
        }
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

    /// Set `bone`'s model-space transform, carrying everything under it along.
    pub fn set(&mut self, bone: RigBone, transform: Transform) {
        let old = self.bones[bone.index()];
        let delta_rotation = transform.rotation * old.rotation.inverse();
        for b in RigBone::ALL {
            if b != bone && b.descends_from(bone) {
                let t = &mut self.bones[b.index()];
                t.translation =
                    transform.translation + delta_rotation * (t.translation - old.translation);
                t.rotation = (delta_rotation * t.rotation).normalize();
            }
        }
        self.bones[bone.index()] = transform;
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
    fn setting_a_bone_keeps_its_children_relative() {
        let mut p = pose();
        let before = p;
        let mut hips = *p.get(RigBone::Hips);
        hips.translation += Vec3::X;
        hips.rotation = Quat::from_rotation_y(1.0);
        p.set(RigBone::Hips, hips);
        let rel_before = before.position(RigBone::Head) - before.position(RigBone::Hips);
        let rel_after = p.position(RigBone::Head) - p.position(RigBone::Hips);
        assert!((Quat::from_rotation_y(1.0) * rel_before).abs_diff_eq(rel_after, 1e-5));
    }
}
