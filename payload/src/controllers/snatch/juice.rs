//! The snatch's sounds and effects, all of them the grapple's own: the device's fire, the hook's
//! material impact where it bites the target, the reel's loop while the line pulls, and the line's
//! retract when it comes away. The custom fire plays no fire animation, and the hook's effects that
//! the game plays from the fire animation's track messages and from the reel's state therefore
//! never play for it on their own; the engine side is "The hook's sounds and effects" in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! The reel's loop belongs to the hook's state: [`GrapplingHook::UpdateLoopingEffects`] plays it
//! while the hook reels in and stops it otherwise, so a detour keeps it playing while the snatch's
//! line pulls ([`set_reeling`]) and hands it back to the game afterwards, which stops it softly.

use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    equipment::{
        attached_effects::AttachedEffectUsage, grappling_hook::GrapplingHook, wire_end::WireEnd,
    },
    physics::{CastRayInfo, GameMaterialId, PfxLayer},
    types::math::Vector3,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions, RayHit},
        },
        snatch::engine,
    },
    grapple::hook::local_hook,
};

/// The juice's hooks, aggregated by [`crate::controllers::snatch::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_LOOPING_EFFECTS_BINDER)
}

/// Keep the reel's loop playing on the local player's hook while `reeling`, or hand it back to the
/// hook's own state. Called from the input tick.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn set_reeling(reeling: bool) {
    let hook = if reeling {
        unsafe { local_hook() }.map_or(0, |hook| hook as *mut GrapplingHook as usize)
    } else {
        0
    };
    REELING.store(hook, Ordering::Release);
}

/// Play the hook's material impact where the snatch's hook bit `object`: a cast from `from` through
/// the hook's end on the custom wire `wire`, kept only when it meets `object`, hands the engine
/// the hit body's material. Nothing plays when the cast misses the target.
///
/// # Safety
///
/// The game thread, before the world update; `object` must be held alive.
pub(super) unsafe fn impact(wire: NonZeroUsize, object: NonZeroUsize, from: Vec3) {
    let Some(end) = (unsafe { engine::hook_end(wire) }) else {
        return;
    };
    let at = Vec3::from_array(end.m_WorldPositionT1.data);
    let reach = from.distance(at) + IMPACT_OVERSHOOT_M;
    let Some(ray) = Ray::new(from, at - from) else {
        return;
    };
    let options = CastOptions {
        ignore: [unsafe { engine::local_player_address() }, 0],
        see_through: true,
        material: false,
    };
    let end: *mut WireEnd = end;
    let play = |hit: RayHit, info: &mut CastRayInfo| {
        if hit.object != Some(object) {
            return false;
        }
        let position = vector(hit.position);
        let normal = vector(hit.normal);
        // SAFETY: the end is the live hook end of the hook's active wire, read this tick on the
        // game thread, and the info is the engine's own for this hit.
        unsafe { (*end).PlayImpactEffect(&position, &normal, info) };
        true
    };
    // The weapons lying in the world are met only on the vehicle layer, as the snatch's aim finds
    // them ([`crate::controllers::aim::grapple_weapons`]).
    let layer = PfxLayer::VEHICLE_RAYCAST as i32;
    let played = unsafe { raycast::cast_on_with(&ray, reach, &options, layer, play) };
    if !matches!(played, Ok(Some(true))) {
        tracing::debug!(target: "controllers", ?played, "snatch: the hook's impact found no target to play on");
    }
}

/// Play the line's retract from the hook's device: the snatch's line came away.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn retract() {
    let Some(hook) = (unsafe { local_hook() }) else {
        return;
    };
    let usage = hook.m_RetractingActiveWireEffectUsage;
    unsafe { play_once(hook, usage) };
}

/// Play the device's fire, the sound and muzzle effect a track message of the game's fire
/// animation plays from the hook's container ([`GrapplingHook::GRAPPLE_FIRE_USAGE`]).
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn fire() {
    let Some(hook) = (unsafe { local_hook() }) else {
        return;
    };
    let usage = unsafe {
        hook.m_AttachedEffects
            .FindAttachedEffectUsage(GrapplingHook::GRAPPLE_FIRE_USAGE)
    };
    let played = unsafe { play_once(hook, usage) };
    if !played {
        tracing::debug!(target: "controllers", found = !usage.is_null(), "snatch: the hook's fire did not play");
    }
}

/// The hook's local player hook the reel's loop is held on, by address, or zero.
static REELING: AtomicUsize = AtomicUsize::new(0);

/// How far past the hook's end the impact cast reaches, metres: the end sits on the target's
/// surface or inside it.
const IMPACT_OVERSHOOT_M: f32 = 0.5;

/// Play `usage` once on `hook`'s container, unless it loops, as the hook's own message playback
/// refuses to: a looping usage would play until it is stopped, which only its owner does. Returns
/// whether it plays.
unsafe fn play_once(hook: &mut GrapplingHook, usage: *mut AttachedEffectUsage) -> bool {
    if usage.is_null() {
        return false;
    }
    let effects = &mut hook.m_AttachedEffects;
    let material = GameMaterialId::EGameMaterialId_Default;
    if unsafe { effects.IsAttachedEffectLooping(usage, material) } {
        return false;
    }
    unsafe { effects.ActivateAttachedEffect(usage, true, material) }
}

#[detour(address = jc3gi::equipment::grappling_hook::GrapplingHook::UpdateLoopingEffects_ADDRESS)]
fn update_looping_effects(hook: *mut GrapplingHook) {
    let held = REELING.load(Ordering::Acquire);
    if held != 0 && held == hook as usize {
        // SAFETY: the engine's own live hook, in its post-camera update.
        let hook = unsafe { &mut *hook };
        if !hook.m_ActiveLoopingEffects[0] {
            let usage = hook.m_ReelingInEffectUsage;
            let material = GameMaterialId::EGameMaterialId_Default;
            // SAFETY: as above; a null usage does nothing.
            unsafe {
                hook.m_AttachedEffects
                    .ActivateAttachedEffect(usage, false, material)
            };
            hook.m_ActiveLoopingEffects[0] = true;
        }
        return;
    }
    UPDATE_LOOPING_EFFECTS.get().unwrap().call(hook);
}

fn vector(v: Vec3) -> Vector3 {
    Vector3 { data: v.to_array() }
}
