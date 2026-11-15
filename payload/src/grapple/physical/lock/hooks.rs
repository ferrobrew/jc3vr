//! The reel's endings the lock holds off while it holds the line: the reel task's arrival
//! ([`NStateTask_ReelIn::FinishReel`]) and the hook's stuck check
//! ([`GrapplingHook::CheckReelCollisions`]). The lock's own endings (a release, a yank, a landing,
//! a break) take their place.
//!
//! The arrival's upside-down ending ([`GrapplingHook::StartReelUpsideDown`], the ceiling attach)
//! and the arrival onto a helicopter's hang entry (the game's stunt hang under the airframe; a
//! door-side arrival is the game's hijack and runs as the game's) are held off for
//! the local player while the hang is on and the lock's tick runs: the reel keeps running, and the
//! lock's next tick hangs the character below the hook instead (`lock::want_hang`). While the
//! lock's tick does not run (the bindings idle, as under the gamepad scheme), or just after it
//! refused a hang, the arrival is the game's (`lock::hang_takes_over`).
//!
//! While the lock holds, the reel's motion ([`NStateTask_ReelIn::UpdateMotion`]) turns the
//! character toward the line's level part rather than up the line, so a hook overhead hangs Rico
//! upright instead of pitching him to face it. The arrival skips that motion: inside about a metre
//! of the hook the reel's arrival test passes on every tick, and without the motion the lock's
//! velocity never reached the proxy, which froze Rico under ceilings. The arrival detour therefore
//! runs the motion itself with the lock's velocity, using the reel task's own instance from its
//! last motion.
//!
//! [`GrapplingHook::StartReelUpsideDown`]: jc3gi::equipment::grappling_hook::GrapplingHook::StartReelUpsideDown
//! [`NStateTask_ReelIn::UpdateMotion`]: jc3gi::equipment::grappling_hook::NStateTask_ReelIn::UpdateMotion
//! [`NStateTask_ReelIn::FinishReel`]: jc3gi::equipment::grappling_hook::NStateTask_ReelIn::FinishReel
//! [`GrapplingHook::CheckReelCollisions`]: jc3gi::equipment::grappling_hook::GrapplingHook::CheckReelCollisions

use std::ffi::c_void;

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::grappling_hook::{GrappleTuning, GrapplingHook},
    types::{math::Vector3, shared_ptr::SharedPtr},
};
use parking_lot::Mutex;
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
        .with_static_binder(&UPDATE_MOTION_BINDER)
}

#[detour(address = jc3gi::equipment::grappling_hook::NStateTask_ReelIn::UpdateMotion_ADDRESS)]
fn update_motion(
    character: *mut Character,
    target_dir: *const Vector3,
    wanted_velocity: *const Vector3,
    instance: *mut c_void,
    properties: *mut c_void,
    layer: i32,
    dt: f32,
) {
    // SAFETY: the reel task's own arguments, on the character update that owns them.
    let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
    if !local {
        UPDATE_MOTION.get().unwrap().call(
            character,
            target_dir,
            wanted_velocity,
            instance,
            properties,
            layer,
            dt,
        );
        return;
    }
    *LAST_MOTION.lock() = Some(Motion {
        character: character as usize,
        instance: instance as usize,
        properties: properties as usize,
        layer,
        dt,
    });
    let level = holds()
        .then(|| {
            // SAFETY: as above.
            let target = unsafe { target_dir.as_ref() }.map(|d| Vec3::from_array(d.data));
            unsafe { character.as_ref() }.and_then(|c| level_direction(c, target))
        })
        .flatten()
        .map(|d| Vector3 { data: d.to_array() });
    let target_dir = level.as_ref().map_or(target_dir, std::ptr::from_ref);
    UPDATE_MOTION.get().unwrap().call(
        character,
        target_dir,
        wanted_velocity,
        instance,
        properties,
        layer,
        dt,
    );
}

/// The reel task's arguments to its last motion, for the arrival's own motion while the lock
/// holds.
#[derive(Debug, Copy, Clone)]
struct Motion {
    character: usize,
    instance: usize,
    properties: usize,
    layer: i32,
    dt: f32,
}

static LAST_MOTION: Mutex<Option<Motion>> = Mutex::new(None);

/// Forget the reel task's last motion: its instance and properties belong to the reel that
/// recorded them, and a later reel's arrival must never run the motion with them. Called whenever
/// the reel or the lock may have ended: the lock's end, the bindings' stand-down, an input tick
/// that finds no reel carrying the character, and the game's own arrival.
pub(super) fn forget_motion() {
    *LAST_MOTION.lock() = None;
}

/// The direction a held line turns the character toward: the level part of `target` (the reel's
/// own direction along the line), or, for a line too near vertical to have one, the character's
/// own level facing. `None` when neither has a direction.
fn level_direction(character: &Character, target: Option<Vec3>) -> Option<Vec3> {
    let level = |v: Vec3| Vec3::new(v.x, 0.0, v.z).try_normalize();
    target
        .filter(|t| t.is_finite() && Vec3::new(t.x, 0.0, t.z).length() > LEVEL_MIN)
        .and_then(level)
        .or_else(|| {
            let (_, rotation, _) =
                Mat4::from(character.m_WorldMatrixT1).to_scale_rotation_translation();
            level(rotation * Vec3::NEG_Z)
        })
}

/// The least level share of the reel's direction that faces the character along it rather than
/// keeping its own facing.
const LEVEL_MIN: f32 = 0.25;

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
            // The arrival skipped the reel's motion this tick; run it with the lock's velocity,
            // so the proxy still moves, or failing the reel task's instance, hand the velocity on.
            if let Some(velocity) = last_velocity() {
                let motion = LAST_MOTION
                    .lock()
                    .filter(|m| m.character == std::ptr::from_mut(character) as usize);
                let direction = level_direction(character, None);
                match (motion, direction) {
                    (Some(motion), Some(direction)) => {
                        let wanted = Vector3 {
                            data: velocity.to_array(),
                        };
                        let direction = Vector3 {
                            data: direction.to_array(),
                        };
                        UPDATE_MOTION.get().unwrap().call(
                            character,
                            &raw const direction,
                            &raw const wanted,
                            motion.instance as *mut c_void,
                            motion.properties as *mut c_void,
                            motion.layer,
                            motion.dt,
                        );
                    }
                    _ => unsafe { engine::set_wanted_velocity(character, velocity) },
                }
            }
            return;
        }
        forget_motion();
        // A reel onto a helicopter's hang entry arrives into the game's stunt hang under the
        // airframe, which holds the body inverted: hang on the line instead, as below a ceiling.
        // A reel onto its door side is the game's hijack, and arrives as the game's.
        let live_hook = unsafe { hook.as_ref() }.and_then(|h| unsafe { h.px.as_ref() });
        let to_helicopter =
            live_hook.is_some_and(|h| unsafe { engine::reeling_to_helicopter_hang(h) });
        if to_helicopter && hang_enabled() && hang_takes_over() {
            want_hang();
            return;
        }
        // The skid's rail stands Rico on the airframe the door-side arrival lands him beside.
        if let Some(body) =
            live_hook.and_then(|h| unsafe { engine::reeling_to_helicopter_door_side(h) })
        {
            crate::controllers::vehicles::rail::note_airframe(body);
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
