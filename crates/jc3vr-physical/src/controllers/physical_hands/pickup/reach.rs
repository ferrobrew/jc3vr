//! What a hand could take: the nearest grabbable object a palm's rays find, or the surface it
//! presses into, or a vehicle's door, or a dropped weapon or a character the floor scan finds near
//! the hand.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{equipment::weapon::WeaponBase, physics::PfxLayer, vehicle::vehicle::Vehicle};
use jc3vr_body::controllers::{
    aim::{ray::Ray, raycast, raycast::CastOptions},
    grab::world_reach::{Reach, TargetKind},
    physics::{
        bodies,
        scan::{self, Aabb},
        vehicles,
    },
};
use parking_lot::Mutex;

use crate::controllers::physical_hands::{
    articulated,
    bodies::{classify, describe},
    characters,
    collider_view::nearby,
    contact::{Surface, SurfaceHit},
    doors,
    doors::{acquisition::PanelReach, tuning::DOORS},
    engine as hand_engine,
    grab_view::model::{RayOutcome, ReachRay},
    grasp,
    palm::PalmBox,
    pickup::{
        PICKUP, PickupTuning,
        diagnostics::scan::{Branch, Scan, ScanEntry, ScanResult},
    },
    world::World,
};

/// What the reach records besides its result: the notes for the grip press's log, the rays for the
/// grab view, the scan of the branches it evaluated, and the vehicles its rays hit.
pub(crate) struct ReachObservations<'a> {
    pub notes: &'a mut Vec<String>,
    pub rays: Option<&'a mut Vec<ReachRay>>,
    pub scan: &'a mut Scan,
    /// The vehicles the rays hit, at most [`ReachLimits::max_hit_vehicles`], for the door scan: a
    /// vehicle the game's target system does not keep, such as a parked car, still has its doors
    /// offered.
    ///
    /// [`ReachLimits::max_hit_vehicles`]: crate::controllers::physical_hands::pickup::tuning::ReachLimits::max_hit_vehicles
    pub vehicles: &'a mut Vec<NonZeroUsize>,
}

/// What a hand's reach takes, and how far it reaches.
#[derive(Copy, Clone)]
pub(crate) struct ReachTuning<'a> {
    /// The VR world scale, which scales the reaches.
    pub world_scale: f32,
    pub pickup: &'a PickupTuning,
    /// The hand bodies, whose grip joints do not make what they hold articulated.
    pub hand_bodies: [Option<u32>; 2],
    /// The live bodies of dropped weapons and characters near the hands, found once for both
    /// hands this simulation tick ([`floor_weapons`]).
    pub(crate) floor_weapons: &'a [FloorWeapon],
}

/// A dynamic body of a dropped weapon or a character near the hands, and its broad-phase bounds.
#[derive(Debug, Copy, Clone)]
pub(crate) struct FloorWeapon {
    pub object: NonZeroUsize,
    pub body: u32,
    pub bounds: Aabb,
}

/// Find the bodies of dropped weapons and characters near either hand, whichever way the wrists
/// point. The scan walks a budget of the world's dynamic bodies each tick and remembers those near
/// the hands across ticks.
///
/// # Safety
///
/// The simulation thread, outside the physics step.
pub(crate) unsafe fn floor_weapons(hands: &[Vec3], radius: f32) -> Vec<FloorWeapon> {
    let Some(first) = hands.first().filter(|point| point.is_finite()) else {
        return Vec::new();
    };
    if !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    let spread = hands
        .iter()
        .map(|point| point.distance(*first))
        .fold(0.0, f32::max);
    let Some(world) = (unsafe { World::get() }) else {
        return Vec::new();
    };
    let world_id = world.world() as *const _ as usize;
    let tuning = &PICKUP.get().floor_weapons;
    let mut scan = FLOOR_SCAN.lock();
    if scan.world != world_id {
        scan.world = world_id;
        scan.next = 0;
        scan.bodies.clear();
    }
    let discovery_radius = radius + spread + tuning.discovery_margin_m;
    let next = scan.next;
    let result = unsafe {
        scan::walk_dynamic(next, tuning.walk_budget, |body, bounds| {
            if bounds.distance(*first) <= discovery_radius
                && scan.bodies.len() < tuning.max_bodies
                && !scan.bodies.contains(&body)
                && let Some(object) = bodies::owner_of(body)
                && (hand_engine::is_character(object)
                    || matches!(classify(object, body), Ok(TargetKind::Weapon)))
            {
                scan.bodies.push(body);
            }
        })
    };
    if let Ok(slice) = result {
        scan.next = slice.next;
    }
    let mut found = Vec::new();
    scan.bodies.retain(|&body| {
        let Some(state) = (unsafe { nearby::dynamic_state(body) }) else {
            return false;
        };
        if state.aabb.distance(*first) > discovery_radius {
            return false;
        }
        let Some(object) = (unsafe { bodies::owner_of(body) }) else {
            return false;
        };
        if !unsafe { hand_engine::is_character(object) }
            && !matches!(unsafe { classify(object, body) }, Ok(TargetKind::Weapon))
        {
            return false;
        }
        if hands
            .iter()
            .any(|point| state.aabb.distance(*point) <= radius)
        {
            found.push(FloorWeapon {
                object,
                body,
                bounds: state.aabb,
            });
        }
        true
    });
    found
}

/// Where the floor scan has walked to in the world's dynamic bodies, and the bodies it remembers.
static FLOOR_SCAN: Mutex<FloorScan> = Mutex::new(FloorScan {
    world: 0,
    next: 0,
    bodies: Vec::new(),
});

struct FloorScan {
    world: usize,
    next: u32,
    bodies: Vec<u32>,
}

/// The directions of the reach rays in the wrist frame: out of the palm (-Y), along the fingers
/// (-Z), between the two, and out of either edge of the hand.
pub(crate) const REACH_DIRECTIONS: [Vec3; 5] = [
    Vec3::new(0.0, -1.0, 0.0),
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::new(0.0, -1.0, -1.0),
    Vec3::new(1.0, -0.5, 0.0),
    Vec3::new(-1.0, -0.5, 0.0),
];

/// What a hand could take: the nearest grabbable object hit by short rays out of the palm (from the
/// back of the hand, through the palm, out to the pickup's reach at the VR world scale
/// `world_scale`), or the object the palm presses into, or a vehicle's door ([`doors::reach`]), or
/// a dropped weapon or a character the floor scan found near the hand ([`floor_weapons`]). An
/// object heavier than [`PickupTuning::max_mass_kg`] is not one, nor the body `standing_on` the
/// player stands on, which a hand would lift from under the player. `cast` returns the first hit
/// between two points. What each hit was, and why it was not taken, goes into `notes` for the grip
/// press's log, and each ray, with what it hit and whether that was taken, into `rays` when given,
/// for the grab view.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn reach(
    palm: &PalmBox,
    press: Option<SurfaceHit>,
    standing_on: Option<u32>,
    tuning: &ReachTuning,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
    observations: ReachObservations<'_>,
) -> Option<Reach> {
    // SAFETY: forwarded from the caller for the native cast and hit evaluation.
    unsafe {
        reach_with(
            palm,
            press,
            standing_on,
            tuning,
            cast,
            &mut |from, to| floor_cast(from, to),
            observations,
        )
    }
}

/// [`reach`] with its casts given: `cast` on the palm's ordinary layer, and `cast_floor` on the
/// layer that meets dropped weapons, so that the tests can stand in for both.
///
/// # Safety
///
/// Hits must identify live engine objects on the main thread, outside the physics step.
/// Missing casts, no press, and no nearby surface hits do not read engine objects.
pub(crate) unsafe fn reach_with(
    palm: &PalmBox,
    press: Option<SurfaceHit>,
    standing_on: Option<u32>,
    tuning: &ReachTuning,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
    cast_floor: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
    observations: ReachObservations<'_>,
) -> Option<Reach> {
    let ReachObservations {
        notes,
        mut rays,
        scan,
        vehicles,
    } = observations;
    scan.ray_ran = true;
    let origin = palm.point(Vec3::new(0.0, 1.0, 0.0));
    let length = (tuning.pickup.reach_m * tuning.world_scale).max(0.0) + 2.0 * palm.half_extents.y;
    let ends = REACH_DIRECTIONS
        .map(|local| origin + (palm.orientation * local).normalize_or_zero() * length);
    if let Some(rays) = rays.as_deref_mut() {
        rays.clear();
        rays.extend(ends.map(|to| ReachRay::missed(origin, to)));
    }
    let hits = ends
        .into_iter()
        .enumerate()
        .flat_map(|(k, to)| {
            let ordinary = cast(origin, to);
            let weapon = cast_floor(origin, to).filter(|hit| {
                let Surface::Solid {
                    object: Some(object),
                    body,
                } = hit.surface
                else {
                    return false;
                };
                matches!(unsafe { classify(object, body) }, Ok(TargetKind::Weapon))
                    && surface_visible(origin, hit, ordinary.as_ref())
            });
            [
                Some((Some(k), to, ordinary)),
                weapon.map(|hit| (Some(k), to, Some(hit))),
            ]
            .into_iter()
            .flatten()
        })
        .chain(press.map(|hit| (None, hit.position, Some(hit))));
    let mut judged = Judged {
        seen: Vec::new(),
        reaches: Vec::new(),
        notes,
        scan,
        vehicles,
        branch: Branch::Ray,
    };
    for (ray, to, hit) in hits {
        let Some(hit) = hit else {
            judged.scan.push(ScanEntry {
                branch: Branch::Ray,
                object: None,
                body: None,
                door: None,
                point: Some(to),
                distance: None,
                mass_kg: None,
                result: ScanResult::Missed,
            });
            continue;
        };
        let before = judged.reaches.len();
        judged.branch = if ray.is_some() {
            Branch::Ray
        } else {
            Branch::Press
        };
        // SAFETY: forwarded from the caller; the hit was found this tick.
        unsafe { judge(&hit, palm, standing_on, tuning, &mut judged) };
        if let (Some(k), Some(rays)) = (ray, rays.as_deref_mut())
            && let Some(recorded) = rays.get_mut(k)
        {
            recorded.hit = Some(hit.position);
            recorded.outcome = RayOutcome::of(true, judged.reaches.len() > before);
        }
    }
    for weapon in tuning.floor_weapons {
        if standing_on == Some(weapon.body) {
            continue;
        }
        let Some(mut reach) = floor_reach(weapon, palm, tuning, &mut |from, to| {
            let hit = cast_floor(from, to)?;
            surface_visible(from, &hit, cast(from, to).as_ref()).then_some(hit)
        }) else {
            if judged.notes.len() < tuning.pickup.reach.max_notes {
                judged.notes.push(format!(
                    "a nearby dynamic body, body {}: no unobstructed surface in reach",
                    weapon.body
                ));
            }
            continue;
        };
        if unsafe { hand_engine::is_character(reach.object) } {
            let hit = SurfaceHit {
                position: reach.point,
                normal: reach.normal,
                surface: Surface::Character {
                    object: reach.object,
                    body: reach.body,
                },
                material: None,
            };
            let Ok(character) = (unsafe {
                characters::reach(
                    reach.object,
                    reach.body,
                    &hit,
                    palm.centre,
                    tuning.world_scale,
                )
            }) else {
                continue;
            };
            reach = character;
        } else if !matches!(
            unsafe { classify(reach.object, reach.body) },
            Ok(TargetKind::Weapon)
        ) || unsafe { bodies::read(reach.body, reach.object) }.is_err()
        {
            continue;
        }
        judged.scan.push(ScanEntry {
            branch: Branch::Ray,
            object: Some(reach.object),
            body: Some(reach.body),
            door: None,
            point: Some(reach.point),
            distance: Some(reach.distance),
            mass_kg: None,
            result: ScanResult::Offered,
        });
        if judged.notes.len() < tuning.pickup.reach.max_notes {
            judged.notes.push(format!(
                "a nearby dynamic body, body {}: in surface reach",
                reach.body
            ));
        }
        judged.reaches.push(reach);
    }
    let selected = grasp::nearest(judged.reaches);
    judged.scan.ray_selected = selected;
    selected
}

fn surface_visible(from: Vec3, hit: &SurfaceHit, blocker: Option<&SurfaceHit>) -> bool {
    blocker.is_none_or(|blocker| {
        let owner = |surface| match surface {
            Surface::Character { object, .. } => Some(object),
            Surface::Solid { object, .. } => object,
        };
        (owner(hit.surface).is_some() && owner(hit.surface) == owner(blocker.surface))
            || blocker.position.distance_squared(from) >= hit.position.distance_squared(from)
    })
}

fn palm_surface_distance(palm: &PalmBox, point: Vec3) -> f32 {
    let local = palm.orientation.inverse() * (point - palm.centre);
    (local.abs() - palm.half_extents.abs())
        .max(Vec3::ZERO)
        .length()
}

/// The first hit between `from` and `to` on the vehicle raycast layer, which meets dropped weapons'
/// bodies: the palm's ordinary query layer passes through them.
unsafe fn floor_cast(from: Vec3, to: Vec3) -> Option<SurfaceHit> {
    let ray = Ray::new(from, to - from)?;
    let options = unsafe { CastOptions::ignoring_player(0) };
    let hit = unsafe {
        raycast::cast_on(
            &ray,
            from.distance(to),
            &options,
            PfxLayer::VEHICLE_RAYCAST as i32,
        )
    }
    .ok()??;
    Some(SurfaceHit {
        position: hit.position,
        normal: hit.normal,
        surface: Surface::Solid {
            object: hit.object,
            body: hit.body,
        },
        material: None,
    })
}

/// The reach of a body the floor scan found: casts from the palm toward the nearest point of the
/// body's bounds and toward their centre, of which only a hit on that body itself, within the
/// pickup's reach of the palm's surface, counts.
fn floor_reach(
    weapon: &FloorWeapon,
    palm: &PalmBox,
    tuning: &ReachTuning,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> Option<Reach> {
    let reach = tuning.pickup.reach_m * tuning.world_scale;
    if !reach.is_finite()
        || reach <= 0.0
        || !palm.centre.is_finite()
        || !palm.orientation.is_finite()
        || !palm.half_extents.is_finite()
        || !palm.orientation.is_normalized()
        || !weapon.bounds.min.is_finite()
        || !weapon.bounds.max.is_finite()
        || !weapon.bounds.min.cmple(weapon.bounds.max).all()
    {
        return None;
    }
    let nearest = palm.centre.clamp(weapon.bounds.min, weapon.bounds.max);
    for toward in [nearest, weapon.bounds.centre()] {
        let direction = (toward - palm.centre).normalize_or_zero();
        if direction == Vec3::ZERO {
            continue;
        }
        let local = palm.orientation.inverse() * direction;
        let extent = local.abs().dot(palm.half_extents.abs());
        let length = reach + extent;
        let Some(hit) = cast(palm.centre, palm.centre + direction * length) else {
            continue;
        };
        let matches_body = match hit.surface {
            Surface::Solid {
                object: Some(object),
                body,
            }
            | Surface::Character { object, body } => object == weapon.object && body == weapon.body,
            _ => false,
        };
        if !matches_body
            || !hit.position.is_finite()
            || !hit.normal.is_finite()
            || palm_surface_distance(palm, hit.position) > reach
        {
            continue;
        }
        let distance = hit.position.distance(palm.centre);
        if distance > length {
            continue;
        }
        return Some(Reach {
            object: weapon.object,
            body: weapon.body,
            point: hit.position,
            distance,
            normal: hit.normal,
            kind: TargetKind::Weapon,
        });
    }
    None
}

/// What the reach's hits have come to so far.
struct Judged<'a> {
    /// The bodies already noted: the rays fan out from one palm, so most hit the same body.
    seen: Vec<(Option<NonZeroUsize>, u32)>,
    /// The hits a hand takes.
    reaches: Vec<Reach>,
    notes: &'a mut Vec<String>,
    scan: &'a mut Scan,
    vehicles: &'a mut Vec<NonZeroUsize>,
    branch: Branch,
}

/// Judge one of the reach's hits: add it to `judged`'s reaches when a hand takes it, and note what
/// it was and why it was not taken, once per body.
///
/// # Safety
///
/// The main thread, outside the physics step; `hit` must come from a cast this tick.
unsafe fn judge(
    hit: &SurfaceHit,
    palm: &PalmBox,
    standing_on: Option<u32>,
    tuning: &ReachTuning,
    judged: &mut Judged,
) {
    let mut hit = *hit;
    if let Surface::Solid {
        object: Some(object),
        body,
    } = hit.surface
        && unsafe { hand_engine::is_character(object) }
    {
        hit.surface = Surface::Character { object, body };
    }
    let hit = &hit;
    let ReachTuning {
        world_scale,
        pickup: config,
        ..
    } = *tuning;
    let (object, body) = match hit.surface {
        Surface::Character { object, body } => (Some(object), body),
        Surface::Solid { object, body } => (object, body),
    };
    let first = !judged.seen.contains(&(object, body));
    if first {
        judged.seen.push((object, body));
    }
    let notes = &mut *judged.notes;
    let max_notes = config.reach.max_notes;
    let mut note = |why: String| {
        if first && notes.len() < max_notes {
            notes.push(why);
        }
    };
    let distance = hit.position.distance(palm.centre);
    let mut entry = ScanEntry {
        branch: judged.branch,
        object,
        body: Some(body),
        door: None,
        point: Some(hit.position),
        distance: Some(distance),
        mass_kg: None,
        result: ScanResult::Offered,
    };
    let at = format!(
        "body {body} at {:.0} mm",
        distance / world_scale.max(1e-3) * 1000.0
    );
    if let Surface::Character { object, body } = hit.surface {
        // SAFETY: forwarded from the caller; the hit was found this tick.
        match unsafe { characters::reach(object, body, hit, palm.centre, world_scale) } {
            Ok(reach) => {
                let what = match reach.kind {
                    TargetKind::Character(target) if target.ragdoll => "a character's ragdoll",
                    _ => "a character",
                };
                note(format!("{what}, {at}: in reach"));
                judged.reaches.push(reach);
            }
            Err(why) => {
                entry.result = ScanResult::CharacterUnavailable;
                note(format!("a character, {at}: not taken, {}", why.label()));
            }
        }
        judged.scan.push(entry);
        return;
    }
    let Surface::Solid {
        object: Some(object),
        body,
    } = hit.surface
    else {
        entry.result = ScanResult::StaticWorld;
        judged.scan.push(entry);
        note(format!("the static world, {at}: climbed, not taken"));
        return;
    };
    // SAFETY: forwarded from the caller; the hit was found this tick.
    if unsafe { bodies::is_type(object, Vehicle::TYPE_ID()) }
        && !judged.vehicles.contains(&object)
        && judged.vehicles.len() < config.reach.max_hit_vehicles
    {
        judged.vehicles.push(object);
    }
    // SAFETY: forwarded from the caller; the hit was found this tick.
    let what = unsafe { describe(object) };
    // A vehicle's door, open or closed, wherever the hand reaches it, before anything else: the
    // player may stand on the vehicle whose door they open.
    // SAFETY: forwarded from the caller; the hit was found this tick.
    if let Some(door) = unsafe {
        doors::observed_reach(
            object,
            body,
            hit,
            palm.centre,
            PanelReach {
                box_m: DOORS.get().grip.box_reach_m * world_scale,
                handle_m: DOORS.get().grip.handle_reach_m * world_scale,
            },
            judged.scan,
        )
    } {
        note(format!("{what}, {at}: a door in reach"));
        if let TargetKind::Door(target) = door.kind {
            entry.door = Some((target.vehicle, target.slot, target.part_index));
        }
        judged.scan.push(entry);
        judged.reaches.push(door);
        return;
    }
    if standing_on == Some(body) {
        entry.result = ScanResult::StandingOn;
        judged.scan.push(entry);
        note(format!("{what}, {at}: the player stands on it"));
        return;
    }
    // SAFETY: forwarded from the caller; the hit was found this tick.
    let classified = unsafe { classify(object, body) };
    // A body the classification refuses stays refused rather than being taken as articulated. A
    // body joined to the world or another body is taken whatever its weight, since its constraint
    // carries it.
    // SAFETY: forwarded from the caller.
    let articulated = match classified {
        Ok(TargetKind::Weapon) => None,
        Err(_) => None,
        _ if unsafe { bodies::is_type(object, WeaponBase::TYPE_ID()) } => None,
        _ => unsafe { articulated_kind(object, body, tuning) },
    };
    let kind = match (articulated, classified) {
        (Some(articulated), _) => articulated,
        (None, Ok(kind)) => kind,
        (None, Err(why)) => {
            entry.result = ScanResult::Unclassified(why);
            judged.scan.push(entry);
            note(format!("{what}, {at}: not taken, {why}"));
            return;
        }
    };
    entry.mass_kg = match kind {
        TargetKind::Prop { mass_kg } | TargetKind::Articulated { mass_kg } => Some(mass_kg),
        _ => None,
    };
    if let TargetKind::Prop { mass_kg } = kind
        && !grasp::can_hold(mass_kg, config)
    {
        entry.result = ScanResult::TooHeavy;
        judged.scan.push(entry);
        note(format!(
            "{what}, {at}: not taken, {mass_kg:.0} kg is over the {:.0} kg a hand takes",
            config.max_mass_kg
        ));
        return;
    }
    judged.scan.push(entry);
    note(format!("{what}, {at}: in reach"));
    judged.reaches.push(Reach {
        object,
        body,
        point: hit.position,
        distance,
        normal: hit.normal,
        kind,
    });
}

/// What the object at `object`, reached on its dynamic body `body`, is when the body is joined to
/// the world or another body by an enabled constraint: an articulated object, light enough for
/// its constraint to carry. `None` for a vehicle or a part still on it, which its doors' reach
/// handles, and for a body that is not joined. A part that has come off its vehicle is judged as
/// any other object.
///
/// # Safety
///
/// The main thread, outside the physics step; `object` and `body` must come from a cast this tick.
unsafe fn articulated_kind(
    object: NonZeroUsize,
    body: u32,
    tuning: &ReachTuning,
) -> Option<TargetKind> {
    if unsafe { vehicles::vehicle_of(object) }.is_some()
        && !unsafe { doors::engine::broken_off(object, body) }
    {
        return None;
    }
    let mass_kg = unsafe { bodies::read(body, object) }.ok()?.mass_kg?;
    if !grasp::can_hold_articulated(mass_kg, tuning.pickup) {
        return None;
    }
    unsafe { articulated::joined(body, tuning.hand_bodies) }
        .map(|_| TargetKind::Articulated { mass_kg })
}

#[cfg(test)]
mod tests {
    use glam::Quat;
    use jc3vr_body::controllers::grab::{
        GrabVolume, GripTarget, Mark, MarkAt, PickupKind, RemoteKind, resolve,
    };
    use jc3vr_input::frame::Hand;

    use super::*;
    use crate::controllers::physical_hands::pickup::PICKUP;

    fn weapon() -> FloorWeapon {
        FloorWeapon {
            object: NonZeroUsize::new(1).unwrap(),
            body: 7,
            bounds: Aabb {
                min: Vec3::new(-0.3, -0.03, -0.03),
                max: Vec3::new(0.3, 0.03, 0.03),
            },
        }
    }

    fn hit(weapon: FloorWeapon, position: Vec3) -> SurfaceHit {
        SurfaceHit {
            position,
            normal: Vec3::Y,
            surface: Surface::Solid {
                object: Some(weapon.object),
                body: weapon.body,
            },
            material: None,
        }
    }

    #[test]
    fn nearby_floor_probes_use_the_injected_layer_without_engine_reads() {
        let pickup = *PICKUP.get();
        let tuning = ReachTuning {
            world_scale: 1.0,
            pickup: &pickup,
            hand_bodies: [None; 2],
            floor_weapons: &[weapon()],
        };
        let palm = PalmBox {
            centre: Vec3::new(0.2, 0.11, 0.0),
            orientation: Quat::IDENTITY,
            half_extents: Vec3::splat(0.02),
        };
        let mut ordinary_casts = 0;
        let mut floor_casts = 0;
        // SAFETY: both cast layers miss, so the fixture's object is never read.
        let reached = unsafe {
            reach_with(
                &palm,
                None,
                None,
                &tuning,
                &mut |_, _| {
                    ordinary_casts += 1;
                    None
                },
                &mut |_, _| {
                    floor_casts += 1;
                    None
                },
                ReachObservations {
                    notes: &mut Vec::new(),
                    rays: None,
                    scan: &mut Scan::new(),
                    vehicles: &mut Vec::new(),
                },
            )
        };
        assert!(reached.is_none());
        assert_eq!(ordinary_casts, REACH_DIRECTIONS.len());
        assert_eq!(floor_casts, REACH_DIRECTIONS.len() + 2);
    }

    #[test]
    fn nearby_character_surface_requires_the_exact_body_and_owner() {
        let pickup = *PICKUP.get();
        let tuning = ReachTuning {
            world_scale: 1.0,
            pickup: &pickup,
            hand_bodies: [None; 2],
            floor_weapons: &[],
        };
        let palm = PalmBox {
            centre: Vec3::new(0.2, 0.11, 0.0),
            orientation: Quat::IDENTITY,
            half_extents: Vec3::splat(0.02),
        };
        for (owner, body) in [
            (weapon().object, weapon().body),
            (weapon().object, 8),
            (NonZeroUsize::new(2).unwrap(), weapon().body),
        ] {
            let reached = floor_reach(&weapon(), &palm, &tuning, &mut |_, _| {
                Some(SurfaceHit {
                    surface: Surface::Character {
                        object: owner,
                        body,
                    },
                    ..hit(weapon(), Vec3::new(0.2, 0.03, 0.0))
                })
            });
            assert_eq!(
                reached.is_some(),
                owner == weapon().object && body == weapon().body
            );
        }
    }

    #[test]
    fn floor_surface_reach_does_not_depend_on_the_five_wrist_directions() {
        let pickup = *PICKUP.get();
        let tuning = ReachTuning {
            world_scale: 1.0,
            pickup: &pickup,
            hand_bodies: [None; 2],
            floor_weapons: &[],
        };
        for orientation in [
            Quat::IDENTITY,
            Quat::from_rotation_x(1.5),
            Quat::from_rotation_z(1.5),
        ] {
            let palm = PalmBox {
                centre: Vec3::new(0.2, 0.11, 0.0),
                orientation,
                half_extents: Vec3::splat(0.02),
            };
            let reached = floor_reach(&weapon(), &palm, &tuning, &mut |from, to| {
                assert!(to.y < from.y);
                Some(hit(weapon(), Vec3::new(0.2, 0.03, 0.0)))
            })
            .unwrap();
            assert_eq!(reached.body, weapon().body);
            assert!((reached.distance - 0.08).abs() < 1e-6);
            let marker = Mark::ring(MarkAt::World(reached.point), 0.05);
            let targets = [
                GripTarget::measured(
                    GrabVolume::Snatch(RemoteKind::Weapon),
                    Hand::Left,
                    0.01,
                    marker,
                ),
                GripTarget::measured(
                    GrabVolume::Pickup(PickupKind::Weapon),
                    Hand::Left,
                    reached.distance,
                    marker,
                ),
            ];
            assert_eq!(
                resolve::resolve(&targets, [Some(palm.centre), None])[0]
                    .chosen
                    .unwrap()
                    .index,
                1
            );
        }
    }

    #[test]
    fn a_diagonal_probe_cannot_extend_reach_beyond_the_palm_surface() {
        let pickup = *PICKUP.get();
        let tuning = ReachTuning {
            world_scale: 1.0,
            pickup: &pickup,
            hand_bodies: [None; 2],
            floor_weapons: &[],
        };
        let palm = PalmBox {
            centre: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            half_extents: Vec3::new(0.1, 0.01, 0.01),
        };
        let point = Vec3::new(1.0, 1.0, 0.0).normalize() * (pickup.reach_m + 0.07);
        let weapon = FloorWeapon {
            bounds: Aabb {
                min: point,
                max: point + Vec3::splat(0.01),
            },
            ..weapon()
        };
        assert!(
            floor_reach(&weapon, &palm, &tuning, &mut |_, _| {
                Some(hit(weapon, point))
            })
            .is_none()
        );
        assert!(palm_surface_distance(&palm, point) > pickup.reach_m);
    }

    #[test]
    fn the_weapon_query_does_not_reach_through_ordinary_query_blockers() {
        let target = hit(weapon(), Vec3::Z * 0.1);
        let wall = SurfaceHit {
            position: Vec3::Z * 0.05,
            surface: Surface::Solid {
                object: None,
                body: 99,
            },
            ..target
        };
        assert!(!surface_visible(Vec3::ZERO, &target, Some(&wall)));
        assert!(surface_visible(Vec3::ZERO, &target, None));
        assert!(surface_visible(Vec3::ZERO, &target, Some(&target)));
        assert!(surface_visible(
            Vec3::ZERO,
            &target,
            Some(&SurfaceHit {
                position: Vec3::Z * 0.2,
                ..wall
            })
        ));
    }

    #[test]
    fn a_broadphase_box_is_not_a_hit_and_another_body_cannot_validate_the_weapon() {
        let pickup = *PICKUP.get();
        let tuning = ReachTuning {
            world_scale: 1.0,
            pickup: &pickup,
            hand_bodies: [None; 2],
            floor_weapons: &[],
        };
        let palm = PalmBox {
            centre: Vec3::Y * 0.11,
            orientation: Quat::IDENTITY,
            half_extents: Vec3::splat(0.02),
        };
        assert!(floor_reach(&weapon(), &palm, &tuning, &mut |_, _| None).is_none());
        for wrong in [
            FloorWeapon {
                body: 8,
                ..weapon()
            },
            FloorWeapon {
                object: NonZeroUsize::new(2).unwrap(),
                ..weapon()
            },
        ] {
            assert!(
                floor_reach(&weapon(), &palm, &tuning, &mut |_, _| Some(hit(
                    wrong,
                    Vec3::Y * 0.03
                )))
                .is_none()
            );
        }
        assert!(
            floor_reach(&weapon(), &palm, &tuning, &mut |_, _| Some(hit(
                weapon(),
                Vec3::Y * -0.3
            )))
            .is_none()
        );
    }
}
