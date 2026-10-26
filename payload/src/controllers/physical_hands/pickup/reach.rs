//! What a hand could take: the nearest grabbable object a palm's rays find, or the surface it
//! presses into, or a vehicle's door.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{character::character::Character, physics::PfxLayer};

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    physical_hands::{
        articulated, bodies,
        characters::{self, CharactersConfig},
        collider_view::{
            self,
            model::{Aabb, BodyKind},
        },
        config::PickupConfig,
        contact::{Surface, SurfaceHit},
        doors::{self, DoorsConfig},
        grab_view::model::{RayOutcome, ReachRay},
        grasp::{self, Reach, TargetKind},
        palm::PalmBox,
        pickup::diagnostics::scan::{Branch, Scan, ScanEntry, ScanResult},
    },
};

/// The existing reach readouts and a bounded copy of its evaluated branches.
pub(crate) struct ReachObservations<'a> {
    pub notes: &'a mut Vec<String>,
    pub rays: Option<&'a mut Vec<ReachRay>>,
    pub scan: &'a mut Scan,
}

/// What a hand's reach takes, and how far it reaches.
#[derive(Copy, Clone)]
pub(crate) struct ReachTuning<'a> {
    /// The VR world scale, which scales the reaches.
    pub world_scale: f32,
    pub pickup: &'a PickupConfig,
    pub doors: &'a DoorsConfig,
    pub characters: &'a CharactersConfig,
    /// The hand bodies, whose grip joints do not make what they hold articulated.
    pub hand_bodies: [Option<u32>; 2],
    /// Live dropped-weapon bodies discovered once for both hands this simulation tick.
    pub floor_weapons: &'a [FloorWeapon],
}

/// A dropped weapon's exact dynamic body and its broad-phase bounds.
#[derive(Debug, Copy, Clone)]
pub(crate) struct FloorWeapon {
    pub object: NonZeroUsize,
    pub body: u32,
    pub bounds: Aabb,
}

/// Discover dropped weapons near either hand, independently of the wrist's pointing direction.
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
    let Ok(snapshot) = (unsafe { collider_view::scan_moving(*first, radius + spread, 64) }) else {
        return Vec::new();
    };
    snapshot
        .bodies
        .into_iter()
        .filter_map(|body| {
            if body.kind != BodyKind::Dynamic
                || !hands
                    .iter()
                    .any(|point| body.aabb.distance(*point) <= radius)
            {
                return None;
            }
            let object = unsafe { bodies::owner_of(body.id) }?;
            if !matches!(
                unsafe { bodies::classify(object, body.id) },
                Ok(TargetKind::Weapon)
            ) || unsafe { bodies::read(body.id, object) }.is_err()
            {
                return None;
            }
            Some(FloorWeapon {
                object,
                body: body.id,
                bounds: body.aabb,
            })
        })
        .collect()
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
/// `world_scale`), or the object the palm presses into, or a vehicle's door ([`doors::reach`]). An
/// object heavier than [`PickupConfig::max_mass_kg`] is not one, nor the body `standing_on` the
/// player stands on, which a hand would lift from under the player. `cast` returns the first hit
/// between two points. What each hit was, and why it was not taken, goes into `notes` for the grip
/// press's log, and each ray, with what it hit and whether that was taken, into `rays` when given,
/// for the grab view.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(crate) unsafe fn reach(
    palm: &PalmBox,
    press: Option<SurfaceHit>,
    standing_on: Option<u32>,
    tuning: &ReachTuning,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
    observations: ReachObservations<'_>,
) -> Option<Reach> {
    let ReachObservations {
        notes,
        mut rays,
        scan,
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
        .map(|(k, to)| (Some(k), to, cast(origin, to)))
        .chain(press.map(|hit| (None, hit.position, Some(hit))));
    let mut judged = Judged {
        seen: Vec::new(),
        reaches: Vec::new(),
        notes,
        scan,
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
        let Some(reach) = floor_reach(weapon, palm, tuning, &mut |from, to| unsafe {
            floor_cast(from, to)
        }) else {
            continue;
        };
        if !matches!(
            unsafe { bodies::classify(reach.object, reach.body) },
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
        if judged.notes.len() < MAX_REACH_NOTES {
            judged.notes.push(format!(
                "a dropped weapon, body {}: in surface reach",
                reach.body
            ));
        }
        judged.reaches.push(reach);
    }
    let selected = grasp::nearest(judged.reaches);
    judged.scan.ray_selected = selected;
    selected
}

/// The ordinary palm query layer excludes dropped weapons; this cast meets their real bodies.
unsafe fn floor_cast(from: Vec3, to: Vec3) -> Option<SurfaceHit> {
    let ray = Ray::new(from, to - from)?;
    let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
    let options = CastOptions {
        ignore: [player, 0],
        see_through: false,
        material: false,
    };
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

/// A body's bounds direct the probe; only a hit on that exact body establishes surface reach.
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
        if !matches!(hit.surface, Surface::Solid { object: Some(object), body }
            if object == weapon.object && body == weapon.body)
            || !hit.position.is_finite()
            || !hit.normal.is_finite()
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

/// The most notes one hand's reach keeps per tick.
const MAX_REACH_NOTES: usize = 8;

/// What the reach's hits have come to so far.
struct Judged<'a> {
    /// The bodies already noted: the rays fan out from one palm, so most hit the same body.
    seen: Vec<(Option<NonZeroUsize>, u32)>,
    /// The hits a hand takes.
    reaches: Vec<Reach>,
    notes: &'a mut Vec<String>,
    scan: &'a mut Scan,
    branch: Branch,
}

/// Judge one of the reach's hits: add it to `judged`'s reaches when a hand takes it, and note what
/// it was and why it was not taken, once per body.
///
/// # Safety
///
/// Main thread, outside the physics step; `hit` must come from a cast this tick.
unsafe fn judge(
    hit: &SurfaceHit,
    palm: &PalmBox,
    standing_on: Option<u32>,
    tuning: &ReachTuning,
    judged: &mut Judged,
) {
    let ReachTuning {
        world_scale,
        pickup: config,
        doors: door_config,
        characters: character_config,
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
    let mut note = |why: String| {
        if first && notes.len() < MAX_REACH_NOTES {
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
        match unsafe {
            characters::reach(
                object,
                body,
                hit,
                palm.centre,
                world_scale,
                character_config,
            )
        } {
            Some(reach) => {
                note(format!("a character, {at}: in reach"));
                judged.reaches.push(reach);
            }
            None => {
                entry.result = ScanResult::CharacterUnavailable;
                note(format!("a character, {at}: not one a hand takes here"));
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
    let what = unsafe { bodies::describe(object) };
    // A vehicle's door, open or at a closed door's handle area, before anything else: the
    // player may stand on the vehicle whose door they open.
    // SAFETY: forwarded from the caller; the hit was found this tick.
    if let Some(door) = unsafe {
        doors::observed_reach(
            object,
            body,
            hit,
            palm.centre,
            door_config.reach_m * world_scale,
            door_config,
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
    let classified = unsafe { bodies::classify(object, body) };
    // A body joined to the world or another body is taken whatever its weight, since its
    // constraint carries it.
    // SAFETY: forwarded from the caller.
    let articulated = match classified {
        Ok(TargetKind::Weapon) => None,
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
/// its constraint to carry. `None` for a vehicle, which its doors' reach handles, and for a body
/// that is not joined.
///
/// # Safety
///
/// Main thread, outside the physics step; `object` and `body` must come from a cast this tick.
unsafe fn articulated_kind(
    object: NonZeroUsize,
    body: u32,
    tuning: &ReachTuning,
) -> Option<TargetKind> {
    if !tuning.pickup.articulated || unsafe { doors::engine::vehicle_of(object) }.is_some() {
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

    use super::*;
    use crate::controllers::{
        frame::Hand,
        grab::{GrabVolume, GripTarget, Mark, MarkAt, PickupKind, RemoteKind, resolve},
    };

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
    fn floor_surface_reach_does_not_depend_on_the_five_wrist_directions() {
        let pickup = PickupConfig::default();
        let doors = DoorsConfig::default();
        let characters = CharactersConfig::default();
        let tuning = ReachTuning {
            world_scale: 1.0,
            pickup: &pickup,
            doors: &doors,
            characters: &characters,
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
    fn a_broadphase_box_is_not_a_hit_and_another_body_cannot_validate_the_weapon() {
        let pickup = PickupConfig::default();
        let doors = DoorsConfig::default();
        let characters = CharactersConfig::default();
        let tuning = ReachTuning {
            world_scale: 1.0,
            pickup: &pickup,
            doors: &doors,
            characters: &characters,
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
