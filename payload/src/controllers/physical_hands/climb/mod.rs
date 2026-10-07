//! Climbing with the hands: an empty hand whose body touches the static world grips it with the
//! grip, and while it grips, the player's body moves against the hand's motion, so that pulling a
//! gripping hand down lifts the body. The design is in `docs/mod/input/hand-climbing.md`, the
//! engine side in `docs/engine/character/hands-locomotion-and-state.md`.
//!
//! - [`config`]: the settings;
//! - [`solve`]: the motion, pure: where the gripping hands would have the body, the velocity that
//!   takes it there, the fling a release keeps, and the mantle over a ledge;
//! - [`engine`]: the player's character proxy: where it is, its support, and its velocity.
//!
//! Each simulation tick runs in this order:
//!
//! 1. The player aim control's post-simulation update ([`observe`], from the palms' tick) publishes
//!    each driven hand's palm, the static surface in its reach, and where the player's body is,
//!    and pulses the hands for the grips the last step took or ended.
//! 2. The input tick ([`finish_tick`]) asks for a grip where a hand's grip press found the climb's
//!    grab volume ([`volume`]), and for a release where the grip lets go.
//! 3. The hand bodies' pre-simulation step ([`step`]) takes and ends the grips, pins each gripping
//!    hand's body where it took hold, and moves the player: the velocity that carries the body to
//!    where the gripping hands would have it, and a support on the gripped surface, so that the
//!    game keeps the player on foot instead of starting a fall. A release keeps the climbing
//!    speed, and a mantle carries the body over a ledge and lets go once it stands there.
//!
//! The grips are taken and the player moved only in the pre-simulation step, on the thread that
//! steps the world, outside the step. The other halves share a leaf lock of their own.

pub mod config;
pub mod engine;
pub mod solve;

use std::time::{Duration, Instant};

pub use config::ClimbConfig;
use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        bindings::grab::GrabVolume,
        frame::Hand,
        mode::{self, Mode, OnFootVariant},
        physical_hands::{
            PhysicalHandsConfig,
            climb::{
                engine::PlayerPlace,
                solve::{
                    ClimbTuning, Follow, Mantle, MantleTuning, body_target, climb_velocity, fling,
                    mantle_starts, smooth,
                },
            },
            contact::{Surface, SurfaceHit},
            palm::PalmBox,
            physics::{self, drive::Goal},
            pickup::REACH_DIRECTIONS,
            pulse,
        },
        roomscale::teleport,
        weapons::haptics::PulseSource,
    },
};

/// A static surface a hand can grip.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Hold {
    /// Where the surface is, game space.
    pub point: Vec3,
    /// The surface's normal, toward the hand.
    pub normal: Vec3,
    /// The surface's static body.
    pub body: u32,
}

/// One hand's climbing on the last tick, for the readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ClimbHandStatus {
    /// The static surface in the hand's reach.
    pub reach: Option<Hold>,
    /// The surface the hand grips.
    pub grip: Option<Hold>,
}

/// The climbing's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClimbStatus {
    /// Whether a hand grips the world.
    pub climbing: bool,
    /// Whether the mantle is carrying the body over a ledge.
    pub mantling: bool,
    /// Indexed by [`Hand::index`].
    pub hands: [ClimbHandStatus; 2],
    /// The velocity the last step gave the body, world space.
    pub velocity: Vec3,
    /// How far the body was from where the hands would have it, metres.
    pub gap_m: f32,
    /// Whether the game judged the player in the air on the last aim update
    /// ([`jc3gi::character::character::Character::IsInAir`]).
    pub in_air: Option<bool>,
    pub grips: u64,
    pub releases: u64,
    pub mantles: u64,
    /// Grips ended against the player's wish.
    pub drops: u64,
    pub last_drop: Option<&'static str>,
    /// The speed of the last fling, metres per second.
    pub last_fling_mps: Option<f32>,
    pub last_error: Option<String>,
}

pub fn status() -> ClimbStatus {
    STATE.lock().status.clone()
}

/// Whether a hand grips the world, as the last pre-simulation step left it: the procedural body
/// hangs the legs while one does.
pub fn climbing() -> bool {
    let state = STATE.lock();
    state.grips.iter().any(Option::is_some) && state.stepped_lately(Instant::now())
}

/// Whether the climbing is on: its switch, under the hand bodies, which it needs.
pub(crate) fn enabled(config: &PhysicalHandsConfig) -> bool {
    config.climb.enabled && physics::enabled(config)
}

/// The climb's grab volume for `hand`: [`GrabVolume::Climb`] when the hand's body reaches a static
/// surface and the hand grips nothing. Called from the input tick.
pub(crate) fn volume(hand: Hand) -> Option<GrabVolume> {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    if !enabled(&config) {
        return None;
    }
    let state = STATE.lock();
    let i = hand.index();
    (state.fresh(Instant::now())
        && state.grips[i].is_none()
        && state.status.hands[i].reach.is_some())
    .then_some(GrabVolume::Climb)
}

/// Act on the input tick's grips: a grip pressed on the climb's grab volume asks the next step to
/// grip the surface in reach, and a released grip asks it to let go. Called from the input tick
/// after the tables, with each hand's held grip and the grab volume its press latched.
pub(crate) fn finish_tick(held: [bool; 2], grabs: [Option<GrabVolume>; 2]) {
    let mut state = STATE.lock();
    let fresh = state.fresh(Instant::now());
    for hand in Hand::BOTH {
        let i = hand.index();
        if !held[i] {
            if state.grips[i].is_some() || matches!(state.requests[i], Some(Request::Grip(_))) {
                state.requests[i] = Some(Request::Release);
            }
            continue;
        }
        if grabs[i] != Some(GrabVolume::Climb) || state.grips[i].is_some() {
            continue;
        }
        if let Some(reach) = state.status.hands[i].reach.filter(|_| fresh)
            && state.requests[i].is_none()
        {
            state.requests[i] = Some(Request::Grip(reach));
        }
    }
}

/// The static surface `palm` reaches: the nearest hit, on a static body, of the rays a hand reaches
/// for an object along ([`REACH_DIRECTIONS`]), at most `reach_m` past the palm. Called from
/// the palms' tick for a hand its body drives.
///
/// # Safety
///
/// The game thread, outside the physics step; `cast` must cast against the live world.
pub(super) unsafe fn reach(
    palm: &PalmBox,
    reach_m: f32,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> Option<Hold> {
    let origin = palm.point(Vec3::new(0.0, 1.0, 0.0));
    let length = reach_m.max(0.0) + 2.0 * palm.half_extents.y;
    REACH_DIRECTIONS
        .iter()
        .filter_map(|&local| {
            let direction = (palm.orientation * local).normalize_or_zero();
            let hit = cast(origin, origin + direction * length)?;
            let Surface::Solid { body, .. } = hit.surface else {
                return None;
            };
            // SAFETY: forwarded from the caller; the hit was found this tick.
            unsafe { engine::is_static(body) }.then_some(Hold {
                point: hit.position,
                normal: hit.normal,
                body,
            })
        })
        .min_by(|a, b| {
            a.point
                .distance_squared(palm.centre)
                .total_cmp(&b.point.distance_squared(palm.centre))
        })
}

/// Publish what the palms' tick found: each driven hand's palm centre and the static surface in
/// its reach, with where the player's body is, and pulse the hands for the last step's grips and
/// releases. Called from the palms' tick, on the game thread outside the physics step.
pub(super) fn observe(
    palms: [Option<Vec3>; 2],
    reaches: [Option<Hold>; 2],
    config: &PhysicalHandsConfig,
) {
    let on = enabled(config);
    // SAFETY: the game thread outside the physics step (the palms' tick).
    let player = on.then(|| unsafe { engine::player_place() }.ok()).flatten();
    // SAFETY: as above.
    let in_air = on.then(|| unsafe { engine::in_air() }).flatten();
    let mode = mode::current().mode;
    let events = {
        let mut state = STATE.lock();
        state.palms = palms;
        state.player = player;
        state.published = (on && player.is_some()).then(Instant::now);
        state.publishes = state.publishes.wrapping_add(1);
        for hand in Hand::BOTH {
            state.status.hands[hand.index()].reach = reaches[hand.index()].filter(|_| on);
        }
        state.status.in_air = in_air;
        let climbing = state.grips.iter().any(Option::is_some);
        if climbing && mode != Mode::OnFoot(OnFootVariant::Ground) && !state.warned_mode {
            state.warned_mode = true;
            tracing::warn!(
                ?mode,
                in_air = ?in_air,
                "hand climbing: the game left on foot on the ground while a hand gripped the world"
            );
        }
        if !climbing {
            state.warned_mode = false;
        }
        std::mem::take(&mut state.events)
    };
    for (hand, event) in events {
        let spec = match event {
            Event::Gripped => config.climb.grab_pulse,
            Event::Released => config.climb.release_pulse,
        };
        pulse(hand, spec, 1.0, config, PulseSource::Pickup);
    }
}

/// The climb's part of the hand bodies' pre-simulation step: take and end the grips, and move the
/// player while a hand grips. `bodies` holds each acting hand body's pose, game space, `None` for
/// a hand whose body does not act this step. Returns where each gripping hand's body is held.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the characters' proxy
/// updates (the pre-simulation callback).
pub(super) unsafe fn step(bodies: [Option<(Vec3, Quat)>; 2]) -> [Option<Goal>; 2] {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let mut state = STATE.lock();
    state.stepped = Some(Instant::now());
    if state.publishes == state.stepped_publish {
        state.stale_steps = state.stale_steps.saturating_add(1);
    } else {
        state.stale_steps = 0;
        state.stepped_publish = state.publishes;
    }
    let reason = if !enabled(&config) {
        Some("climbing is off")
    } else if teleport::epoch() != state.epoch {
        state.epoch = teleport::epoch();
        Some("the player was teleported")
    } else if state.stale_steps > MAX_STALE_STEPS {
        Some("the hands stopped publishing")
    } else {
        None
    };
    if let Some(reason) = reason {
        state.requests = [None; 2];
        state.drop_all(reason);
        state.finish_climb();
        return [None; 2];
    }

    let mut released_by_grip = false;
    for hand in Hand::BOTH {
        let i = hand.index();
        match state.requests[i].take() {
            Some(Request::Grip(hold)) if state.grips[i].is_none() => {
                // SAFETY: forwarded from the caller.
                let grip = match (bodies[i], state.palms[i]) {
                    (Some(pin), Some(palm)) if unsafe { engine::is_static(hold.body) } => {
                        Some(Grip { hold, palm, pin })
                    }
                    _ => None,
                };
                if let Some(grip) = grip {
                    tracing::info!(
                        hand = hand.label(),
                        body = hold.body,
                        point = ?hold.point,
                        normal = ?hold.normal,
                        "hand climbing: a hand gripped the world"
                    );
                    state.grips[i] = Some(grip);
                    state.status.grips += 1;
                    state.events.push((hand, Event::Gripped));
                } else {
                    tracing::debug!(
                        hand = hand.label(),
                        body = hold.body,
                        has_body = bodies[i].is_some(),
                        "hand climbing: a grip found nothing to take"
                    );
                }
            }
            Some(Request::Release) if state.grips[i].take().is_some() => {
                released_by_grip = true;
                state.status.releases += 1;
                state.events.push((hand, Event::Released));
                tracing::debug!(hand = hand.label(), "hand climbing: a hand let go");
            }
            _ => {}
        }
        if state.grips[i].is_some() && bodies[i].is_none() {
            state.drop_hand(hand, "the hand's body stopped acting");
        }
    }

    if state.grips.iter().all(Option::is_none) {
        if state.climbing && released_by_grip && state.mantle.is_none() && config.climb.fling {
            let kept = fling(
                state.smoothed,
                config.climb.fling_scale,
                config.climb.max_fling_speed_mps,
            );
            // SAFETY: forwarded from the caller.
            match unsafe { engine::set_velocity(kept) } {
                Ok(()) => {
                    state.status.last_fling_mps = Some(kept.length());
                    tracing::info!(
                        speed_mps = kept.length(),
                        velocity = ?kept,
                        "hand climbing: let go of the world"
                    );
                }
                Err(e) => state.status.last_error = Some(e.to_string()),
            }
        }
        state.finish_climb();
        return [None; 2];
    }

    // SAFETY: forwarded from the caller.
    let place = match unsafe { engine::player_place() } {
        Ok(place) => place,
        Err(e) => {
            state.status.last_error = Some(e.to_string());
            state.drop_all("the player has no body to move");
            state.finish_climb();
            return [None; 2];
        }
    };
    if !state.climbing {
        state.climbing = true;
        state.smoothed = Vec3::ZERO;
        tracing::info!(feet = ?place.feet, "hand climbing: climbing starts");
    }
    let step_s = jc3gi::physics::PhysicsSystem::STEP_SECONDS;
    let tuning = ClimbTuning {
        gain: config.climb.gain,
        max_speed_mps: config.climb.max_speed_mps,
        step_s,
    };
    let follows: Vec<Follow> = Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            let grip = state.grips[hand.index()]?;
            let palm = state.palms[hand.index()]?;
            Some(Follow {
                grip: grip.palm,
                palm,
            })
        })
        .collect();
    let published = state.player.filter(|_| state.stale_steps == 0);
    let target = published
        .and_then(|player| body_target(&follows, player.body))
        .unwrap_or(place.body);
    let gap = target - place.body;
    state.status.gap_m = gap.length();
    // The mantle carries the body away from where the hands would have it, on purpose.
    if state.mantle.is_none() && gap.length() > config.climb.break_distance_m {
        state.drop_all("the body could not follow the hands");
        state.finish_climb();
        return [None; 2];
    }
    let mut velocity = climb_velocity(target, place.body, &tuning);

    // SAFETY: forwarded from the caller; the proxy's update ran before this callback.
    let stands = unsafe { engine::stands(config.climb.min_ledge_normal_y) };
    let mantle_tuning = MantleTuning {
        start_m: config.climb.mantle_start_m,
        clearance_m: config.climb.mantle_clearance_m,
        forward_m: config.climb.mantle_forward_m,
        speed_mps: config.climb.mantle_speed_mps,
        step_s,
    };
    if config.climb.mantle {
        if state.mantle.is_none()
            && let Some(top) = state.ledge_top(config.climb.min_ledge_normal_y)
            && mantle_starts(place.feet.y, top.y, velocity.y > 0.0, &mantle_tuning)
            && let Some(mantle) = Mantle::start(top, place.feet)
        {
            tracing::info!(top = ?top, feet = ?place.feet, "hand climbing: mantling over a ledge");
            state.mantle = Some(mantle);
            state.mantle_steps = 0;
            state.status.mantles += 1;
        }
        if let Some(mantle) = state.mantle {
            state.mantle_steps += 1;
            if stands && place.feet.y >= mantle.top.y - MANTLE_LANDED_BELOW_M {
                tracing::info!(feet = ?place.feet, "hand climbing: mantled onto the ledge");
                for hand in Hand::BOTH {
                    if state.grips[hand.index()].take().is_some() {
                        state.events.push((hand, Event::Released));
                    }
                }
                state.finish_climb();
                return [None; 2];
            }
            if state.mantle_steps > MAX_MANTLE_STEPS {
                state.drop_all("the mantle did not reach the ledge's top");
                state.finish_climb();
                return [None; 2];
            }
            velocity = mantle.velocity(place.feet, &mantle_tuning);
        }
    }

    let ground = Hand::BOTH
        .into_iter()
        .find_map(|hand| state.grips[hand.index()].map(|g| g.hold.body));
    let Some(ground) = ground else {
        unreachable!("a hand grips the world");
    };
    // SAFETY: forwarded from the caller; the ground is a static body, checked at the grip.
    if let Err(e) = unsafe { engine::hold(velocity, ground) } {
        state.status.last_error = Some(e.to_string());
        state.drop_all("the player could not be held");
        state.finish_climb();
        return [None; 2];
    }
    state.smoothed = smooth(
        state.smoothed,
        velocity,
        step_s,
        config.climb.fling_smoothing_s,
    );
    state.status.velocity = velocity;
    state.status.climbing = true;
    state.status.mantling = state.mantle.is_some();
    let now = Instant::now();
    if state
        .logged_at
        .is_none_or(|at| now.saturating_duration_since(at) >= LOG_PERIOD)
    {
        state.logged_at = Some(now);
        tracing::debug!(
            velocity = ?velocity,
            gap_m = gap.length(),
            hands = follows.len(),
            mantling = state.mantle.is_some(),
            stands,
            "hand climbing: climb"
        );
    }
    for hand in Hand::BOTH {
        state.status.hands[hand.index()].grip = state.grips[hand.index()].map(|g| g.hold);
    }
    state.grips.map(|grip| {
        grip.map(|g| Goal {
            position: g.pin.0,
            orientation: g.pin.1,
        })
    })
}

/// Let go of everything without a fling: the hands stood down. Called from the palms' tick.
pub(super) fn stand_down() {
    let mut state = STATE.lock();
    state.requests = [None; 2];
    state.palms = [None; 2];
    state.published = None;
    state.status.hands = [ClimbHandStatus::default(); 2];
}

/// How long a publish stays current for the input tick, which runs every frame.
const STALE_AFTER: Duration = Duration::from_millis(200);

/// How many steps in a row may run without a publish before the grips let go: the aim update runs
/// every simulation tick, and a game pause stops the steps as well as the publishes.
const MAX_STALE_STEPS: u32 = 6;

/// How far below the gripped top feet that stand count as on the ledge, metres.
const MANTLE_LANDED_BELOW_M: f32 = 0.15;

/// How many steps a mantle may take before it gives up: three seconds.
const MAX_MANTLE_STEPS: u32 = 90;

/// How often the climb is written to the debug log.
const LOG_PERIOD: Duration = Duration::from_secs(1);

#[derive(Debug, Copy, Clone, PartialEq)]
enum Request {
    Grip(Hold),
    Release,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum Event {
    Gripped,
    Released,
}

/// A hand gripping the world.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Grip {
    hold: Hold,
    /// Where the palm was when it took hold, game space.
    palm: Vec3,
    /// Where the hand's body was when it took hold, where it stays.
    pin: (Vec3, Quat),
}

struct State {
    requests: [Option<Request>; 2],
    grips: [Option<Grip>; 2],
    palms: [Option<Vec3>; 2],
    player: Option<PlayerPlace>,
    published: Option<Instant>,
    publishes: u64,
    stepped_publish: u64,
    stale_steps: u32,
    stepped: Option<Instant>,
    climbing: bool,
    mantle: Option<Mantle>,
    mantle_steps: u32,
    smoothed: Vec3,
    events: Vec<(Hand, Event)>,
    epoch: u64,
    warned_mode: bool,
    logged_at: Option<Instant>,
    status: ClimbStatus,
}

impl State {
    fn fresh(&self, now: Instant) -> bool {
        self.published
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    fn stepped_lately(&self, now: Instant) -> bool {
        self.stepped
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    /// The highest gripped surface that faces up enough to be a ledge's top.
    fn ledge_top(&self, min_normal_y: f32) -> Option<Vec3> {
        self.grips
            .iter()
            .flatten()
            .filter(|g| g.hold.normal.y >= min_normal_y)
            .map(|g| g.hold.point)
            .max_by(|a, b| a.y.total_cmp(&b.y))
    }

    fn drop_hand(&mut self, hand: Hand, reason: &'static str) {
        if self.grips[hand.index()].take().is_none() {
            return;
        }
        self.status.drops += 1;
        self.status.last_drop = Some(reason);
        self.events.push((hand, Event::Released));
        tracing::info!(
            hand = hand.label(),
            reason,
            "hand climbing: a grip was let go"
        );
    }

    fn drop_all(&mut self, reason: &'static str) {
        for hand in Hand::BOTH {
            self.drop_hand(hand, reason);
        }
    }

    /// Forget the climb once no hand grips.
    fn finish_climb(&mut self) {
        if self.climbing {
            tracing::info!("hand climbing: climbing ends");
        }
        self.climbing = false;
        self.mantle = None;
        self.mantle_steps = 0;
        self.smoothed = Vec3::ZERO;
        self.status.climbing = false;
        self.status.mantling = false;
        self.status.velocity = Vec3::ZERO;
        for hand in &mut self.status.hands {
            hand.grip = None;
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    requests: [None; 2],
    grips: [None; 2],
    palms: [None; 2],
    player: None,
    published: None,
    publishes: 0,
    stepped_publish: 0,
    stale_steps: 0,
    stepped: None,
    climbing: false,
    mantle: None,
    mantle_steps: 0,
    smoothed: Vec3::ZERO,
    events: Vec::new(),
    epoch: 0,
    warned_mode: false,
    logged_at: None,
    status: ClimbStatus {
        climbing: false,
        mantling: false,
        hands: [ClimbHandStatus {
            reach: None,
            grip: None,
        }; 2],
        velocity: Vec3::ZERO,
        gap_m: 0.0,
        in_air: None,
        grips: 0,
        releases: 0,
        mantles: 0,
        drops: 0,
        last_drop: None,
        last_fling_mps: None,
        last_error: None,
    },
});
