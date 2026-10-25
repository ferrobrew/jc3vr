//! The doors near the hands, offered by their handles whatever the reach's rays hit: every door of
//! every vehicle near the player is a target at its handle ([`door_handle`]: the embedded table's,
//! or the box guess for a door the table lacks) for a palm within
//! [`DoorsConfig::handle_reach_m`] of it. A latched door is merged into its vehicle's body, so the
//! grip that takes it needs that body, which proves the vehicle alive while the hand holds the
//! latch: a short cast from the palm to the handle finds it. What each door near a palm was, and
//! why it was not offered, is noted for the grip press's log.
//!
//! The vehicles come from the target system's vehicle set
//! ([`crate::controllers::aim::targets`]), which the game keeps of every vehicle near the player,
//! read on the aim update's thread as the palms' tick runs.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{
    physics::PhysicsGameObject,
    vehicle::vehicle::{DoorSlot, Vehicle},
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::targets::{self, TargetSet},
    physical_hands::{
        bodies::is_type,
        contact::{Surface, SurfaceHit},
        doors::{
            DoorTarget, DoorsConfig,
            engine::{self, DoorRead},
            geometry::{DoorKind, door_handle},
            slot_label, table,
        },
        grab_view::model::{HandleOutcome, HandleSeen},
        grasp::{Reach, TargetKind},
        pickup::diagnostics::scan::{Branch, Scan, ScanEntry, ScanResult},
    },
};

/// The vehicles whose doors a palm at any of `palms` might reach: those in the target system's
/// vehicle set within [`NEAR_M`] of one, at the VR world scale `world_scale`.
///
/// # Safety
///
/// The thread that runs the player's aim update, outside the physics step.
pub(crate) unsafe fn near(palms: &[Vec3], world_scale: f32) -> Vec<NonZeroUsize> {
    let range = NEAR_M * world_scale.max(1e-3);
    let mut out = Vec::new();
    for target in unsafe { targets::targets(TargetSet::Vehicles) } {
        let Some(game_object) = (unsafe { targets::game_object(target) }) else {
            continue;
        };
        let Some(object) = (game_object as usize)
            .checked_sub(PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize)
            .and_then(NonZeroUsize::new)
        else {
            continue;
        };
        if !unsafe { is_type(object, Vehicle::TYPE_ID()) } || out.contains(&object) {
            continue;
        }
        let at = unsafe { engine::vehicle_frame(object) }.w_axis.truncate();
        if palms.iter().any(|p| p.distance(at) <= range) {
            out.push(object);
        }
        if out.len() >= MAX_VEHICLES {
            break;
        }
    }
    out
}

/// The existing handle readouts and a bounded copy of its evaluated branches.
pub(crate) struct HandleObservations<'a> {
    pub notes: &'a mut Vec<String>,
    pub handles: Option<&'a mut Vec<HandleSeen>>,
    pub scan: &'a mut Scan,
}

/// The nearest door handle of `vehicles` a palm at `palm` is within the handle reach of, at the VR
/// world scale `world_scale`, with `cast` returning the first hit between two points. What each
/// door near the palm was goes into `notes`, and each handle near it, with how it stood to the
/// palm, into `handles` when given, for the grab view.
///
/// # Safety
///
/// The game thread, outside the physics step; `vehicles` must be live vehicles from [`near`] this
/// tick.
pub(crate) unsafe fn handle_reach(
    palm: Vec3,
    vehicles: &[NonZeroUsize],
    world_scale: f32,
    config: &DoorsConfig,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
    observations: HandleObservations<'_>,
) -> Option<Reach> {
    let HandleObservations {
        notes,
        mut handles,
        scan,
    } = observations;
    scan.handles_ran = true;
    if let Some(handles) = handles.as_deref_mut() {
        handles.clear();
    }
    let mut seen = |at: Vec3, outcome: HandleOutcome| {
        if let Some(handles) = handles.as_deref_mut()
            && handles.len() < MAX_HANDLES_SEEN
        {
            handles.push(HandleSeen { at, outcome });
        }
    };
    if !config.enabled {
        return None;
    }
    let scale = world_scale.max(1e-3);
    let reach_m = config.handle_reach_m.max(0.0) * scale;
    let mut best: Option<Reach> = None;
    for &vehicle in vehicles {
        let origin = unsafe { engine::vehicle_frame(vehicle) }.w_axis.truncate();
        let doors = unsafe { engine::doors(vehicle) };
        unsafe { log_sources(vehicle, &doors) };
        for door in doors {
            let mut entry = ScanEntry {
                branch: Branch::DoorHandle,
                object: Some(vehicle),
                body: door.body.map(|(_, body)| body),
                door: Some((vehicle, door.slot, door.part_index)),
                point: None,
                distance: None,
                mass_kg: None,
                result: ScanResult::Offered,
            };
            let what = format!(
                "vehicle {:#x} {} door{}",
                vehicle.get(),
                slot_label(door.slot),
                if door.latched { " (shut)" } else { " (open)" }
            );
            let Some(door_box) = door.door_box else {
                entry.result = ScanResult::NoBox;
                scan.push(entry);
                note(notes, format!("{what}: no model box to find its handle on"));
                continue;
            };
            let table_entry = door.part_hash.and_then(table::lookup);
            let Some(handle) = door_handle(&door_box, kind_of(door.slot), table_entry) else {
                entry.result = ScanResult::NoHandle;
                scan.push(entry);
                note(
                    notes,
                    format!("{what}: its box has no faces to find its handle on"),
                );
                continue;
            };
            let distance = palm.distance(handle.point);
            entry.point = Some(handle.point);
            entry.distance = Some(distance);
            let mm = |d: f32| d / scale * 1000.0;
            if distance > reach_m {
                entry.result = ScanResult::OutOfReach;
                scan.push(entry);
                if distance <= NOTED_M * scale {
                    seen(handle.point, HandleOutcome::OutOfReach);
                    note(
                        notes,
                        format!(
                            "{what}: handle {:.0} mm away, past the {:.0} mm reach",
                            mm(distance),
                            config.handle_reach_m * 1000.0
                        ),
                    );
                }
                continue;
            }
            let Some((object, body, vehicle_body)) =
                (unsafe { holdable(vehicle, &door, handle.point, handle.normal, palm, cast) })
            else {
                entry.result = ScanResult::NoBody;
                scan.push(entry);
                seen(handle.point, HandleOutcome::NoBody);
                note(
                    notes,
                    format!(
                        "{what}: handle {:.0} mm away, but no body was found at it",
                        mm(distance)
                    ),
                );
                continue;
            };
            entry.object = Some(object);
            entry.body = Some(body);
            scan.push(entry);
            seen(handle.point, HandleOutcome::InReach);
            note(
                notes,
                format!(
                    "{what}: handle {:.0} mm away, in reach (handle from {})",
                    mm(distance),
                    handle.source.label()
                ),
            );
            let reach = Reach {
                object,
                body,
                point: handle.point,
                distance,
                normal: handle.normal,
                kind: TargetKind::Door(DoorTarget {
                    vehicle,
                    vehicle_body,
                    part_index: door.part_index,
                    slot: door.slot,
                    latched: door.latched,
                    outward: door_box.outward(origin),
                }),
            };
            if best.is_none_or(|b| reach.distance < b.distance) {
                best = Some(reach);
            }
        }
    }
    scan.handle_selected = best;
    best
}

/// How far from a palm the vehicles whose doors it might reach are looked for, metres at the
/// player's scale: past the end of the longest vehicle's door from its middle.
const NEAR_M: f32 = 6.0;

/// The most vehicles a tick looks at, a bound against a crowded street.
const MAX_VEHICLES: usize = 8;

/// How far from a palm a door out of reach is still noted, metres at the player's scale.
const NOTED_M: f32 = 1.0;

/// The most door handles a hand records for the grab view per tick.
const MAX_HANDLES_SEEN: usize = 8;

/// The most door notes a hand keeps per tick.
const MAX_NOTES: usize = 12;

fn note(notes: &mut Vec<String>, note: String) {
    if notes.len() < MAX_NOTES {
        notes.push(note);
    }
}

/// Log, once per vehicle model, where each of `vehicle`'s doors takes its handle from: the
/// embedded table or the door's box. A model is known by the smallest of its door parts' name
/// hashes, which every vehicle of the model shares.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
unsafe fn log_sources(vehicle: NonZeroUsize, doors: &[DoorRead]) {
    static LOGGED: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    let Some(key) = doors.iter().filter_map(|d| d.part_hash).min() else {
        return;
    };
    {
        let mut logged = LOGGED.lock();
        if logged.contains(&key) {
            return;
        }
        logged.push(key);
    }
    let mut model = None;
    let sources: Vec<String> = doors
        .iter()
        .map(|door| {
            let entry = door.part_hash.and_then(table::lookup);
            let name = entry
                .map(|e| e.part.clone())
                .or_else(|| unsafe { engine::part_name(vehicle, door.part_index) })
                .unwrap_or_else(|| format!("the {} door", slot_label(door.slot)));
            if model.is_none() {
                model = entry
                    .map(|e| e.vehicle.clone())
                    .or_else(|| name.split('_').next().map(str::to_owned));
            }
            let source = door
                .door_box
                .and_then(|b| door_handle(&b, kind_of(door.slot), entry))
                .map_or("nowhere (no model box)", |h| h.source.label());
            format!("{name} from {source}")
        })
        .collect();
    tracing::info!(
        target: "controllers",
        model = model.as_deref().unwrap_or("unknown"),
        "physical hands: doors: door handles: {}",
        sources.join(", ")
    );
}

/// How a door in `slot` opens, for finding its hinge.
fn kind_of(slot: DoorSlot) -> DoorKind {
    match slot {
        DoorSlot::HOOD => DoorKind::Front,
        DoorSlot::TRUNK
        | DoorSlot::REAR_DOOR
        | DoorSlot::CARGO_BAY_DOOR
        | DoorSlot::MISSILE_HATCH => DoorKind::Rear,
        DoorSlot::FRONT_LEFT
        | DoorSlot::FRONT_RIGHT
        | DoorSlot::REAR_LEFT
        | DoorSlot::REAR_RIGHT
        | DoorSlot::CENTER_LEFT
        | DoorSlot::CENTER_RIGHT
        | DoorSlot::TURRET_DOOR
        | DoorSlot::FOLDING_WING => DoorKind::Side,
    }
}

/// What a hand takes of `door` at its handle `point` (outer face `normal`): an open door's own body,
/// or a latched door's vehicle and the vehicle's body under the handle, found by a cast from the
/// palm at `palm` and, failing that, across the face at the handle. `None` when neither cast finds
/// the vehicle there.
///
/// # Safety
///
/// The game thread, outside the physics step; `vehicle` must be live.
unsafe fn holdable(
    vehicle: NonZeroUsize,
    door: &DoorRead,
    point: Vec3,
    normal: Vec3,
    palm: Vec3,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> Option<(NonZeroUsize, u32, Option<u32>)> {
    if !door.latched {
        let (owner, body) = door.body?;
        return Some((owner, body, None));
    }
    let past = point + (point - palm).normalize_or_zero() * CAST_PAST_M;
    let across = (point + normal * CAST_PAST_M, point - normal * CAST_PAST_M);
    [(palm, past), across]
        .into_iter()
        .filter_map(|(from, to)| cast(from, to))
        .find_map(|hit| match hit.surface {
            Surface::Solid {
                object: Some(object),
                body,
            } if unsafe { engine::vehicle_of(object) } == Some(vehicle) => {
                Some((vehicle, body, Some(body)))
            }
            _ => None,
        })
}

/// How far past the handle the casts that find a latched door's vehicle body reach, metres.
const CAST_PAST_M: f32 = 0.1;
