//! Physical hands: each empty hand's palm collides with the world. The design is in
//! `docs/mod/input/physical-hands.md`, the engine side in `docs/engine/gameplay/melee-and-damage.md`.
//!
//! - [`palm`]: the palm box at the tracked wrist, and the points its contacts are probed from;
//! - [`contact`]: whether each probe point has passed into a surface, and how deep;
//! - [`push`]: the body's push away from what the palms press into;
//! - [`strike`]: a palm's hit on a character as a punch or a slap, and what it deals;
//! - [`config`]: the settings.
//!
//! The palms are probed rather than simulated: no body is added to the physics world. Each tick,
//! from the player aim control's post-simulation update ([`after_aim`], the simulation-phase update
//! where the engine casts its own aim ray), each probe point of each acting palm is cast from where
//! it last was in the open to where it is now, through the player aim's ray filter, which never
//! hits the player. What the casts find drives four things:
//!
//! 1. A press into a solid surface pushes the body away from it ([`push`]), as movement added to
//!    the on-foot locomotion task's own ([`push_body`], from the roomscale simulation half), so
//!    that the view moves with the body as it does for stick movement.
//! 2. The rendered hand is held at the surface ([`apply`], from [`crate::controllers::hands`]).
//! 3. A fast hit on a character strikes it ([`strike`]): the game's melee damage and hit reaction,
//!    scaled by the hand's speed into the character, as a punch with a closed fist and as a slap
//!    with an open hand.
//! 4. A fast hit on a dynamic object pushes its body.
//!
//! A palm acts only on foot on the ground, for a tracked hand the controller drives that holds
//! nothing: not a gun, not the foregrip, not a grenade or a charge being thrown. While a palm acts
//! and strikes are on, the physical-melee gesture does not start the game's melee attack for that
//! hand ([`replaces_melee`]); the palm's own strike replaces it.
//!
//! The strikes and the object pushes change engine state that is the main thread's, so they are
//! made only when the aim update runs on the thread the input tick runs on ([`input_tick`] notes
//! it); otherwise the palms stand down and the readout says why.

pub mod config;
pub mod contact;
mod draw;
mod engine;
pub mod palm;
pub mod push;
pub mod strike;

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

pub use config::PhysicalHandsConfig;
use glam::{Quat, Vec2, Vec3};
use jc3gi::{character::character::Character, types::math::Vector3};
use parking_lot::Mutex;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::{
    config::Config,
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions},
        },
        body, fingers,
        frame::{self, Hand, Pose},
        hands::{self, CockpitHand, pose as hand_pose, reach, tiers},
        mode::{self, Mode, OnFootVariant},
        motion,
        physical_hands::{
            contact::{HandContact, PointContact, PointProbe, Surface, SurfaceHit},
            palm::{PROBE_POINTS, PalmBox},
            strike::{StrikeGate, StrikeKind, StrikePlan, impact_speed, strength},
        },
        roomscale::teleport,
        scheme,
        weapons::{
            self, c4,
            config::PulseSpec,
            foregrip, grenade,
            haptics::{self, PulseSource},
        },
    },
    headpose, hooks,
};

/// Why the palms did nothing on the last aim update.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Idle {
    /// The physical hands are off, or the controller scheme is not in effect.
    Disabled,
    /// Not on foot on the ground.
    Mode,
    /// No tracked hands.
    NoHands,
    /// The aim update ran on another thread than the input tick.
    OffThread,
}

impl Idle {
    pub const fn label(self) -> &'static str {
        match self {
            Idle::Disabled => "off",
            Idle::Mode => "not on foot on the ground",
            Idle::NoHands => "no tracked hands",
            Idle::OffThread => "the aim update is off the main thread",
        }
    }
}

/// One palm's state on the last tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandStatus {
    /// Whether the palm acted.
    pub acting: bool,
    pub palm: Option<PalmBox>,
    /// The deepest press into a solid surface, metres, and whether the surface is an object.
    pub press: Option<(f32, bool)>,
    /// The push this palm asked for, world `(x, z)` metres per second.
    pub push: Vec2,
    /// The hand's speed, metres per second.
    pub speed: f32,
    /// How the hand would strike now.
    pub kind: Option<StrikeKind>,
    /// Whether the strike gate is armed.
    pub armed: bool,
}

/// One strike, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrikeReport {
    pub hand: Hand,
    pub plan: StrikePlan,
    /// The hand's speed into the character, metres per second.
    pub impact_mps: f32,
    /// The damage the character took.
    pub dealt: i16,
}

/// The physical hands' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhysicalHandsStatus {
    pub idle: Option<Idle>,
    /// Indexed by [`Hand::index`].
    pub hands: [HandStatus; 2],
    /// The combined push, world `(x, z)` metres per second.
    pub push: Vec2,
    /// Casts made on the last tick.
    pub casts: u32,
    pub strikes: u64,
    pub last_strike: Option<StrikeReport>,
    /// Object pushes made.
    pub object_pushes: u64,
    /// Ticks the aim update ran off the main thread.
    pub off_thread: u64,
    /// The last cast or engine failure.
    pub last_error: Option<String>,
}

pub fn status() -> PhysicalHandsStatus {
    STATE.lock().status.clone()
}

/// Note the input tick's thread, the main thread. Called from the input tick.
pub(crate) fn input_tick() {
    // SAFETY: reads the calling thread's own id.
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

/// Whether the palm of `hand` replaces the physical-melee gesture: it acted on the last tick with
/// strikes on and the replacement chosen.
pub(crate) fn replaces_melee(hand: Hand) -> bool {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    if !(config.enabled && config.strikes && config.replace_melee_gesture) {
        return false;
    }
    let state = STATE.lock();
    state.fresh(Instant::now()) && state.status.hands[hand.index()].acting
}

/// Hold each pressed hand at the surface it presses into, in the cockpit frame. Called by
/// [`crate::controllers::hands::active_hands`].
pub fn apply(cockpit: &mut [Option<CockpitHand>; 2]) {
    let enabled = Config::lock_query(|c| {
        c.controllers.physical_hands.enabled && c.controllers.physical_hands.clamp_hand
    });
    if !enabled {
        return;
    }
    let offsets = {
        let state = STATE.lock();
        if !state.fresh(Instant::now()) {
            return;
        }
        state.clamp
    };
    for (hand, offset) in cockpit.iter_mut().zip(offsets) {
        if let Some(hand) = hand.as_mut()
            && offset != Vec3::ZERO
        {
            hand.wrist.position += offset;
            hand.grip.position += offset;
            hand.aim.position += offset;
        }
    }
}

/// Add the palms' push to the on-foot locomotion task's velocity. Called by the roomscale
/// simulation half before its chase reads the task's velocity, so that the push counts as the
/// task's own movement.
///
/// # Safety
///
/// `character` must be the live local player character, after the locomotion task wrote its
/// velocity.
pub(crate) unsafe fn push_body(character: &mut Character) {
    let enabled = Config::lock_query(|c| {
        c.controllers.physical_hands.enabled && c.controllers.physical_hands.push
    });
    if !enabled {
        return;
    }
    let push = {
        let state = STATE.lock();
        if !state.fresh(Instant::now()) {
            return;
        }
        state.status.push
    };
    if push == Vec2::ZERO || !push.is_finite() {
        return;
    }
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    let [x, y, z] = wanted.data;
    let pushed = Vector3 {
        data: [x + push.x, y, z + push.y],
    };
    unsafe { character.SetWantedWorldVelocity(&pushed) };
}

/// The palms' tick, after the player aim control's post-simulation update.
///
/// # Safety
///
/// `owner` must be the live local player character, called from its aim control's update after
/// the original has run.
pub(crate) unsafe fn after_aim(owner: &Character, dt: f32) {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let idle = if !config.enabled || !scheme::is_controllers() || !hooks::in_gameplay() {
        Some(Idle::Disabled)
    } else if MAIN_THREAD.load(Ordering::Relaxed) != unsafe { GetCurrentThreadId() } {
        Some(Idle::OffThread)
    } else {
        let detection = mode::current();
        (detection.mode != Mode::OnFoot(OnFootVariant::Ground) || detection.traversal.is_some())
            .then_some(Idle::Mode)
    };
    if let Some(idle) = idle {
        stand_down(idle);
        return;
    }
    let Some(active) = hands::active_hands_raw() else {
        stand_down(Idle::NoHands);
        return;
    };
    let head = headpose::query();
    let body_rotation = headpose::xr::body_rotation();
    let compose = |pose: Pose| {
        hand_pose::compose_world(
            head.position,
            body_rotation,
            active.head.position,
            pose,
            active.world_scale,
        )
    };
    let at = |point: Vec3| {
        compose(Pose {
            position: point,
            orientation: Quat::IDENTITY,
        })
        .position
    };
    let shoulders = body::shoulder_positions(
        active.head.position,
        reach::yaw_only(active.head.orientation),
        &body::player(),
    )
    .map(at);
    let free = free_hands();
    let curls = finger_curls();
    // SAFETY: forwarded from the caller.
    let melee = unsafe { engine::melee_tuning(owner) };
    let options = CastOptions {
        ignore: [owner as *const Character as usize, 0],
        see_through: false,
    };

    let mut state = STATE.lock();
    let now = Instant::now();
    if teleport::epoch() != state.epoch {
        state.epoch = teleport::epoch();
        state.reset_probes();
    }
    let mut casts = 0;
    let mut error = None;
    let mut cast = |from: Vec3, to: Vec3| -> Option<SurfaceHit> {
        casts += 1;
        let ray = Ray::new(from, to - from)?;
        // SAFETY: the simulation phase outside the physics step; the ignored object is the live
        // local character.
        match unsafe { raycast::cast(&ray, (to - from).length(), &options) } {
            Ok(hit) => hit.map(|hit| unsafe { engine::surface(&hit) }),
            Err(e) => {
                error = Some(e.to_string());
                None
            }
        }
    };

    let mut events = Vec::new();
    let mut hand_status = [HandStatus::default(); 2];
    let mut clamp = [Vec3::ZERO; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let velocity = state.hand_velocity(hand);
        let speed = velocity.length();
        state.gates[i].advance(dt, speed, &config);
        let Some(cockpit) = active.hands[i].filter(|_| free[i]) else {
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            continue;
        };
        let palm = PalmBox::from_wrist(compose(cockpit.wrist), active.world_scale, &config);
        let points = palm.probe_points();
        let mut contacts = [PointContact::Clear; PROBE_POINTS];
        for (k, point) in points.into_iter().enumerate() {
            contacts[k] = state.probes[i][k].step(point, shoulders[i], &mut cast);
        }
        let contact = HandContact::fold(&contacts);
        let world_velocity = body_rotation * velocity;
        let kind = curls[i].map(|curl| StrikeKind::of_curl(curl, &config));

        if let Some(hit) = contact.strike.filter(|_| config.strikes) {
            events.push(Event::Strike {
                hand,
                hit,
                velocity: world_velocity,
                kind: kind.unwrap_or(StrikeKind::Slap),
            });
        }
        let pressed = contact.press.is_some();
        if let Some((hit, _)) = contact.press
            && !state.pressed[i]
        {
            events.push(Event::Touch {
                hand,
                hit,
                velocity: world_velocity,
            });
        }
        state.pressed[i] = pressed;
        let hand_push = contact.press.map_or(Vec2::ZERO, |(hit, depth)| {
            push::hand_push(hit.normal, depth, &config)
        });
        let offset = contact.clamp_offset();
        clamp[i] = (body_rotation.inverse() * offset) / active.world_scale.max(1e-3);
        hand_status[i] = HandStatus {
            acting: true,
            palm: Some(palm),
            press: contact.press.map(|(hit, depth)| {
                (
                    depth,
                    matches!(
                        hit.surface,
                        Surface::Solid {
                            object: Some(_),
                            ..
                        }
                    ),
                )
            }),
            push: if config.push { hand_push } else { Vec2::ZERO },
            speed,
            kind,
            armed: state.gates[i].armed(),
        };
    }

    let push = push::combine(hand_status.map(|h| h.push), &config);
    for event in events {
        // SAFETY: forwarded from the caller; the events' objects were found by this tick's casts.
        unsafe { state.act(event, owner, melee, &config) };
    }
    if let Some(error) = error {
        state.status.last_error = Some(error);
    }
    state.status.idle = None;
    state.status.hands = hand_status;
    state.status.push = push;
    state.status.casts = casts;
    state.clamp = clamp;
    state.at = Some(now);
    let snapshot = state.status.hands;
    drop(state);
    draw::submit(&snapshot, &config);
}

/// How long a tick's results stay current: the aim update runs every simulation tick while the
/// palms act, so an older result means they have stopped.
const STALE_AFTER: Duration = Duration::from_millis(200);

/// The main thread's id, as the input tick last saw it; zero before the first.
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

/// Something a tick's contacts do to the world.
#[derive(Debug, Copy, Clone, PartialEq)]
enum Event {
    /// A palm hit a character.
    Strike {
        hand: Hand,
        hit: SurfaceHit,
        velocity: Vec3,
        kind: StrikeKind,
    },
    /// A palm met a solid surface.
    Touch {
        hand: Hand,
        hit: SurfaceHit,
        velocity: Vec3,
    },
}

struct State {
    status: PhysicalHandsStatus,
    probes: [[PointProbe; PROBE_POINTS]; 2],
    gates: [StrikeGate; 2],
    /// Whether each palm pressed into a surface on the last tick.
    pressed: [bool; 2],
    /// Each hand's latest motion sample time read.
    last_sample: [Option<f64>; 2],
    /// Each hand's clamp offset, cockpit frame.
    clamp: [Vec3; 2],
    /// When the last tick ran.
    at: Option<Instant>,
    epoch: u64,
}

impl State {
    fn fresh(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    fn reset_probes(&mut self) {
        for probes in &mut self.probes {
            probes.iter_mut().for_each(PointProbe::reset);
        }
        self.pressed = [false; 2];
    }

    /// The hand's fastest velocity since the last tick, cockpit frame, metres per second.
    fn hand_velocity(&mut self, hand: Hand) -> Vec3 {
        let samples = motion::history(hand);
        let since = self.last_sample[hand.index()];
        self.last_sample[hand.index()] = samples.last().map(|s| s.time_s).or(since);
        since
            .and_then(|since| motion::peak_velocity(&samples, since))
            .map_or(Vec3::ZERO, |(_, v)| v)
    }

    /// Act on one event.
    ///
    /// # Safety
    ///
    /// As [`after_aim`], with the event's objects found by this tick's casts.
    unsafe fn act(
        &mut self,
        event: Event,
        owner: &Character,
        melee: Option<(i32, f32)>,
        config: &PhysicalHandsConfig,
    ) {
        match event {
            Event::Strike {
                hand,
                hit,
                velocity,
                kind,
            } => {
                let Surface::Character(victim) = hit.surface else {
                    return;
                };
                let impact = impact_speed(velocity, hit.normal);
                let Some(strength) = strength(
                    impact,
                    config.min_strike_speed_mps,
                    config.full_strike_speed_mps,
                ) else {
                    return;
                };
                let Some((damage, impulse)) = melee else {
                    return;
                };
                if !self.gates[hand.index()].take(config) {
                    return;
                }
                let plan = strike::plan(kind, strength, damage, impulse, config);
                let direction = velocity.try_normalize().unwrap_or(-hit.normal);
                // SAFETY: forwarded from the caller.
                let dealt = unsafe {
                    engine::strike_character(owner, victim, hit.position, direction, &plan)
                };
                tracing::debug!(target: "controllers", ?hand, kind = kind.label(), strength, damage = plan.damage, dealt, "physical hands: strike");
                self.status.strikes += 1;
                self.status.last_strike = Some(StrikeReport {
                    hand,
                    plan,
                    impact_mps: impact,
                    dealt,
                });
                pulse(
                    hand,
                    config.strike_pulse,
                    0.3 + 0.7 * strength,
                    config,
                    PulseSource::MeleeStrike,
                );
            }
            Event::Touch {
                hand,
                hit,
                velocity,
            } => {
                let impact = impact_speed(velocity, hit.normal);
                let share = (impact / config.full_strike_speed_mps.max(1e-3)).clamp(0.0, 1.0);
                pulse(
                    hand,
                    config.contact_pulse,
                    0.25 + 0.75 * share,
                    config,
                    PulseSource::Collision,
                );
                let Surface::Solid {
                    object: Some(_),
                    body,
                } = hit.surface
                else {
                    return;
                };
                if !config.object_impulses {
                    return;
                }
                let (Some(strength), Some((_, melee_impulse))) = (
                    strength(
                        impact,
                        config.min_object_speed_mps,
                        config.full_strike_speed_mps,
                    ),
                    melee,
                ) else {
                    return;
                };
                let direction = velocity.try_normalize().unwrap_or(-hit.normal);
                let impulse =
                    direction * (melee_impulse * config.object_impulse * strength.max(0.1));
                // SAFETY: forwarded from the caller.
                match unsafe { engine::push_body(body, impulse, hit.position) } {
                    Ok(()) => self.status.object_pushes += 1,
                    Err(e) => self.status.last_error = Some(e.to_string()),
                }
            }
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    status: PhysicalHandsStatus {
        idle: None,
        hands: [HandStatus {
            acting: false,
            palm: None,
            press: None,
            push: Vec2::ZERO,
            speed: 0.0,
            kind: None,
            armed: true,
        }; 2],
        push: Vec2::ZERO,
        casts: 0,
        strikes: 0,
        last_strike: None,
        object_pushes: 0,
        off_thread: 0,
        last_error: None,
    },
    probes: [[PointProbe::new(); PROBE_POINTS]; 2],
    gates: [StrikeGate::new(); 2],
    pressed: [false; 2],
    last_sample: [None; 2],
    clamp: [Vec3::ZERO; 2],
    at: None,
    epoch: 0,
});

/// Stop: forget the probes and the push, and say why.
fn stand_down(idle: Idle) {
    let mut state = STATE.lock();
    if idle == Idle::OffThread {
        state.status.off_thread += 1;
    }
    state.reset_probes();
    state.last_sample = [None; 2];
    state.clamp = [Vec3::ZERO; 2];
    state.at = None;
    state.status.idle = Some(idle);
    state.status.hands = [HandStatus::default(); 2];
    state.status.push = Vec2::ZERO;
    state.status.casts = 0;
    drop(state);
    draw::clear();
}

/// Which hands hold nothing: no gun, not the foregrip, no grenade or charge being thrown, and the
/// controller drives the hand.
fn free_hands() -> [bool; 2] {
    let wielded = weapons::status()
        .wielded
        .map_or([false; 2], |w| w.weapon.wielded.hands);
    let foregrip = foregrip::status()
        .hold
        .filter(|h| h.engaged)
        .map(|h| h.holder.other());
    let throwing = [grenade::throwing_hand(), c4::throwing_hand()];
    let weights = tiers::weights();
    Hand::BOTH.map(|hand| {
        !wielded[hand.index()]
            && foregrip != Some(hand)
            && !throwing.contains(&Some(hand))
            && weights[hand.index()] > 0.0
    })
}

/// Each hand's mean curl of its four fingers, `0..=1`: from the finger curls when they drive the
/// hand, else from the grip's squeeze.
fn finger_curls() -> [Option<f32>; 2] {
    let fingers = fingers::status();
    let squeeze = frame::with_latest(|f| Hand::BOTH.map(|h| f.hand(h).inputs.squeeze));
    Hand::BOTH.map(|hand| {
        let status = &fingers[hand.index()];
        if status.source.is_some() {
            let curls = &status.curls.curl[1..];
            Some(curls.iter().sum::<f32>() / curls.len() as f32)
        } else {
            squeeze.map(|s| s[hand.index()])
        }
    })
}

/// Pulse `hand` with `spec` scaled by `scale`.
fn pulse(
    hand: Hand,
    spec: PulseSpec,
    scale: f32,
    config: &PhysicalHandsConfig,
    source: PulseSource,
) {
    if !config.haptics {
        return;
    }
    haptics::pulse(
        hand,
        PulseSpec {
            amplitude: (spec.amplitude * scale).clamp(0.0, 1.0),
            duration_s: spec.duration_s,
        },
        source,
    );
}
