//! The physics bodies around Rico as recording events, once per simulation step: the dynamic
//! bodies near him and his hands ([`BodiesSample`]), each introduced with its identity and shape
//! when it joins ([`BodyIntro`]), and what each hand holds ([`HeldSample`]). With the forces the
//! mod applies ([`crate::controllers::diagnostics::note_force`]), a viewer can show what Rico
//! touches and how it moves.
//!
//! A walk of every body slot streams the world's whole body array, so each step walks an eighth of
//! the slots to discover bodies and reads only the kept set otherwise: a body is found within eight
//! steps of coming near, and a held body joins at once. The step's cost is recorded with it.

use std::time::{Duration, Instant};

use jc3gi::character::character::Character;
use parking_lot::Mutex;
use tracking_format::{
    event::{BodiesSample, BodyIntro, BodySample, HeldMotion, HeldSample, HoldReport, Label},
    model::Pose,
};

use crate::{
    controllers::{
        frame::Hand,
        physical_hands::{
            self, HeldBody,
            collider_view::nearby::{self, BodyState},
            physics,
        },
    },
    frame_timing::{self, Slot},
    hooks,
    recorder::tracking::{
        events::{self, EventKind},
        format::hand_to_file,
    },
};

/// Record the bodies near Rico and what his hands hold, after a simulation step.
///
/// # Safety
///
/// The game thread, after the step's physics and outside it.
pub(crate) unsafe fn after_step() {
    if !events::collecting() || !hooks::in_gameplay() {
        *NEAR.lock() = Near::EMPTY;
        return;
    }
    let _timing = frame_timing::scope(Slot::Recorder);
    let started = Instant::now();
    // SAFETY: the game thread outside the physics step, where the local player is stable.
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return;
    };
    let origin = glam::Mat4::from(character.m_WorldMatrixT1)
        .w_axis
        .truncate();
    let hands = Hand::BOTH.map(|hand| physics::body_pose(hand).map(|(position, _)| position));
    let held = physical_hands::held_bodies();
    let near = |aabb: &physical_hands::collider_view::model::Aabb| {
        aabb.distance(origin) <= RICO_RADIUS_M
            || hands
                .iter()
                .flatten()
                .any(|hand| aabb.distance(*hand) <= HAND_RADIUS_M)
    };

    let mut state = NEAR.lock();
    let state = &mut *state;
    state.step += 1;
    let slice_len = (state.peak.saturating_add(1))
        .div_ceil(WALK_STEPS)
        .max(MIN_SLICE);
    let mut candidates: Vec<u32> = state.kept.iter().map(|(id, _)| *id).collect();
    // SAFETY: forwarded from the caller.
    let slice = unsafe {
        nearby::walk_dynamic(state.cursor, slice_len, |id, aabb| {
            if near(aabb) && !candidates.contains(&id) {
                candidates.push(id);
            }
        })
    };
    let (walked, peak) = match slice {
        Ok(slice) => {
            state.cursor = slice.next;
            state.peak = slice.peak;
            (slice.walked, slice.peak)
        }
        Err(_) => {
            state.kept.clear();
            return;
        }
    };
    for held in held.iter().flatten() {
        if !candidates.contains(&held.body) {
            candidates.push(held.body);
        }
    }

    let mut read: Vec<(u32, BodyState, f32, [bool; 2])> = candidates
        .into_iter()
        .filter_map(|id| {
            // SAFETY: forwarded from the caller.
            let body = unsafe { nearby::dynamic_state(id) }?;
            let holding =
                Hand::BOTH.map(|hand| held[hand.index()].is_some_and(|h: HeldBody| h.body == id));
            let distance = hands
                .iter()
                .flatten()
                .map(|hand| body.aabb.distance(*hand) - (HAND_RADIUS_M - RICO_RADIUS_M))
                .fold(body.aabb.distance(origin), f32::min);
            (holding[0] || holding[1] || near(&body.aabb)).then_some((id, body, distance, holding))
        })
        .collect();
    // Held bodies first, then the nearest.
    read.sort_by(|a, b| {
        let held = |e: &(u32, BodyState, f32, [bool; 2])| !(e.3[0] || e.3[1]);
        held(a).cmp(&held(b)).then(a.2.total_cmp(&b.2))
    });
    read.truncate(MAX_BODIES);

    let now = Instant::now();
    let mut kept = Vec::with_capacity(read.len());
    let mut samples = Vec::with_capacity(read.len());
    let mut intros = Vec::new();
    for (id, body, _, holding) in &read {
        let resting = body.linear.length_squared() < REST_SPEED_SQ
            && body.angular.length_squared() < REST_SPEED_SQ;
        let previous = state
            .kept
            .iter()
            .find(|(kept, _)| kept == id)
            .map(|(_, kept)| *kept);
        let introduced = match previous {
            Some(p) if now.saturating_duration_since(p.introduced) < INTRO_REPEAT => p.introduced,
            _ => {
                intros.push((*id, previous.is_some()));
                now
            }
        };
        kept.push((
            *id,
            Kept {
                introduced,
                resting,
            },
        ));
        if resting && previous.is_some_and(|p| p.resting) && !holding[0] && !holding[1] {
            continue;
        }
        samples.push(BodySample {
            id: *id,
            offset: body.position - origin,
            orientation: body.orientation,
            linear: body.linear,
            angular: body.angular,
            resting,
            held: *holding,
        });
    }
    state.kept = kept;
    let step = state.step;

    for (id, repeat) in intros {
        // SAFETY: forwarded from the caller.
        let Some(description) = (unsafe { nearby::describe(id) }) else {
            continue;
        };
        events::note(EventKind::BodyIntro(Box::new(BodyIntro {
            id,
            object: description.object.map(|object| object.get() as u64),
            kind: Label::Borrowed(description.kind),
            mass_kg: description.mass_kg,
            shape: Label::Borrowed(description.shape),
            half_extents: description.half_extents,
            filter: description.filter,
            repeat,
        })));
    }
    for (hand, held) in Hand::BOTH.into_iter().zip(held) {
        if let Some(held) = held {
            note_held(hand, held, &read);
        }
    }
    events::note(EventKind::Bodies(Box::new(BodiesSample {
        step,
        origin,
        walked,
        peak,
        cost_us: u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX),
        bodies: samples,
    })));
}

/// Within this of Rico's root, or [`HAND_RADIUS_M`] of a hand, a dynamic body is near.
const RICO_RADIUS_M: f32 = 8.0;
const HAND_RADIUS_M: f32 = 3.0;

/// The most bodies kept, held ones first and then the nearest.
const MAX_BODIES: usize = 32;

/// Each step walks this share of the slots, and at least [`MIN_SLICE`] of them.
const WALK_STEPS: u32 = 8;
const MIN_SLICE: u32 = 256;

/// A kept body is introduced again after this long, so a stretch of a recording names its bodies.
const INTRO_REPEAT: Duration = Duration::from_secs(5);

/// Below this squared speed, in either velocity, a body is at rest.
const REST_SPEED_SQ: f32 = 1.0e-6;

/// The near set between steps.
struct Near {
    step: u64,
    cursor: u32,
    peak: u32,
    /// The kept bodies by id, at most [`MAX_BODIES`].
    kept: Vec<(u32, Kept)>,
}

impl Near {
    const EMPTY: Near = Near {
        step: 0,
        cursor: 0,
        peak: 0,
        kept: Vec::new(),
    };
}

/// What the near set remembers of a kept body.
#[derive(Debug, Copy, Clone)]
struct Kept {
    introduced: Instant,
    resting: bool,
}

static NEAR: Mutex<Near> = Mutex::new(Near::EMPTY);

fn note_held(hand: Hand, held: HeldBody, read: &[(u32, BodyState, f32, [bool; 2])]) {
    let motion = read
        .iter()
        .find(|(id, ..)| *id == held.body)
        .map(|(_, body, ..)| HeldMotion {
            pose: Pose {
                position: body.position,
                orientation: body.orientation,
            },
            linear: body.linear,
            angular: body.angular,
        });
    events::note(EventKind::Held(Box::new(HeldSample {
        hand: hand_to_file(hand),
        kind: Label::Owned(format!("{:?}", held.kind)),
        object: held.object.get() as u64,
        body: held.body,
        motion,
        report: held.report.map(|r| HoldReport {
            mass_kg: r.mass_kg,
            centre: r.centre,
            anchor: r.anchor,
            target: r.target,
            lag_m: r.lag_m,
            load: r.load,
            twist: r.twist,
            joined: r.joined,
            shared: r.shared,
            liftable: r.liftable,
        }),
    })));
}
