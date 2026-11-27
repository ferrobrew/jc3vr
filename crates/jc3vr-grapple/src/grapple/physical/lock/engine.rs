//! The lock's engine side: the far end of the local player's hook, its point, motion, and body, the character's mass and support, and the line let go of. The engine side is in "The
//! reel's motion and endings" in `docs/engine/gameplay/grapple-pipeline.md`.

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::{
        grapple_point_proxy::{
            AttachmentObjectInfo, GrapplePointProxy, VehicleGrapplePointProxy, VehicleGrappleType,
        },
        grappling_hook::{GrapplingHook, GrapplingHookState},
    },
    interaction::INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE,
    physics::{PfxCharacterInstance, hknpCharacterSurfaceInfo},
    types::math::{Matrix4, Vector3},
    vehicle::vehicle::Helicopter,
};
use jc3vr_body::controllers::{
    physics,
    physics::{
        bodies::{BodyState, game_object_of},
        characters::{character_position, proxy_body},
        grapple_hook as hook, wanted,
        wanted::Contributor,
    },
};

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
    unsafe { hook::attached_hook_end(wire)?.m_PointProxy.as_ref() }
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
    let (owner, state) = unsafe { physics::line_body(body) }?;
    Some((body, owner, state))
}

/// Whether `hook` reels in along a line whose far end is attached to a helicopter, and the reel's
/// chosen entry is the airframe's stunt hang. The reel prefers a valid entry on the helicopter's
/// door side and falls back to the hang; a door-side arrival is the game's hijack and is left to
/// it.
///
/// # Safety
///
/// The game thread, outside the physics step; `hook` must be live.
pub(super) unsafe fn reeling_to_helicopter_hang(hook: &GrapplingHook) -> bool {
    let Some(proxy) = (unsafe { reeling_far_end(hook) }) else {
        return false;
    };
    if proxy.m_AttachmentObjectInfo != AttachmentObjectInfo::AOI_Vehicle {
        return false;
    }
    let helicopter = unsafe { far_body(proxy) }.is_some_and(|(_, owner, _)| unsafe {
        (*game_object_of(owner)).IsType(Helicopter::TYPE_ID())
    });
    // SAFETY: a vehicle attachment's proxy is a vehicle grapple point proxy.
    let vehicle = unsafe { &*std::ptr::from_ref(proxy).cast::<VehicleGrapplePointProxy>() };
    helicopter
        && unsafe { vehicle.GetVehicleType() } == VehicleGrappleType::Helicopter
        && unsafe { vehicle.IsCurrentEntryHangNode() }
}

/// The body of the helicopter `hook` reels in onto when the reel's chosen entry is the door side
/// rather than the stunt hang.
///
/// # Safety
///
/// The game thread, outside the physics step; `hook` must be live.
pub(super) unsafe fn reeling_to_helicopter_door_side(hook: &GrapplingHook) -> Option<u32> {
    let proxy = unsafe { reeling_far_end(hook) }?;
    if proxy.m_AttachmentObjectInfo != AttachmentObjectInfo::AOI_Vehicle {
        return None;
    }
    let (body, owner, _) = unsafe { far_body(proxy) }?;
    if !unsafe { (*game_object_of(owner)).IsType(Helicopter::TYPE_ID()) } {
        return None;
    }
    // SAFETY: a vehicle attachment's proxy is a vehicle grapple point proxy.
    let vehicle = unsafe { &*std::ptr::from_ref(proxy).cast::<VehicleGrapplePointProxy>() };
    (unsafe { vehicle.GetVehicleType() } == VehicleGrappleType::Helicopter
        && !unsafe { vehicle.IsCurrentEntryHangNode() })
    .then_some(body)
}

/// How Y below a helicopter set the reel's entry.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum DoorPull {
    /// A door-side entry is chosen: the nearest of those that validated, whose start lies
    /// `distance_m` from the character.
    DoorSide { candidates: u32, distance_m: f32 },
    /// No door-side entry validated: the choice is cleared for the reel to select afresh.
    Reselect,
    /// The line's far end is not a vehicle the entries can be read from.
    NoVehicle,
}

/// Point the reel onto `hook`'s far end, a helicopter, at the door-side entry nearest `character`,
/// so the resumed reel pulls him up to the door rather than into the stunt hang. Each enabled
/// door-side entry of the vehicle's graph is chosen in turn and put through the reel's own update
/// of its choice ([`VehicleGrapplePointProxy::UpdateVehicleReelInEntrySelection`]), which checks
/// its act from the character's state and recalculates its start; the nearest that stands is kept.
/// Each is tried with the previous goal cleared, since the update's side check compares the new
/// entry with the goal it finds stored. With none standing, the choice is cleared for the reel to
/// select afresh.
///
/// # Safety
///
/// The game thread, outside the physics step and the world update; `character` and `hook` must be
/// live, the hook reeling along its line.
pub(super) unsafe fn pull_to_door_side(
    character: &mut Character,
    hook: &GrapplingHook,
) -> DoorPull {
    let Some(proxy) = (unsafe { reeling_far_end(hook) })
        .filter(|proxy| proxy.m_AttachmentObjectInfo == AttachmentObjectInfo::AOI_Vehicle)
    else {
        return DoorPull::NoVehicle;
    };
    let Some((hook_point, _)) = (unsafe { point(proxy) }) else {
        return DoorPull::NoVehicle;
    };
    let Some(wire) = (unsafe { hook.m_ActiveWire.px.as_ref() }) else {
        return DoorPull::NoVehicle;
    };
    let Some(end) = hook::attached_hook_end(wire) else {
        return DoorPull::NoVehicle;
    };
    // SAFETY: a vehicle attachment's proxy is a vehicle grapple point proxy, live with the wire.
    let Some(vehicle_proxy) =
        (unsafe { end.m_PointProxy.cast::<VehicleGrapplePointProxy>().as_mut() })
    else {
        return DoorPull::NoVehicle;
    };
    // SAFETY: the vehicle owns its interaction context and its graphs while it lives, as it does
    // while the line holds it.
    let graph = unsafe { vehicle_proxy.GetVehiclePtr().as_ref() }
        .and_then(|vehicle| unsafe { vehicle.m_InteractionContext.as_ref() })
        .and_then(|context| unsafe { context.m_InteractionGraphs[0].px.as_mut() });
    let Some(graph) = graph else {
        vehicle_proxy.m_ReelInEntryIndex = NO_ENTRY;
        return DoorPull::Reselect;
    };
    let mut to_world = Matrix4::default();
    unsafe { graph.GetWorldTransform(&mut to_world) };
    let to_world = Mat4::from(to_world);
    let at = character_position(character);
    let extension = Vector3 {
        data: hook_point.to_array(),
    };
    let direction = Vector3 {
        data: (hook_point - at).normalize_or_zero().to_array(),
    };
    let character_ptr = std::ptr::from_mut(character);
    let mut choose = |index: i32| -> Option<f32> {
        vehicle_proxy.m_ReelInEntryIndex = index;
        vehicle_proxy.m_ReelInGoalPositionGS = Vector3 { data: [0.0; 3] };
        let stands = unsafe {
            vehicle_proxy.UpdateVehicleReelInEntrySelection(character_ptr, &extension, &direction)
        };
        (stands && vehicle_proxy.m_ReelInEntryIndex == index).then(|| {
            let start = Vec3::from_array(vehicle_proxy.m_ReelInEntryPositionGS.data);
            to_world.transform_point3(start).distance(at)
        })
    };
    let mut candidates = 0;
    let mut best: Option<(i32, f32)> = None;
    for index in 0..graph.m_NofEntries {
        let Ok(index) = i32::try_from(index) else {
            break;
        };
        let door_side = unsafe { graph.GetEntryByIndex(index).as_ref() }.is_some_and(|entry| {
            entry.m_Enabled && entry.m_UseTagHash == INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE
        });
        if !door_side {
            continue;
        }
        candidates += 1;
        if let Some(distance) = choose(index).filter(|d| d.is_finite())
            && best.is_none_or(|(_, nearest)| distance < nearest)
        {
            best = Some((index, distance));
        }
    }
    match best.and_then(|(index, _)| choose(index).map(|distance| (index, distance))) {
        Some((_, distance_m)) => DoorPull::DoorSide {
            candidates,
            distance_m,
        },
        None => {
            vehicle_proxy.m_ReelInEntryIndex = NO_ENTRY;
            DoorPull::Reselect
        }
    }
}

/// The vehicle grapple proxy's index for no chosen entry.
const NO_ENTRY: i32 = -1;

/// The character's mass: its proxy body's.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(super) unsafe fn rider_mass_kg(character: &Character) -> Option<f32> {
    let body = unsafe { proxy_body(character) }?;
    unsafe { physics::bodies::mass_kg(body) }.filter(|m| *m > 0.0)
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
/// cancelling the reel into the fall. The caller captures the character's wanted velocity before
/// this and hands it back, since the cancel can clear it.
///
/// # Safety
///
/// The game thread, before the world update; `hook` must be live.
pub(super) unsafe fn let_go(hook: &mut GrapplingHook) {
    unsafe { hook.RetractActiveWire() };
}

/// The character's wanted world velocity: the whole of its motion while the reel drives it.
///
/// # Safety
///
/// The game thread; `character` must be live.
pub(super) unsafe fn wanted_velocity(character: &Character) -> Option<Vec3> {
    let velocity = unsafe { wanted::read(character) };
    velocity.is_finite().then_some(velocity)
}

/// Give the character `velocity` as its wanted world velocity, as the reel task's motion does: the
/// lock's whole contribution ([`Contributor::GrappleLock`]).
///
/// # Safety
///
/// The character update, or the game thread before the world update; `character` must be live.
pub(super) unsafe fn set_wanted_velocity(character: &mut Character, velocity: Vec3) {
    unsafe { wanted::set(character, Contributor::GrappleLock, velocity) };
}
