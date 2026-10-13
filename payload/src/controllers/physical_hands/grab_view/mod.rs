//! The grab view: what the grip and the palms' reach saw, drawn in the world-space overlay around
//! each hand, for seeing why a hand does or does not take something. A diagnostic, on by default;
//! the design is in `docs/mod/input/grab-view.md`.
//!
//! - [`model`]: what is drawn and how, pure: the rays' outcomes, the palms' state, the candidates,
//!   how a hand stands to a grip target, and their colours;
//! - [`draw`]: the primitives, within a budget;
//! - [`candidates`]: the read of the world for the objects near the hands;
//! - [`config`]: the settings.
//!
//! The palms' simulation tick records what its reach cast ([`record`]) and reads the candidates
//! from the world a few times a second ([`scan_candidates`]); a grab that a subsystem refuses is
//! noted as it is refused ([`refused`]). The input tick draws it all with the grip model's
//! resolution of the same tick ([`submit`]). Everything here runs on the main thread; the view's
//! state is a leaf lock, with the overlay store's and the readout's leaf locks briefly under it,
//! and the readout may be read from any thread.

pub mod candidates;
pub mod config;
pub mod draw;
pub mod model;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use glam::Vec3;
use parking_lot::Mutex;

pub use self::config::GrabViewConfig;
use self::{
    draw::{DrawInputs, Drawn},
    model::{Candidate, CandidateKind, HandleSeen, PalmState, REFUSAL_FLASH_S, ReachRay},
};
use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        grab::{
            markers::{MarkerFrame, anchor as marker_anchor},
            tick::Tick,
        },
        overlay::{
            primitive::{Anchor, Primitive, Source},
            store,
        },
        physical_hands::{PhysicalHandsConfig, palm_readout},
    },
};

/// The view's readout for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrabViewStatus {
    /// Whether the view is switched on.
    pub enabled: bool,
    /// What the last input tick drew.
    pub drawn: Drawn,
    /// The primitives the last input tick submitted.
    pub primitives: usize,
    /// Each hand's palm state at the last tick, by [`Hand::index`].
    pub palms: [Option<PalmState>; 2],
    /// The candidates found at the last read, by [`CandidateKind::index`].
    pub candidates: [usize; CandidateKind::COUNT],
    /// The reads of the world for candidates since the payload loaded.
    pub candidate_reads: u64,
    /// How long the last read for candidates took, microseconds.
    pub candidate_read_us: u32,
    /// The refused grabs noted since the payload loaded.
    pub refusals: u64,
    /// The last refused grab: the hand and why.
    pub last_refusal: Option<(Hand, String)>,
    pub last_error: Option<String>,
}

/// The latest readout.
pub fn status() -> GrabViewStatus {
    STATUS.lock().clone()
}

/// Switch the view on or off.
pub fn toggle() {
    let mut config = crate::config::edit();
    let view = &mut config.controllers.physical_hands.grab_view;
    view.enabled = !view.enabled;
    tracing::info!(enabled = view.enabled, "grab view: toggled");
}

/// Whether the palms' tick should record its reach for the view, under `config`.
pub(crate) fn recording(config: &PhysicalHandsConfig) -> bool {
    config.grab_view.enabled
}

/// What the palms' tick cast, for the view: each hand's reach rays and the door handles near it,
/// world space, with the head the tick placed them against.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Recorded {
    pub head: Vec3,
    pub rays: [Vec<ReachRay>; 2],
    pub handles: [Vec<HandleSeen>; 2],
}

/// Record what the palms' tick cast. Called from the tick, outside its own state's lock.
pub(crate) fn record(recorded: Recorded) {
    STATE.lock().recorded = Some((Instant::now(), recorded));
}

/// Read the candidates near `hands` (each hand's palm, game space) when the last read is old
/// enough, skipping the body `standing_on` the player stands on.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn scan_candidates(
    hands: [Option<Vec3>; 2],
    standing_on: Option<u32>,
    config: &PhysicalHandsConfig,
) {
    if !config.grab_view.candidates {
        let mut state = STATE.lock();
        state.candidates.clear();
        state.candidates_at = None;
        return;
    }
    let now = Instant::now();
    let due = STATE
        .lock()
        .candidates_at
        .is_none_or(|at| now.saturating_duration_since(at) >= CANDIDATE_PERIOD);
    if !due {
        return;
    }
    // SAFETY: forwarded from the caller.
    let found = unsafe { candidates::scan(hands, standing_on, config) };
    let took = now.elapsed();
    let mut counts = [0; CandidateKind::COUNT];
    let error = match found {
        Ok(found) => {
            for candidate in &found {
                counts[candidate.kind.index()] += 1;
            }
            let mut state = STATE.lock();
            state.candidates = found;
            state.candidates_at = Some(now);
            None
        }
        Err(error) => {
            let mut state = STATE.lock();
            state.candidates.clear();
            state.candidates_at = Some(now);
            Some(error.to_string())
        }
    };
    let mut status = STATUS.lock();
    status.candidates = counts;
    status.candidate_reads += 1;
    status.candidate_read_us = u32::try_from(took.as_micros()).unwrap_or(u32::MAX);
    if error.is_some() {
        status.last_error = error;
    }
}

/// Note that a grab by `hand` was refused, `why`, flashing at `at` (game space) or, without it, at
/// the target the hand's grip chose on the last input tick. Called on the main thread where the
/// refusal is made, outside the view's own lock.
pub(crate) fn refused(hand: Hand, at: Option<Vec3>, why: &str) {
    {
        let mut state = STATE.lock();
        let anchor = at
            .filter(|p| p.is_finite())
            .map(Anchor::World)
            .or(state.chosen[hand.index()]);
        if let Some(anchor) = anchor {
            if state.flashes.len() >= MAX_FLASHES {
                state.flashes.remove(0);
            }
            state.flashes.push((anchor, Instant::now()));
        }
    }
    let mut status = STATUS.lock();
    status.refusals += 1;
    status.last_refusal = Some((hand, why.to_owned()));
}

/// Draw the view for the input tick `tick`, placed by `frame`, or take it down when `frame` is
/// `None` (the overlay is off, or a menu is up). Called by the input tick after the grip model's
/// own finish.
pub(crate) fn submit(tick: &Tick, frame: Option<&MarkerFrame>) {
    let config = Config::lock_query(|c| c.controllers.physical_hands.grab_view);
    let Some(frame) = frame.filter(|_| config.enabled) else {
        stand_down(config.enabled);
        return;
    };
    WAS_ACTIVE.store(true, Ordering::Relaxed);
    let now = Instant::now();
    // Read before the view's lock: the readout takes the palms' own state.
    let palms = Hand::BOTH.map(|hand| PalmState::of(&palm_readout(hand)));

    let mut state = STATE.lock();
    let state = &mut *state;
    for hand in Hand::BOTH {
        let i = hand.index();
        state.chosen[i] = tick.resolved[i].chosen.and_then(|chosen| {
            let target = tick.targets.get(chosen.index)?;
            marker_anchor(target, Some(hand), frame)
        });
    }
    state
        .flashes
        .retain(|(_, born)| now.saturating_duration_since(*born).as_secs_f32() < REFUSAL_FLASH_S);
    let flashes: Vec<(Anchor, f32)> = state
        .flashes
        .iter()
        .map(|&(anchor, born)| (anchor, now.saturating_duration_since(born).as_secs_f32()))
        .collect();
    let recorded = state
        .recorded
        .as_ref()
        .filter(|(at, _)| now.saturating_duration_since(*at) <= RECORD_MAX_AGE)
        .map(|(_, recorded)| recorded);
    let candidates: &[Candidate] = match state.candidates_at {
        Some(at) if now.saturating_duration_since(at) <= CANDIDATE_MAX_AGE => &state.candidates,
        _ => &[],
    };
    let drawn = draw::build(
        &DrawInputs {
            config: &config,
            frame,
            tick,
            recorded,
            palms,
            candidates,
            flashes: &flashes,
        },
        &mut state.primitives,
    );
    let primitives = state.primitives.len();
    store::submit(Source::GrabView, &mut state.primitives, TTL);

    let mut status = STATUS.lock();
    status.enabled = true;
    status.drawn = drawn;
    status.primitives = primitives;
    status.palms = palms.map(Some);
}

/// Take the view down once, after it last drew, keeping the counters.
fn stand_down(enabled: bool) {
    if !WAS_ACTIVE.swap(false, Ordering::Relaxed) {
        return;
    }
    store::clear(Source::GrabView);
    {
        let mut state = STATE.lock();
        state.chosen = [None; 2];
        state.flashes.clear();
    }
    let mut status = STATUS.lock();
    status.enabled = enabled;
    status.drawn = Drawn::default();
    status.primitives = 0;
    status.palms = [None; 2];
}

/// The view's state across ticks.
struct State {
    /// The palms' last tick, and when it was recorded.
    recorded: Option<(Instant, Recorded)>,
    /// The candidates at the last read.
    candidates: Vec<Candidate>,
    /// When the candidates were last read.
    candidates_at: Option<Instant>,
    /// Where each hand's chosen target's marker was on the last input tick, by [`Hand::index`].
    chosen: [Option<Anchor>; 2],
    /// The refused grabs still flashing: where, and when.
    flashes: Vec<(Anchor, Instant)>,
    /// The tick's primitives, reused across submissions.
    primitives: Vec<Primitive>,
}

static STATE: Mutex<State> = Mutex::new(State {
    recorded: None,
    candidates: Vec::new(),
    candidates_at: None,
    chosen: [None; 2],
    flashes: Vec::new(),
    primitives: Vec::new(),
});

static STATUS: Mutex<GrabViewStatus> = Mutex::new(GrabViewStatus {
    enabled: false,
    drawn: Drawn {
        target_lines: 0,
        rays: 0,
        handles: 0,
        candidates: 0,
        flashes: 0,
        dropped: 0,
    },
    primitives: 0,
    palms: [None; 2],
    candidates: [0; CandidateKind::COUNT],
    candidate_reads: 0,
    candidate_read_us: 0,
    refusals: 0,
    last_refusal: None,
    last_error: None,
});

/// Whether the view drew on the last input tick, so that switching it off clears its slot once.
static WAS_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The view is submitted at the input tick's rate, so it lives a little past one tick.
const TTL: Duration = Duration::from_millis(150);

/// How long the palms' last tick is drawn after it ran: they tick every simulation tick while they
/// act, so an older record means they have stopped.
const RECORD_MAX_AGE: Duration = Duration::from_millis(200);

/// How often the world is read for candidates: a pass over every body slot, a few times a second
/// rather than every tick.
const CANDIDATE_PERIOD: Duration = Duration::from_millis(100);

/// How long the candidates are drawn after their read, past which they are taken as stale.
const CANDIDATE_MAX_AGE: Duration = Duration::from_millis(500);

/// The most refused grabs flashing at once.
const MAX_FLASHES: usize = 8;
