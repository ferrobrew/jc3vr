//! The snatch's calls into the game: the custom fire of the local player's hook at a weapon and its
//! wire, and the weapon's body.

use std::{mem::MaybeUninit, num::NonZeroUsize, ptr};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::{
        grappling_hook::{GrapplingHook, GrapplingHookState, GrapplingHookWire, WireState},
        weapon::{WeaponBase, WeaponState},
        wire_end::{HookTargetInfo, WireEnd, WireEndState},
    },
    physics::PhysicsGameObject,
    types::{math::Vector3, shared_ptr::WeakPtrGameObject},
};

use crate::{
    controllers::{
        aim::gun,
        snatch::{Refusal, sling_hooks},
        weapons::{engine, grenade},
    },
    grapple::hook::{self, local_hook},
};

/// Why the hook cannot fire now, if it cannot: a line is out, or a fire or a reel is under way.
pub(super) fn busy(hook: &GrapplingHook) -> Option<Refusal> {
    if hook.m_ActiveWire.exists() || hook.m_AttachmentWire.exists() {
        return Some(Refusal::LineOut);
    }
    if hook::firing(hook) {
        return Some(Refusal::HookBusy);
    }
    (hook.m_State != GrapplingHookState::GHS_INACTIVE).then_some(Refusal::HookBusy)
}

/// Fire the hook as a ghost wire at the physics game object `object`, latched at `local` in the
/// object's space with `at` its world position now, and return the wire it fired.
///
/// # Safety
///
/// `object` must be a live physics game object held alive, on the game thread before the world
/// update.
pub(super) unsafe fn fire(
    object: NonZeroUsize,
    local: Vec3,
    at: Vec3,
) -> Result<NonZeroUsize, Refusal> {
    let hook = unsafe { local_hook() }.ok_or(Refusal::NoPlayer)?;
    if let Some(refusal) = busy(hook) {
        return Err(refusal);
    }
    // Every field is a pointer, an integer, a flag, or a float, so all-zero is a valid value.
    let mut info = unsafe { MaybeUninit::<HookTargetInfo>::zeroed().assume_init() };
    info.m_TargetPosition = vector(at);
    info.m_WasInRange = true;
    info.m_BoneIndex = NO_BONE;
    let game_object =
        (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *mut _;
    unsafe {
        info.SetupTargetFromGameObject(game_object, &vector(local));
        hook.FireHookCustom(&mut info, ptr::null_mut(), true);
        // The fire copied the target info into the wire's end; this copy's weak reference is ours.
        info.m_TargetGameObject.Assign(&null_weak());
    }
    let wire = NonZeroUsize::new(hook.m_ActiveWire.px as usize).ok_or(Refusal::FireRefused)?;
    if hook.m_State != GrapplingHookState::GHS_CUSTOM_ACTIVE_WIRE {
        return Err(Refusal::FireRefused);
    }
    Ok(wire)
}

/// What the custom wire `wire` is doing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum WireReading {
    /// The hook is still flying.
    Flying,
    /// The hook holds its target.
    Attached,
    /// The wire is no longer the hook's active wire, or its hook end is retracting or dead: it
    /// missed, ran out of line, or the game ended it.
    Gone,
}

/// Read the custom wire `wire`, with the longest it may run, metres.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn read_wire(wire: NonZeroUsize) -> (WireReading, Option<f32>) {
    let Some(active) = (unsafe { active_wire(wire) }) else {
        return (WireReading::Gone, None);
    };
    let reading = if active.m_WireState == WireState::GHWS_INVALID {
        WireReading::Gone
    } else {
        let end = hook::hook_end(active);
        match end.m_WireEndState {
            WireEndState::GWES_ATTACHED if !end.m_PointProxy.is_null() => WireReading::Attached,
            WireEndState::GWES_EXTENDING | WireEndState::GWES_ATTACHED => WireReading::Flying,
            _ => WireReading::Gone,
        }
    };
    (reading, Some(active.m_MaxLength))
}

/// End the custom wire `wire`, while it is still the hook's active wire.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn cancel(wire: NonZeroUsize) {
    if unsafe { active_wire(wire) }.is_none() {
        return;
    }
    if let Some(hook) = unsafe { local_hook() } {
        unsafe { hook.CancelActiveWire() };
    }
}

/// The owned native wire's device proxy. No wire address is dereferenced after ownership is lost.
pub(super) unsafe fn device_proxy(wire: NonZeroUsize) -> Option<usize> {
    let wire = unsafe { active_wire(wire) }?;
    NonZeroUsize::new(hook::device_end(wire).m_PointProxy as usize).map(NonZeroUsize::get)
}

/// Remove a native rope before substituting the same tension model for both target categories.
pub(super) unsafe fn remove_constraint(wire: NonZeroUsize) {
    if unsafe { active_wire(wire) }.is_some() {
        unsafe { (&mut *(wire.get() as *mut GrapplingHookWire)).DestroyConstraint() };
    }
}

/// The hook end of the custom wire `wire`, while it is the hook's active wire.
///
/// # Safety
///
/// The game thread, before the world update; the end is not kept past the tick.
pub(super) unsafe fn hook_end(wire: NonZeroUsize) -> Option<&'static mut WireEnd> {
    unsafe { active_wire(wire) }?;
    let wire = unsafe { &mut *(wire.get() as *mut GrapplingHookWire) };
    Some(hook::hook_end_mut(wire))
}

/// Exclude the native rope from the custom wire `wire` from now on: its constraint is removed and
/// neither made again nor tensioned while the exclusion stands ([`sling_hooks::exclude`]), so the
/// snatch's tether is the one force on the target.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn exclude_rope(wire: NonZeroUsize) {
    sling_hooks::exclude(Some(wire.get()));
    unsafe { remove_constraint(wire) };
}

/// The weapon at `address`, while it lies in the world, with its first gun's body.
///
/// # Safety
///
/// `address` must be a weapon held alive, on the game thread before the world update.
pub(super) unsafe fn dropped(
    address: NonZeroUsize,
) -> Result<(&'static mut WeaponBase, u32), Refusal> {
    let weapon = unsafe { &mut *(address.get() as *mut WeaponBase) };
    // Read as the raw value: a state the enum does not name is not a dropped one.
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    if state != WeaponState::E_WS_DROPPED as i32 {
        return Err(Refusal::NotDropped);
    }
    let body = unsafe { gun::guns(weapon) }
        .next()
        .flatten()
        .and_then(|gun| unsafe { gun.m_PfxInstance.as_ref() })
        .map(|instance| instance.m_BodyId)
        .ok_or(Refusal::NoBody)?;
    Ok((weapon, body))
}

/// What a hand grenade at `address` is to the snatch: `None` for a weapon that is not a hand
/// grenade, `Some(true)` for a finite one lying in the world, which the snatch yanks in and the
/// catch takes as the grenade it is ([`grenade::finite_world_pickup`]), and `Some(false)` for a
/// grenade on a display, which stays where it is for the hand in reach to take.
///
/// # Safety
///
/// `address` must be a weapon held alive, on the game thread.
pub(super) unsafe fn hand_grenade(address: NonZeroUsize) -> Option<bool> {
    let weapon = unsafe { &*(address.get() as *const WeaponBase) };
    unsafe { engine::is_hand_grenade(weapon) }.then(|| grenade::finite_world_pickup(weapon))
}

/// Where the local player stands, world space.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn player_position() -> Option<Vec3> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let position = glam::Mat4::from(character.m_WorldMatrixT1)
        .w_axis
        .truncate();
    position.is_finite().then_some(position)
}

/// A target info's bone for an object that is not a character.
const NO_BONE: i32 = -1;

/// The local player's active wire, when it is `wire`.
unsafe fn active_wire(wire: NonZeroUsize) -> Option<&'static GrapplingHookWire> {
    let hook = unsafe { local_hook() }?;
    if hook.m_ActiveWire.px as usize != wire.get() {
        return None;
    }
    unsafe { hook.m_ActiveWire.px.as_ref() }
}

fn vector(v: Vec3) -> Vector3 {
    Vector3 { data: v.to_array() }
}

fn null_weak() -> WeakPtrGameObject {
    WeakPtrGameObject {
        px: ptr::null_mut(),
        pn: ptr::null_mut(),
    }
}
