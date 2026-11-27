//! The seat grapple's game calls: the target's capture along the grapple's aim, the fire as a
//! custom wire the seat owns on the local player's hook, the wire's reading, its car end following
//! the grapple hand, the tether to the vehicle, and the call back. A wire is known by its address
//! and read only while the local player's hook still holds it as its active wire and the seat
//! still owns it ([`active_wire`]). Every function runs on the game thread in the input tick.

use std::{mem::MaybeUninit, num::NonZeroUsize};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::{
        grapple_point_proxy::{AttachmentObjectInfo, VehicleGrapplePointProxy},
        grappling_hook::{GrapplingHookState, GrapplingHookWire, WireState},
        wire_end::{HookTargetInfo, WireEndState},
    },
    physics::{CastRayInfo, CastRayInfoBase},
    types::{
        math::{Matrix4, Vector3},
        shared_ptr::SharedPtr,
    },
};
use jc3vr_body::controllers::{
    aim::{
        ray::Ray,
        raycast,
        raycast::{CastOptions, RayHit},
    },
    body_frame, hands, physics,
    physics::{
        bodies::game_object_of,
        grapple_hook as hook,
        grapple_hook::{BodyHit, CustomOwner},
        local_player::LocalPlayer,
        refs::{Strong, null_weak},
        vehicles::vehicle_body,
    },
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;
use jc3vr_view::controllers::cockpit::CockpitTransform;

use crate::grapple::{seat::Refusal, tuning::GRAPPLE};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WireReading {
    Flying,
    Attached,
    Gone,
    Ready,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Target {
    pub point: Vec3,
    pub hit: Option<RayHit>,
    pub body: Option<BodyHit>,
}

impl Target {
    /// The target at `at`, a point along the grapple's aim: the world hit a ray from the aim's
    /// origin toward it meets close to it, passing through the local player and the vehicle it
    /// rides, with the physics body there. `None` when the point is not finite, the grapple has no
    /// aim, or the ray meets nothing near the point.
    pub fn capture(game: &GameTick<'_>, at: Vec3) -> Option<Self> {
        if !at.is_finite() {
            return None;
        }
        let local_player = LocalPlayer::get(game)?;
        let character = local_player.character();
        let aim = body_frame::aim().grapple?;
        let ray = Ray::new(aim.ray.origin, at - aim.ray.origin)?;
        let options = unsafe { CastOptions::ignoring_rider(character) }.seeing_through();
        let seat = &GRAPPLE.get().seat;
        let hit =
            unsafe { raycast::cast(&ray, ray.origin.distance(at) + seat.ray_past_m, &options) }
                .ok()?
                .filter(|hit| hit.position.distance(at) <= seat.hit_within_m)?;
        let body = hit
            .object
            .and_then(|owner| unsafe { BodyHit::at(hit.body, owner, hit.position) });
        Some(Self {
            point: hit.position,
            hit: Some(hit),
            body,
        })
    }

    unsafe fn current(self, game: &GameTick<'_>) -> Option<(Vec3, RayHit)> {
        let mut hit = self.hit?;
        if let Some(body) = self.body {
            hit.position = unsafe { body.point() }?;
        } else if hit.object.is_some() {
            let fresh = Self::capture(game, self.point)?.hit?;
            if fresh.body != hit.body || fresh.object != hit.object {
                return None;
            }
            hit = fresh;
        }
        Some((hit.position, hit))
    }
}

pub(super) unsafe fn fire(
    game: &GameTick<'_>,
    target: Target,
    hand: Hand,
) -> Result<NonZeroUsize, Refusal> {
    let mut local_player = LocalPlayer::get(game).ok_or(Refusal::NoPlayer)?;
    let (character, hook) = local_player.character_and_hook().ok_or(Refusal::NoPlayer)?;
    let vehicle = NonZeroUsize::new(unsafe { character.GetVehiclePtr() } as usize)
        .ok_or(Refusal::NoPlayer)?;
    let body = unsafe { vehicle_body(vehicle) }.ok_or(Refusal::NoPlayer)?;
    let state = unsafe { physics::bodies::read(body, vehicle) }.map_err(|_| Refusal::NoPlayer)?;
    let speed = unsafe { hook.GetTuning().as_ref() }
        .ok_or(Refusal::FireRefused)?
        .m_GrappleFireSpeed;
    let holder = unsafe { Strong::lock(&hook.m_Holder) }.ok_or(Refusal::NoPlayer)?;
    let shared = SharedPtr::<Character> {
        px: std::ptr::from_ref(character).cast_mut(),
        pn: holder.control_block(),
    };
    if !unsafe { hook::claim_custom(CustomOwner::Seat) } {
        return Err(Refusal::Busy);
    }
    let Some((at, hit)) = (unsafe { target.current(game) }) else {
        hook::release_custom(CustomOwner::Seat);
        return Err(Refusal::NoTarget);
    };
    let mut info = unsafe { target_info(at, Some(hit)) };
    unsafe {
        hook.FireHookCustom(&mut info, vehicle.get() as *mut _, false);
        info.m_TargetGameObject.Assign(&null_weak());
    }
    let wire = NonZeroUsize::new(hook.m_ActiveWire.px as usize);
    let Some(wire) = wire.filter(|_| hook.m_State == GrapplingHookState::GHS_CUSTOM_ACTIVE_WIRE)
    else {
        hook::release_custom(CustomOwner::Seat);
        return Err(Refusal::FireRefused);
    };
    hook::own_custom(CustomOwner::Seat, wire);
    let active = unsafe { hook.m_ActiveWire.px.as_mut() }.ok_or(Refusal::FireRefused)?;
    let car_at = hand_point(hand).unwrap_or(state.pose.centre);
    let car_hit = RayHit {
        position: car_at,
        normal: Vec3::Y,
        distance: 0.0,
        object: Some(vehicle),
        body,
        material: None,
    };
    let mut cast = unsafe { cast_info(car_hit) };
    unsafe {
        active.DestroyConstraint();
        hook::device_end_mut(active).Detach();
        active.m_IsActiveWire = false;
        active.m_WireState = WireState::GHWS_ACTIVE_VIRTUAL;
        hook::device_end_mut(active).Attach(
            &Vector3 {
                data: car_at.to_array(),
            },
            &mut cast,
            &shared,
            false,
        );
    }
    if hook::device_end(active).m_WireEndState != WireEndState::GWES_ATTACHED
        || hook::device_end(active).m_PointProxy.is_null()
    {
        unsafe { cancel(wire) };
        return Err(Refusal::FireRefused);
    }
    let mut launch = unsafe { target_info(at, Some(hit)) };
    unsafe {
        hook::hook_end_mut(active).SetExtend(
            &Vector3 {
                data: car_at.to_array(),
            },
            speed,
            &mut launch,
            false,
        );
        launch.m_TargetGameObject.Assign(&null_weak());
    }
    Ok(wire)
}

pub(super) unsafe fn read(wire: NonZeroUsize) -> WireReading {
    let Some(active) = (unsafe { active_wire(wire) }) else {
        return if unsafe { ready() } {
            hook::release_custom(CustomOwner::Seat);
            WireReading::Ready
        } else {
            WireReading::Gone
        };
    };
    if matches!(
        active.m_WireState,
        WireState::GHWS_INVALID | WireState::GHWS_BROKEN
    ) || hook::device_end(active).m_WireEndState != WireEndState::GWES_ATTACHED
        || hook::device_end(active).m_PointProxy.is_null()
    {
        return WireReading::Gone;
    }
    let end = hook::hook_end(active);
    match end.m_WireEndState {
        WireEndState::GWES_ATTACHED if !end.m_PointProxy.is_null() => WireReading::Attached,
        WireEndState::GWES_EXTENDING => WireReading::Flying,
        _ => WireReading::Gone,
    }
}

pub(super) unsafe fn track_hand(wire: NonZeroUsize, hand: Hand) -> bool {
    let Some(at) = hand_point(hand) else {
        return false;
    };
    let Some(active) = (unsafe { active_wire(wire) }) else {
        return false;
    };
    let end = hook::device_end(active);
    let Some(proxy) = (unsafe { end.m_PointProxy.as_ref() }) else {
        return false;
    };
    if proxy.m_AttachmentObjectInfo != AttachmentObjectInfo::AOI_Vehicle {
        return false;
    }
    let mut position = Vector3 { data: [0.0; 3] };
    let mut normal = Vector3 { data: [0.0; 3] };
    if !unsafe { proxy.GetVisualPositionAndNormalWS(&mut position, &mut normal) } {
        return false;
    }
    if Vec3::from_array(position.data).distance(at) < 0.005 {
        return true;
    }
    let physical = active.m_WireState == WireState::GHWS_ACTIVE_PHYSICAL;
    let length = active.m_MaxLength;
    let proxy = end.m_PointProxy.cast::<VehicleGrapplePointProxy>();
    unsafe {
        (*proxy).ReattachToNewPos(
            &Vector3 {
                data: at.to_array(),
            },
            &normal,
        )
    };
    let Some(hook) = (unsafe { hook::local_hook() }) else {
        return false;
    };
    let Some(active) = (unsafe { hook.m_ActiveWire.px.as_mut() }) else {
        return false;
    };
    hook::device_end_mut(active).m_WorldPositionT1 = Vector3 {
        data: at.to_array(),
    };
    if physical {
        let distance = Vec3::from_array(hook::hook_end(active).m_WorldPositionT1.data).distance(at);
        unsafe { active.DestroyConstraint() };
        active.m_IsActiveWire = true;
        unsafe { active.CreateConstraint(distance - length, false) };
        active.m_IsActiveWire = false;
        active.m_MaxLength = length;
    }
    true
}

/// Where the grapple hand's grip is in the world.
fn hand_point(hand: Hand) -> Option<Vec3> {
    let active = hands::active_hands_raw()?;
    let hand = active.hands[hand.index()]?;
    let transform = CockpitTransform::current(active.head.position, active.world_scale);
    let point = transform.pose_to_world(hand.grip).position;
    point.is_finite().then_some(point)
}

pub(super) unsafe fn attachment_point(wire: NonZeroUsize) -> Option<Vec3> {
    let end = hook::attached_hook_end(unsafe { active_wire(wire) }?)?;
    let at = Vec3::from_array(end.m_WorldPositionT1.data);
    at.is_finite().then_some(at)
}

pub(super) unsafe fn tether(
    game: &GameTick<'_>,
    wire: NonZeroUsize,
    second: Option<Target>,
) -> bool {
    if unsafe { active_wire(wire) }.is_none() {
        return false;
    }
    let Some(hook) = (unsafe { hook::local_hook() }) else {
        return false;
    };
    let Some(active) = (unsafe { hook.m_ActiveWire.px.as_mut() }) else {
        return false;
    };
    if let Some(second) = second {
        let Some((at, hit)) = (unsafe { second.current(game) }) else {
            return false;
        };
        let Some(tuning) = (unsafe { hook.GetTuning().as_ref() }) else {
            return false;
        };
        let mut info = unsafe { target_info(at, Some(hit)) };
        let start = hook::device_end(active).m_WorldPositionT1;
        unsafe {
            active.DestroyConstraint();
            hook::device_end_mut(active).SetExtend(
                &start,
                tuning.m_GrappleFireSpeed,
                &mut info,
                false,
            );
            info.m_TargetGameObject.Assign(&null_weak());
        }
        active.m_WireState = WireState::GHWS_ACTIVE_VIRTUAL;
    }
    active.m_IsActiveWire = false;
    unsafe {
        GrapplingHookWire::PushGrapplingHookWire(&mut hook.m_Wires, &hook.m_ActiveWire);
        GrapplingHookWire::ResetGrapplingHookWire(&mut hook.m_ActiveWire);
    }
    hook.m_State = GrapplingHookState::GHS_INACTIVE;
    hook::release_custom(CustomOwner::Seat);
    true
}

pub(super) unsafe fn cancel(wire: NonZeroUsize) {
    if unsafe { active_wire(wire) }.is_some()
        && let Some(hook) = unsafe { hook::local_hook() }
    {
        unsafe { hook.CancelActiveWire() };
    }
}

unsafe fn ready() -> bool {
    unsafe { hook::local_hook() }.is_some_and(|hook| {
        hook.m_State == GrapplingHookState::GHS_INACTIVE
            && !hook.m_ActiveWire.exists()
            && !hook.m_AttachmentWire.exists()
            && !hook::firing(hook)
    })
}

unsafe fn target_info(at: Vec3, hit: Option<RayHit>) -> HookTargetInfo {
    let mut info = unsafe { MaybeUninit::<HookTargetInfo>::zeroed().assume_init() };
    info.m_TargetPosition = Vector3 {
        data: at.to_array(),
    };
    info.m_WasInRange = true;
    info.m_BoneIndex = -1;
    let Some(object) = hit.and_then(|hit| hit.object) else {
        return info;
    };
    let object = game_object_of(object);
    let mut matrix = Matrix4::from(Mat4::IDENTITY);
    if !unsafe { (*object).GetTransform(&mut matrix) } {
        return info;
    }
    let inverse = Mat4::from(matrix).inverse();
    let local = inverse.transform_point3(at);
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

unsafe fn cast_info(hit: RayHit) -> CastRayInfo {
    let mut info = unsafe { MaybeUninit::<CastRayInfo>::zeroed().assume_init() };
    info.base.__vftable = CastRayInfoBase::VFTABLE;
    unsafe { info.base.Reset() };
    info.base.__vftable = CastRayInfo::VFTABLE;
    info.base.m_Normal = Vector3 {
        data: hit.normal.to_array(),
    };
    info.base.m_BodyId = hit.body;
    info.m_GameObject = hit
        .object
        .map_or(std::ptr::null_mut(), |owner| owner.get() as *mut _);
    info
}

unsafe fn active_wire(wire: NonZeroUsize) -> Option<&'static GrapplingHookWire> {
    if !hook::owns_custom(CustomOwner::Seat, wire) {
        return None;
    }
    let hook = unsafe { hook::local_hook() }?;
    if hook.m_ActiveWire.px as usize != wire.get() {
        return None;
    }
    unsafe { hook.m_ActiveWire.px.as_ref() }
}
