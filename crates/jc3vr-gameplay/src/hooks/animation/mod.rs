//! Animation hooks: the local player's animation updates, for the pose publication and the authored
//! camera, and the pose producer's asynchronous dispatch, which cannot certify completed output.

use detours_macro::detour;
use jc3gi::{
    animation::rule_system::{AnimationRuleSystem, StateMachineInstance},
    character::character::{AnimationController, Character},
};
use re_utilities::hook_library::HookLibrary;

pub mod canopy;
pub(crate) mod publication;

pub fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_ANIMATIONS_TIME_BINDER)
        .with_static_binder(&UPDATE_ANIMATIONS_BINDER)
        .with_static_binder(&KICK_UPDATE_JOB_BINDER)
        .with_hook_library(canopy::hook_library())
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
        unsafe { jc3vr_view::hooks::camera::authored::animation_update(controller, None) };
    }
}

/// Synchronous pose evaluation: the local player's evaluation is published and feeds the authored
/// camera.
#[detour(
    address = jc3gi::character::character::AnimationController::UpdateAnimations_ADDRESS
)]
fn update_animations(controller: *mut AnimationController, dt: f32, num_of_bones: i32) {
    let local = unsafe { is_local_controller(controller) };
    if local {
        // SAFETY: the local controller's update entry, on the game thread, before evaluation.
        unsafe { canopy::skip_before_evaluation() };
    }
    let pose_evaluation = unsafe { controller.as_ref() }
        .filter(|_| local)
        .and_then(|controller| unsafe { publication::begin(controller, num_of_bones) });
    let evaluation = unsafe {
        controller
            .as_ref()
            .filter(|_| local)
            .and_then(|controller| {
                jc3vr_view::hooks::camera::authored::animation_evaluation(controller, num_of_bones)
            })
    };
    UPDATE_ANIMATIONS
        .get()
        .unwrap()
        .call(controller, dt, num_of_bones);
    if local && let Some(controller) = unsafe { controller.as_ref() } {
        unsafe { publication::complete(controller, pose_evaluation) };
        unsafe { jc3vr_view::hooks::camera::authored::animation_update(controller, evaluation) };
    }
}

unsafe fn is_local_controller(controller: *mut AnimationController) -> bool {
    let selected = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .map(|player| player.m_AnimatedModel.m_AnimationController)
        .filter(|controller| !controller.is_null());
    publication::select(selected.map(|controller| controller as usize));
    jc3vr_view::hooks::camera::authored::select_controller(
        selected.map(|controller| controller as u64),
    );
    selected.is_some_and(|selected| std::ptr::eq(selected, controller))
}

/// The character's active animation rule-state id (`hashlittle(state_name)`, e.g. `S_IDLE`), read
/// through its first (body) rule system's state machine, mirroring `CCharacter::IsInVehicleAttachState`.
/// `None` if any link in the chain is absent (loading, no rule system).
///
/// # Safety
///
/// `character` must be a live engine character: the chain from its animated model to the rule
/// system's state machine is read through raw engine pointers.
pub unsafe fn active_state_type_id(character: &Character) -> Option<u32> {
    unsafe {
        Some(
            body_rule_system(character)?
                .m_StateMachineInstance
                .as_ref()?
                .m_CurrentState
                .as_ref()?
                .m_HashID
                .m_Hash,
        )
    }
}

/// The current body machine when its owner, current state, and loaded definition agree.
///
/// # Safety
///
/// The character is live on the game thread, outside an animation update.
pub(crate) unsafe fn ready_body_machine(character: &Character) -> Option<&StateMachineInstance> {
    if character.m_ControlParameters.is_null() || character.m_AnimatedModel.m_InstanceData.is_null()
    {
        return None;
    }
    let machine = unsafe { body_rule_system(character)?.m_StateMachineInstance.as_ref() }?;
    unsafe { machine.m_CurrentState.as_ref() }?;
    let expected_user = std::ptr::from_ref(character)
        .cast::<u8>()
        .wrapping_add(Character::GAME_OBJECT_BASE_OFFSET as usize);
    if !std::ptr::eq(machine.m_UserData.cast::<u8>(), expected_user) {
        return None;
    }
    let handle = unsafe { machine.m_StateMachine.as_ref() }?;
    let cache = unsafe { handle.m_ResourceCache.as_ref() }?;
    if cache.m_Resources.is_null() {
        return None;
    }
    let resource = unsafe { cache.m_Resources.add(usize::from(handle.m_Index)).as_ref() }?;
    if resource.m_Resource.is_null() {
        return None;
    }
    Some(machine)
}

/// The character's first (body) rule system, which also holds the height over the ground its
/// ground information last wrote. `None` while the character has none.
///
/// # Safety
///
/// `character` must be a live engine character: its rule systems are read through raw engine
/// pointers.
pub unsafe fn body_rule_system(character: &Character) -> Option<&AnimationRuleSystem> {
    unsafe {
        character
            .m_AnimatedModel
            .m_RuleSystems
            .as_slice()
            .first()?
            .as_ref()
    }
}
