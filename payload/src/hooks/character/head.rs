//! The post-solve head override: hide the player's head, publish the render-block head-hide
//! inputs, and drive the head and neck bones from the headpose on top of the IK-solved pose. Runs
//! at the very end of the pose-finalization pass, after the HumanIK solve and the model-space pose
//! computation.

use jc3gi::{
    character::character::{AnimatedModel, Character, Joint, SafeBoneIndex},
    hash::hashlittle,
};

use crate::{
    config::Config,
    headpose,
    hooks::{
        self,
        character::body_ik::{PRE_IK_POSE, body_ik_status},
        graphics_engine::render_block,
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

        let Some(animation_controller) = character.m_AnimatedModel.m_AnimationController.as_mut()
        else {
            return;
        };

        let head_index = character.GetSafeIndex(SafeBoneIndex::HEAD);

        // Publish the render-block head-hide inputs (the draws run on the render thread and only
        // load these): the player root pair for ownership, and the collapse set — the HEAD bone
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
        render_block::publish_player_rbi_infos(&rbi_infos);
        render_block::publish_player_root(
            glam::Mat4::from(character.m_WorldMatrixT0)
                .w_axis
                .truncate(),
            glam::Mat4::from(character.m_WorldMatrixT1)
                .w_axis
                .truncate(),
        );
        let bone = |name: &[u8]| animation_controller.GetBoneIndex(hashlittle(name) as u32);
        render_block::publish_collapse_bones(&[
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

        // HEAD: optionally the legacy scale-hide, plus the full headpose pose, in a single
        // SetJoint. The head/neck/eye anchors are captured PRE-IK in
        // `character_update_pass_finalize_pose_parallel`: this hook runs last, after the HumanIK
        // solve *and* after CalculateModelSpacePose, so a capture here would read the IK-solved head
        // and chase the very target HIK pulls it toward (the feedback loop). This hook only consumes
        // the published anchors and applies the head override on top of the solved pose.
        let hide_scale = Config::lock_query(|c| c.camera.hide_head_scale);
        let scale = 0.001;
        let mut joint = Joint::default();
        animation_controller.GetJoint(head_index, &mut joint);

        let character_world = glam::Mat4::from(character.m_WorldMatrixT1);
        let (_, character_rotation, _) = character_world.to_scale_rotation_translation();
        let joint_translation = |joint: &Joint| {
            let [x, y, z] = joint.m_Translation.data;
            glam::Vec3::new(x, y, z)
        };

        let neck_index = character.GetSafeIndex(SafeBoneIndex::NECK);
        let mut neck_joint = Joint::default();
        animation_controller.GetJoint(neck_index, &mut neck_joint);
        // The head-hide collapse target: the render side cannot read positions out of the
        // palette (the translation slots depend on each block's layout), so the model-space neck
        // point comes from the skeleton here.
        render_block::publish_collapse_target(joint_translation(&neck_joint));

        // The pre-IK head/neck orientations captured this frame before the solve. When body_ik is
        // driving the head, the joints read above are the *post-IK* pose, so composing the
        // override's body-relative offset onto their orientation would double-count the yaw HumanIK
        // already bent toward the target. Composing onto the pre-IK base instead makes the override
        // "set the exact head orientation" — identical to the no-IK case — with HIK's spine/neck
        // bend sitting underneath it. When body_ik is off the pre-IK and post-IK orientations match,
        // so the fallback to the freshly read orientation preserves the flatscreen path exactly.
        let body_ik = Config::lock_query(|c| c.body_ik);
        let pre_ik = body_ik
            .enabled
            .then(|| {
                PRE_IK_POSE
                    .lock()
                    .as_ref()
                    .map(|p| (p.head_orientation, p.neck_orientation))
            })
            .flatten();

        if hide_scale {
            joint.m_Scale.data = [scale, scale, scale];
        }

        // Only drive the head/neck from the headpose while gameplay owns the character. Outside
        // E_GAME_RUN the engine repositions Rico for a teleport (issue #27), so the mod stops driving
        // his pose and lets the animation/engine place him; the head-hide above still applies.
        let in_gameplay = hooks::in_gameplay();

        // Only override the pose once a valid anchor exists; until then (loading screens, garbage
        // bone data) the bone keeps its animated pose and only the legacy scale-hide applies.
        if in_gameplay && headpose::is_active() && headpose::anchor().is_some() {
            // Compose the player's body-relative offset onto the *animated* model-space
            // orientation, exactly like the neck twist below. The previous absolute write assumed
            // the bone's rest frame matched the model axes, which it does not — observed in the
            // (now headful) shadow as the head collapsing into the shoulders. Model space is the
            // body frame, so the sim's body-relative angles apply directly, and the animated
            // translation is kept (plus the roomscale offset brought into the body frame), so the
            // head stays anatomically placed while turning where the player looks.
            let offset_model = headpose::body_relative_rotation();
            let animated = pre_ik.map(|(head, _)| head).unwrap_or_else(|| {
                let [qx, qy, qz, qw] = joint.m_Orientation.data;
                glam::Quat::from_xyzw(qx, qy, qz, qw)
            });
            let composed = offset_model * animated;
            // glam Quat (x,y,z,w) -> Havok AlignedQuat [x,y,z,w] is a direct copy.
            joint.m_Orientation.data = [composed.x, composed.y, composed.z, composed.w];

            // The roomscale positional offset, brought into the body frame — but only when HumanIK
            // is not already placing the head: both paths aim at the same tracked position, so
            // applying both displaces it twice. "Placing" requires an effective reach, not just a
            // queued target — with the reach dialled to zero the solve moves nothing and this add
            // must take over, or roomscale placement silently dies.
            let hik_places_head = body_ik.enabled
                && body_ik.head_reach_t * body_ik.weight > 0.0
                && body_ik_status().is_some_and(|s| s.skip.is_none());
            let animated_head_world = headpose::anchor().unwrap_or(glam::Vec3::ZERO);
            let world_offset = headpose::query().position - animated_head_world;
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

        animation_controller.SetJoint(head_index, &mut joint);

        // Twist the neck along with the head beyond the configured start: the head bone carries
        // the whole body-relative yaw, and past a real head's range the neck must follow or the
        // skinning between the (hidden) head and the animated neck knots up — this is what makes
        // the extended free-look yaw range anatomically plausible. Body-relative yaw is exactly a
        // model-space Y rotation, so the twist pre-multiplies the *animated* model-space neck
        // orientation captured above — no rest-frame knowledge needed, and the neck's translation
        // (its own origin) is untouched.
        if in_gameplay && headpose::is_active() && headpose::anchor().is_some() {
            let yaw = headpose::sim::body_yaw_of(headpose::body_relative_rotation());
            let (start_deg, max_deg) = Config::lock_query(|c| {
                (
                    c.headpose.neck_twist_start_deg,
                    c.headpose.neck_twist_max_deg,
                )
            });
            let excess_deg = (yaw.abs().to_degrees() - start_deg).clamp(0.0, max_deg.max(0.0));
            if excess_deg > 0.0 {
                let twist = excess_deg.to_radians().copysign(yaw);
                // Pre-multiply onto the pre-IK neck orientation for the same reason as the head
                // above: when body_ik has bent the neck toward the target, twisting the post-IK
                // orientation would compound HIK's neck rotation with this manual twist.
                let animated = pre_ik.map(|(_, neck)| neck).unwrap_or_else(|| {
                    let [qx, qy, qz, qw] = neck_joint.m_Orientation.data;
                    glam::Quat::from_xyzw(qx, qy, qz, qw)
                });
                let twisted = glam::Quat::from_rotation_y(twist) * animated;
                neck_joint.m_Orientation.data = [twisted.x, twisted.y, twisted.z, twisted.w];
                animation_controller.SetJoint(neck_index, &mut neck_joint);
            }
        }

        // Facial bones: scale only (the legacy head-hide behaviour).
        if !hide_scale {
            return;
        }
        let facial_indices = [
            // "offset_facialOrienter",
            "fJaw",
            "fMidLwrLip",
            "fLeftMouthCorner",
            "fRightMouthCorner",
            // "fNose",
            "fMidUprLip",
            // "fUprLids",
            // "fLwrLids",
            // "fLeftBrowMidA",
            // "fRightBrowMidA",
            // "fLeftEye",
            // "fRightEye",
            // "fLeftEar",
            // "fRightEar",
        ];
        for s in facial_indices {
            let index = animation_controller.GetBoneIndex(hashlittle(s.as_bytes()) as u32);
            let mut joint = Joint::default();
            animation_controller.GetJoint(index, &mut joint);
            joint.m_Scale.data = [scale, scale, scale];
            animation_controller.SetJoint(index, &mut joint);
        }
    }
}
