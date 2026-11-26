//! The climbing feet's casts: asked for at the solve gate, cast from the player aim control's
//! post-simulation update, and answered on a later gate, as the feet's ground probes are
//! ([`crate::controllers::body::ik_body::probe`]).
//!
//! Each foot asks for two casts while the hands climb: from its hip along Rico's facing, and from
//! its ankle forward and down. The nearest hit on a face that looks back along the cast is the
//! foot's candidate brace ([`body_rig::brace`]), named with its support
//! ([`crate::controllers::body::ik_body::support`]) so a brace on a moving body is carried with it.

use std::time::{Duration, Instant};

use body_rig::{brace::BraceHit, contact::Side};
use glam::Vec3;
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    body::ik_body::support,
};

/// A cast a foot asks for, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct BraceCast {
    pub from: Vec3,
    /// The cast's direction, unit length.
    pub direction: Vec3,
    /// How far it reaches, metres.
    pub length: f32,
}

/// Ask for `side`'s casts `casts`, and return the latest answer while it is fresh.
pub(crate) fn ask(side: Side, casts: [BraceCast; 2], now: Instant) -> Option<BraceHit> {
    let mut slots = SLOTS.lock();
    let slot = &mut slots[side.index()];
    slot.request = casts
        .iter()
        .all(|cast| cast.from.is_finite() && cast.direction.is_finite() && cast.length > 0.0)
        .then_some(casts);
    slot.answer
        .filter(|answer| now.saturating_duration_since(answer.when) <= ANSWER_FOR)
        .and_then(|answer| answer.hit)
}

/// Cast every foot's casts that the gate asked for recently, for the local player `character`.
///
/// # Safety
///
/// `character` must be the live local character, in the simulation phase outside the physics
/// step.
pub(crate) unsafe fn tick(character: &Character, now: Instant) {
    let requests = {
        let mut slots = SLOTS.lock();
        slots.each_mut().map(|slot| slot.request.take())
    };
    if requests.iter().all(Option::is_none) {
        return;
    }
    // SAFETY: the caller's contract: the live local character, on the thread updating it.
    let options = unsafe { CastOptions::ignoring_rider(character) };
    for (index, request) in requests.into_iter().enumerate() {
        let Some(casts) = request else {
            continue;
        };
        let hit = casts
            .iter()
            // SAFETY: forwarded from the caller.
            .filter_map(|cast| unsafe { cast_one(cast, &options, now) })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, hit)| hit);
        SLOTS.lock()[index].answer = Some(Answer { hit, when: now });
    }
}

/// Drop every request and answer: a teleport, or the climb's end.
pub(crate) fn forget() {
    *SLOTS.lock() = [Slot::EMPTY; 2];
}

/// The hit of `cast` on a face that looks back along it, with its distance.
///
/// # Safety
///
/// As [`tick`]; the ignored objects are the live local character and the vehicle it rides, or zero.
unsafe fn cast_one(
    cast: &BraceCast,
    options: &CastOptions,
    now: Instant,
) -> Option<(f32, BraceHit)> {
    let ray = Ray::new(cast.from, cast.direction)?;
    let hit = match unsafe { raycast::cast(&ray, cast.length, options) } {
        Ok(hit) => hit?,
        Err(error) => {
            tracing::debug!(target: "body", "ik body: brace cast: {error}");
            return None;
        }
    };
    let normal = hit.normal.try_normalize()?;
    if normal.dot(-cast.direction) <= 0.0 {
        return None;
    }
    // SAFETY: the simulation phase, outside the physics step.
    let support = unsafe { support::identify(hit.body, now) };
    Some((
        hit.distance,
        BraceHit {
            point: hit.position,
            normal,
            support,
        },
    ))
}

/// An answer stands this long: a few ticks, so a brace taken from it is near where the leg is.
const ANSWER_FOR: Duration = Duration::from_millis(200);

#[derive(Debug, Copy, Clone, PartialEq)]
struct Answer {
    hit: Option<BraceHit>,
    when: Instant,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Slot {
    request: Option<[BraceCast; 2]>,
    answer: Option<Answer>,
}

impl Slot {
    const EMPTY: Self = Self {
        request: None,
        answer: None,
    };
}

static SLOTS: Mutex<[Slot; 2]> = Mutex::new([Slot::EMPTY; 2]);

#[cfg(test)]
mod tests {
    use body_rig::contact::SupportId;

    use super::*;

    #[test]
    fn a_cast_that_is_not_finite_asks_for_nothing_and_no_answer_is_stale() {
        forget();
        let now = Instant::now();
        let bad = BraceCast {
            from: Vec3::NAN,
            direction: Vec3::NEG_Z,
            length: 1.0,
        };
        assert_eq!(ask(Side::Left, [bad; 2], now), None);
        assert_eq!(SLOTS.lock()[0].request, None);
        SLOTS.lock()[0].answer = Some(Answer {
            hit: Some(BraceHit {
                point: Vec3::ZERO,
                normal: Vec3::Z,
                support: SupportId::WORLD,
            }),
            when: now,
        });
        let good = BraceCast {
            from: Vec3::Y,
            direction: Vec3::NEG_Z,
            length: 1.0,
        };
        assert!(ask(Side::Left, [good; 2], now).is_some());
        assert!(SLOTS.lock()[0].request.is_some());
        assert_eq!(ask(Side::Left, [good; 2], now + ANSWER_FOR * 2), None);
        forget();
    }
}
