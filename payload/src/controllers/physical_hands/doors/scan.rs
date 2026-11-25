//! The doors near the hands, offered by their handles whatever the reach's rays hit: every door of
//! every vehicle near the player is a target at its handle ([`door_handle`]: the embedded table's,
//! or the box guess for a door the table lacks) for a palm within
//! [`HANDLE_REACH_M`] of it. A latched door is merged into its vehicle's body, so the
//! grip that takes it needs that body, which proves the vehicle alive while the hand holds the
//! latch: a short cast from the palm to the handle finds it. What each door near a palm was, and
//! why it was not offered, is noted for the grip press's log.
//!
//! The vehicles come from the target system's vehicle set
//! ([`crate::controllers::aim::targets`]), read on the aim update's thread as the palms' tick runs,
//! and from the vehicles a hand's reach rays hit this tick: the set does not keep every vehicle
//! near the player (a parked car can be missing from it), and a door a hand is at is on a vehicle
//! its rays meet.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{
    physics::{GameMaterialId, PhysicsGameObject},
    vehicle::vehicle::{DoorSlot, Vehicle},
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::targets::{self, TargetSet},
    physical_hands::{
        bodies::is_type,
        contact::{Surface, SurfaceHit},
        doors::{
            DoorTarget, DoorsConfig, HANDLE_REACH_M,
            acquisition::PanelReach,
            engine::{self, DoorRead},
            geometry::{DoorKind, door_handle},
            observed_reach, slot_label, table,
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
/// The game thread, outside the physics step; `vehicles` must be live vehicles this tick: from
/// [`near`], or hit by the reach's rays.
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
    let reach_m = HANDLE_REACH_M * scale;
    let mut best: Option<Reach> = None;
    for &vehicle in vehicles {
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
            if table::is_canopy(door.part_hash) {
                let candidate = canopy_hits(&door, palm, reach_m, cast)
                    .into_iter()
                    .flatten()
                    .filter_map(|hit| {
                        let Surface::Solid {
                            object: Some(object),
                            body,
                        } = hit.surface
                        else {
                            return None;
                        };
                        let reached = unsafe {
                            observed_reach(
                                object,
                                body,
                                &hit,
                                palm,
                                PanelReach::uniform(reach_m),
                                config,
                                scan,
                            )
                        }?;
                        matches!(
                            reached.kind,
                            TargetKind::Door(target)
                                if target.vehicle == vehicle && target.part_index == door.part_index
                        )
                        .then_some(reached)
                    })
                    .min_by(|a, b| a.distance.total_cmp(&b.distance));
                if let Some(reached) = candidate {
                    entry.object = Some(reached.object);
                    entry.body = Some(reached.body);
                    entry.point = Some(reached.point);
                    entry.distance = Some(reached.distance);
                    seen(reached.point, HandleOutcome::InReach);
                    note(notes, format!("{what}: the canopy surface is in reach"));
                    if best.is_none_or(|b| reached.distance < b.distance) {
                        best = Some(reached);
                    }
                } else {
                    entry.result = ScanResult::NoBody;
                }
                scan.push(entry);
                continue;
            }
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
                            HANDLE_REACH_M * 1000.0
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
                    outward: handle.normal,
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
pub(crate) fn kind_of(slot: DoorSlot) -> DoorKind {
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
/// palm at `palm` and, failing that, across the face at the handle. The native vehicle body is a
/// fallback when the handle lies outside the collision shape; its current owner must be the vehicle.
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
    let mut body = None;
    for (from, to) in [(palm, past), across] {
        let Some(hit) = cast(from, to) else {
            continue;
        };
        if hit.material == Some(GameMaterialId::EGameMaterialId_Glass as u32) {
            return None;
        }
        if let Some(hit_body) = vehicle_hit(vehicle, &hit) {
            body = Some(hit_body);
            break;
        }
    }
    let body = body.or_else(|| unsafe { engine::vehicle_body(vehicle) })?;
    Some((vehicle, body, Some(body)))
}

fn canopy_hits(
    door: &DoorRead,
    palm: Vec3,
    reach_m: f32,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> [Option<SurfaceHit>; 2] {
    let Some(door_box) = door.door_box.filter(|_| table::is_canopy(door.part_hash)) else {
        return [None; 2];
    };
    if door_box.reach(palm, reach_m).is_none() {
        return [None; 2];
    }
    let Some(nearest) = door_box.surface_point(palm) else {
        return [None; 2];
    };
    [nearest, door_box.centre()].map(|point| {
        let direction = (point - palm).normalize_or_zero();
        if direction == Vec3::ZERO {
            return None;
        }
        cast(palm, point + direction * CAST_PAST_M)
    })
}

fn vehicle_hit(vehicle: NonZeroUsize, hit: &SurfaceHit) -> Option<u32> {
    if hit.material == Some(GameMaterialId::EGameMaterialId_Glass as u32) {
        return None;
    }
    match hit.surface {
        Surface::Solid {
            object: Some(object),
            body,
        } if object == vehicle => Some(body),
        _ => None,
    }
}

/// How far past the handle the casts that find a latched door's vehicle body reach, metres.
const CAST_PAST_M: f32 = 0.1;

#[cfg(test)]
mod tests {
    use glam::Mat4;

    use super::*;
    use crate::controllers::physical_hands::doors::geometry::DoorBox;

    fn canopy() -> DoorRead {
        let entry = table::parse(include_str!(
            "../../../../../tools/door_handles/door_handles.json"
        ))
        .unwrap()
        .into_iter()
        .find(|entry| entry.part == "fighterjet_door_f")
        .unwrap();
        DoorRead {
            slot: DoorSlot::FRONT_LEFT,
            part_index: 1,
            latched: true,
            door_box: Some(
                DoorBox::new(
                    Mat4::IDENTITY,
                    Mat4::IDENTITY,
                    Vec3::new(-1.0, 1.0, -2.0),
                    Vec3::new(1.0, 2.0, 2.0),
                )
                .unwrap(),
            ),
            body: None,
            part_hash: Some(entry.part_hash),
        }
    }

    #[test]
    fn a_nearby_canopy_with_a_distant_handle_is_probed_at_the_palm() {
        let door = canopy();
        let palm = Vec3::new(-1.05, 1.5, 1.5);
        let handle = door_handle(
            &door.door_box.unwrap(),
            kind_of(door.slot),
            door.part_hash.and_then(table::lookup),
        )
        .unwrap();
        assert!(palm.distance(handle.point) > 1.0);
        let hit = SurfaceHit {
            surface: Surface::Solid {
                object: NonZeroUsize::new(8),
                body: 5,
            },
            position: Vec3::new(-1.0, 1.5, 1.5),
            normal: Vec3::NEG_X,
            material: Some(GameMaterialId::EGameMaterialId_Glass as u32),
        };
        let mut rays = Vec::new();
        let hits = canopy_hits(&door, palm, 0.1, &mut |from, to| {
            rays.push((from, to));
            Some(hit)
        });
        assert_eq!(hits, [Some(hit); 2]);
        assert_eq!(rays[0].0, palm);
        assert!(rays[0].1.abs_diff_eq(Vec3::new(-0.9, 1.5, 1.5), 1e-5));
    }

    #[test]
    fn a_canopy_box_with_no_ray_hit_offers_no_surface() {
        let door = canopy();
        assert_eq!(
            canopy_hits(&door, Vec3::new(-1.05, 1.5, 1.5), 0.1, &mut |_, _| None),
            [None; 2]
        );
    }

    #[test]
    fn distant_canopies_and_ordinary_doors_do_not_cast_panel_probes() {
        let mut door = canopy();
        let mut unexpected_cast = |_, _| panic!("no panel probes outside a nearby known canopy");
        assert_eq!(
            canopy_hits(&door, Vec3::new(-2.0, 1.5, 1.5), 0.1, &mut unexpected_cast),
            [None; 2]
        );
        door.part_hash = None;
        assert_eq!(
            canopy_hits(&door, Vec3::new(-1.05, 1.5, 1.5), 0.1, &mut unexpected_cast),
            [None; 2]
        );
    }

    #[test]
    fn ordinary_vehicle_glass_cannot_prove_a_handle_body() {
        let vehicle = NonZeroUsize::new(8).unwrap();
        let hit = SurfaceHit {
            surface: Surface::Solid {
                object: Some(vehicle),
                body: 5,
            },
            position: Vec3::ZERO,
            normal: Vec3::X,
            material: Some(GameMaterialId::EGameMaterialId_Glass as u32),
        };
        assert_eq!(vehicle_hit(vehicle, &hit), None);
    }

    #[test]
    fn an_ordinary_glass_hit_does_not_fall_back_to_the_native_vehicle_body() {
        let vehicle = NonZeroUsize::new(8).unwrap();
        let mut door = canopy();
        door.part_hash = None;
        let result = unsafe {
            holdable(
                vehicle,
                &door,
                Vec3::ZERO,
                Vec3::X,
                Vec3::X * 0.05,
                &mut |_, _| {
                    Some(SurfaceHit {
                        surface: Surface::Solid {
                            object: Some(vehicle),
                            body: 5,
                        },
                        position: Vec3::ZERO,
                        normal: Vec3::X,
                        material: Some(GameMaterialId::EGameMaterialId_Glass as u32),
                    })
                },
            )
        };
        assert_eq!(result, None);
    }

    #[test]
    fn a_vehicle_part_cannot_prove_ownership_of_the_vehicle_body() {
        let vehicle = NonZeroUsize::new(8).unwrap();
        let mut hit = SurfaceHit {
            surface: Surface::Solid {
                object: NonZeroUsize::new(16),
                body: 3,
            },
            position: Vec3::ZERO,
            normal: Vec3::X,
            material: None,
        };
        assert_eq!(vehicle_hit(vehicle, &hit), None);
        hit.surface = Surface::Solid {
            object: Some(vehicle),
            body: 5,
        };
        assert_eq!(vehicle_hit(vehicle, &hit), Some(5));
    }
}
