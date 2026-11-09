//! A neutral visual body for stable roof and wing standing.
//!
//! The reference pose is placed at the character's own root height, not the tracked head, so the
//! body keeps the character's actual height. The native attachment and velocity remain authoritative;
//! the torso and arms solve to the tracked head and hands as they do on foot.

use glam::Quat;

use crate::controllers::body::procedural::{rig::RigBone, solver::pose::RigPose};

/// The reference pose turned about the hips by the tracked body heading. The hips stay at the
/// reference pose's model-space height, so the body is the character's height rather than the
/// headset's. The torso solve bends the upper body to the tracked head.
pub(super) fn reference_frame(reference: &RigPose, heading: Quat) -> RigPose {
    if !heading.is_finite() {
        return *reference;
    }
    let mut pose = *reference;
    let hips = pose.position(RigBone::Hips);
    pose.rotate_about(RigBone::Hips, heading.normalize(), hips);
    if pose.is_finite() { pose } else { *reference }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::rig::{Rig, tests::skeleton};

    #[test]
    fn the_reference_body_keeps_the_character_height_and_turns_about_the_hips() {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let reference = RigPose {
            bones: rig.reference,
        };
        let heading = Quat::from_rotation_y(0.6) * Quat::from_rotation_z(-0.3);
        let pose = reference_frame(&reference, heading);
        let hips = reference.position(RigBone::Hips);
        assert!(pose.position(RigBone::Hips).abs_diff_eq(hips, 1e-5));
        let head = hips + heading * (reference.position(RigBone::Head) - hips);
        assert!(pose.position(RigBone::Head).abs_diff_eq(head, 1e-5));
        assert!(
            !pose
                .position(RigBone::Head)
                .abs_diff_eq(reference.position(RigBone::Head), 1e-5)
        );
        let feet = hips + heading * (reference.position(RigBone::LeftLeg) - hips);
        assert!(pose.position(RigBone::LeftLeg).abs_diff_eq(feet, 1e-5));
    }

    #[test]
    fn a_non_finite_heading_keeps_the_reference_pose() {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let reference = RigPose {
            bones: rig.reference,
        };
        assert_eq!(
            reference_frame(&reference, Quat::from_xyzw(f32::NAN, 0.0, 0.0, 0.0)),
            reference
        );
    }
}
