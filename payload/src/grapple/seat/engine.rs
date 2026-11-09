use std::{mem::MaybeUninit, num::NonZeroUsize};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::{
        grappling_hook::{GrapplingHookState, GrapplingHookWire, WireState},
        wire_end::{HookTargetInfo, WireEndState},
    },
    types::{
        math::{Matrix4, Vector3},
        shared_ptr::SharedPtr,
    },
};

use crate::{
    controllers::{
        aim::{
            self,
            ray::Ray,
            raycast::{self, CastOptions},
        },
        physical_hands::game_object_of,
        weapons::hold::refs::{Strong, null_weak},
    },
    grapple::{
        hook::{self, CustomOwner, local},
        seat::Refusal,
    },
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum WireReading {
    Flying,
    Attached,
    Gone,
    Ready,
}

pub(super) unsafe fn fire(at: Vec3) -> Result<NonZeroUsize, Refusal> {
    let (character, hook) = unsafe { local() }.ok_or(Refusal::NoPlayer)?;
    let vehicle = unsafe { character.GetVehiclePtr() };
    if vehicle.is_null() {
        return Err(Refusal::NoPlayer);
    }
    if !unsafe { hook::claim_custom(CustomOwner::Seat) } {
        return Err(Refusal::Busy);
    }
    let mut info = unsafe { target_info(character, at) };
    unsafe {
        hook.FireHookCustom(&mut info, vehicle.cast(), false);
        info.m_TargetGameObject.Assign(&null_weak());
    }
    let wire = NonZeroUsize::new(hook.m_ActiveWire.px as usize);
    if hook.m_State != GrapplingHookState::GHS_CUSTOM_ACTIVE_WIRE || wire.is_none() {
        hook::release_custom(CustomOwner::Seat);
        return Err(Refusal::FireRefused);
    }
    let wire = wire.ok_or_else(|| {
        hook::release_custom(CustomOwner::Seat);
        Refusal::FireRefused
    })?;
    hook::own_custom(CustomOwner::Seat, wire);
    Ok(wire)
}

pub(super) unsafe fn read(wire: NonZeroUsize) -> WireReading {
    let Some(active) = (unsafe { active_wire(wire) }) else {
        hook::release_custom(CustomOwner::Seat);
        return if unsafe { ready() } {
            WireReading::Ready
        } else {
            WireReading::Gone
        };
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

pub(super) unsafe fn attachment_point(wire: NonZeroUsize) -> Option<Vec3> {
    let end = hook::attached_hook_end(unsafe { active_wire(wire) }?)?;
    let at = Vec3::from_array(end.m_WorldPositionT1.data);
    at.is_finite().then_some(at)
}

pub(super) unsafe fn tether(wire: NonZeroUsize, second: Option<Vec3>) -> bool {
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
    let shared = SharedPtr::<Character> {
        px: std::ptr::from_ref(character).cast_mut(),
        pn: holder.control_block(),
    };
    unsafe { hook.TetherActiveWireToVehicleCOM(&shared, vehicle) };
    if hook.m_ActiveWire.exists() {
        return false;
    }
    hook::release_custom(CustomOwner::Seat);
    let Some(at) = second else { return true };
    let Some(wire) = (unsafe { hook.m_Wires.as_slice() })
        .iter()
        .find(|candidate| candidate.px as usize == wire.get())
        .and_then(|wire| unsafe { wire.px.as_mut() })
    else {
        return false;
    };
    let Some(tuning) = (unsafe { hook.GetTuning().as_ref() }) else {
        return false;
    };
    let mut info = unsafe { target_info(character, at) };
    let start = hook::device_end(wire).m_WorldPositionT1;
    unsafe {
        wire.DestroyConstraint();
        wire.m_WireEnds[1].SetExtend(&start, tuning.m_GrappleFireSpeed, &mut info, false);
        info.m_TargetGameObject.Assign(&null_weak());
    }
    true
}

pub(super) unsafe fn cancel(wire: NonZeroUsize) {
    if unsafe { active_wire(wire) }.is_some()
        && let Some(hook) = unsafe { hook::local_hook() }
    {
        unsafe { hook.CancelActiveWire() };
    }
    hook::release_custom(CustomOwner::Seat);
}

unsafe fn ready() -> bool {
    unsafe { hook::local_hook() }.is_some_and(|hook| {
        hook.m_State == GrapplingHookState::GHS_INACTIVE
            && !hook.m_ActiveWire.exists()
            && !hook.m_AttachmentWire.exists()
            && !hook::firing(hook)
    })
}

unsafe fn target_info(character: &Character, at: Vec3) -> HookTargetInfo {
    let mut info = unsafe { MaybeUninit::<HookTargetInfo>::zeroed().assume_init() };
    info.m_TargetPosition = Vector3 {
        data: at.to_array(),
    };
    info.m_WasInRange = true;
    info.m_BoneIndex = -1;
    let Some(aim) = aim::status().grapple else {
        return info;
    };
    let Some(ray) = Ray::new(aim.ray.origin, at - aim.ray.origin) else {
        return info;
    };
    let options = unsafe { CastOptions::ignoring_rider(character) };
    let Some(hit) = (unsafe { raycast::cast(&ray, ray.origin.distance(at) + 0.25, &options) })
        .ok()
        .flatten()
        .filter(|hit| hit.position.distance(at) <= 0.5)
    else {
        return info;
    };
    let Some(object) = hit.object else {
        return info;
    };
    let object = game_object_of(object);
    let mut matrix = Matrix4::from(Mat4::IDENTITY);
    if !unsafe { (*object).GetTransform(&mut matrix) } {
        return info;
    }
    let inverse = Mat4::from(matrix).inverse();
    let local = inverse.transform_point3(hit.position);
    if !inverse.is_finite() || !local.is_finite() {
        return info;
    }
    unsafe {
        info.SetupTargetFromGameObject(
            object.cast(),
            &Vector3 {
                data: local.to_array(),
            },
        )
    };
    info
}

unsafe fn active_wire(wire: NonZeroUsize) -> Option<&'static GrapplingHookWire> {
    let hook = unsafe { hook::local_hook() }?;
    if hook.m_ActiveWire.px as usize != wire.get() {
        return None;
    }
    unsafe { hook.m_ActiveWire.px.as_ref() }
}
