//! The lock's engine side: the local player's hook and its far end, the far end's point, motion,
//! and body, the character's mass and support, and the line let go of. The engine side is in "The
//! reel's motion and endings" in `docs/engine/gameplay/grapple-pipeline.md`.

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::{
        grapple_point_proxy::{AttachmentObjectInfo, GrapplePointProxy},
        grappling_hook::{GrapplingHook, GrapplingHookState, WireState},
        wire_end::WireEndState,
    },
    physics::{PfxCharacterInstance, hknpCharacterSurfaceInfo},
    types::math::Vector3,
};

use crate::controllers::physical_hands::{self, BodyState};

/// The local player's character and hook.
///
/// # Safety
///
/// The game thread.
pub(super) unsafe fn local() -> Option<(&'static mut Character, &'static mut GrapplingHook)> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_mut() }?;
    let hook = unsafe { character.m_Inventory.m_GrapplingHook.px.as_mut() }?;
    Some((character, hook))
}

/// The far end's proxy of `hook`'s active wire while the hook reels in along it, the far end is
/// attached, and what it is attached to is live.
///
/// # Safety
///
/// The game thread; `hook` must be live.
pub(super) unsafe fn reeling_far_end(hook: &GrapplingHook) -> Option<&GrapplePointProxy> {
    unsafe { attached_far_end(hook) }.filter(|proxy| unsafe { proxy.IsOk() })
}

/// The far end's proxy of `hook`'s active wire while the hook reels in along it and the far end is
/// attached, live or not.
///
/// # Safety
///
/// The game thread; `hook` must be live.
pub(super) unsafe fn attached_far_end(hook: &GrapplingHook) -> Option<&GrapplePointProxy> {
    if hook.m_State != GrapplingHookState::GHS_REELING_IN {
        return None;
    }
    let wire = unsafe { hook.m_ActiveWire.px.as_ref() }?;
    if wire.m_WireState == WireState::GHWS_INVALID {
        return None;
    }
    let end = &wire.m_WireEnds[0];
    if end.m_WireEndState != WireEndState::GWES_ATTACHED {
        return None;
    }
    unsafe { end.m_PointProxy.as_ref() }
}

/// Whether the lock may hold a far end attached to `kind`: anything but a character, whose own
/// grapple handling the lock leaves alone.
pub(super) fn lockable_kind(kind: AttachmentObjectInfo) -> bool {
    matches!(
        kind,
        AttachmentObjectInfo::AOI_Vehicle
            | AttachmentObjectInfo::AOI_Static
            | AttachmentObjectInfo::AOI_Generic
    )
}

/// The far end's world point and its velocity there, or `None` when the attached thing is gone.
///
/// # Safety
///
/// The game thread; `proxy` must be live.
pub(super) unsafe fn point(proxy: &GrapplePointProxy) -> Option<(Vec3, Vec3)> {
    let mut position = Vector3 { data: [0.0; 3] };
    let mut normal = Vector3 { data: [0.0; 3] };
    if !unsafe { proxy.GetVisualPositionAndNormalWS(&mut position, &mut normal) } {
        return None;
    }
    let mut velocity = Vector3 { data: [0.0; 3] };
    unsafe { proxy.GetObjectAttachPointVelocity(&mut velocity) };
    let (position, velocity) = (
        Vec3::from_array(position.data),
        Vec3::from_array(velocity.data),
    );
    (position.is_finite() && velocity.is_finite()).then_some((position, velocity))
}

/// The dynamic body behind the far end, with its owner and state, or `None` for static geometry
/// or a body that is not dynamic.
///
/// # Safety
///
/// The game thread, outside the physics step; `proxy` must be live.
pub(super) unsafe fn far_body(proxy: &GrapplePointProxy) -> Option<(u32, NonZeroUsize, BodyState)> {
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetRigidBodyForConstraint(&mut body) };
    if body == PfxCharacterInstance::INVALID_BODY_ID {
        return None;
    }
    let (owner, state) = unsafe { physical_hands::line_body(body) }?;
    Some((body, owner, state))
}

/// The character's position, as the reel task reads it.
pub(super) fn position(character: &Character) -> Vec3 {
    Mat4::from(character.m_WorldMatrixT1).w_axis.truncate()
}

/// The character's mass: its proxy body's.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(super) unsafe fn rider_mass_kg(character: &Character) -> Option<f32> {
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetCharacterRigidBodyId(&mut body) };
    if body == PfxCharacterInstance::INVALID_BODY_ID {
        return None;
    }
    unsafe { physical_hands::body_mass_kg(body) }.filter(|m| *m > 0.0)
}

/// Whether the character's proxy stands on a surface.
///
/// # Safety
///
/// The game thread.
pub(super) unsafe fn supported(character: &Character) -> bool {
    unsafe { character.m_PfxCharacter.px.as_ref() }.is_some_and(|proxy| {
        proxy.m_GroundInformation.m_supportedState == hknpCharacterSurfaceInfo::SUPPORTED
    })
}

/// Let go of the line: retract the active wire, which the reel task answers on its next update by
/// cancelling the reel into the fall, the character keeping its velocity.
///
/// # Safety
///
/// The game thread, before the world update; `hook` must be live.
pub(super) unsafe fn let_go(hook: &mut GrapplingHook) {
    unsafe { hook.RetractActiveWire() };
}

/// Give the character `velocity` as its wanted world velocity, as the reel task's motion does.
///
/// # Safety
///
/// The character update; `character` must be live.
pub(super) unsafe fn set_wanted_velocity(character: &mut Character, velocity: Vec3) {
    let velocity = Vector3 {
        data: velocity.to_array(),
    };
    unsafe { character.SetWantedWorldVelocity(&velocity) };
}
