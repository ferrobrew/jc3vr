//! The physical hands' side of the grapple snatch ([`crate::controllers::snatch`]): what a thing out
//! of reach under the grapple's aim is to a hand, by the same rules as the reach in reach, and the
//! hand-over of a snatched prop to the hand's ordinary hold.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::character::character::Character;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        STATE,
        bodies::{self, BodyError},
        characters::ragdoll::{Bone, Ragdoll},
        collider_view::{self, model::Aabb},
        contact::{Surface, SurfaceHit},
        doors::{self, DoorTarget, DoorsConfig, acquisition::PanelReach, engine as door_engine},
        engine,
        grasp::{self, Reach, TargetKind},
        pickup::{PICKUP, Request},
    },
};

/// The mass of `object`'s body `body`, when it is a loose object: not a character, a vehicle or its
/// part, a weapon, or a grenade, and dynamic with a finite mass.
///
/// # Safety
///
/// The main thread, outside the physics step; `object` and `body` must be held alive or come from
/// a cast this tick.
pub(crate) unsafe fn loose_mass(object: NonZeroUsize, body: u32) -> Option<f32> {
    if unsafe { engine::is_character(object) }
        || unsafe { door_engine::vehicle_of(object) }.is_some()
    {
        return None;
    }
    let TargetKind::Prop { mass_kg } = (unsafe { bodies::classify(object, body) }).ok()? else {
        return None;
    };
    Some(mass_kg)
}

/// A ragdolling character under the grapple's aim: the bone of its ragdoll the line pulls, and
/// the whole ragdoll's mass, which that bone drags.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RagdollTarget {
    pub bone: Bone,
    /// The whole ragdoll's mass, kilograms.
    pub mass_kg: f32,
}

/// The ragdoll of the character `object`, hit on its body `body` at `point`, when it ragdolls,
/// taken by the bone the hit lands on ([`Ragdoll::bone_at`]). `None` for anything that is not a
/// ragdolling character.
///
/// # Safety
///
/// The main thread, outside the physics step; `object` must be held alive or come from a cast
/// this tick.
pub(crate) unsafe fn ragdoll_target(
    object: NonZeroUsize,
    body: u32,
    point: Vec3,
) -> Option<RagdollTarget> {
    if !unsafe { engine::is_character(object) } {
        return None;
    }
    let character = unsafe { &*(object.get() as *const Character) };
    let ragdoll = unsafe { Ragdoll::of(character) }?;
    Some(RagdollTarget {
        bone: ragdoll.bone_at(body, point)?,
        mass_kg: ragdoll.mass_kg(),
    })
}

/// Push the body `body` of `owner` by `impulse` at `at`, world space, after checking it still
/// belongs to `owner`: the line's pull on one body of a ragdoll, which the ragdoll's constraints
/// share with the rest of it.
///
/// # Safety
///
/// The main thread, outside the physics step.
#[track_caller]
pub(crate) unsafe fn push(
    body: u32,
    owner: NonZeroUsize,
    impulse: Vec3,
    at: Vec3,
) -> Result<(), BodyError> {
    unsafe { bodies::shove(body, owner, impulse, at) }
}

/// The heaviest loose object one hand lifts and carries, kilograms: within the pickup's heaviest,
/// and no heavier than one hand's strongest pull holds up.
pub(crate) fn carry_max_kg() -> f32 {
    let lifted = PICKUP.hand_force_n / PICKUP.gravity_mps2;
    if grasp::can_hold(lifted, &PICKUP) {
        lifted
    } else {
        PICKUP.max_mass_kg
    }
}

/// A weapon lying in the world, found by its box rather than by a cast.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct DroppedWeapon {
    /// The weapon (its physics game object, a `CWeaponBase`).
    pub object: NonZeroUsize,
    pub body: u32,
    /// The body's centre, world space.
    pub centre: Vec3,
}

/// The weapons lying in the world whose dynamic bodies' boxes `keep` accepts, at most
/// [`MAX_DROPPED_WEAPONS`] boxes considered. A weapon's body is on a layer the ordinary casts never
/// meet, so a thin gun far off is found by its box and then checked as a dropped weapon whose body
/// still reads as its own.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn dropped_weapons(keep: impl FnMut(&Aabb) -> bool) -> Vec<DroppedWeapon> {
    let Ok(boxes) = (unsafe { collider_view::dynamic_boxes(keep, MAX_DROPPED_WEAPONS) }) else {
        return Vec::new();
    };
    boxes
        .into_iter()
        .filter_map(|(body, _)| {
            let object = unsafe { bodies::owner_of(body) }?;
            if !matches!(
                unsafe { bodies::classify(object, body) },
                Ok(TargetKind::Weapon)
            ) {
                return None;
            }
            let state = unsafe { bodies::read(body, object) }.ok()?;
            Some(DroppedWeapon {
                object,
                body,
                centre: state.pose.centre,
            })
        })
        .collect()
}

/// The most boxes [`dropped_weapons`] considers: its filter keeps only boxes near the aim, so this
/// bounds a pathological pile of debris, not an ordinary scene.
const MAX_DROPPED_WEAPONS: usize = 32;

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
    let reach = unsafe {
        doors::reach(
            object,
            body,
            &hit,
            point,
            PanelReach::uniform(reach_m),
            config,
        )
    }?;
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
#[track_caller]
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
