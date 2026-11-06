//! The seat grapple's calls into the game: the hook fired as a scripted wire from the seat, its
//! wire read, tethered to the vehicle, or called back. The steps are the game's own vehicle
//! grapple's ([`VehicleGrappleHook::PreSimUpdate`]).
//!
//! [`VehicleGrappleHook::PreSimUpdate`]: jc3gi::equipment::grappling_hook::VehicleGrappleHook::PreSimUpdate

use std::{mem::MaybeUninit, num::NonZeroUsize};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::{
        grappling_hook::{GrapplingHook, GrapplingHookState, GrapplingHookWire, WireState},
        wire_end::{HookTargetInfo, WireEndState},
    },
    types::{math::Vector3, shared_ptr::SharedPtr},
};

use crate::{
    controllers::weapons::hold::refs::Strong,
    grapple::{
        hook::{self, local},
        seat::Refusal,
    },
};

/// What the seat grapple's wire is doing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum WireReading {
    /// The hook is still flying.
    Flying,
    /// The hook holds what it hit.
    Attached,
    /// The wire is no longer the hook's active wire, or its hook end is retracting or dead.
    Gone,
}

/// Fire the local player's hook from the seat at `at`, ignoring the vehicle, as the game's vehicle
/// grapple does: a target position in range with no game object and no bone, not a ghost wire.
/// The hook flies there and holds whatever it meets.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn fire(at: Vec3) -> Result<NonZeroUsize, Refusal> {
    let (character, hook) = unsafe { local() }.ok_or(Refusal::NoPlayer)?;
    let vehicle = unsafe { character.GetVehiclePtr() };
    if vehicle.is_null() {
        return Err(Refusal::NoPlayer);
    }
    if busy(hook) {
        return Err(Refusal::Busy);
    }
    // Every field is a pointer, an integer, a flag, or a float, so all-zero is a valid value, and
    // with no game object set there is no reference to release afterwards.
    let mut info = unsafe { MaybeUninit::<HookTargetInfo>::zeroed().assume_init() };
    info.m_TargetPosition = Vector3 {
        data: at.to_array(),
    };
    info.m_WasInRange = true;
    info.m_BoneIndex = NO_BONE;
    unsafe { hook.FireHookCustom(&mut info, vehicle.cast(), false) };
    let wire = NonZeroUsize::new(hook.m_ActiveWire.px as usize).ok_or(Refusal::FireRefused)?;
    if hook.m_State != GrapplingHookState::GHS_CUSTOM_ACTIVE_WIRE {
        return Err(Refusal::FireRefused);
    }
    Ok(wire)
}

/// Read the seat grapple's wire `wire`.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn read(wire: NonZeroUsize) -> WireReading {
    let Some(active) = (unsafe { active_wire(wire) }) else {
        return WireReading::Gone;
    };
    if active.m_WireState == WireState::GHWS_INVALID {
        return WireReading::Gone;
    }
    let end = hook::hook_end(active);
    match end.m_WireEndState {
        WireEndState::GWES_ATTACHED if !end.m_PointProxy.is_null() => WireReading::Attached,
        WireEndState::GWES_EXTENDING | WireEndState::GWES_ATTACHED => WireReading::Flying,
        _ => WireReading::Gone,
    }
}

/// Turn the wire `wire`, whose hook holds, into a tether between what it holds and the vehicle,
/// returning whether the hook took it.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn tether(wire: NonZeroUsize) -> bool {
    if unsafe { active_wire(wire) }.is_none() {
        return false;
    }
    let Some((character, hook)) = (unsafe { local() }) else {
        return false;
    };
    let vehicle = unsafe { character.GetVehiclePtr() };
    if vehicle.is_null() {
        return false;
    }
    let Some(holder) = (unsafe { Strong::lock(&hook.m_Holder) }) else {
        return false;
    };
    // A non-owning copy of the holder's reference, valid while `holder` is held: the hook's holder
    // reference is to the character, so its object pointer is the character's.
    let shared = SharedPtr::<Character> {
        px: holder.object().cast(),
        pn: holder.control_block(),
    };
    unsafe { hook.TetherActiveWireToVehicleCOM(&shared, vehicle) };
    drop(holder);
    !hook.m_ActiveWire.exists()
}

/// Call back the seat grapple's wire `wire`, while it is still the hook's active wire.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn cancel(wire: NonZeroUsize) {
    if unsafe { active_wire(wire) }.is_none() {
        return;
    }
    if let Some(hook) = unsafe { hook::local_hook() } {
        unsafe { hook.CancelActiveWire() };
    }
}

/// Whether the hook cannot fire now: a line is out, or a fire is under way. The hook's state alone
/// is not read: it can keep naming the last reel's ending with no line out.
fn busy(hook: &GrapplingHook) -> bool {
    hook.m_ActiveWire.exists() || hook.m_AttachmentWire.exists() || hook::firing(hook)
}

/// `wire`, while it is the local player's hook's active wire.
///
/// # Safety
///
/// The game thread; the wire is not kept past the tick.
unsafe fn active_wire(wire: NonZeroUsize) -> Option<&'static GrapplingHookWire> {
    let hook = unsafe { hook::local_hook() }?;
    if hook.m_ActiveWire.px as usize != wire.get() {
        return None;
    }
    unsafe { hook.m_ActiveWire.px.as_ref() }
}

/// A target info's bone index for a target with no bone.
const NO_BONE: i32 = -1;
