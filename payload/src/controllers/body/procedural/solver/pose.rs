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

    /// `self` (the animated pose) mixed toward `solved` region by region: the torso by `torso`,
    /// and the legs by `legs`, each leg pose carried by the mixed hips so the legs stay on the
    /// hips whatever the two weights. With the legs on the animation and the torso procedural,
    /// the animated legs hang from the procedural hips.
    pub fn blend_regions(&self, solved: &Self, legs: f32, torso: f32) -> Self {
        let mut out = self.blend(solved, torso);
        let hips = *out.get(RigBone::Hips);
        let animated = self.carried_legs(&hips);
        let procedural = solved.carried_legs(&hips);
        let w = legs.clamp(0.0, 1.0);
        for bone in RigBone::ALL {
            if bone.descends_from(RigBone::Pelvis) {
                let (a, b) = (animated.get(bone), procedural.get(bone));
                out.bones[bone.index()] = Transform {
                    translation: a.translation.lerp(b.translation, w),
                    rotation: a.rotation.slerp(b.rotation, w).normalize(),
                    scale: a.scale.lerp(b.scale, w),
                };
            }
        }
        out
    }

    /// The legs moved rigidly from this pose's hips to `hips`.
    fn carried_legs(&self, hips: &Transform) -> Self {
        let from = self.get(RigBone::Hips);
        let turn = hips.rotation * from.rotation.inverse();
        let mut out = *self;
        for bone in RigBone::ALL {
            if bone.descends_from(RigBone::Pelvis) {
                let t = &mut out.bones[bone.index()];
                t.translation = hips.translation + turn * (t.translation - from.translation);
                t.rotation = (turn * t.rotation).normalize();
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

/// Which of the two solves posed the body: the body built under the player's head, or the torso on
/// the animation's hips.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SolveKind {
    UnderHead,
    OnAnimatedHips,
}

/// The fade over a switch between the two solves while the body stays procedural (a walk into the
/// water, a jump into the parachute): the solves place the hips differently, so the written pose
/// would jump. The last pose written before the switch is held and faded out over the blend time,
/// in model space, as the region blend fades the animation.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SolveSwitch {
    last: Option<(SolveKind, RigPose)>,
    from: Option<RigPose>,
    progress: f32,
}

impl SolveSwitch {
    pub const fn new() -> Self {
        Self {
            last: None,
            from: None,
            progress: 0.0,
        }
    }

    /// The pose to write for `pose`, solved by `kind`, `dt` seconds after the last: the pose itself,
    /// or, within `time_s` of a switch of solve, the last pose before it faded into it, eased.
    pub fn step(&mut self, kind: SolveKind, pose: RigPose, dt: f32, time_s: f32) -> RigPose {
        if let Some((last_kind, last_pose)) = self.last
            && last_kind != kind
        {
            self.from = Some(last_pose);
            self.progress = 0.0;
        }
        let out = match self.from {
            Some(from) if time_s > 0.0 => {
                let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
                self.progress = (self.progress + dt / time_s).min(1.0);
                let t = self.progress;
                let eased = t * t * (3.0 - 2.0 * t);
                if t >= 1.0 {
                    self.from = None;
                }
                from.blend(&pose, eased)
            }
            _ => {
                self.from = None;
                pose
            }
        };
        self.last = Some((kind, out));
        out
    }

    /// Whether a switch is still fading.
    pub fn fading(&self) -> bool {
        self.from.is_some()
    }

    /// Forget the last pose: the body went back to the animation, whose blend covers the return.
    pub fn reset(&mut self) {
        *self = Self::new();
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
    fn region_blends_keep_the_legs_on_the_hips() {
        let animated = pose();
        let mut solved = pose();
        solved.translate(RigBone::Hips, Vec3::new(0.0, -0.2, 0.1));
        solved.rotate_about(
            RigBone::LeftUpLeg,
            Quat::from_rotation_x(0.4),
            solved.position(RigBone::LeftUpLeg),
        );
        // Both procedural: the solved pose.
        let full = animated.blend_regions(&solved, 1.0, 1.0);
        for bone in RigBone::ALL {
            assert!(full.position(bone).abs_diff_eq(solved.position(bone), 1e-5));
        }
        // Legs animated under procedural hips: the animated legs, carried by the solved hips.
        let jump = animated.blend_regions(&solved, 0.0, 1.0);
        let offset = solved.position(RigBone::Hips) - animated.position(RigBone::Hips);
        for bone in [RigBone::Pelvis, RigBone::LeftUpLeg, RigBone::LeftFoot] {
            assert!(
                jump.position(bone)
                    .abs_diff_eq(animated.position(bone) + offset, 1e-5)
            );
        }
        let none = animated.blend_regions(&solved, 0.0, 0.0);
        for bone in RigBone::ALL {
            assert!(
                none.position(bone)
                    .abs_diff_eq(animated.position(bone), 1e-5)
            );
        }
    }

    #[test]
    fn a_switch_of_solve_fades_from_the_last_pose() {
        let under = pose();
        let mut on_hips = pose();
        on_hips.translate(RigBone::Hips, Vec3::new(0.0, 0.3, 0.0));
        let mut switch = SolveSwitch::new();
        assert_eq!(switch.step(SolveKind::UnderHead, under, 0.1, 0.4), under);
        assert_eq!(switch.step(SolveKind::UnderHead, under, 0.1, 0.4), under);
        // The first pose after the switch is still mostly the last one, and the fade is monotone.
        let mut last = under.position(RigBone::Hips).y;
        let first = switch.step(SolveKind::OnAnimatedHips, on_hips, 0.1, 0.4);
        assert!(first.position(RigBone::Hips).y < on_hips.position(RigBone::Hips).y);
        assert!(switch.fading());
        for _ in 0..5 {
            let y = switch
                .step(SolveKind::OnAnimatedHips, on_hips, 0.1, 0.4)
                .position(RigBone::Hips)
                .y;
            assert!(y >= last - 1e-6);
            last = y;
        }
        assert!(!switch.fading());
        assert_eq!(
            switch.step(SolveKind::OnAnimatedHips, on_hips, 0.1, 0.4),
            on_hips
        );
        // After a reset (the animation had the body), there is nothing to fade from.
        switch.reset();
        assert_eq!(switch.step(SolveKind::UnderHead, under, 0.1, 0.4), under);
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
