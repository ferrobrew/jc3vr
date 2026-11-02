//! The snatch's sounds and effects, all of them the grapple's own: the hook's material impact where
//! it bites the target, the reel's loop while the line pulls, and the line's retract when it comes
//! away. The custom fire plays no fire animation, and the hook's effects that the game plays from
//! the fire animation's track messages and from the reel's state therefore never play for it on
//! their own; the engine side is "The hook's sounds and effects" in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! The reel's loop belongs to the hook's state: [`GrapplingHook::UpdateLoopingEffects`] plays it
//! while the hook reels in and stops it otherwise, so a detour keeps it playing while the snatch's
//! line pulls ([`set_reeling`]) and hands it back to the game afterwards, which stops it softly.

use std::{
    num::NonZeroUsize,
    sync::{
        Once,
        atomic::{AtomicUsize, Ordering},
    },
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    equipment::{
        attached_effects::AttachedEffectUsage, grappling_hook::GrapplingHook, wire_end::WireEnd,
    },
    hash::hashlittle,
    physics::{CastRayInfo, GameMaterialId, PfxLayer},
    types::math::Vector3,
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions, RayHit},
    },
    snatch::engine,
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
        unsafe { engine::local_hook() }.map_or(0, |hook| hook as *mut GrapplingHook as usize)
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
    let Some(hook) = (unsafe { engine::local_hook() }) else {
        return;
    };
    let usage = hook.m_RetractingActiveWireEffectUsage;
    unsafe { play_once(hook, usage) };
}

/// Play the hook's fire one-off, when the hook's effect container has one by any of the names the
/// fire animation's track message is likely to send; and, once, log every usage the container
/// holds, so a recorded session names the fire's.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn fire() {
    let Some(hook) = (unsafe { engine::local_hook() }) else {
        return;
    };
    LISTED.call_once(|| unsafe { list_usages(hook) });
    let found = FIRE_USAGE_NAMES.iter().find_map(|name| {
        let usage = unsafe {
            hook.m_AttachedEffects
                .FindAttachedEffectUsage(hashlittle(name.as_bytes()) as u32)
        };
        (!usage.is_null()).then_some((*name, usage))
    });
    if let Some((name, usage)) = found {
        tracing::debug!(target: "controllers", name, "snatch: the hook plays its fire usage");
        unsafe { play_once(hook, usage) };
    }
}

/// The hook's local player hook the reel's loop is held on, by address, or zero.
static REELING: AtomicUsize = AtomicUsize::new(0);
static LISTED: Once = Once::new();

/// How far past the hook's end the impact cast reaches, metres: the end sits on the target's
/// surface or inside it.
const IMPACT_OVERSHOOT_M: f32 = 0.5;

/// The usage names a grapple fire's track message may play from the hook's container, tried in
/// order. The container's other usages share this naming (`ReelingIn`, `Retracting`,
/// `ReleaseFromAttached`); which of these the fire uses, if any, is unverified.
const FIRE_USAGE_NAMES: [&str; 5] = ["Fire", "GrappleFire", "HookFire", "FireHook", "Shoot"];

/// The most usages [`list_usages`] reads.
const MAX_LISTED_USAGES: u16 = 64;

/// Play `usage` once on `hook`'s container, unless it loops: a looping usage would play until it
/// is stopped, which only its owner does.
unsafe fn play_once(hook: &mut GrapplingHook, usage: *mut AttachedEffectUsage) {
    if usage.is_null() {
        return;
    }
    let effects = &mut hook.m_AttachedEffects;
    let material = GameMaterialId::EGameMaterialId_Default;
    if unsafe { effects.IsAttachedEffectLooping(usage, material) } {
        return;
    }
    unsafe { effects.ActivateAttachedEffect(usage, true, material) };
}

/// Log the hashes of the usages in `hook`'s effect container.
unsafe fn list_usages(hook: &GrapplingHook) {
    let effects = &hook.m_AttachedEffects;
    let count = effects.m_UsageCount.min(MAX_LISTED_USAGES);
    if effects.m_UsageEntries.is_null() {
        return;
    }
    let hashes: Vec<String> = (0..usize::from(count))
        // SAFETY: the table holds `m_UsageCount` entries from its start.
        .map(|i| unsafe { (*effects.m_UsageEntries.add(i)).m_Hash })
        .map(|hash| format!("{hash:#010x}"))
        .collect();
    tracing::info!(target: "controllers", count = effects.m_UsageCount, ?hashes, "snatch: the hook's effect usages");
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
