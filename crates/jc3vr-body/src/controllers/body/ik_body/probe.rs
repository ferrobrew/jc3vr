//! The feet's ground probes: asked for at the solve gate, cast from the player aim control's
//! post-simulation update, and answered on a later gate.
//!
//! The solve gate runs inside the pose-finalization pass, where the mod does not cast, so a foot's
//! probe is posted to a mailbox, one per foot, and cast where the head probe casts
//! ([`crate::controllers::body::rig::collision`]): the simulation phase, outside the physics step.
//! The answer reaches the contact model a tick or two after the step is planned, well inside a
//! swing, and a step whose answer has not come by its landing waits a little for it
//! ([`body_rig::contact::ContactTuning::landing_wait_s`]).
//!
//! The probe names the body it lands on as the foot's support ([`support::identify`]), and the
//! same update snapshots every moving support the gate still asks about ([`support::tick`]).

use std::time::{Duration, Instant};

use body_rig::contact::{Ground, Probe, Side};
use glam::{Vec2, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    body::ik_body::{brace, support},
};

/// The probe for `side` at the world floor point `at`, near the height `near`: its answer, once a
/// cast of that point has been made recently, and otherwise asked for.
pub(crate) fn ask(side: Side, at: Vec2, near: f32) -> Probe {
    if !at.is_finite() || !near.is_finite() {
        return Probe::Miss;
    }
    let now = Instant::now();
    let mut probes = PROBES.lock();
    let slot = &mut probes[side.index()];
    if let Some(answer) = slot.answer
        && answer.at.distance(at) <= SAME_POINT_M
        && (answer.near - near).abs() <= SAME_HEIGHT_M
        && now.saturating_duration_since(answer.when) <= ANSWER_FOR
    {
        return match answer.ground {
            Some(ground) => Probe::Hit(ground),
            None => Probe::Miss,
        };
    }
    let same = slot
        .request
        .is_some_and(|r| r.at.distance(at) <= SAME_POINT_M);
    if !same {
        slot.request = Some(Request { at, near });
    }
    Probe::Pending
}

/// Cast every probe asked for, for the local player `character`.
///
/// # Safety
///
/// `character` must be the live local character, in the simulation phase outside the physics
/// step.
pub(crate) unsafe fn tick(character: &Character) {
    let now = Instant::now();
    let requests = {
        let probes = PROBES.lock();
        [probes[0].request, probes[1].request]
    };
    if requests.iter().any(Option::is_some) {
        // SAFETY: forwarded from the caller.
        unsafe { cast(character, requests, now) };
    }
    // SAFETY: forwarded from the caller.
    unsafe { brace::tick(character, now) };
    // SAFETY: the simulation phase, outside the physics step.
    unsafe { support::tick(now) };
}

/// Drop every request and answer, and every support: a teleport.
pub(crate) fn forget() {
    *PROBES.lock() = [Slot::EMPTY; 2];
    support::forget();
}

/// Cast the probes `requests` asked for.
///
/// # Safety
///
/// As [`tick`].
unsafe fn cast(character: &Character, requests: [Option<Request>; 2], now: Instant) {
    let options = cast_options(character);
    for (index, request) in requests.into_iter().enumerate() {
        let Some(request) = request else {
            continue;
        };
        let from = Vec3::new(request.at.x, request.near + ABOVE_M, request.at.y);
        let ground = Ray::new(from, Vec3::NEG_Y).and_then(|ray| {
            match unsafe { raycast::cast(&ray, ABOVE_M + BELOW_M, &options) } {
                Ok(hit) => hit.map(|hit| Ground {
                    height: hit.position.y,
                    // SAFETY: the simulation phase, outside the physics step.
                    support: unsafe { support::identify(hit.body, now) },
                }),
                Err(error) => {
                    tracing::debug!(target: "body", "ik body: ground probe: {error}");
                    None
                }
            }
        });
        let mut probes = PROBES.lock();
        let slot = &mut probes[index];
        slot.answer = Some(Answer {
            at: request.at,
            near: request.near,
            ground,
            when: now,
        });
        if slot.request == Some(request) {
            slot.request = None;
        }
    }
}

fn cast_options(character: *const Character) -> CastOptions {
    CastOptions {
        ignore: [character as usize, 0],
        ..CastOptions::default()
    }
}

/// How far above the height asked about a probe starts, metres: a step up.
const ABOVE_M: f32 = 0.6;

/// How far below it a probe reaches, metres: a step down; past it, the foot is unsupported.
const BELOW_M: f32 = 0.9;

/// A request this close to an answer's point is answered by it, metres.
const SAME_POINT_M: f32 = 0.01;

/// ...and this close to its height, metres.
const SAME_HEIGHT_M: f32 = 0.3;

/// An answer stands for its point this long.
const ANSWER_FOR: Duration = Duration::from_millis(500);

#[derive(Debug, Copy, Clone, PartialEq)]
struct Request {
    at: Vec2,
    near: f32,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Answer {
    at: Vec2,
    near: f32,
    ground: Option<Ground>,
    when: Instant,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Slot {
    request: Option<Request>,
    answer: Option<Answer>,
}

impl Slot {
    const EMPTY: Self = Self {
        request: None,
        answer: None,
    };
}

static PROBES: Mutex<[Slot; 2]> = Mutex::new([Slot::EMPTY; 2]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foot_probes_ignore_only_the_supplied_character() {
        let character = std::ptr::dangling::<Character>();
        let options = cast_options(character);

        assert_eq!(options.ignore, [character as usize, 0]);
        assert!(!options.see_through);
        assert!(!options.material);
    }
}
