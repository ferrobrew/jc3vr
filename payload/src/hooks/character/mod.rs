//! The character hooks: the pose-finalization pass (headset-driven body IK, the controller hands'
//! wrist targets, and the roomscale crouch's hips and feet before the HumanIK solve, the head
//! override and the hands' post-solve publish after it) and the act filter.
//!
//! - [`body_ik`]: the pre-solve anchor capture and the head, torso, and hand-pin targets;
//! - [`head`]: the post-solve head override and the render-block head-hide inputs;
//! - [`acts`]: the local player's act filter.

use detours_macro::detour;
use jc3gi::character::character::Character;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    self,
    hands::{self, ik::HandPlan},
};

pub(crate) mod acts;
pub(crate) mod body_ik;
pub(crate) mod config;
pub(crate) mod head;

pub(crate) use acts::resolve_act_id;
pub(crate) use body_ik::{BODY_IK_TARGET, body_ik_status};
pub(crate) use config::BodyIkConfig;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHARACTER_UPDATE_PASS_FINALIZE_POSE_PARALLEL_BINDER)
        .with_static_binder(&CHARACTER_UPDATE_PROP_EFFECTS_BINDER)
        .with_hook_library(acts::hook_library())
}

/// Pre-call seam for headset-driven upper-body IK. Runs at the entry of the pose-finalization pass,
/// *before* the HumanIK `MAIN` solve and its `HasTargets` gate (docs/engine/character/humanik.md): it captures the
/// headpose anchors from the freshly animated (pre-IK) pose and queues the head effector target so
/// the engine's own solver bends the spine and head toward the headpose the same frame. The
/// `SetJoint` head override still runs at the very end of this pass, in
/// [`character_update_prop_effects`], on top of the HIK-bent spine.
#[detour(
    address = jc3gi::character::character::Character::UpdatePassFinalizePose_Parallel_ADDRESS
)]
fn character_update_pass_finalize_pose_parallel(
    character: *mut Character,
    context: *mut std::ffi::c_void,
) {
    // Queue targets and capture anchors BEFORE the trampoline: the solve and the anchor-consuming
    // gate both run inside the real function, so entry-queued targets are solved this frame and the
    // pre-solve pose is the pure animation result.
    //
    // The controller hands are planned first so that the body IK's hand pins stand down for the
    // wrists the controllers drive, and queued after it because the hand targets compose through
    // the head anchors the body IK publishes.
    let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
    let plan = local.then(hands::ik::plan);
    unsafe {
        body_ik::capture_anchors_and_queue_body_ik(
            character,
            plan.as_ref().map_or([false; 2], HandPlan::driven),
        );
        if let Some(plan) = &plan {
            hands::ik::queue(character, plan);
        }
        controllers::roomscale::queue_crouch_ik(character);
    }

    CHARACTER_UPDATE_PASS_FINALIZE_POSE_PARALLEL
        .get()
        .unwrap()
        .call(character, context);
}

#[detour(address = jc3gi::character::character::Character::UpdatePropEffects_ADDRESS)]
fn character_update_prop_effects(character: *mut Character, dt: f32) {
    CHARACTER_UPDATE_PROP_EFFECTS
        .get()
        .unwrap()
        .call(character, dt);

    unsafe {
        head::override_head(character);
        hands::ik::after_solve(character);
    }
}
