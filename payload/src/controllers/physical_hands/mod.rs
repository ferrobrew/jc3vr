//! Physical hands: each empty hand's palm collides with the world. The design is in
//! `docs/mod/input/physical-hands.md`, the engine side in `docs/engine/gameplay/melee-and-damage.md`.
//!
//! - [`palm`]: the palm box at the tracked wrist, and the points its contacts are probed from;
//! - [`contact`]: whether each probe point has passed into a surface, and how deep;
//! - [`push`]: the body's push away from what the palms press into;
//! - [`strike`]: a palm's hit on a character as a punch or a slap, and what it deals;
//! - [`shove`]: a palm's soft contact with a loose body, and how much the body gives;
//! - [`pickup`]: taking hold of loose objects and picking up weapons with the grip, with the pure
//!   grip in [`grasp`] and the solver that carries what the hands hold in [`hold`];
//! - [`rigid`]: a rigid body's response to impulses;
//! - [`touch`]: what the contacts do to the world: strikes, knocks, shoves, and pressure pulses;
//! - [`config`]: the settings.
//!
//! The palms are probed rather than simulated: no body is added to the physics world. Each tick,
//! from the player aim control's post-simulation update ([`after_aim`], the simulation-phase update
//! where the engine casts its own aim ray), each probe point of each acting palm is cast from where
//! it last was in the open to where it is now, through the player aim's ray filter, which never
//! hits the player. What the casts find drives these:
//!
//! 1. A press into a solid surface pushes the body away from it ([`push`]), as movement added to
//!    the on-foot locomotion task's own ([`push_body`], from the roomscale simulation half), so
//!    that the view moves with the body as it does for stick movement.
//! 2. The rendered hand is held at the surface ([`apply`], from [`crate::controllers::hands`]).
//! 3. A fast hit on a character strikes it ([`strike`]): the game's melee damage and hit reaction,
//!    scaled by the hand's speed into the character, as a punch with a closed fist and as a slap
//!    with an open hand.
//! 4. A press into a dynamic object shoves it every tick, within the hand's strength ([`shove`]);
//!    the share it does not give pushes the body away, as a wall does. A fast hit also knocks it.
//! 5. An object within reach of a palm can be taken with the grip ([`pickup`]), held at the point
//!    the hand touched ([`grasp`]) and carried within the hands' strength ([`hold`]); the rendered
//!    hand stays on the grab point, and what the hand cannot move pushes the body.
//!
//! A palm acts only on foot on the ground, for a tracked hand the controller drives that holds
//! nothing: not a gun, not the foregrip, not a grenade or a charge being thrown. While a palm acts
//! and strikes are on, the physical-melee gesture does not start the game's melee attack for that
//! hand ([`replaces_melee`]); the palm's own strike replaces it.
//!
//! The strikes, the shoves, and the holds change engine state that is the main thread's, so they
//! are made only when the aim update runs on the thread the input tick runs on ([`input_tick`]
//! notes it); otherwise the palms stand down and the readout says why.

mod bodies;
pub mod config;
pub mod contact;
mod draw;
mod engine;
pub mod grasp;
pub mod hold;
pub mod palm;
pub mod pickup;
pub mod push;
pub mod rigid;
pub mod shove;
pub mod strike;
mod touch;

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

pub use config::PhysicalHandsConfig;
use glam::{Mat4, Quat, Vec2, Vec3};
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
            grasp::{AngularTracker, PalmMotion, Reach, VelocityTracker},
            hold::BodySpace,
            palm::{PROBE_POINTS, PalmBox},
            pickup::{HeldReport, PickupState, PickupStatus},
            shove::{Give, ShoveTuning},
            strike::{StrikeGate, StrikeKind, StrikePlan},
            touch::{Event, ShoveInput, pressure_pulses},
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
    /// What the hand could take with its grip.
    pub reach: Option<Reach>,
    /// What the hand holds.
    pub held: Option<HeldReport>,
    /// The palm's shove of the object it presses into.
    pub shove: Option<ShoveReport>,
}

/// One palm's shove of a loose object on a tick, for the readout and the overlay.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ShoveReport {
    /// Where the palm touches the object, world space.
    pub at: Vec3,
    /// The impulse applied, world space, newton seconds.
    pub impulse: Vec3,
    /// The share of the palm's strongest push used, `0..=1`.
    pub load: f32,
    /// The share of the push the object does not give, `0..=1`, which pushes the body.
    pub resistance: f32,
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
    /// Object knocks made by fast hits.
    pub object_pushes: u64,
    /// Ticks a palm shoved an object.
    pub shoves: u64,
    /// Ticks the aim update ran off the main thread.
    pub off_thread: u64,
    /// The last cast or engine failure.
    pub last_error: Option<String>,
    pub pickup: PickupStatus,
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
    let player_at = Mat4::from(owner.m_WorldMatrixT1).w_axis.truncate();
    let space = (config.pickup.keep_out_radius_m > 0.0).then(|| BodySpace {
        base: player_at,
        height_m: body::player().height * active.world_scale,
        radius_m: config.pickup.keep_out_radius_m * active.world_scale,
        forward: body_rotation * Vec3::NEG_Z,
    });
    let shove_tuning = ShoveTuning::of(&config.shove);
    // SAFETY: forwarded from the caller.
    let melee = unsafe { engine::melee_tuning(owner) };

    let mut state = STATE.lock();
    let now = Instant::now();
    if teleport::epoch() != state.epoch {
        state.epoch = teleport::epoch();
        state.reset_probes();
        state.pickup.drop_all();
    }
    let pickup_on = config.pickup.enabled;
    if !pickup_on {
        state.pickup.drop_all();
    }
    // The palms pass through what a hand holds, so that a free palm never pushes off it; the reach
    // rays do not, so that the other hand can take hold of it too.
    let held_objects = state.pickup.held_objects();
    let probe_options = CastOptions {
        ignore: [
            owner as *const Character as usize,
            held_objects.into_iter().find(|&o| o != 0).unwrap_or(0),
        ],
        see_through: false,
    };
    let reach_options = CastOptions {
        ignore: [owner as *const Character as usize, 0],
        see_through: false,
    };
    let mut casts = 0;
    let mut error = None;
    let mut cast = |from: Vec3, to: Vec3, options: &CastOptions| -> Option<SurfaceHit> {
        casts += 1;
        let ray = Ray::new(from, to - from)?;
        // SAFETY: the simulation phase outside the physics step; the ignored objects are the live
        // local character and the objects the hands hold, which this tick's pickup checks.
        match unsafe { raycast::cast(&ray, (to - from).length(), options) } {
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
    let mut palms = [None; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let velocity = state.hand_velocity(hand);
        let speed = velocity.length();
        state.gates[i].advance(dt, speed, &config);
        let Some(cockpit) = active.hands[i].filter(|_| free[i]) else {
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            state.motion[i].0.reset();
            state.motion[i].1.reset();
            continue;
        };
        let palm = PalmBox::from_wrist(compose(cockpit.wrist), active.world_scale, &config);
        let motion = PalmMotion {
            pose: Pose {
                position: palm.centre,
                orientation: palm.orientation,
            },
            velocity: state.motion[i]
                .0
                .update(palm.centre, dt, VELOCITY_SMOOTHING_S),
            angular_velocity: state.motion[i]
                .1
                .update(palm.orientation, dt, VELOCITY_SMOOTHING_S),
        };
        palms[i] = Some(motion);
        if state.pickup.holding(hand) {
            // A holding hand neither probes, pushes, nor strikes: its palm is around the object.
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            hand_status[i] = HandStatus {
                acting: true,
                palm: Some(palm),
                speed,
                armed: state.gates[i].armed(),
                ..HandStatus::default()
            };
            continue;
        }
        let points = palm.probe_points();
        let mut contacts = [PointContact::Clear; PROBE_POINTS];
        for (k, point) in points.into_iter().enumerate() {
            contacts[k] = state.probes[i][k].step(point, shoulders[i], &mut |from, to| {
                cast(from, to, &probe_options)
            });
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
        let shove = match contact.press {
            Some((hit, depth)) if config.shove.enabled => {
                // SAFETY: the simulation phase outside the physics step; the hit was found by
                // this tick's casts.
                unsafe {
                    state.shove(
                        hand,
                        ShoveInput {
                            hit,
                            depth,
                            hand_velocity: motion.velocity,
                            dt,
                        },
                        &shove_tuning,
                        &config,
                    )
                }
            }
            _ => {
                state.gives[i].reset();
                None
            }
        };
        let resistance = shove.map_or(1.0, |s| s.resistance);
        let hand_push = contact.press.map_or(Vec2::ZERO, |(hit, depth)| {
            push::hand_push(hit.normal, depth, &config) * resistance
        });
        let offset = contact.clamp_offset();
        clamp[i] = (body_rotation.inverse() * offset) / active.world_scale.max(1e-3);
        let reach = if pickup_on {
            // SAFETY: the simulation phase outside the physics step.
            unsafe {
                pickup::reach(
                    &palm,
                    config.pickup.reach_m * active.world_scale,
                    contact.press.map(|(hit, _)| hit),
                    &config.pickup,
                    &mut |from, to| cast(from, to, &reach_options),
                )
            }
        } else {
            None
        };
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
            reach,
            held: None,
            shove,
        };
    }

    if pickup_on {
        // SAFETY: the simulation phase outside the physics step.
        let outcome = unsafe { pickup::tick(&mut state.pickup, palms, space, dt, &config.pickup) };
        state.record_pickup(outcome, &mut hand_status, &config);
        for hand in Hand::BOTH {
            let i = hand.index();
            let Some(held) = hand_status[i].held else {
                continue;
            };
            // The rendered hand stays on the grab point, wherever the body is.
            let offset =
                (held.anchor - held.target).clamp_length_max(config.pickup.break_distance_m);
            clamp[i] = (body_rotation.inverse() * offset) / active.world_scale.max(1e-3);
            if config.push && config.pickup.resist {
                hand_status[i].push =
                    hold::resist_push(held.anchor, held.target, held.load, &config);
            }
        }
    }
    pressure_pulses(&hand_status, &config);
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

/// How long the palms' velocities are smoothed over, seconds: short, so that a throw keeps the
/// flick.
const VELOCITY_SMOOTHING_S: f32 = 0.03;

/// The main thread's id, as the input tick last saw it; zero before the first.
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

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
    /// Each palm's motion across ticks, world space.
    motion: [(VelocityTracker, AngularTracker); 2],
    /// How much each palm's pressed object gives.
    gives: [Give; 2],
    /// When the last tick ran.
    at: Option<Instant>,
    epoch: u64,
    pickup: PickupState,
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
        self.gives = [Give::new(); 2];
        self.motion = [(VelocityTracker::new(), AngularTracker::new()); 2];
    }

    /// Fold a pickup tick into the readout, and pulse the hands that took hold or let go.
    fn record_pickup(
        &mut self,
        outcome: pickup::TickOutcome,
        hand_status: &mut [HandStatus; 2],
        config: &PhysicalHandsConfig,
    ) {
        let status = &mut self.status.pickup;
        for hand in Hand::BOTH {
            let i = hand.index();
            hand_status[i].held = outcome.reports[i];
            if outcome.taken[i] {
                status.holds += 1;
                pulse(
                    hand,
                    config.pickup.grab_pulse,
                    1.0,
                    config,
                    PulseSource::Pickup,
                );
            }
            if outcome.released[i] {
                status.releases += 1;
                pulse(
                    hand,
                    config.pickup.release_pulse,
                    1.0,
                    config,
                    PulseSource::Pickup,
                );
            }
        }
        if let Some(speed) = outcome.throw {
            status.last_throw_mps = Some(speed);
        }
        status.lost += outcome.lost;
        if let Some(error) = outcome.error {
            status.last_error = Some(error);
        }
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
            reach: None,
            held: None,
            shove: None,
        }; 2],
        push: Vec2::ZERO,
        casts: 0,
        strikes: 0,
        last_strike: None,
        object_pushes: 0,
        shoves: 0,
        off_thread: 0,
        last_error: None,
        pickup: PickupStatus {
            holds: 0,
            weapons: 0,
            releases: 0,
            last_throw_mps: None,
            lost: 0,
            last_error: None,
        },
    },
    probes: [[PointProbe::new(); PROBE_POINTS]; 2],
    gates: [StrikeGate::new(); 2],
    pressed: [false; 2],
    last_sample: [None; 2],
    clamp: [Vec3::ZERO; 2],
    motion: [(VelocityTracker::new(), AngularTracker::new()); 2],
    gives: [Give::new(); 2],
    at: None,
    epoch: 0,
    pickup: PickupState::new(),
});

/// Stop: forget the probes and the push, and say why.
fn stand_down(idle: Idle) {
    let mut state = STATE.lock();
    if idle == Idle::OffThread {
        state.status.off_thread += 1;
    }
    state.reset_probes();
    state.pickup.drop_all();
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
