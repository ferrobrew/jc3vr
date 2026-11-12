//! The reel's endings the lock holds off while it holds the line: the reel task's arrival
//! ([`NStateTask_ReelIn::FinishReel`]) and the hook's stuck check
//! ([`GrapplingHook::CheckReelCollisions`]). The lock's own endings (a release, a yank, a landing,
//! a break) take their place.
//!
//! The arrival's upside-down ending ([`GrapplingHook::StartReelUpsideDown`], the ceiling attach)
//! and the arrival onto a helicopter (the game's stunt hang under the airframe) are held off for
//! the local player while the hang is on and the lock's tick runs: the reel keeps running, and the
//! lock's next tick hangs the character below the hook instead (`lock::want_hang`). While the
//! lock's tick does not run (the bindings idle, as under the gamepad scheme), or just after it
//! refused a hang, the arrival is the game's (`lock::hang_takes_over`).
//!
//! [`GrapplingHook::StartReelUpsideDown`]: jc3gi::equipment::grappling_hook::GrapplingHook::StartReelUpsideDown
//! [`NStateTask_ReelIn::FinishReel`]: jc3gi::equipment::grappling_hook::NStateTask_ReelIn::FinishReel
//! [`GrapplingHook::CheckReelCollisions`]: jc3gi::equipment::grappling_hook::GrapplingHook::CheckReelCollisions

use detours_macro::detour;
use jc3gi::{
    character::character::Character,
    equipment::grappling_hook::{GrappleTuning, GrapplingHook},
    types::{math::Vector3, shared_ptr::SharedPtr},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    grapple::physical::lock::{engine, hang_takes_over, holds, last_velocity, want_hang},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&FINISH_REEL_BINDER)
        .with_static_binder(&CHECK_REEL_COLLISIONS_BINDER)
        .with_static_binder(&START_REEL_UPSIDE_DOWN_BINDER)
}

#[detour(address = jc3gi::equipment::grappling_hook::GrapplingHook::StartReelUpsideDown_ADDRESS)]
fn start_reel_upside_down(hook: *mut GrapplingHook, holder: *const SharedPtr<Character>) {
    // SAFETY: the arrival's own arguments, on the character update that owns them.
    let local = unsafe { holder.as_ref() }
        .and_then(|h| unsafe { h.px.as_ref() })
        .is_some_and(|c| c.m_IsLocalCharacter);
    if local && hang_enabled() && hang_takes_over() {
        want_hang();
        return;
    }
    START_REEL_UPSIDE_DOWN.get().unwrap().call(hook, holder);
}

/// Whether the lock and its hang are on.
fn hang_enabled() -> bool {
    Config::lock_query(|c| {
        let lock = &c.controllers.grapple.lock;
        lock.enabled && lock.hang
    })
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
    if let Some(character) = local {
        if holds() {
            // The arrival skipped the reel's motion this tick; hand the lock's velocity on
            // instead.
            if let Some(velocity) = last_velocity() {
                unsafe { engine::set_wanted_velocity(character, velocity) };
            }
            return;
        }
        // A reel onto a helicopter arrives into the game's stunt hang under the airframe, which
        // holds the body inverted: hang on the line instead, as below a ceiling.
        let to_helicopter = unsafe { hook.as_ref() }
            .and_then(|h| unsafe { h.px.as_ref() })
            .is_some_and(|h| unsafe { engine::reeling_to_helicopter(h) });
        if to_helicopter && hang_enabled() && hang_takes_over() {
            want_hang();
            return;
        }
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
