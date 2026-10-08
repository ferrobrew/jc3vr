//! The character hooks: the pose-finalization pass (the weapon-agnostic body's decision, headset-driven
//! body IK, the controller hands' wrist targets, the roomscale crouch's hips and feet, and the
//! squared standing stance before the HumanIK solve, the secondary hand targets switched off before
//! the `SECONDARY` pass, the head override, the hands' post-solve publish, the finger curls, and the
//! body alignment's skeleton read after it), the act filter, the animation modifiers, and the
//! animation-layer mask.
//!
//! - [`body_ik`]: the pre-solve anchor capture and the head, torso, and hand-pin targets;
//! - [`head`]: the post-solve head override and the render-block head-hide inputs;
//! - [`acts`]: the local player's act filter;
//! - [`gate`]: the HumanIK solve gate, where the procedural body writes the pose;
//! - [`modifiers`]: the weapon-agnostic body and the standing stance's relaxed idle, which keep the
//!   weapon's animation modifiers off the body's rule systems;
//! - [`layers`]: the weapon-agnostic body's layer mask, which leaves layers out of the pose;
//! - [`signals`]: the grapple's ledge climb and the hit reactions, for the procedural body's
//!   hand-over table;
//! - the `SECONDARY` hand pass's entry, where the game's secondary hand targets are switched off
//!   ([`hands::ik::before_secondary`]).

use std::time::Instant;

use detours_macro::detour;
use jc3gi::{character::character::Character, game::GameObjectUpdateContext};
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        self,
        body::procedural::{gate_skipped, signals::set_local_character, skeleton, torso_weight},
        hands::{self, ik::HandPlan},
        roomscale::stance,
    },
    frame_timing::{self, Slot},
};

pub(crate) mod acts;
pub(crate) mod body_ik;
pub(crate) mod config;
pub(crate) mod gate;
pub(crate) mod head;
pub(crate) mod layers;
pub(crate) mod modifiers;
pub(crate) mod signals;

pub(crate) use acts::resolve_act_id;
pub(crate) use body_ik::{BODY_IK_TARGET, body_ik_status};
pub(crate) use config::BodyIkConfig;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHARACTER_UPDATE_PASS_FINALIZE_POSE_PARALLEL_BINDER)
        .with_static_binder(&CHARACTER_UPDATE_SECONDARY_HAND_IK_PASS_BINDER)
        .with_static_binder(&CHARACTER_UPDATE_PROP_EFFECTS_BINDER)
        .with_hook_library(acts::hook_library())
        .with_hook_library(gate::hook_library())
        .with_hook_library(layers::hook_library())
        .with_hook_library(modifiers::hook_library())
        .with_hook_library(signals::hook_library())
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
    //
    // The stance is planned before the body IK, which takes its chest turn into the torso
    // rotation, and queued after the crouch, so that its squared ankles replace the crouch's pins.
    //
    // The weapon-agnostic body's decision only publishes, for the next tick's modifier pass and
    // pose job, so its place here is free.
    let timing = frame_timing::scope(Slot::CharacterPose);
    let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
    let plan = local.then(hands::ik::plan);
    // SAFETY: the engine passes the pass's update context, whose first field is the step.
    let dt = unsafe { context.cast::<GameObjectUpdateContext>().as_ref() }.map_or(0.0, |c| c.m_Dt);
    if let Some(character) = unsafe { character.as_ref() }.filter(|c| c.m_IsLocalCharacter) {
        unsafe { skeleton::dump_once(character) };
        set_local_character(character, Instant::now());
        gate::begin_pass(dt);
    }
    unsafe { stance::unarmed::tick(character) };
    let stance_plan = unsafe { stance::plan(character, dt) };
    unsafe {
        controllers::roomscale::sim::complete_movement(character);
        body_ik::capture_anchors_and_queue_body_ik(
            character,
            plan.as_ref().map_or([false; 2], HandPlan::driven),
            stance_plan.as_ref().map_or(0.0, |p| p.chest_yaw),
            dt,
        );
        // While the procedural body poses the torso, the wrist targets wait for the solve gate,
        // where the torso they are solved on exists.
        if let Some(plan) = plan {
            hands::ik::queue_or_defer(character, plan);
        }
        // The procedural body poses the hips and legs itself at the solve gate. Below its full
        // weight these are queued as before, and the gate fades them by what is left of it.
        if torso_weight() < 1.0 {
            controllers::roomscale::queue_crouch_ik(character);
            if let Some(stance_plan) = stance_plan.as_ref() {
                stance::queue(character, stance_plan);
            }
        }
    }
    drop(timing);

    CHARACTER_UPDATE_PASS_FINALIZE_POSE_PARALLEL
        .get()
        .unwrap()
        .call(character, context);
    let _timing = frame_timing::scope(Slot::CharacterPose);
    if gate::end_pass() == Some(false) {
        // Nothing wrote the procedural pose this pass, so nothing may go on standing down for it,
        // and the wrist targets kept for the gate are queued now.
        gate_skipped();
        unsafe { hands::ik::flush_deferred(character) };
    }
    hands::ik::end_finalize();
}

/// Pre-call seam for the `SECONDARY` hand pass: every writer of its blackboard keys has run by now,
/// including the state tasks the pose-finalization pass runs after its entry, so the hands the
/// controllers drive (and, under the weapon-agnostic body, the off-hand weapon grip) are switched
/// off here rather than at the pass's entry.
#[detour(address = jc3gi::character::character::Character::UpdateSecondaryHandIKPass_ADDRESS)]
fn character_update_secondary_hand_ik_pass(
    character: *mut Character,
    dt: f32,
    ik_has_run: *mut bool,
) {
    {
        let _timing = frame_timing::scope(Slot::CharacterPose);
        unsafe { hands::ik::before_secondary(character) };
    }
    CHARACTER_UPDATE_SECONDARY_HAND_IK_PASS
        .get()
        .unwrap()
        .call(character, dt, ik_has_run);
}

#[detour(address = jc3gi::character::character::Character::UpdatePropEffects_ADDRESS)]
fn character_update_prop_effects(character: *mut Character, dt: f32) {
    CHARACTER_UPDATE_PROP_EFFECTS
        .get()
        .unwrap()
        .call(character, dt);

    let _timing = frame_timing::scope(Slot::CharacterPose);
    unsafe {
        head::override_head(character);
        hands::ik::after_solve(character);
        controllers::fingers::after_solve(character);
        controllers::body::alignment::after_solve(character);
    }
}
