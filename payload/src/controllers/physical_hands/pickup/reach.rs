//! What a hand could take: the nearest grabbable object a palm's rays find, or the surface it
//! presses into, or a vehicle's door.

use std::num::NonZeroUsize;

use glam::Vec3;

use crate::controllers::physical_hands::{
    articulated, bodies,
    characters::{self, CharactersConfig},
    config::PickupConfig,
    contact::{Surface, SurfaceHit},
    doors::{self, DoorsConfig},
    grab_view::model::{RayOutcome, ReachRay},
    grasp::{self, Reach, TargetKind},
    palm::PalmBox,
};

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
    notes: &mut Vec<String>,
    mut rays: Option<&mut Vec<ReachRay>>,
) -> Option<Reach> {
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
        .filter_map(|(k, to)| cast(origin, to).map(|hit| (Some(k), hit)))
        .chain(press.map(|hit| (None, hit)));
    let mut judged = Judged {
        seen: Vec::new(),
        reaches: Vec::new(),
        notes,
    };
    for (ray, hit) in hits {
        let before = judged.reaches.len();
        // SAFETY: forwarded from the caller; the hit was found this tick.
        unsafe { judge(&hit, palm, standing_on, tuning, &mut judged) };
        if let (Some(k), Some(rays)) = (ray, rays.as_deref_mut())
            && let Some(recorded) = rays.get_mut(k)
        {
            recorded.hit = Some(hit.position);
            recorded.outcome = RayOutcome::of(true, judged.reaches.len() > before);
        }
    }
    grasp::nearest(judged.reaches)
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
            None => note(format!("a character, {at}: not one a hand takes here")),
        }
        return;
    }
    let Surface::Solid {
        object: Some(object),
        body,
    } = hit.surface
    else {
        note(format!("the static world, {at}: climbed, not taken"));
        return;
    };
    // SAFETY: forwarded from the caller; the hit was found this tick.
    let what = unsafe { bodies::describe(object) };
    // A vehicle's door, open or at a closed door's handle area, before anything else: the
    // player may stand on the vehicle whose door they open.
    // SAFETY: forwarded from the caller; the hit was found this tick.
    if let Some(door) = unsafe {
        doors::reach(
            object,
            body,
            hit,
            palm.centre,
            door_config.reach_m * world_scale,
            door_config,
        )
    } {
        note(format!("{what}, {at}: a door in reach"));
        judged.reaches.push(door);
        return;
    }
    if standing_on == Some(body) {
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
            note(format!("{what}, {at}: not taken, {why}"));
            return;
        }
    };
    if let TargetKind::Prop { mass_kg } = kind
        && !grasp::can_hold(mass_kg, config)
    {
        note(format!(
            "{what}, {at}: not taken, {mass_kg:.0} kg is over the {:.0} kg a hand takes",
            config.max_mass_kg
        ));
        return;
    }
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
