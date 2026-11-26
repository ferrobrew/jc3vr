//! A grapple reel's speed carried into the fall and the wingsuit after it.
//!
//! The falls and the wingsuit move the character through the air physics
//! ([`NAirMovement_UpdateAirPhysics`]), which caps the speed at its settings' `m_MaxVelocity`
//! (and, without steering, the horizontal speed at `m_MaxVelocityXZ`) by rescaling the whole
//! velocity. A reel out of the air ends in a fall whose cap is far below a reel's speed, so on the
//! 14 November take every component of a reel's 30 to 36 m/s dropped to about 0.43 of itself in one
//! tick, and the wingsuit opened from what was left. A velocity written before the update is capped
//! again in it, and one written after is overwritten by the next tick's.
//!
//! So when a reel or the grapple lock lets the local player go in the air with a speed above the
//! fall's, the cap itself is raised to that speed and eased back to the game's over
//! [`CARRY_S`]: the fall and the suit then keep the reel's speed and lose it as smoothly as the
//! drag and the cap allow. Gravity, drag, lift, and steering stay the game's.
//!
//! [`NAirMovement_UpdateAirPhysics`]: jc3gi::input::parachute::NAirMovement_UpdateAirPhysics

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use body_rig::math::smooth::smoothstep;
use detours_macro::detour;
use jc3gi::{
    character::character::Character, input::parachute::CharacterAirMovementSettings,
    types::math::Vector2,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::hooks::input::{locomotion::head_decoupled, tuning::INPUT_HOOKS};

pub fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_AIR_PHYSICS_BINDER)
}

/// Carry `speed_mps`, the speed a reel or the grapple lock let the local player go at, into the
/// air physics' cap for the next input tuning's `air.carry_s`. A speed that is not a positive
/// number carries nothing.
pub fn carry_speed(speed_mps: f32) {
    if speed_mps.is_finite() && speed_mps > 0.0 {
        *CARRY.lock() = Some(Carry {
            speed_mps,
            since: Instant::now(),
        });
        tracing::info!(
            target: "controllers",
            speed_mps,
            "air: carrying a reel's speed into the air physics' cap"
        );
    }
}

/// How many air physics updates ran with a raised cap, for the debug UI.
pub fn carried_updates() -> u64 {
    CARRIED_UPDATES.load(Ordering::Relaxed)
}

#[detour(address = jc3gi::input::parachute::NAirMovement_UpdateAirPhysics_ADDRESS)]
fn update_air_physics(
    character: *mut Character,
    dt: f32,
    input: *const Vector2,
    settings: *const CharacterAirMovementSettings,
) {
    // SAFETY: the movement task passes its live character and settings for the call.
    let local =
        unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter && head_decoupled(c));
    let raised = local
        .then(carried_cap)
        .flatten()
        .and_then(|cap| unsafe { settings.as_ref() }.map(|s| (cap, *s)))
        .filter(|(cap, s)| *cap > s.m_MaxVelocity)
        .map(|(cap, mut copy)| {
            copy.m_MaxVelocityXZ = copy.m_MaxVelocityXZ.max(cap);
            copy.m_MaxVelocity = cap;
            copy
        });
    let settings_in = match raised.as_ref() {
        Some(copy) => {
            CARRIED_UPDATES.fetch_add(1, Ordering::Relaxed);
            std::ptr::from_ref(copy)
        }
        None => settings,
    };
    UPDATE_AIR_PHYSICS
        .get()
        .unwrap()
        .call(character, dt, input, settings_in);
}

/// The cap the carried speed asks for now, easing from the carried speed to nothing (the game's
/// own cap) over the input tuning's `air.carry_s`; `None` once the carry has run out.
fn carried_cap() -> Option<f32> {
    let mut carry = CARRY.lock();
    let current = (*carry)?;
    let elapsed = current.since.elapsed().as_secs_f32();
    let carry_s = INPUT_HOOKS.get().air.carry_s;
    if elapsed >= carry_s {
        *carry = None;
        return None;
    }
    Some(current.speed_mps * (1.0 - smoothstep(elapsed / carry_s)))
}

/// A carried speed and when it began.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Carry {
    speed_mps: f32,
    since: Instant,
}

static CARRY: Mutex<Option<Carry>> = Mutex::new(None);

static CARRIED_UPDATES: AtomicU64 = AtomicU64::new(0);
