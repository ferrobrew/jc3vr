//! Animation hooks: the local player's animation updates, for the pose publication and the authored
//! camera, and the pose producer's asynchronous dispatch, which cannot certify completed output.

use detours_macro::detour;
use jc3gi::character::character::{AnimationController, Character};
use re_utilities::hook_library::HookLibrary;

pub(crate) mod publication;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_ANIMATIONS_TIME_BINDER)
        .with_static_binder(&UPDATE_ANIMATIONS_BINDER)
        .with_static_binder(&KICK_UPDATE_JOB_BINDER)
}

/// Async dispatch cannot certify completed animation output.
#[detour(address = jc3gi::animation::pose_producer::PoseProducer::KickUpdateJob_ADDRESS)]
fn kick_update_job(
    producer: *mut jc3gi::animation::pose_producer::PoseProducer,
    dt: f32,
    layers: i32,
    bones: i32,
) {
    publication::invalidate_producer(producer as usize);
    KICK_UPDATE_JOB
        .get()
        .unwrap()
        .call(producer, dt, layers, bones);
}

/// The controller's time-only path: the local player's update feeds the authored camera.
#[detour(
    address = jc3gi::character::character::AnimationController::UpdateAnimationsTime_ADDRESS
)]
fn update_animations_time(controller: *mut AnimationController, dt: f32) {
    UPDATE_ANIMATIONS_TIME.get().unwrap().call(controller, dt);
    if unsafe { is_local_controller(controller) }
        && let Some(controller) = unsafe { controller.as_ref() }
    {
        unsafe { crate::hooks::camera::authored::animation_update(controller, None) };
    }
}

/// Synchronous pose evaluation: the local player's evaluation is published and feeds the authored
/// camera.
#[detour(
    address = jc3gi::character::character::AnimationController::UpdateAnimations_ADDRESS
)]
fn update_animations(controller: *mut AnimationController, dt: f32, num_of_bones: i32) {
    let local = unsafe { is_local_controller(controller) };
    let pose_evaluation = unsafe { controller.as_ref() }
        .filter(|_| local)
        .and_then(|controller| unsafe { publication::begin(controller, num_of_bones) });
    let evaluation = unsafe {
        controller
            .as_ref()
            .filter(|_| local)
            .and_then(|controller| {
                crate::hooks::camera::authored::animation_evaluation(controller, num_of_bones)
            })
    };
    UPDATE_ANIMATIONS
        .get()
        .unwrap()
        .call(controller, dt, num_of_bones);
    if local && let Some(controller) = unsafe { controller.as_ref() } {
        unsafe { publication::complete(controller, pose_evaluation) };
        unsafe { crate::hooks::camera::authored::animation_update(controller, evaluation) };
    }
}

unsafe fn is_local_controller(controller: *mut AnimationController) -> bool {
    let selected = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .map(|player| player.m_AnimatedModel.m_AnimationController)
        .filter(|controller| !controller.is_null());
    publication::select(selected.map(|controller| controller as usize));
    crate::hooks::camera::authored::select_controller(selected.map(|controller| controller as u64));
    selected.is_some_and(|selected| std::ptr::eq(selected, controller))
}

/// The character's active animation rule-state id (`hashlittle(state_name)`, e.g. `S_IDLE`), read
/// through its first (body) rule system's state machine, mirroring `CCharacter::IsInVehicleAttachState`.
/// `None` if any link in the chain is absent (loading, no rule system).
pub(crate) unsafe fn active_state_type_id(character: &Character) -> Option<u32> {
    unsafe {
        Some(
            character
                .m_AnimatedModel
                .m_RuleSystems
                .as_slice()
                .first()?
                .as_ref()?
                .m_StateMachineInstance
                .as_ref()?
                .m_CurrentState
                .as_ref()?
                .m_HashID
                .m_Hash,
        )
    }
}
