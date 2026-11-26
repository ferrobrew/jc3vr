//! The post-solve head override: hide the player's head, publish the render-block head-hide
//! inputs, and drive the head and neck bones from the headpose on top of the IK-solved pose. Runs
//! at the very end of the pose-finalization pass, after the HumanIK solve and the model-space pose
//! computation.

use glam::Quat;
use jc3gi::character::character::{AnimatedModel, Character, Joint, SafeBoneIndex};
use jc3vr_view::{headpose, headpose::Source, hooks::graphics_engine::player_draw, vr::VR_CONFIG};

use crate::{
    controllers::body::procedural::head::head_bone_world,
    hooks::character::{
        body_ik::{PRE_IK_POSE, PreIkPose, body_ik_status},
        tuning::CHARACTER,
    },
};

/// Hide the player's head and drive its full pose from the headpose.
///
/// # Safety
///
/// `character` must be null or a live character, called from the post-call `UpdatePropEffects`
/// detour.
pub(crate) unsafe fn override_head(character: *mut Character) {
    unsafe {
        let Some(character) = character.as_mut().filter(|c| c.m_IsLocalCharacter) else {
            return;
        };

        if !crate::hooks::character::pose::eligible(character) {
            crate::hooks::character::body_ik::capture_unavailable();
            return;
        }
        let Some(animation_controller) = character.m_AnimatedModel.m_AnimationController.as_mut()
        else {
            crate::hooks::character::body_ik::capture_unavailable();
            return;
        };

        let Some(mut pose) = crate::hooks::character::pose::LivePose::new(animation_controller)
        else {
            crate::hooks::character::body_ik::capture_unavailable();
            return;
        };
        let (Some(head_index), Some(neck_index)) = (
            pose.safe_index(character.GetSafeIndex(SafeBoneIndex::HEAD)),
            pose.safe_index(character.GetSafeIndex(SafeBoneIndex::NECK)),
        ) else {
            crate::hooks::character::body_ik::capture_unavailable();
            return;
        };
        let Some(mut joint) = pose.read(head_index) else {
            crate::hooks::character::body_ik::capture_unavailable();
            return;
        };
        let Some(mut neck_joint) = pose.read(neck_index) else {
            crate::hooks::character::body_ik::capture_unavailable();
            return;
        };

        // Publish the render-block head-hide inputs (the draws run on the render thread and only
        // load these): the instance-info pointers for ownership, and the collapse set — the HEAD bone
        // plus the facial bones, so every vertex weighted anywhere on the head (face, eyes, ears,
        // and the hair riding HEAD) collapses. Invalid lookups are filtered by the publish (a
        // missing name must not collapse the root).
        // The instance-info pointers for exact draw ownership: each model instance's embedded
        // CRBIInfo is the `info` every one of its block draws receives.
        let rbi_infos: [usize; AnimatedModel::MODEL_INSTANCE_SLOTS as usize] =
            character.m_AnimatedModel.m_ModelInstances.map(|instance| {
                if instance == 0 {
                    0
                } else {
                    instance as usize + AnimatedModel::MODEL_INSTANCE_RBI_INFO_OFFSET as usize
                }
            });
        player_draw::publish_player_rbi_infos(&rbi_infos);
        let bone = |name: &[u8]| pose.named(name).unwrap_or(u32::MAX);
        player_draw::publish_collapse_bones(&[
            head_index,
            bone(b"offset_facialOrienter"),
            bone(b"fJaw"),
            bone(b"fMidLwrLip"),
            bone(b"fLeftMouthCorner"),
            bone(b"fRightMouthCorner"),
            bone(b"fNose"),
            bone(b"fMidUprLip"),
            bone(b"fUprLids"),
            bone(b"fLwrLids"),
            bone(b"fLeftBrowMidA"),
            bone(b"fRightBrowMidA"),
            bone(b"fLeftEye"),
            bone(b"fRightEye"),
            bone(b"fLeftEar"),
            bone(b"fRightEar"),
        ]);

        // HEAD: the full headpose pose, in a single SetJoint. The head/neck/eye anchors are captured PRE-IK in
        // `character_update_pass_finalize_pose_parallel`: this hook runs last, after the HumanIK
        // solve *and* after CalculateModelSpacePose, so a capture here would read the IK-solved head
        // and chase the very target HIK pulls it toward (the feedback loop). This hook only consumes
        // the published anchors and applies the head override on top of the solved pose.
        let character_world = glam::Mat4::from(character.m_WorldMatrixT1);
        let (_, character_rotation, _) = character_world.to_scale_rotation_translation();
        let joint_translation = |joint: &Joint| {
            let [x, y, z] = joint.m_Translation.data;
            glam::Vec3::new(x, y, z)
        };

        // The head-hide collapse target: the render side cannot read positions out of the
        // palette (the translation slots depend on each block's layout), so the model-space neck
        // point comes from the skeleton here.
        player_draw::publish_collapse_target(joint_translation(&neck_joint));

        // The pre-IK capture carries the bind frame. VR uses that frame with the filtered
        // animation and fresh tracking.
        let pre_ik_pose = *PRE_IK_POSE.lock();
        let source = headpose::source();

        // Only drive the head/neck from the headpose while gameplay owns the character. Outside
        // E_GAME_RUN the engine repositions Rico for a teleport (issue #27), so the mod stops driving
        // his pose and lets the animation/engine place him.
        let in_gameplay = jc3vr_core::hooks::in_gameplay();

        // Only override the pose once a valid anchor exists; until then (loading screens, garbage
        // bone data) the bone keeps its animated pose.
        if in_gameplay && headpose::is_active() && headpose::anchor().is_some() {
            // The VR head uses the same filtered synthetic frame as the camera, followed by
            // tracking and the bone's bind frame. The sim retains its animated head offset.
            let offset_model = headpose::body_relative_rotation();
            let animated = pre_ik_pose
                .map(|pose| pose.head_orientation)
                .unwrap_or_else(|| {
                    let [qx, qy, qz, qw] = joint.m_Orientation.data;
                    Quat::from_xyzw(qx, qy, qz, qw)
                });
            let Some(composed) = head_orientation(source, pre_ik_pose, offset_model, animated)
            else {
                crate::hooks::character::body_ik::capture_unavailable();
                return;
            };
            // glam Quat (x,y,z,w) -> Havok AlignedQuat [x,y,z,w] is a direct copy.
            joint.m_Orientation.data = [composed.x, composed.y, composed.z, composed.w];

            // The roomscale positional offset, brought into the body frame — but only when HumanIK
            // is not already placing the head: both paths aim at the same place, the published
            // head carried back onto the neck pivot, so applying both displaces it twice. "Placing"
            // requires an effective reach, not just a queued target — with the reach dialled to
            // zero the solve moves nothing and this add must take over, or roomscale placement
            // silently dies.
            let body_ik = &CHARACTER.get().body_ik;
            let hik_places_head = body_ik.head_reach_t * body_ik.weight > 0.0
                && body_ik_status().is_some_and(|s| s.skip.is_none());
            let animated_head_world = headpose::anchor().unwrap_or(glam::Vec3::ZERO);
            let scale = VR_CONFIG.lock().world_scale;
            let world_offset = head_bone_world(character_rotation, scale) - animated_head_world;
            if !hik_places_head && world_offset != glam::Vec3::ZERO {
                let model_offset = character_rotation.inverse() * world_offset;
                let [tx, ty, tz] = joint.m_Translation.data;
                joint.m_Translation.data = [
                    tx + model_offset.x,
                    ty + model_offset.y,
                    tz + model_offset.z,
                ];
            }
        }

        // Twist the neck along with the head beyond the character tuning's `neck.twist_start_deg`:
        // the head bone carries the whole body-relative yaw, and past a real head's range the neck
        // must follow or the skinning between the (hidden) head and the animated neck knots up —
        // this is what makes the extended free-look yaw range anatomically plausible. Body-relative
        // yaw is exactly a model-space Y rotation, so the twist pre-multiplies the *animated*
        // model-space neck orientation captured above — no rest-frame knowledge needed, and the
        // neck's translation (its own origin) is untouched.
        if in_gameplay && headpose::is_active() && headpose::anchor().is_some() {
            let yaw = headpose::sim::body_yaw_of(headpose::body_relative_rotation());
            let neck = &CHARACTER.get().neck;
            let excess_deg =
                (yaw.abs().to_degrees() - neck.twist_start_deg).clamp(0.0, neck.twist_max_deg);
            if excess_deg > 0.0 {
                let twist = excess_deg.to_radians().copysign(yaw);
                // Pre-multiply onto the pre-IK neck orientation for the same reason as the head
                // above: when body_ik has bent the neck toward the target, twisting the post-IK
                // orientation would compound HIK's neck rotation with this manual twist.
                let animated = pre_ik_pose
                    .map(|pose| pose.neck_orientation)
                    .unwrap_or_else(|| {
                        let [qx, qy, qz, qw] = neck_joint.m_Orientation.data;
                        Quat::from_xyzw(qx, qy, qz, qw)
                    });
                let twisted = Quat::from_rotation_y(twist) * animated;
                neck_joint.m_Orientation.data = [twisted.x, twisted.y, twisted.z, twisted.w];
                pose.write(neck_index, &mut neck_joint);
            }
        }
        pose.write(head_index, &mut joint);
    }
}

fn head_orientation(
    source: Source,
    pre_ik: Option<PreIkPose>,
    tracking: Quat,
    animated: Quat,
) -> Option<Quat> {
    match source {
        Source::Vr => pre_ik.map(|pose| pose.tracked_head_orientation(tracking)),
        Source::Sim => Some(tracking * animated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rotation(actual: Quat, expected: Quat) {
        assert!(body_rig::rotation::angle_between(actual, expected) < 0.001);
    }

    #[test]
    fn vr_uses_the_bind_capture_without_a_body_ik_enable_gate() {
        let bind = Quat::from_euler(glam::EulerRot::YXZ, 0.3, 1.1, -0.4);
        let capture = PreIkPose {
            head_orientation: Quat::from_rotation_x(-0.8) * bind,
            neck_orientation: Quat::IDENTITY,
            head_bind_orientation: bind,
        };
        let tracking = Quat::from_rotation_y(2.8) * Quat::from_rotation_x(-0.5);
        let expected = capture.tracked_head_orientation(tracking);
        for raw_animated in [capture.head_orientation, Quat::from_rotation_z(1.2)] {
            let actual =
                head_orientation(Source::Vr, Some(capture), tracking, raw_animated).unwrap();
            assert_rotation(actual, expected);
        }
    }

    #[test]
    fn vr_without_a_bind_capture_refuses_the_raw_animation_fallback() {
        assert!(
            head_orientation(
                Source::Vr,
                None,
                Quat::from_rotation_y(0.7),
                Quat::from_rotation_x(-1.0),
            )
            .is_none()
        );
    }

    #[test]
    fn flatscreen_keeps_its_animated_fallback_without_a_capture() {
        let tracking = Quat::from_rotation_y(0.7);
        let animated = Quat::from_rotation_x(-1.0);
        let actual = head_orientation(Source::Sim, None, tracking, animated).unwrap();
        assert_rotation(actual, tracking * animated);
    }
}
