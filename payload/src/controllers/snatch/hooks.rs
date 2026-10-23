//! What the grapple button does with a weapon under the grapple's aim, which the aim sees only
//! through the scoped patch ([`crate::controllers::aim::grapple_weapons`]).
//!
//! - A hook fired at a weapon flies as a ghost wire ([`set_extend`]): the hook's casts never hit a
//!   weapon's body, so a plain hook would attach to whatever lies under or behind the gun; a ghost
//!   hook attaches to the gun itself. This covers the first end of a tap or a hold and the second
//!   end of a tether, so a gun tethers to a car, or two guns together, through the game's own
//!   tethers.
//! - A tap at a gun reels the gun in, not Rico ([`on_pre_sim_update`]): when the player's hook is
//!   about to start the reel on an attached line to a weapon (the button up, no tether pending),
//!   the wire is ended instead, and the input tick tosses the gun to the player's feet
//!   ([`take_toss`]).

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::{
    character::character::Character,
    equipment::{
        grappling_hook::{GrapplingHook, GrapplingHookState, WireState},
        weapon::WeaponBase,
        wire_end::{HookTargetInfo, WireEnd, WireEndState},
    },
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::hold::refs::{Strong, Weak};

/// The hooks, aggregated by [`crate::controllers::snatch::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SET_EXTEND_BINDER)
        .with_static_binder(&ON_PRE_SIM_UPDATE_BINDER)
}

/// Turn the hooks on or off, and say whether the grapple button is held, as the input tick last
/// injected it.
pub(super) fn publish(enabled: bool, button_held: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    BUTTON_HELD.store(button_held, Ordering::Relaxed);
}

/// The weapon a tap reeled in since the last call, to toss.
pub(super) fn take_toss() -> Option<Weak> {
    TOSS.lock().take()
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static BUTTON_HELD: AtomicBool = AtomicBool::new(false);
static TOSS: Mutex<Option<Weak>> = Mutex::new(None);

/// The latched object of `info` and a strong reference to it, while it lives, when it is a weapon.
///
/// # Safety
///
/// `info` must be a live target info.
unsafe fn latched_weapon(info: &HookTargetInfo) -> Option<Strong> {
    if info.m_TargetGameObject.px.is_null() {
        return None;
    }
    let strong = unsafe { Strong::lock(&info.m_TargetGameObject) }?;
    unsafe { (*strong.object()).IsType(WeaponBase::TYPE_ID()) }.then_some(strong)
}

/// An end starting toward its target, as a ghost wire when the target is a weapon.
#[detour(address = jc3gi::equipment::wire_end::WireEnd::SetExtend_ADDRESS)]
fn set_extend(
    end: *mut WireEnd,
    start: *const Vector3,
    speed: f32,
    info: *mut HookTargetInfo,
    ghost: bool,
) {
    let weapon = ENABLED.load(Ordering::Relaxed)
        && !ghost
        // SAFETY: the engine's own target info, live for the call.
        && unsafe { info.as_ref() }.is_some_and(|info| unsafe { latched_weapon(info) }.is_some());
    SET_EXTEND
        .get()
        .unwrap()
        .call(end, start, speed, info, ghost || weapon)
}

/// The hook's pre-simulation update, which ends the player's line to a weapon where the game would
/// start its reel.
#[detour(address = jc3gi::equipment::grappling_hook::GrapplingHook::OnPreSimUpdate_ADDRESS)]
fn on_pre_sim_update(hook: *mut GrapplingHook, dt: f32) {
    if ENABLED.load(Ordering::Relaxed) && !BUTTON_HELD.load(Ordering::Relaxed) {
        // SAFETY: the engine's own hook, in its update.
        unsafe { reel_in_weapon(hook) };
    }
    ON_PRE_SIM_UPDATE.get().unwrap().call(hook, dt)
}

/// When `hook` is the local player's and its active line holds a weapon, inactive with no tether
/// pending, end the line and leave the weapon to be tossed.
///
/// # Safety
///
/// `hook` must be null or live, in its update.
unsafe fn reel_in_weapon(hook: *mut GrapplingHook) {
    let Some(hook) = (unsafe { hook.as_mut() }) else {
        return;
    };
    let local = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .map(|c| c.m_Inventory.m_GrapplingHook.px);
    if local != Some(hook as *mut GrapplingHook)
        || hook.m_State != GrapplingHookState::GHS_INACTIVE
        || hook.m_CachedDualTetherFire
    {
        return;
    }
    let Some(wire) = (unsafe { hook.m_ActiveWire.as_ref() }) else {
        return;
    };
    let end = &wire.m_WireEnds[0];
    if wire.m_WireState == WireState::GHWS_INVALID
        || end.m_WireEndState != WireEndState::GWES_ATTACHED
    {
        return;
    }
    let Some(weapon) = (unsafe { latched_weapon(&end.m_TargetInfo) }) else {
        return;
    };
    let address = weapon.object() as usize - WeaponBase::GAME_OBJECT_BASE_OFFSET as usize;
    // SAFETY: the strong reference keeps the weapon, and its weak reference to itself, alive.
    let weak = unsafe { Weak::of(&(*(address as *const WeaponBase)).m_WeakThis) };
    unsafe { hook.CancelActiveWire() };
    *TOSS.lock() = Some(weak);
    tracing::info!(target: "controllers", "snatch: a tap of the grapple reeled a weapon in");
}
