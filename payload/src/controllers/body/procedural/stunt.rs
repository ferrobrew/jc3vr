//! A neutral visual waist and torso for stable vehicle stunt stances. The native attachment and
//! root remain unchanged; the legs retain their animated transforms relative to the waist.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    rig::RigBone,
    solver::{SolveInput, pose::RigPose},
};

/// Replace the stunt idle's waist and upper body with the reference pose in the tracked body frame.
/// The reference head coincides with the tracked head target, so the waist remains reachable even
/// when the native idle crouches or hangs sideways against the vehicle.
pub(super) fn neutral_frame(input: &SolveInput, reference: &RigPose, heading: Quat) -> RigPose {
    if !heading.is_finite() || !input.head.is_finite() {
        return input.animated;
    }
    let mut neutral = *reference;
    neutral.rotate_about(RigBone::Hips, heading.normalize(), Vec3::ZERO);
    neutral.translate(RigBone::Hips, input.head - neutral.position(RigBone::Head));
    let animated_hips = input.animated.get(RigBone::Hips);
    let hips = *neutral.get(RigBone::Hips);
    let transport = hips.rotation * animated_hips.rotation.inverse();
    for bone in RigBone::ALL {
        if bone.descends_from(RigBone::Pelvis) {
            let animated = input.animated.get(bone);
            let out = &mut neutral.bones[bone.index()];
            out.translation =
                hips.translation + transport * (animated.translation - animated_hips.translation);
            out.rotation = (transport * animated.rotation).normalize();
            out.scale = animated.scale;
        }
    }
    if neutral.is_finite() {
        neutral
    } else {
        input.animated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::rig::{Rig, tests::skeleton};

    #[test]
    fn a_crouched_twisted_stunt_keeps_native_leg_motion_but_not_its_waist_frame() {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let reference = RigPose {
            bones: rig.reference,
        };
        let mut animated = reference;
        animated.rotate_about(RigBone::Hips, Quat::from_rotation_z(0.9), Vec3::ZERO);
        animated.translate(RigBone::Hips, Vec3::new(0.2, -0.4, 0.3));
        let knee = animated.position(RigBone::LeftLeg);
        animated.rotate_about(RigBone::LeftLeg, Quat::from_rotation_x(0.7), knee);
        let input = SolveInput {
            animated,
            head: Vec3::new(0.1, 1.45, -0.2),
            eye: Vec3::new(0.1, 1.45, -0.3),
            head_rotation: Quat::IDENTITY,
            torso_yaw: Some(0.0),
        };
        // The cockpit may bank with the filtered animated head; neutral is not world-upright.
        let heading = Quat::from_rotation_y(0.6) * Quat::from_rotation_z(-0.3);
        let pose = neutral_frame(&input, &reference, heading);
        assert!(pose.position(RigBone::Head).abs_diff_eq(input.head, 1e-5));
        let expected =
            heading * (reference.position(RigBone::Hips) - reference.position(RigBone::Head));
        assert!((pose.position(RigBone::Hips) - input.head).abs_diff_eq(expected, 1e-5));
        let animated_hips = animated.get(RigBone::Hips);
        let hips = pose.get(RigBone::Hips);
        for bone in RigBone::ALL {
            if bone.descends_from(RigBone::Pelvis) {
                let old = animated.get(bone);
                let new = pose.get(bone);
                let before = animated_hips.rotation.inverse()
                    * (old.translation - animated_hips.translation);
                let after = hips.rotation.inverse() * (new.translation - hips.translation);
                assert!(before.abs_diff_eq(after, 1e-5), "{bone:?}");
                assert!(
                    (animated_hips.rotation.inverse() * old.rotation)
                        .abs_diff_eq(hips.rotation.inverse() * new.rotation, 1e-5)
                );
            }
        }
        assert_ne!(pose.get(RigBone::Hips), animated.get(RigBone::Hips));
        assert!(
            animated
                .blend(&pose, 0.0)
                .position(RigBone::Hips)
                .abs_diff_eq(animated.position(RigBone::Hips), 1e-5)
        );
    }
}
