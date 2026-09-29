//! The wingsuit's head look-steer removed, reusing the parachute's toggle
//! ([`MovementConfig::suppress_parachute_look_steer`](crate::hooks::input::MovementConfig::suppress_parachute_look_steer)).
//!
//! The air movement core adds a camera look-steer to the stick's steering: the camera's pitch and
//! yaw against the character's, shaped by the look-steer tuning. In VR the camera follows the head,
//! so looking around steers the wingsuit. The look-steer is computed by its own side-effect-free
//! function, [`NAirMovement_UpdateStdLookSteering`], which the core calls for the local player and
//! whose output it adds into its steering; a post-call detour that zeroes the output removes the
//! head's steering and leaves the stick's. The wingsuit task's own update is never touched.
//!
//! The core also serves the falls, whose camera steering the same detour would remove, so it acts
//! only while the mode detector reads the wingsuit or the parachute, under the controller scheme,
//! for a head-decoupled local player. The parachute's own look-steer is removed by its own
//! detour ([`crate::hooks::input::parachute`]).
//!
//! [`NAirMovement_UpdateStdLookSteering`]: jc3gi::input::parachute::NAirMovement_UpdateStdLookSteering

use std::sync::atomic::{AtomicU64, Ordering};

use detours_macro::detour;
use jc3gi::{
    character::character::Character, input::parachute::CharacterAirMovementSettings,
    types::math::Vector3,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        mode::{self, AirborneVariant, Mode},
        scheme,
    },
    hooks::input::locomotion::head_decoupled,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_STD_LOOK_STEERING_BINDER)
}

/// How often the look-steer was removed, for the debug UI.
pub fn suppressions() -> u64 {
    SUPPRESSIONS.load(Ordering::Relaxed)
}

#[detour(address = jc3gi::input::parachute::NAirMovement_UpdateStdLookSteering_ADDRESS)]
fn update_std_look_steering(
    out: *mut Vector3,
    target_steering: *const Vector3,
    char_pitch: f32,
    char_yaw: f32,
    settings: *const CharacterAirMovementSettings,
) {
    UPDATE_STD_LOOK_STEERING.get().unwrap().call(
        out,
        target_steering,
        char_pitch,
        char_yaw,
        settings,
    );
    if !should_suppress() {
        return;
    }
    if let Some(out) = unsafe { out.as_mut() } {
        out.data = [0.0; 3];
        SUPPRESSIONS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Whether to remove the look-steer: the toggle is on, the controller scheme is in effect, the
/// player flies the wingsuit or the parachute, and the local player's head is decoupled.
fn should_suppress() -> bool {
    let (enabled, toggle) = Config::lock_query(|c| {
        (
            c.controllers.enabled,
            c.movement.suppress_parachute_look_steer,
        )
    });
    if !(enabled && toggle && scheme::is_controllers()) {
        return false;
    }
    let flying = matches!(
        mode::current().mode,
        Mode::Airborne(AirborneVariant::Wingsuit | AirborneVariant::Parachute)
    );
    // SAFETY: the air movement runs in the character's state update, which the local player's
    // record outlives.
    flying && unsafe { Character::GetLocalPlayerCharacter().as_ref() }.is_some_and(head_decoupled)
}

static SUPPRESSIONS: AtomicU64 = AtomicU64::new(0);
