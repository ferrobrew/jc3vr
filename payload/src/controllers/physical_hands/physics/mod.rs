//! Hand bodies: each empty hand is a body in the game's physics world, driven toward its controller
//! within the hand's strength. The design is in `docs/mod/input/physical-hands.md`, the engine side
//! in `docs/engine/physics/havok-bodies.md`.
//!
//! - [`body`]: one hand body in the physics world, made and destroyed as the engine's own free
//!   bodies are;
//! - [`callback`]: the physics system's pre-simulation callback that drives the bodies;
//! - [`lifecycle`]: the bodies' own state across steps, and the transitions that make, add, drive,
//!   park, join, and destroy them;
//! - [`contact`]: the contacts each body starts, as the physics system's collision listener
//!   reports them, and [`blows`], which pulses the hands by them and deals the blows they strike
//!   characters with, as worked out in [`hit`];
//! - [`drive`]: the drive, pure: the velocities that carry a body to its hand within the hand's
//!   strength, and the push a held-back hand gives the player;
//! - [`joint`]: the fixed constraint that joins a hand body to an object its grip holds, with
//!   `joint_grabs`;
//! - [`config`]: the settings.
//!
//! Each simulation tick runs in this order:
//!
//! 1. The player aim control's post-simulation update ([`observe`]) publishes each acting hand's
//!    palm pose and velocity, world space, and takes the last step's results: where each body was
//!    left against where it was sent, which draws the hand at its body ([`apply`]), and the share
//!    of its strength each hand used, which pulses it.
//! 2. Before the next step, the pre-simulation callback ([`step`]) makes, adds, or parks each
//!    hand's body, reads where the last step left it, and drives it toward where its palm will be
//!    at the end of this step, predicted from the published pose and velocity; the push a held-back
//!    hand gives the player is worked out here ([`push`]).
//! 3. The step integrates the velocities and resolves the bodies' contacts: a hand stops at a wall,
//!    shoves a crate, and leans on a car.
//!
//! The bodies change the physics world, so they are touched only from the callback, which the
//! physics system calls on the game thread outside the step, and from the shutdown cleanup on the
//! game thread. The other halves read and write a leaf lock of their own, never the VR runtime's.
//!
//! A hand acts on foot on the ground, for a tracked hand the controllers drive that holds nothing
//! and is not carrying a loose object with the grip; a hand that holds one through a joint acts,
//! and its body is joined to the object's. A hand that stops acting for a moment keeps
//! its body in place; one that stays out (a drawn gun, a vehicle, a swim) has its body taken out of
//! the simulation, and every body is destroyed when the game leaves gameplay, the hand bodies are
//! switched off, the player's character changes, or the payload unloads.

mod blows;
mod body;
mod callback;
pub mod config;
mod contact;
pub mod drive;
pub mod hit;
mod joint;
mod lifecycle;

use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub use blows::BlowReport;
pub use config::{HandBodiesConfig, HandHitsConfig};
use glam::{Quat, Vec2, Vec3};
use jc3gi::physics::PhysicsSystem;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        hands::{self, CockpitHand, pose as hand_pose},
        mode::{self, Mode, OnFootVariant},
        physical_hands::{
            PhysicalHandsConfig, STATE, free_hands,
            grasp::{AngularTracker, VelocityTracker},
            palm::PalmBox,
            physics::{
                body::{BodySpec, Capsule, HandBody, player_system_group},
                drive::{DriveTuning, Goal, PushTuning, lag_push, smooth_push},
                joint::Joint,
                lifecycle::{ActRequest, HANDS},
            },
            pulse,
        },
        roomscale::teleport,
        scheme,
        weapons::haptics::PulseSource,
    },
    headpose, hooks,
};

/// What a hand body is doing.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum BodyPhase {
    /// No body.
    #[default]
    Absent,
    /// A body out of the simulation, waiting for its hand to act again.
    Parked,
    /// A body in the simulation, driven toward its hand.
    Acting,
}

impl BodyPhase {
    pub const fn label(self) -> &'static str {
        match self {
            BodyPhase::Absent => "no body",
            BodyPhase::Parked => "parked",
            BodyPhase::Acting => "acting",
        }
    }
}

/// One hand body on the last step, for the readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandBodyStatus {
    pub phase: BodyPhase,
    pub body_id: Option<u32>,
    /// The body's collision-filter word as the world holds it.
    pub filter_info: Option<u32>,
    /// How far the body is short of where it was sent, metres.
    pub lag_m: f32,
    pub speed_mps: f32,
    /// The share of the hand's strongest pull used, `0..=1`.
    pub force_share: f32,
    /// The share of the hand's strongest twist used, `0..=1`.
    pub torque_share: f32,
    /// Times the body fell too far behind and was put back on its hand.
    pub resets: u64,
    /// The id of the constraint that joins the body to what its grip holds.
    pub joint: Option<u32>,
}

/// The hand bodies' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HandBodiesStatus {
    /// Whether the pre-simulation callback is registered.
    pub registered: bool,
    /// Whether a panic stopped the hand bodies for the session.
    pub failed: bool,
    /// Pre-simulation steps the callback has run.
    pub steps: u64,
    /// The player's collision system group the bodies share.
    pub system_group: Option<u32>,
    /// Indexed by [`Hand::index`].
    pub hands: [HandBodyStatus; 2],
    /// The push the hands give the player, world `(x, z)` metres per second.
    pub push: Vec2,
    pub created: u64,
    pub destroyed: u64,
    /// Contacts the bodies started, as the collision listener reported them.
    pub contacts: u64,
    /// Contacts dropped because more arrived between two frame ticks than the queue holds.
    pub dropped_contacts: u64,
    /// Blows dealt to characters.
    pub blows: u64,
    pub last_blow: Option<BlowReport>,
    pub last_error: Option<String>,
}

pub fn status() -> HandBodiesStatus {
    let mut status = SHARED.lock().status.clone();
    status.failed = callback::failed();
    status
}

/// Whether the hand bodies are on: their switch, under the physical hands' master switch, unless a
/// panic stopped them.
pub(crate) fn enabled(config: &PhysicalHandsConfig) -> bool {
    config.enabled && config.bodies.enabled && !callback::failed()
}

/// An object's body a hand's grip holds through a joint.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HoldTarget {
    pub object: NonZeroUsize,
    pub body: u32,
}

/// Publish what each hand's grip holds through a joint, for the next step to join. Called from the
/// player aim control's post-simulation update.
pub(crate) fn publish_holds(holds: [Option<HoldTarget>; 2]) {
    SHARED.lock().holds = holds;
}

/// Take which hands lost the joint their grip held since the last call: the joint could not be
/// made, the object's body went away, or the hand's body stopped.
pub(crate) fn take_joint_losses() -> [bool; 2] {
    std::mem::take(&mut SHARED.lock().joint_lost)
}

/// Whether `hand` is driven by its body: the hand acts, and the last step drove its body. The
/// palm's probes leave such a hand alone, and take it back as soon as its body stops.
pub(crate) fn drives(hand: Hand) -> bool {
    let shared = SHARED.lock();
    let now = Instant::now();
    shared.fresh(now)
        && shared.stepped_lately(now)
        && shared.targets[hand.index()].is_some()
        && shared.status.hands[hand.index()].phase == BodyPhase::Acting
}

/// Register the pre-simulation callback when the hand bodies are wanted, and the cleanup that
/// removes it. Called once per frame on the game thread, ahead of the game's update.
pub(crate) fn frame_tick() {
    if crate::is_shutting_down() || callback::failed() {
        blows::discard();
        return;
    }
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    if !enabled(&config) || !hooks::in_gameplay() {
        blows::discard();
        return;
    }
    // SAFETY: the game thread, ahead of the game's update.
    let delivered = unsafe { blows::deliver(&config) };
    // SAFETY: the game thread, ahead of the game's update, which fires the callbacks.
    let registered = unsafe { callback::ensure_registered() };
    if registered && !CLEANUP_REGISTERED.swap(true, Ordering::AcqRel) {
        crate::lifecycle::on_cleanup(|_renderer| {
            // SAFETY: the shutdown cleanups run on the game thread, outside the game's update.
            unsafe { shutdown() };
        });
    }
    let mut shared = SHARED.lock();
    let status = &mut shared.status;
    status.registered = registered;
    status.contacts += delivered.contacts;
    status.dropped_contacts += delivered.dropped;
    status.blows += delivered.blows;
    if delivered.last.is_some() {
        status.last_blow = delivered.last;
    }
}

/// Publish each acting hand's palm pose and velocity for the next step, and take the last step's
/// results. Called from the player aim control's post-simulation update, `dt` seconds after the
/// last.
pub(crate) fn observe(dt: f32) {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let palms = enabled(&config).then(|| acting_palms(&config)).flatten();
    let now = Instant::now();
    let body_rotation = headpose::xr::body_rotation();
    let mut loads = [0.0; 2];
    {
        let mut shared = SHARED.lock();
        if teleport::epoch() != shared.epoch {
            shared.epoch = teleport::epoch();
            shared.trackers = TRACKERS_AT_REST;
        }
        let world_scale = palms.as_ref().map_or(1.0, |(_, scale)| *scale);
        let mut targets = [None; 2];
        for hand in Hand::BOTH {
            let i = hand.index();
            let palm = palms.as_ref().and_then(|(palms, _)| palms[i]);
            let Some(palm) = palm else {
                shared.trackers[i].0.reset();
                shared.trackers[i].1.reset();
                continue;
            };
            let linear = shared.trackers[i]
                .0
                .update(palm.centre, dt, VELOCITY_SMOOTHING_S);
            let angular = shared.trackers[i]
                .1
                .update(palm.orientation, dt, VELOCITY_SMOOTHING_S);
            targets[i] = Some(Target {
                position: palm.centre,
                orientation: palm.orientation,
                linear,
                angular,
            });
        }
        shared.targets = targets;
        shared.world_scale = world_scale;
        shared.published = palms.is_some().then_some(now);
        let stepped = shared.stepped_lately(now);
        let show = stepped && config.bodies.render_at_body;
        let scale = world_scale.max(1e-3);
        let offsets = shared.world_offsets;
        for hand in Hand::BOTH {
            let i = hand.index();
            shared.cockpit_offsets[i] = if show && targets[i].is_some() {
                body_rotation.inverse() * offsets[i] / scale
            } else {
                Vec3::ZERO
            };
            if stepped && targets[i].is_some() {
                loads[i] = shared.loads[i];
            }
        }
    }
    if config.bodies.pressure_haptics {
        for hand in Hand::BOTH {
            let load = loads[hand.index()];
            if load >= MIN_PRESSURE_LOAD {
                pulse(
                    hand,
                    config.pressure_pulse,
                    load,
                    &config,
                    PulseSource::Collision,
                );
            }
        }
    }
}

/// Draw each driven hand where its body is, in the cockpit frame. Called by
/// [`crate::controllers::physical_hands::apply`].
pub(crate) fn apply(cockpit: &mut [Option<CockpitHand>; 2]) {
    let offsets = {
        let shared = SHARED.lock();
        if !shared.fresh(Instant::now()) {
            return;
        }
        shared.cockpit_offsets
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

/// The push the hand bodies give the player, world `(x, z)` metres per second: zero when no step
/// has run lately.
pub(crate) fn push() -> Vec2 {
    let shared = SHARED.lock();
    let now = Instant::now();
    if shared.stepped_lately(now) && shared.fresh(now) {
        shared.push
    } else {
        Vec2::ZERO
    }
}

/// The pre-simulation step: make, add, park, and drive the hand bodies. Called by the callback.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub(super) unsafe fn step() {
    let now = Instant::now();
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let mut hands = HANDS.lock();
    hands.steps += 1;
    let reason = if !enabled(&config) {
        Some("the hand bodies are off")
    } else if !hooks::in_gameplay() {
        Some("the game left gameplay")
    } else if crate::is_shutting_down() {
        Some("the payload is shutting down")
    } else {
        None
    };
    if let Some(reason) = reason {
        let destroyed = unsafe { hands.destroy_all(reason) };
        let mut shared = SHARED.lock();
        shared.status.destroyed += destroyed;
        shared.status.steps = hands.steps;
        shared.status.hands = [HandBodyStatus::default(); 2];
        shared.status.push = Vec2::ZERO;
        shared.push = Vec2::ZERO;
        shared.world_offsets = [Vec3::ZERO; 2];
        shared.loads = [0.0; 2];
        return;
    }

    let (targets, published, world_scale, mut push, holds) = {
        let shared = SHARED.lock();
        (
            shared.targets,
            shared.published,
            shared.world_scale,
            shared.push,
            shared.holds,
        )
    };
    let fresh = published.is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER);
    let mut joint_lost = [false; 2];
    let holds = if !fresh {
        [None; 2]
    } else if config.bodies.joint_grabs {
        holds
    } else {
        // A grip that holds through a joint while the joints are switched off holds nothing.
        joint_lost = holds.map(|h| h.is_some());
        [None; 2]
    };
    let age_s = published
        .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32())
        .min(MAX_LEAD_S);
    let step_s = PhysicsSystem::STEP_SECONDS;
    let lead_s = age_s + step_s;
    // SAFETY: forwarded from the caller.
    let group = unsafe { player_system_group() };
    let b = &config.bodies;
    let scale = if world_scale.is_finite() && world_scale > 0.0 {
        world_scale
    } else {
        1.0
    };
    let tuning = DriveTuning {
        mass_kg: b.mass_kg,
        max_force_n: b.force_n(),
        max_torque_nm: b.torque_nm(),
        linear_gain: b.linear_gain,
        angular_gain: b.angular_gain,
        max_speed_mps: b.max_speed_mps,
        max_angular_speed_rps: b.max_angular_speed_rps,
    };
    let push_tuning = PushTuning {
        hand_mass_kg: b.mass_kg,
        max_force_n: b.force_n(),
        body_mass_kg: b.body_mass_kg,
        deadband_m: b.push_deadband_m * scale,
        min_horizontal: b.min_push_horizontal,
    };

    let mut statuses = [HandBodyStatus::default(); 2];
    let mut offsets = [Vec3::ZERO; 2];
    let mut loads = [0.0; 2];
    let mut created = 0;
    let mut destroyed = 0;
    let mut error = None;
    let mut hand_push = Vec2::ZERO;
    for hand in Hand::BOTH {
        let i = hand.index();
        let target = targets[i].filter(|_| fresh);
        match (target, group) {
            (Some(target), Some(group)) => {
                hands.missing_since[i] = None;
                let spec = BodySpec {
                    capsule: Capsule {
                        half_length_m: b.half_length_m * scale,
                        radius_m: b.radius_m * scale,
                    },
                    mass_kg: b.mass_kg,
                    system_group: group,
                    contact_events: b.hits.enabled,
                };
                let goal = Goal {
                    position: target.position + target.linear * lead_s,
                    orientation: Quat::from_scaled_axis(target.angular * lead_s)
                        * target.orientation,
                };
                let request = ActRequest {
                    hand,
                    spec,
                    goal,
                    break_distance_m: b.break_distance_m * scale,
                    tuning: &tuning,
                    step_s,
                };
                // SAFETY: forwarded from the caller.
                match unsafe { hands.act(request) } {
                    Ok(acted) => {
                        created += u64::from(acted.created);
                        destroyed += u64::from(acted.destroyed);
                        // SAFETY: forwarded from the caller.
                        joint_lost[i] |= acted.joint_lost || unsafe { hands.join(hand, holds[i]) };
                        statuses[i] = acted.status;
                        statuses[i].joint = hands.joints[i].as_ref().map(Joint::id);
                        offsets[i] = (-acted.lag).clamp_length_max(b.break_distance_m * scale);
                        loads[i] = press_load(acted.lag, b, scale);
                        if b.push {
                            hand_push += lag_push(acted.lag, &push_tuning, step_s);
                        }
                    }
                    Err(e) => {
                        error = Some(e.to_string());
                        // SAFETY: forwarded from the caller.
                        joint_lost[i] |= unsafe { hands.unjoin(hand, "the hand body failed") };
                        statuses[i].phase = hands.phase(hand);
                    }
                }
            }
            (_, group) => {
                let since = *hands.missing_since[i].get_or_insert(now);
                let reason = if group.is_none() {
                    "the player has no collision group"
                } else {
                    "the hand stopped acting"
                };
                // SAFETY: forwarded from the caller.
                joint_lost[i] |= unsafe { hands.unjoin(hand, "the hand stopped acting") };
                if group.is_none() || now.saturating_duration_since(since) >= PARK_AFTER {
                    // SAFETY: forwarded from the caller.
                    unsafe { hands.park(hand, reason) };
                } else {
                    // SAFETY: forwarded from the caller.
                    unsafe { hands.hold(hand) };
                }
                statuses[i].phase = hands.phase(hand);
                statuses[i].body_id = hands.bodies[i].as_ref().map(HandBody::body_id);
            }
        }
    }
    push = if b.push {
        smooth_push(
            push,
            hand_push,
            step_s,
            b.push_smoothing_s,
            b.max_push_speed_mps,
        )
    } else {
        Vec2::ZERO
    };
    let log_now = hands
        .last_log
        .is_none_or(|at| now.saturating_duration_since(at) >= READOUT_PERIOD);
    if log_now && statuses.iter().any(|s| s.phase == BodyPhase::Acting) {
        hands.last_log = Some(now);
        for hand in Hand::BOTH {
            let s = &statuses[hand.index()];
            if s.phase == BodyPhase::Acting {
                tracing::debug!(
                    hand = hand.label(),
                    lag_mm = s.lag_m * 1000.0,
                    speed_mps = s.speed_mps,
                    force_share = s.force_share,
                    torque_share = s.torque_share,
                    push_x = push.x,
                    push_z = push.y,
                    "hand bodies: drive"
                );
            }
        }
    }

    let mut shared = SHARED.lock();
    for (lost, now_lost) in shared.joint_lost.iter_mut().zip(joint_lost) {
        *lost |= now_lost;
    }
    shared.world_offsets = offsets;
    shared.loads = loads;
    shared.push = push;
    shared.stepped = Some(now);
    let status = &mut shared.status;
    status.steps = hands.steps;
    status.system_group = group;
    status.hands = statuses;
    status.push = push;
    status.created += created;
    status.destroyed += destroyed;
    if let Some(error) = error {
        status.last_error = Some(error);
    }
}

/// How long a published pose stays current: the aim update runs every simulation tick while the
/// hands act, so an older one means they have stopped.
const STALE_AFTER: Duration = Duration::from_millis(200);

/// How long a hand may stop acting before its body leaves the simulation, so that a moment's lost
/// tracking does not churn the world.
const PARK_AFTER: Duration = Duration::from_millis(250);

/// The longest a published pose is carried forward by its velocity, seconds.
const MAX_LEAD_S: f32 = 0.1;

/// How long the palms' velocities are smoothed over, seconds.
const VELOCITY_SMOOTHING_S: f32 = 0.03;

/// The least share of a hand's strength that pulses it.
const MIN_PRESSURE_LOAD: f32 = 0.1;

/// How often the drive is written to the debug log while a hand acts.
const READOUT_PERIOD: Duration = Duration::from_secs(1);

const TRACKERS_AT_REST: [(VelocityTracker, AngularTracker); 2] =
    [(VelocityTracker::new(), AngularTracker::new()); 2];

static CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Where a hand's palm is, world space, and how it moves.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Target {
    position: Vec3,
    orientation: Quat,
    linear: Vec3,
    angular: Vec3,
}

/// What the aim update and the step share.
struct Shared {
    /// Each hand's palm, as last published; `None` for a hand that does not act.
    targets: [Option<Target>; 2],
    /// When the targets were published; `None` while no hand acts.
    published: Option<Instant>,
    world_scale: f32,
    trackers: [(VelocityTracker, AngularTracker); 2],
    epoch: u64,
    /// Each body against where it was last sent, world space.
    world_offsets: [Vec3; 2],
    /// The same in the cockpit frame, for drawing the hands.
    cockpit_offsets: [Vec3; 2],
    /// The share of each hand's strength the last step used.
    loads: [f32; 2],
    /// The smoothed push, world `(x, z)` metres per second.
    push: Vec2,
    /// What each hand's grip holds through a joint, as last published.
    holds: [Option<HoldTarget>; 2],
    /// Which hands lost their joint since the aim update last took it.
    joint_lost: [bool; 2],
    /// When the last step ran.
    stepped: Option<Instant>,
    status: HandBodiesStatus,
}

impl Shared {
    fn fresh(&self, now: Instant) -> bool {
        self.published
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    fn stepped_lately(&self, now: Instant) -> bool {
        self.stepped
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    targets: [None; 2],
    published: None,
    world_scale: 1.0,
    trackers: TRACKERS_AT_REST,
    epoch: 0,
    world_offsets: [Vec3::ZERO; 2],
    cockpit_offsets: [Vec3::ZERO; 2],
    loads: [0.0; 2],
    push: Vec2::ZERO,
    holds: [None; 2],
    joint_lost: [false; 2],
    stepped: None,
    status: HandBodiesStatus {
        registered: false,
        failed: false,
        steps: 0,
        system_group: None,
        hands: [HandBodyStatus {
            phase: BodyPhase::Absent,
            body_id: None,
            filter_info: None,
            lag_m: 0.0,
            speed_mps: 0.0,
            force_share: 0.0,
            torque_share: 0.0,
            resets: 0,
            joint: None,
        }; 2],
        push: Vec2::ZERO,
        created: 0,
        destroyed: 0,
        contacts: 0,
        dropped_contacts: 0,
        blows: 0,
        last_blow: None,
        last_error: None,
    },
});

/// The hands' palms when they act, with the VR world scale: on foot on the ground, for a tracked
/// hand the controllers drive that holds nothing.
fn acting_palms(config: &PhysicalHandsConfig) -> Option<([Option<PalmBox>; 2], f32)> {
    if !scheme::is_controllers() || !hooks::in_gameplay() {
        return None;
    }
    let detection = mode::current();
    if detection.mode != Mode::OnFoot(OnFootVariant::Ground) || detection.traversal.is_some() {
        return None;
    }
    let active = hands::active_hands_raw()?;
    let head = headpose::query();
    let body_rotation = headpose::xr::body_rotation();
    let free = free_hands();
    // A hand that carries what it holds, rather than holding it through a joint, has no body, so
    // that it never fights the object.
    let carrying = {
        let state = STATE.lock();
        Hand::BOTH.map(|hand| state.pickup.carrying(hand))
    };
    let palms = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let cockpit = active.hands[i].filter(|_| free[i] && !carrying[i])?;
        let wrist = hand_pose::compose_world(
            head.position,
            body_rotation,
            active.head.position,
            cockpit.wrist,
            active.world_scale,
        );
        Some(PalmBox::from_wrist(wrist, active.world_scale, config))
    });
    palms
        .iter()
        .any(Option::is_some)
        .then_some((palms, active.world_scale))
}

/// How hard a hand presses into what holds it back, `0..=1`: its lag past the push deadband, as a
/// share of the lag that presses fully.
fn press_load(lag: Vec3, config: &HandBodiesConfig, scale: f32) -> f32 {
    let full = config.pressure_full_lag_m * scale;
    if full <= 0.0 || !lag.is_finite() {
        return 0.0;
    }
    ((lag.length() - config.push_deadband_m * scale) / full).clamp(0.0, 1.0)
}

/// Remove the callback and destroy the bodies.
///
/// # Safety
///
/// The game thread, outside the game's update.
unsafe fn shutdown() {
    unsafe { callback::unregister() };
    let destroyed = unsafe { HANDS.lock().destroy_all("the payload is unloading") };
    let mut shared = SHARED.lock();
    shared.status.registered = false;
    shared.status.destroyed += destroyed;
}
