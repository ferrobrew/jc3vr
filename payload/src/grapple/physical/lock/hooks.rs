//! The reel's endings the lock holds off while it holds the line: the reel task's arrival
//! ([`NStateTask_ReelIn::FinishReel`]) and the hook's stuck check
//! ([`GrapplingHook::CheckReelCollisions`]). The lock's own endings (a release, a yank, a landing,
//! a break) take their place.
//!
//! [`NStateTask_ReelIn::FinishReel`]: jc3gi::equipment::grappling_hook::NStateTask_ReelIn::FinishReel
//! [`GrapplingHook::CheckReelCollisions`]: jc3gi::equipment::grappling_hook::GrapplingHook::CheckReelCollisions

use detours_macro::detour;
use jc3gi::{
    character::character::Character,
    equipment::grappling_hook::{GrappleTuning, GrapplingHook},
    types::{math::Vector3, shared_ptr::SharedPtr},
};
use re_utilities::hook_library::HookLibrary;

use crate::grapple::physical::lock::{engine, holds, last_velocity};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&FINISH_REEL_BINDER)
        .with_static_binder(&CHECK_REEL_COLLISIONS_BINDER)
}

#[detour(address = jc3gi::equipment::grappling_hook::NStateTask_ReelIn::FinishReel_ADDRESS)]
fn finish_reel(
    character: *mut Character,
    hook: *const SharedPtr<GrapplingHook>,
    tuning: *const GrappleTuning,
    target_position: *const Vector3,
) {
    // SAFETY: the reel task's own arguments, on the character update that owns them.
    let local = unsafe { character.as_mut() }.filter(|c| c.m_IsLocalCharacter);
    if let Some(character) = local
        && holds()
    {
        // The arrival skipped the reel's motion this tick; hand the lock's velocity on instead.
        if let Some(velocity) = last_velocity() {
            unsafe { engine::set_wanted_velocity(character, velocity) };
        }
        return;
    }
    FINISH_REEL
        .get()
        .unwrap()
        .call(character, hook, tuning, target_position);
}

#[detour(address = jc3gi::equipment::grappling_hook::GrapplingHook::CheckReelCollisions_ADDRESS)]
fn check_reel_collisions(hook: *mut GrapplingHook, holder: *const SharedPtr<Character>, dt: f32) {
    // SAFETY: the hook's own arguments, on its post-simulation update.
    let local = unsafe { holder.as_ref() }
        .and_then(|h| unsafe { h.px.as_ref() })
        .is_some_and(|c| c.m_IsLocalCharacter);
    if local && holds() {
        return;
    }
    CHECK_REEL_COLLISIONS.get().unwrap().call(hook, holder, dt);
}
