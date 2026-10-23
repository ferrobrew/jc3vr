//! The physical hands' side of the grapple snatch ([`crate::controllers::snatch`]): what a thing out
//! of reach under the grapple's aim is to a hand, by the same rules as the reach in reach, and the
//! hand-over of a snatched prop to the hand's ordinary hold.

use std::num::NonZeroUsize;

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        STATE, bodies,
        config::PickupConfig,
        contact::{Surface, SurfaceHit},
        doors::{self, DoorTarget, DoorsConfig, engine as door_engine},
        engine,
        grasp::{self, Reach, TargetKind},
        pickup::Request,
    },
};

/// The mass of `object`'s body `body`, when it is a loose object one hand can lift and carry: not a
/// character, a vehicle or its part, or a weapon, dynamic, within the pickup's heaviest, and no
/// heavier than one hand's strongest pull holds up.
///
/// # Safety
///
/// The main thread, outside the physics step; `object` and `body` must be held alive or come from
/// a cast this tick.
pub(crate) unsafe fn loose_prop(
    object: NonZeroUsize,
    body: u32,
    config: &PickupConfig,
) -> Option<f32> {
    if unsafe { engine::is_character(object) }
        || unsafe { door_engine::vehicle_of(object) }.is_some()
    {
        return None;
    }
    let TargetKind::Prop { mass_kg } = (unsafe { bodies::classify(object, body) }).ok()? else {
        return None;
    };
    (grasp::can_hold(mass_kg, config) && mass_kg * config.gravity_mps2 <= config.hand_force_n)
        .then_some(mass_kg)
}

/// The closed door of the vehicle `object` (or the vehicle a part of it belongs to) whose handle
/// area is within `reach_m` of `point`, a point the grapple's aim hit on its body `body` with the
/// surface normal `normal`, with the handle's point.
///
/// # Safety
///
/// The main thread, outside the physics step; `object` and `body` must come from a cast this tick.
pub(crate) unsafe fn closed_door(
    object: NonZeroUsize,
    body: u32,
    point: Vec3,
    normal: Vec3,
    reach_m: f32,
    config: &DoorsConfig,
) -> Option<(DoorTarget, Vec3)> {
    let hit = SurfaceHit {
        position: point,
        normal,
        surface: Surface::Solid {
            object: Some(object),
            body,
        },
        material: None,
    };
    let reach = unsafe { doors::reach(object, body, &hit, point, reach_m, config) }?;
    match reach.kind {
        TargetKind::Door(door) if door.latched => Some((door, reach.point)),
        _ => None,
    }
}

/// Open the latched door `door` and push it outward at `at` with `speed` metres per second's worth
/// of its own mass.
///
/// # Safety
///
/// The main thread, outside the physics step; the door's vehicle must be live.
pub(crate) unsafe fn swing_open(door: &DoorTarget, at: Vec3, speed: f32) -> Result<(), String> {
    let (owner, body) = unsafe { door_engine::unlatch(door.vehicle, door.part_index) }
        .map_err(|e| e.to_string())?;
    let mass = unsafe { bodies::read(body, owner) }
        .map_err(|e| e.to_string())?
        .mass_kg
        .unwrap_or(0.0);
    let impulse = door.outward.normalize_or_zero() * mass * speed.max(0.0);
    unsafe { bodies::shove(body, owner, impulse, at) }.map_err(|e| e.to_string())
}

/// Whether the vehicle of `door` moves faster than `max_speed_mps`, by the body the door was found
/// on; a vehicle whose body cannot be read counts as moving.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn door_moving(door: &DoorTarget, max_speed_mps: f32) -> bool {
    let Some(body) = door.vehicle_body else {
        return true;
    };
    unsafe { bodies::read(body, door.vehicle) }.map_or(true, |state| {
        state.velocities.linear.length() > max_speed_mps
    })
}

/// Hand a snatched loose object, its body `body` arrived at the hand, over to the hand's ordinary
/// hold, taken at `point`: the next palms' tick takes hold of it as of a grab in reach, and the
/// grip's release lets go of it.
pub(crate) fn hold_remote(hand: Hand, object: NonZeroUsize, body: u32, point: Vec3, mass_kg: f32) {
    let mut state = STATE.lock();
    state.pickup.requests[hand.index()] = Some(Request::Grab(Reach {
        object,
        body,
        point,
        distance: 0.0,
        normal: Vec3::ZERO,
        kind: TargetKind::Prop { mass_kg },
    }));
}
