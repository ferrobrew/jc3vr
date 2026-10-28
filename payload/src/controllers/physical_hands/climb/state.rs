//! The hand climbing's shared state between the tick's halves: the grips, the mantle and the fling
//! carrying the body, the readout, and the small readers both halves and the rest of the
//! controllers take.

use std::time::{Duration, Instant};

use glam::{Affine3A, Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    fingers::grip::{self, Grip as FingerGrip, GripSource},
    frame::Hand,
    grab::GrabVolume,
    mode::{self, AirborneVariant, Mode, OnFootVariant},
    physical_hands::{
        PhysicalHandsConfig,
        climb::{
            engine,
            native::Context,
            solve::{Mantle, carried, carried_turn},
            types::{ClimbHandStatus, ClimbStatus, Hold, Landing},
        },
        physics::{self, drive::Goal},
    },
};

pub fn status() -> ClimbStatus {
    STATE.lock().status.clone()
}

/// Whether a hand grips the world, as the last pre-simulation step left it: the procedural body
/// hangs the legs while one does. A grip held while the game holds the player in a stunt position
/// moves no body, so it is no climb.
pub fn climbing() -> bool {
    let state = STATE.lock();
    state.grips.iter().any(Option::is_some)
        && !state.held_by_game
        && state.stepped_lately(Instant::now())
}

/// What each hand grips, as the last pre-simulation step left it, indexed by [`Hand::index`].
pub(crate) fn gripped() -> [Option<Hold>; 2] {
    let state = STATE.lock();
    if !state.stepped_lately(Instant::now()) {
        return [None; 2];
    }
    state.grips.map(|grip| grip.map(|g| g.hold))
}

/// The last mantle that landed on a moving body, once: the stunt positions take the player into
/// a vehicle's stunt area from it. A landing older than a moment is dropped.
pub(crate) fn take_landing()
-> Option<crate::controllers::physical_hands::climb::native::LandingOffer<'static>> {
    let context = unsafe { engine::gate_context() };
    crate::controllers::physical_hands::climb::native::observe(context);
    crate::controllers::physical_hands::climb::native::take_landing(Instant::now(), |landing| {
        (unsafe { engine::body_identity(landing.body) }) == Some(landing.identity)
    })
}

/// The static surface each hand reaches, as the palms' tick last found it: the hands push the body
/// up only from a surface under them, which may be written as the player's support
/// ([`crate::controllers::physical_hands::vault`]).
pub(crate) fn static_reaches() -> [Option<Hold>; 2] {
    let state = STATE.lock();
    let fresh = state.fresh(Instant::now());
    state
        .status
        .hands
        .map(|h| h.reach.filter(|r| fresh && !r.moving))
}

/// Whether the climbing is on: its switch, under the hand bodies, which it needs.
pub(crate) fn enabled(config: &PhysicalHandsConfig) -> bool {
    config.climb.enabled && physics::enabled(config)
}

/// The surface `hand` could climb: the one its reach found, while the hand grips nothing. A hand
/// that only presses into something its reach did not find offers nothing, since a grip there would
/// have nothing to hold. Called from the input tick.
pub(crate) fn reach_hold(hand: Hand) -> Option<Hold> {
    let state = STATE.lock();
    let i = hand.index();
    if !state.fresh(Instant::now()) || state.grips[i].is_some() {
        return None;
    }
    state.status.hands[i].reach
}

/// Act on the input tick's grips: a grip pressed on the climb's grab volume asks the next step to
/// grip the surface in reach, and a released grip asks it to let go. Called from the input tick
/// after the tables, with each hand's held grip and the grab volume its press latched.
pub(crate) fn finish_tick(held: [bool; 2], grabs: [Option<GrabVolume>; 2]) {
    crate::controllers::physical_hands::climb::native::observe(unsafe { engine::gate_context() });
    let mut state = STATE.lock();
    let now = Instant::now();
    let fresh = state.fresh(now);
    let input_mode = mode::current().mode;
    let allowed = matches!(
        input_mode,
        Mode::OnFoot(OnFootVariant::Ground) | Mode::Airborne(AirborneVariant::Falling)
    );
    if state
        .input_mode
        .is_some_and(|previous| previous != input_mode)
    {
        state.requests = [None; 2];
    }
    state.input_at = allowed.then_some(now);
    state.input_held = held.map(|h| h && allowed);
    state.input_mode = Some(input_mode);
    if !allowed {
        crate::controllers::physical_hands::climb::native::clear();
        state.requests = [None; 2];
        state.drop_all("the player left physical climbing input");
        state.finish_climb();
        return;
    }
    for hand in Hand::BOTH {
        let i = hand.index();
        if state.grips[i].is_some() && state.stepped_lately(now) {
            grip::declare(hand, FingerGrip::new(GripSource::Climb));
        }
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

pub(crate) fn stand_down() {
    crate::controllers::physical_hands::climb::native::clear();
    let mut state = STATE.lock();
    state.requests = [None; 2];
    state.input_at = None;
    state.input_mode = None;
    state.input_held = [false; 2];
    state.drop_all("the hand input stood down");
    state.finish_climb();
    state.palms = [None; 2];
    state.pressing = [false; 2];
    state.published = None;
    state.status.hands = [ClimbHandStatus::default(); 2];
}

/// How long a publish stays current for the input tick, which runs every frame.
pub(super) const STALE_AFTER: Duration = Duration::from_millis(200);

/// How many steps in a row may run without a publish, or with a gripping hand's body not acting,
/// before the grips let go: the aim update runs every simulation tick, and a game pause stops the
/// steps as well as the publishes. Counted at the game's 30 Hz tick, and scaled to the same
/// duration at another sim rate ([`crate::sim_rate::scale_ticks`]).
pub(super) const MAX_STALE_STEPS: u32 = 6;

/// How far below the gripped top feet that stand count as on the ledge, metres.
pub(super) const MANTLE_LANDED_BELOW_M: f32 = 0.15;

/// How many steps a mantle may take before it gives up: three seconds at the game's 30 Hz tick,
/// scaled to the same duration at another sim rate ([`crate::sim_rate::scale_ticks`]).
pub(super) const MAX_MANTLE_STEPS: u32 = 90;

/// The least share of the tension pulse that pulses a gripping hand.
pub(super) const MIN_TENSION: f32 = 0.1;

/// How often the climb is written to the debug log.
pub(super) const LOG_PERIOD: Duration = Duration::from_secs(1);

#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum Request {
    Grip(Hold),
    Release,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum Event {
    Gripped,
    Released,
    /// A release that flung the body, at this share of the fastest fling.
    Flung(f32),
}

impl Grip {
    /// Whether the gripped body is still one the step may write as the player's ground: a static
    /// grip's body still static, a moving grip's body still there.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step.
    pub(super) unsafe fn body_holds(&self) -> bool {
        unsafe { self.hold.valid() }
    }

    /// Where a grip on a moving body has been carried since the hand took hold, or `None` for a
    /// static grip or a body that is gone.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step.
    pub(super) unsafe fn carried(&self) -> Option<Carried> {
        let from = self.frame?;
        if !unsafe { self.hold.valid() } {
            return None;
        }
        let now = unsafe { engine::body_frame(self.hold.body) }?;
        let point = carried(from, now.transform, self.hold.point);
        Some(Carried {
            point,
            pin: (
                carried(from, now.transform, self.pin.0),
                carried_turn(from, now.transform) * self.pin.1,
            ),
            velocity: now.point_velocity(point),
            frame: now.transform,
        })
    }
}

/// A grip on a moving body, carried with it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Carried {
    /// The gripped point, where the body has carried it.
    pub(super) point: Vec3,
    /// The hand body's pin, carried with the body.
    pub(super) pin: (Vec3, Quat),
    /// The gripped point's velocity.
    pub(super) velocity: Vec3,
    /// The body's transform now.
    pub(super) frame: Affine3A,
}

/// A fling still carrying the body after its release.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Carry {
    /// The velocity the release kept.
    pub(super) kept: Vec3,
    /// The steps since the release.
    pub(super) steps: u32,
}

/// A hand gripping the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Grip {
    pub(super) hand: Hand,
    pub(super) hold: Hold,
    /// Whether the gripped surface is a ledge's top a mantle may carry the body over, judged at
    /// the grip ([`is_ledge`]).
    pub(super) ledge: bool,
    /// Where the player's body was when the hand took hold, game space.
    pub(super) body: Vec3,
    /// Where the palm was in the rig when it took hold, world metres.
    pub(super) palm: Vec3,
    /// Where the hand's body was when it took hold, where it stays.
    pub(super) pin: (Vec3, Quat),
    /// The gripped body's transform when the hand took hold, for a body that moves.
    pub(super) frame: Option<Affine3A>,
    /// The rig the grip was taken in ([`State::rig_epoch`]).
    pub(super) rig_epoch: u64,
    /// How many steps in a row the hand's body has not acted.
    pub(super) missing_steps: u32,
}

#[derive(Clone, Copy)]
pub(super) struct MantleFrame {
    pub owner: Hold,
    pub transform: Affine3A,
}

impl MantleFrame {
    pub fn completion(self, context: Context, feet: Vec3) -> Landing {
        Landing {
            body: self.owner.body,
            identity: self.owner.identity,
            character: context.character,
            proxy: context.proxy,
            epoch: context.epoch,
            feet,
        }
    }
}

pub(super) struct State {
    pub(super) requests: [Option<Request>; 2],
    pub(super) grips: [Option<Grip>; 2],
    /// Each driven hand's palm in the rig, world metres.
    pub(super) palms: [Option<Vec3>; 2],
    /// Whether each driven hand's body presses into something that holds it back.
    pub(super) pressing: [bool; 2],
    /// The body's facing and the recenter count the palms were last published with.
    pub(super) rig: Option<(Quat, u32)>,
    /// Counts the recenters and snap turns that moved the rig under the palms.
    pub(super) rig_epoch: u64,
    pub(super) published: Option<Instant>,
    pub(super) publishes: u64,
    pub(super) stepped_publish: u64,
    pub(super) stale_steps: u32,
    pub(super) stepped: Option<Instant>,
    pub(super) climbing: bool,
    pub(super) mantle: Option<Mantle>,
    pub(super) mantle_steps: u32,
    pub(super) mantle_frame: Option<MantleFrame>,
    pub(super) player: Option<(usize, usize)>,
    pub(super) smoothed: Vec3,
    pub(super) smoothed_carry: Vec3,
    pub(super) motion_grips: [Option<Hold>; 2],
    pub(super) input_at: Option<Instant>,
    pub(super) input_held: [bool; 2],
    pub(super) input_mode: Option<Mode>,
    /// The feet's height when the climb started.
    pub(super) start_feet_y: f32,
    /// The fling still carrying the body, if one is.
    pub(super) carry: Option<Carry>,
    pub(super) events: Vec<(Hand, Event)>,
    pub(super) epoch: u64,
    pub(super) warned_mode: bool,
    pub(super) logged_at: Option<Instant>,
    /// Whether the game held the player in a stunt position on the last step.
    pub(super) held_by_game: bool,
    pub(super) status: ClimbStatus,
}

impl State {
    pub(super) fn fresh(&self, now: Instant) -> bool {
        self.published
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    pub(super) fn stepped_lately(&self, now: Instant) -> bool {
        self.stepped
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    /// The highest gripped surface that is a ledge's top ([`is_ledge`]).
    pub(super) unsafe fn ledge_top(&self) -> Option<Vec3> {
        let grip = self.ledge_grip()?;
        Some(unsafe { grip.carried() }.map_or(grip.hold.point, |c| c.point))
    }

    /// The grip on the highest ledge's top.
    pub(super) fn ledge_grip(&self) -> Option<Grip> {
        self.grips
            .iter()
            .flatten()
            .filter(|g| g.ledge)
            .max_by(|a, b| a.hold.point.y.total_cmp(&b.hold.point.y))
            .copied()
    }

    /// The step's part while the game holds the player in a stunt position: each grip holds its
    /// hand where it took hold, carried with a moving body, and nothing moves the player.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step.
    pub(super) unsafe fn hold_only(&mut self) -> [Option<Goal>; 2] {
        self.finish_climb();
        for hand in Hand::BOTH {
            self.status.hands[hand.index()].grip = self.grips[hand.index()].map(|g| g.hold);
        }
        self.grips.map(|grip| {
            let grip = grip?;
            // SAFETY: forwarded from the caller.
            let (position, orientation) = unsafe { grip.carried() }.map_or(grip.pin, |c| c.pin);
            Some(Goal {
                position,
                orientation,
            })
        })
    }

    pub(super) fn drop_hand(&mut self, hand: Hand, reason: &'static str) {
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

    pub(super) fn drop_all(&mut self, reason: &'static str) {
        for hand in Hand::BOTH {
            self.drop_hand(hand, reason);
        }
    }

    /// Forget the climb once no hand grips.
    pub(super) fn finish_climb(&mut self) {
        self.finish_climb_with(
            Instant::now(),
            crate::controllers::physical_hands::climb::native::finish_grips,
        );
    }

    pub(super) fn finish_climb_with(&mut self, now: Instant, finish_grips: impl FnOnce(Instant)) {
        if self.climbing {
            tracing::info!("hand climbing: climbing ends");
        }
        self.climbing = false;
        self.mantle = None;
        self.mantle_steps = 0;
        self.mantle_frame = None;
        self.smoothed = Vec3::ZERO;
        self.smoothed_carry = Vec3::ZERO;
        self.motion_grips = [None; 2];
        finish_grips(now);
        self.status.climbing = false;
        self.status.mantling = false;
        self.status.velocity = Vec3::ZERO;
        for hand in &mut self.status.hands {
            hand.grip = None;
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    requests: [None; 2],
    grips: [None; 2],
    palms: [None; 2],
    pressing: [false; 2],
    rig: None,
    rig_epoch: 0,
    published: None,
    publishes: 0,
    stepped_publish: 0,
    stale_steps: 0,
    stepped: None,
    climbing: false,
    mantle: None,
    mantle_steps: 0,
    mantle_frame: None,
    player: None,
    smoothed: Vec3::ZERO,
    smoothed_carry: Vec3::ZERO,
    motion_grips: [None; 2],
    input_at: None,
    input_held: [false; 2],
    input_mode: None,
    start_feet_y: 0.0,
    carry: None,
    events: Vec::new(),
    epoch: 0,
    warned_mode: false,
    logged_at: None,
    held_by_game: false,
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
        moving_grips: 0,
        releases: 0,
        mantles: 0,
        drops: 0,
        last_drop: None,
        last_fling_mps: None,
        last_error: None,
    },
});
