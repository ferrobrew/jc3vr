//! The snatch's calls into the game: the custom fire of the local player's hook at a weapon and its
//! wire, and the weapon's body.

use std::{mem::MaybeUninit, num::NonZeroUsize, ptr};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::{
        grappling_hook::{GrapplingHook, GrapplingHookState, GrapplingHookWire, WireState},
        weapon::{WeaponBase, WeaponState},
        wire_end::{HookTargetInfo, WireEnd, WireEndState},
    },
    physics::{CastRayInfo, CastRayInfoBase, PfxCharacterInstance},
    types::{
        math::{Matrix4, Vector3},
        shared_ptr::SharedPtr,
    },
};
use jc3vr_body::controllers::{
    aim::gun,
    physics,
    physics::{
        bodies::game_object_of,
        characters::character_position,
        grapple_hook as hook,
        grapple_hook::{BodyHit, CustomOwner, local_hook},
        local_player::LocalPlayer,
        refs::{Strong, null_weak},
    },
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_weapons::controllers::weapons::{engine, grenade};

use crate::controllers::snatch::{Refusal, config::SNATCH, sling_hooks};

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

/// Fire the hook as a ghost wire at the physics game object `object` and its selected body hit,
/// and return the wire it fired and the body hit the attachment proxy verified.
///
/// # Safety
///
/// `object` must be a live physics game object held alive, on the game thread before the world
/// update.
pub(super) unsafe fn fire(
    object: NonZeroUsize,
    hit: BodyHit,
) -> Result<(NonZeroUsize, BodyHit), Refusal> {
    let at = unsafe { hit.point() }.ok_or(Refusal::Gone)?;
    let (wire, attached) = unsafe { fire_target(object, at, Some(hit)) }?;
    let attached = attached.ok_or(Refusal::FireRefused)?;
    Ok((wire, attached))
}

pub(super) unsafe fn fire_object(object: NonZeroUsize, at: Vec3) -> Result<NonZeroUsize, Refusal> {
    unsafe { fire_target(object, at, None) }.map(|(wire, _)| wire)
}

unsafe fn fire_target(
    object: NonZeroUsize,
    at: Vec3,
    hit: Option<BodyHit>,
) -> Result<(NonZeroUsize, Option<BodyHit>), Refusal> {
    let hook = unsafe { local_hook() }.ok_or(Refusal::NoPlayer)?;
    if let Some(refusal) = busy(hook) {
        return Err(refusal);
    }
    // Every field is a pointer, an integer, a flag, or a float, so all-zero is a valid value.
    let mut info = unsafe { MaybeUninit::<HookTargetInfo>::zeroed().assume_init() };
    info.m_TargetPosition = vector(at);
    info.m_WasInRange = true;
    info.m_BoneIndex = NO_BONE;
    let local = unsafe { object_local(object, at) }.ok_or(Refusal::Gone)?;
    let game_object = game_object_of(object).cast();
    if !unsafe { hook::claim_custom(CustomOwner::Snatch) } {
        return Err(Refusal::HookBusy);
    }
    unsafe {
        info.SetupTargetFromGameObject(game_object, &vector(local));
        hook.FireHookCustom(&mut info, ptr::null_mut(), true);
        // The fire copied the target info into the wire's end; this copy's weak reference is ours.
        info.m_TargetGameObject.Assign(&null_weak());
    }
    let wire = NonZeroUsize::new(hook.m_ActiveWire.px as usize);
    if hook.m_State != GrapplingHookState::GHS_CUSTOM_ACTIVE_WIRE || wire.is_none() {
        hook::release_custom(CustomOwner::Snatch);
        return Err(Refusal::FireRefused);
    }
    let wire = wire.unwrap();
    hook::own_custom(CustomOwner::Snatch, wire);
    let attached = match hit {
        Some(hit) => match unsafe { attach_body(hook, wire, hit) } {
            Some(hit) => Some(hit),
            None => {
                unsafe { cancel(wire) };
                return Err(Refusal::FireRefused);
            }
        },
        None => None,
    };
    if attached.is_some() {
        // The mod owns the line's force from the fire: the native rope must not make a constraint
        // or tension that can break the wire before the bite takes it off.
        unsafe { exclude_rope(wire) };
    }
    Ok((wire, attached))
}

/// How the engine's attachment proxy compares with the body hit the snatch selected.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ProxyMatch {
    /// The proxy holds the selected body at the selected point.
    Selected,
    /// The proxy holds another body of the same object: the object's own body.
    ObjectBody,
    /// The proxy holds no body of the object, or the point is not the one asked for.
    Rejected,
}

/// How the proxy `proxy_body` at `offset_m` from the selected point compares with the selected
/// body, given whether the proxy's body still belongs to the object.
fn verify_proxy(
    selected_body: u32,
    proxy_body: u32,
    offset_m: f32,
    tolerance_m: f32,
    proxy_body_owned: bool,
) -> ProxyMatch {
    if !offset_m.is_finite() || offset_m > tolerance_m.max(0.0) {
        return ProxyMatch::Rejected;
    }
    if proxy_body == selected_body {
        ProxyMatch::Selected
    } else if proxy_body_owned {
        ProxyMatch::ObjectBody
    } else {
        ProxyMatch::Rejected
    }
}

/// What the line's live proxy says about the body it holds: the selected body while the proxy
/// holds it, the object's own body when the proxy holds another one of the object's, or lost.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum AnchorCheck {
    /// The selected body still belongs to the object, and the proxy holds it.
    Selected,
    /// The proxy holds another body of the object: follow that body instead.
    ObjectBody,
    /// Neither the selected body nor the proxy's body belongs to the object.
    Lost,
}

/// What the line should pull, from whether the selected body still belongs to the object and what
/// the live proxy `proxy` holds (its body and whether that body still belongs to the object).
pub(super) fn check_anchor(
    selected_owned: bool,
    selected_body: u32,
    proxy: Option<(u32, bool)>,
) -> AnchorCheck {
    match proxy {
        Some((body, _)) if body == selected_body && selected_owned => AnchorCheck::Selected,
        Some((body, _)) if body == selected_body => AnchorCheck::Lost,
        Some((_, true)) => AnchorCheck::ObjectBody,
        Some((_, false)) => AnchorCheck::Lost,
        None if selected_owned => AnchorCheck::Selected,
        None => AnchorCheck::Lost,
    }
}

unsafe fn attach_body(
    hook: &mut GrapplingHook,
    wire: NonZeroUsize,
    hit: BodyHit,
) -> Option<BodyHit> {
    let at = unsafe { hit.point() }?;
    let holder = unsafe { Strong::lock(&hook.m_Holder) }?;
    let character = unsafe { Character::GetLocalPlayerCharacter() };
    if character.is_null() {
        return None;
    }
    let owner = SharedPtr::<Character> {
        px: character,
        pn: holder.control_block(),
    };
    if hook.m_ActiveWire.px as usize != wire.get() {
        return None;
    }
    let active = unsafe { &mut *(wire.get() as *mut GrapplingHookWire) };
    let end = hook::hook_end_mut(active);
    if end.m_WireEndState != WireEndState::GWES_EXTENDING || !end.m_PointProxy.is_null() {
        return None;
    }
    let mut cast = unsafe { MaybeUninit::<CastRayInfo>::zeroed().assume_init() };
    cast.base.__vftable = CastRayInfoBase::VFTABLE;
    unsafe { cast.base.Reset() };
    cast.base.__vftable = CastRayInfo::VFTABLE;
    cast.base.m_Normal = vector(-Vec3::from_array(end.m_HookDirection.data));
    cast.base.m_BodyId = hit.body;
    cast.m_GameObject = hit.owner.get() as *mut _;
    unsafe { end.Attach(&vector(at), &mut cast, &owner, false) };
    if end.m_WireEndState != WireEndState::GWES_ATTACHED {
        return None;
    }
    let proxy = unsafe { end.m_PointProxy.as_ref() }?;
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    let mut position = vector(Vec3::ZERO);
    let mut normal = vector(Vec3::ZERO);
    unsafe {
        proxy.GetRigidBodyForConstraint(&mut body);
        if !proxy.GetVisualPositionAndNormalWS(&mut position, &mut normal) {
            return None;
        }
    }
    let position = Vec3::from_array(position.data);
    let offset_m = position.distance(at);
    let proxy_body_owned =
        body != hit.body && unsafe { physics::bodies::read(body, hit.owner) }.is_ok();
    let matched = verify_proxy(
        hit.body,
        body,
        offset_m,
        SNATCH.get().attach_tolerance_m,
        proxy_body_owned,
    );
    tracing::info!(
        target: "controllers",
        selected_body = hit.body,
        proxy_body = body,
        proxy = proxy as *const _ as usize,
        offset_m,
        matched = ?matched,
        "snatch: the hook's attachment proxy"
    );
    match matched {
        ProxyMatch::Selected => Some(hit),
        ProxyMatch::ObjectBody => {
            let rebound = unsafe { BodyHit::at(body, hit.owner, position) }?;
            tracing::info!(
                target: "controllers",
                selected_body = hit.body,
                proxy_body = body,
                "snatch: the hook follows the object's own body"
            );
            Some(rebound)
        }
        ProxyMatch::Rejected => None,
    }
}

/// What the hook end of `wire` holds, while `wire` is the hook's active wire and its end is
/// attached.
///
/// # Safety
///
/// The game thread, before the world update; the anchor is not kept past the tick.
pub(super) unsafe fn anchor(wire: NonZeroUsize) -> Option<Anchor> {
    let end = unsafe { hook_end(wire) }?;
    let proxy = unsafe { end.m_PointProxy.as_ref() }?;
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    let mut position = vector(Vec3::ZERO);
    let mut normal = vector(Vec3::ZERO);
    unsafe {
        proxy.GetRigidBodyForConstraint(&mut body);
        if !proxy.GetVisualPositionAndNormalWS(&mut position, &mut normal) {
            return None;
        }
    }
    let position = Vec3::from_array(position.data);
    (body != PfxCharacterInstance::INVALID_BODY_ID && position.is_finite()).then_some(Anchor {
        proxy: end.m_PointProxy as usize,
        body,
        position,
    })
}

/// What the hook end of an owned wire is attached to.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Anchor {
    /// The attachment proxy's address.
    pub proxy: usize,
    /// The body the proxy's constraint holds.
    pub body: u32,
    /// The proxy's visual position, world space.
    pub position: Vec3,
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
        hook::release_custom(CustomOwner::Snatch);
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
    if reading == WireReading::Gone {
        hook::release_custom(CustomOwner::Snatch);
    }
    (reading, Some(active.m_MaxLength))
}

/// End the custom wire `wire`, while it is still the hook's active wire.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn cancel(wire: NonZeroUsize) {
    if unsafe { active_wire(wire) }.is_some()
        && let Some(hook) = unsafe { local_hook() }
    {
        unsafe { hook.CancelActiveWire() };
    }
    hook::release_custom(CustomOwner::Snatch);
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

/// Whether the weapon at `address` is still on its mount ([`WeaponState::E_WS_MOUNTED`]). A gun
/// torn or blown off its mount is dropped and is snatched as any weapon: the hands' table holsters
/// a heavy weapon the pickup leaves dropped, so it can take one from the world.
///
/// # Safety
///
/// `address` must be a weapon held alive, on the game thread.
pub(super) unsafe fn on_mount(address: NonZeroUsize) -> bool {
    unsafe { (*(address.get() as *const WeaponBase)).m_State == WeaponState::E_WS_MOUNTED }
}

/// Where the local player stands, world space.
pub(super) fn player_position(game: &GameTick<'_>) -> Option<Vec3> {
    let local_player = LocalPlayer::get(game)?;
    let character = local_player.character();
    let position = character_position(character);
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

pub(super) unsafe fn object_local(object: NonZeroUsize, at: Vec3) -> Option<Vec3> {
    let inverse = unsafe { object_frame(object) }?.inverse();
    let local = inverse.transform_point3(at);
    (inverse.is_finite() && local.is_finite()).then_some(local)
}

pub(super) unsafe fn object_frame(object: NonZeroUsize) -> Option<Mat4> {
    let mut matrix = Matrix4::from(Mat4::IDENTITY);
    if !unsafe { (*game_object_of(object)).GetTransform(&mut matrix) } {
        return None;
    }
    let matrix = Mat4::from(matrix);
    matrix.is_finite().then_some(matrix)
}

fn vector(v: Vec3) -> Vector3 {
    Vector3 { data: v.to_array() }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const TOLERANCE_M: f32 = 0.02;

    #[test]
    fn a_proxy_on_the_selected_body_verifies_it() {
        assert_eq!(
            verify_proxy(7, 7, 0.0, TOLERANCE_M, false),
            ProxyMatch::Selected
        );
        assert_eq!(
            verify_proxy(7, 7, TOLERANCE_M, TOLERANCE_M, false),
            ProxyMatch::Selected
        );
    }

    #[test]
    fn a_proxy_on_another_body_of_the_object_is_the_object_s_own_body() {
        assert_eq!(
            verify_proxy(7, 9, 0.01, TOLERANCE_M, true),
            ProxyMatch::ObjectBody
        );
    }

    #[test]
    fn a_proxy_on_another_objects_body_is_rejected() {
        assert_eq!(
            verify_proxy(7, 9, 0.01, TOLERANCE_M, false),
            ProxyMatch::Rejected
        );
    }

    #[test]
    fn a_proxy_away_from_the_selected_point_is_rejected() {
        assert_eq!(
            verify_proxy(7, 7, 0.03, TOLERANCE_M, false),
            ProxyMatch::Rejected
        );
        assert_eq!(
            verify_proxy(7, 7, f32::NAN, TOLERANCE_M, false),
            ProxyMatch::Rejected
        );
        assert_eq!(
            verify_proxy(7, 7, f32::INFINITY, TOLERANCE_M, false),
            ProxyMatch::Rejected
        );
    }

    #[test]
    fn the_line_keeps_the_selected_body_while_it_and_the_proxy_agree() {
        assert_eq!(
            check_anchor(true, 7, Some((7, true))),
            AnchorCheck::Selected
        );
        assert_eq!(
            check_anchor(true, 7, Some((7, false))),
            AnchorCheck::Selected
        );
        // The selected body and the proxy body are checked with the same ownership read, so a
        // selected body that is not owned is lost even when the proxy still holds it.
        assert_eq!(check_anchor(false, 7, Some((7, true))), AnchorCheck::Lost);
        assert_eq!(check_anchor(true, 7, None), AnchorCheck::Selected);
    }

    #[test]
    fn the_line_follows_the_object_s_own_body_when_the_proxy_moved_to_it() {
        assert_eq!(
            check_anchor(true, 7, Some((9, true))),
            AnchorCheck::ObjectBody
        );
        assert_eq!(
            check_anchor(false, 7, Some((9, true))),
            AnchorCheck::ObjectBody
        );
    }

    #[test]
    fn the_line_is_lost_when_neither_body_belongs_to_the_object() {
        assert_eq!(check_anchor(false, 7, Some((9, false))), AnchorCheck::Lost);
        assert_eq!(check_anchor(false, 7, Some((7, false))), AnchorCheck::Lost);
        assert_eq!(check_anchor(false, 7, None), AnchorCheck::Lost);
    }

    #[test]
    fn a_proxy_on_another_objects_body_is_lost() {
        // The line is attached to a body of something else: pulling the selected body while the
        // line holds the stranger would bind the two silently, so the snatch fails instead.
        assert_eq!(check_anchor(true, 7, Some((9, false))), AnchorCheck::Lost);
    }
}
