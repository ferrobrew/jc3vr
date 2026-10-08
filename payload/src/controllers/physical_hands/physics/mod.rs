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
//! - [`render`]: the offset that draws a hand where its body is, pure;
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
//!    hand gives the player is worked out here ([`push`]). The climb
//!    ([`crate::controllers::physical_hands::climb`]) first takes and ends its grips and moves the
//!    player, and a gripping hand's body is held where it took hold instead.
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
mod render;

use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub use blows::BlowReport;
pub use config::{HandBodiesConfig, HandHitsConfig};
use glam::{Quat, Vec2, Vec3};
use jc3gi::{character::character::Character, physics::PhysicsSystem};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions},
        },
        frame::Hand,
        hands::{self, CockpitHand, pose as hand_pose},
        mode,
        physical_hands::{
            PhysicalHandsConfig, STATE, climb, free_hands,
            grasp::{AngularTracker, VelocityTracker},
            mode_acts,
            palm::PalmBox,
            physics::{
                body::{BodySpec, Capsule, HandBody, player_system_group},
                drive::{
                    DriveTuning, Goal, PushTuning, contact_gain, lag_push, rig_jumped, smooth_push,
                },
                joint::Joint,
                lifecycle::{ActRequest, HANDS, RenderTuning},
                render::RenderOffset,
            },
            pulse,
        },
        roomscale::teleport,
        scheme,
        weapons::haptics::PulseSource,
    },
    headpose, hooks,
    vr::recenter_count,
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
    SHARED.lock().status.clone()
}

/// The pre-simulation callback's live state, read from atomics only, so any thread may read it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CallbackStatus {
    /// Whether the callback is registered with the physics system.
    pub registered: bool,
    /// Times the callback has fired.
    pub fires: u64,
    /// The game thread's id, as the registration saw it; zero before it.
    pub game_thread: u32,
    /// The id of the thread the callback last fired on; zero before it first fired.
    pub firing_thread: u32,
    /// Whether each hand's body has contact events on, indexed by [`Hand::index`].
    pub listening: [bool; 2],
}

pub fn callback_status() -> CallbackStatus {
    CallbackStatus {
        registered: callback::registered(),
        fires: callback::fires(),
        game_thread: callback::registering_thread(),
        firing_thread: callback::firing_thread(),
        listening: Hand::BOTH.map(contact::listening),
    }
}

/// Whether the hand bodies are on: their switch, under the physical hands' master switch.
pub(crate) fn enabled(config: &PhysicalHandsConfig) -> bool {
    config.enabled && config.bodies.enabled
}

/// An object's body a hand's grip holds through a joint.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HoldTarget {
    pub object: NonZeroUsize,
    pub body: u32,
    /// How far the hand's goal is moved so that the object's centre stays out of the player's
    /// body, world space.
    pub keep_out: Vec3,
    /// Whether both hands hold the object.
    pub shared: bool,
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

/// Where `hand`'s body was before the last step, game space, while it acts: the place its reach
/// is cast from, so that what the hand reaches is what its body touches.
pub(crate) fn body_pose(hand: Hand) -> Option<(Vec3, Quat)> {
    let shared = SHARED.lock();
    let now = Instant::now();
    (shared.fresh(now) && shared.stepped_lately(now))
        .then_some(shared.body_poses[hand.index()])
        .flatten()
}

/// Whether `hand`'s body presses into something that holds it back, as the last step found it.
pub(crate) fn pressing(hand: Hand) -> bool {
    let shared = SHARED.lock();
    let now = Instant::now();
    shared.fresh(now)
        && shared.stepped_lately(now)
        && shared.loads[hand.index()] >= MIN_PRESSURE_LOAD
}

/// Register the pre-simulation callback when the hand bodies are wanted, and the cleanup that
/// removes it. Called once per frame on the game thread, ahead of the game's update.
pub(crate) fn frame_tick() {
    if crate::is_shutting_down() {
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
    let now = Instant::now();
    let follow_through = Duration::from_secs_f32(config.bodies.hits.follow_through_s.max(0.0));
    for (until, struck) in shared
        .follow_through
        .iter_mut()
        .zip(delivered.follow_through)
    {
        if struck {
            *until = Some(now + follow_through);
        }
    }
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
    let now = Instant::now();
    let acting = enabled(&config).then(|| acting_palms(&config)).flatten();
    let park_now = acting.as_ref().map(|a| (now, a.park_now));
    let palms = acting
        .filter(|a| a.palms.iter().any(Option::is_some))
        .map(|a| (a.palms, a.world_scale));
    let body_rotation = headpose::xr::body_rotation();
    let recenters = recenter_count();
    let mut loads = [0.0; 2];
    {
        let mut shared = SHARED.lock();
        if teleport::epoch() != shared.epoch {
            shared.epoch = teleport::epoch();
            shared.trackers = TRACKERS_AT_REST;
        }
        // A snap turn or a recenter moves every palm at once without the hands moving: the
        // palms' motion starts afresh, and the next step carries the bodies with the rig.
        let jumped = rig_jumped(shared.rig, body_rotation, recenters);
        shared.rig = Some((body_rotation, recenters));
        if jumped {
            shared.trackers = TRACKERS_AT_REST;
        }
        let previous = shared.targets;
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
            if jumped && let Some(from) = previous[i] {
                shared.rig_moves[i] =
                    Some((from.goal(), targets[i].map_or(from.goal(), |t| t.goal())));
            }
        }
        shared.targets = targets;
        shared.park_now = park_now;
        shared.world_scale = world_scale;
        shared.published = palms.is_some().then_some(now);
        let stepped = shared.stepped_lately(now);
        let show = stepped && config.bodies.render_at_body;
        let scale = world_scale.max(1e-3);
        let offsets = shared.world_offsets;
        for hand in Hand::BOTH {
            let i = hand.index();
            shared.cockpit_offsets[i] = if show && targets[i].is_some() {
                offsets[i].to_cockpit(body_rotation, scale)
            } else {
                RenderOffset::NONE
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

/// Draw each driven hand where its body is, in the cockpit frame, turning it about the palm's
/// centre at `palm_offset` in the wrist frame. Called by
/// [`crate::controllers::physical_hands::apply`].
pub(crate) fn apply(cockpit: &mut [Option<CockpitHand>; 2], palm_offset: Vec3) {
    let offsets = {
        let shared = SHARED.lock();
        if !shared.fresh(Instant::now()) {
            return;
        }
        shared.cockpit_offsets
    };
    for (hand, offset) in cockpit.iter_mut().zip(offsets) {
        if let Some(hand) = hand.as_mut()
            && !offset.is_none()
        {
            offset.apply(hand, palm_offset);
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
    contact::step_tick();
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
        // SAFETY: forwarded from the caller; no hand body acts, so every grip lets go.
        unsafe { climb::step([None; 2]) };
        let destroyed = unsafe { hands.destroy_all(reason) };
        let mut shared = SHARED.lock();
        shared.status.destroyed += destroyed;
        shared.status.steps = hands.steps;
        shared.status.hands = [HandBodyStatus::default(); 2];
        shared.status.push = Vec2::ZERO;
        shared.push = Vec2::ZERO;
        shared.world_offsets = [RenderOffset::NONE; 2];
        shared.loads = [0.0; 2];
        shared.body_poses = [None; 2];
        return;
    }

    let (
        targets,
        published,
        world_scale,
        mut push,
        holds,
        rig_moves,
        park_now,
        follow_through,
        pressed,
    ) = {
        let mut shared = SHARED.lock();
        (
            shared.targets,
            shared.published,
            shared.world_scale,
            shared.push,
            shared.holds,
            std::mem::take(&mut shared.rig_moves),
            shared
                .park_now
                .filter(|(at, _)| now.saturating_duration_since(*at) <= STALE_AFTER)
                .map_or([false; 2], |(_, park)| park),
            shared
                .follow_through
                .map(|until| until.is_some_and(|at| now < at)),
            shared.loads,
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
    let render = RenderTuning {
        max_shift_m: b.break_distance_m * scale,
        deadband_rad: RENDER_TURN_DEADBAND_RAD,
        max_turn_rad: RENDER_MAX_TURN_RAD,
    };
    let push_tuning = PushTuning {
        stiffness_n_per_m: b.push_stiffness_n_per_m,
        max_force_n: b.force_n(),
        body_mass_kg: b.body_mass_kg,
        deadband_m: b.push_deadband_m * scale,
        min_horizontal: b.min_push_horizontal,
    };

    // The climb takes its grips where the acting bodies are now, and pins the gripping hands there.
    let poses = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let acting = fresh && group.is_some() && targets[i].is_some();
        let body = hands.bodies[i]
            .as_ref()
            .filter(|_| acting && hands.phase(hand) == BodyPhase::Acting)?;
        // SAFETY: forwarded from the caller.
        let motion = unsafe { body.motion() }.ok()?;
        Some((motion.position, motion.orientation))
    });
    // SAFETY: forwarded from the caller; the characters' proxy updates ran before this callback.
    let grips = unsafe { climb::step(poses) };
    // A gripping hand's body stays on the wall; every other acting body moves with the rig.
    for hand in Hand::BOTH {
        let i = hand.index();
        if let Some((from, to)) = rig_moves[i]
            && grips[i].is_none()
        {
            // SAFETY: forwarded from the caller.
            unsafe { hands.carry(hand, from, to) };
        }
    }
    let climbing = grips.iter().any(Option::is_some);

    let mut statuses = [HandBodyStatus::default(); 2];
    let mut offsets = [RenderOffset::NONE; 2];
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
                let palm = Goal {
                    position: target.position + target.linear * lead_s,
                    orientation: Quat::from_scaled_axis(target.angular * lead_s)
                        * target.orientation,
                };
                // A hand that holds an object through a joint is moved by the keep-out, and pulls
                // and twists with a holding hand's strength, the same budget as a carried hold.
                let holding = holds[i];
                let goal = grips[i].unwrap_or(Goal {
                    position: palm.position + holding.map_or(Vec3::ZERO, |h| h.keep_out),
                    ..palm
                });
                let held = holds[i].map_or(0, |h| h.object.get());
                let mut clear = |from: Vec3, to: Vec3| unsafe { path_clear(from, to, held) };
                // A hand held back rests on what holds it, and one that just struck hard follows
                // through rather than stopping dead.
                let mut linear_gain = contact_gain(tuning.linear_gain, b.contact_gain, pressed[i]);
                if follow_through[i] {
                    linear_gain *= b.hits.follow_through_gain.clamp(0.0, 1.0);
                }
                let hand_tuning = DriveTuning {
                    linear_gain,
                    ..holding.map_or(tuning, |h| holding_tuning(&tuning, &config, h.shared))
                };
                let request = ActRequest {
                    hand,
                    spec,
                    goal,
                    palm,
                    render: &render,
                    anchor: grips[i].map_or(target.position, |g| g.position),
                    clear: &mut clear,
                    break_distance_m: b.break_distance_m * scale,
                    tuning: &hand_tuning,
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
                        offsets[i] = acted.render;
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
                let park = group.is_none()
                    || park_now[i]
                    || now.saturating_duration_since(since) >= PARK_AFTER;
                let reason = if group.is_none() {
                    "the player has no collision group"
                } else if park_now[i] {
                    "the hand took a gun or carries what it holds"
                } else {
                    "the hand stopped acting"
                };
                // SAFETY: forwarded from the caller.
                joint_lost[i] |= unsafe { hands.unjoin(hand, "the hand stopped acting") };
                if park {
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
    push = if b.push && !climbing {
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
    shared.body_poses = poses;
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
/// tracking or a flicker of the mode does not churn the world. A hand that takes a gun or carries
/// what it grabbed leaves at once.
const PARK_AFTER: Duration = Duration::from_millis(250);

/// The longest a published pose is carried forward by its velocity, seconds.
const MAX_LEAD_S: f32 = 0.1;

/// How long the palms' velocities are smoothed over, seconds.
const VELOCITY_SMOOTHING_S: f32 = 0.03;

/// The turn between a body and its controller's palm that is not drawn, radians: a free hand keeps
/// the controller's own rotation.
const RENDER_TURN_DEADBAND_RAD: f32 = 0.05;

/// The largest turn a hand is drawn at against its controller, radians.
const RENDER_MAX_TURN_RAD: f32 = 1.5;

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

impl Target {
    fn goal(&self) -> Goal {
        Goal {
            position: self.position,
            orientation: self.orientation,
        }
    }
}

/// What the aim update and the step share.
struct Shared {
    /// Each hand's palm, as last published; `None` for a hand that does not act.
    targets: [Option<Target>; 2],
    /// Which hands stopped acting on purpose, and park at once, and when that was published.
    park_now: Option<(Instant, [bool; 2])>,
    /// When the targets were published; `None` while no hand acts.
    published: Option<Instant>,
    world_scale: f32,
    trackers: [(VelocityTracker, AngularTracker); 2],
    epoch: u64,
    /// The body's facing and the recenter count of the last palms' tick.
    rig: Option<(Quat, u32)>,
    /// Each palm's move with the rig since the last step, from and to, world space.
    rig_moves: [Option<(Goal, Goal)>; 2],
    /// Each body against its controller's palm, world space.
    world_offsets: [RenderOffset; 2],
    /// The same in the cockpit frame, for drawing the hands.
    cockpit_offsets: [RenderOffset; 2],
    /// The share of each hand's strength the last step used.
    loads: [f32; 2],
    /// Each acting body's pose before the last step, game space.
    body_poses: [Option<(Vec3, Quat)>; 2],
    /// The smoothed push, world `(x, z)` metres per second.
    push: Vec2,
    /// What each hand's grip holds through a joint, as last published.
    holds: [Option<HoldTarget>; 2],
    /// Which hands lost their joint since the aim update last took it.
    joint_lost: [bool; 2],
    /// Until when each hand follows through a hard blow with a softened drive.
    follow_through: [Option<Instant>; 2],
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
    park_now: None,
    published: None,
    world_scale: 1.0,
    trackers: TRACKERS_AT_REST,
    epoch: 0,
    rig: None,
    rig_moves: [None; 2],
    world_offsets: [RenderOffset::NONE; 2],
    cockpit_offsets: [RenderOffset::NONE; 2],
    loads: [0.0; 2],
    body_poses: [None; 2],
    push: Vec2::ZERO,
    holds: [None; 2],
    joint_lost: [false; 2],
    follow_through: [None; 2],
    stepped: None,
    status: HandBodiesStatus {
        registered: false,
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

/// The hands' palms when they act, with the VR world scale: on foot on the ground or falling, for
/// a tracked hand the controllers drive that holds nothing.
fn acting_palms(config: &PhysicalHandsConfig) -> Option<ActingPalms> {
    if !scheme::is_controllers() || !hooks::in_gameplay() {
        return None;
    }
    if !mode_acts(mode::current()) {
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
    Some(ActingPalms {
        palms,
        world_scale: active.world_scale,
        // A tracked hand that takes a gun or carries what it grabbed has stopped acting on
        // purpose: its body leaves at once rather than standing frozen in the world.
        park_now: Hand::BOTH.map(|hand| {
            let i = hand.index();
            active.hands[i].is_some() && (!free[i] || carrying[i])
        }),
    })
}

/// The palms' tick's view of the hands.
struct ActingPalms {
    /// Each acting hand's palm, world space.
    palms: [Option<PalmBox>; 2],
    world_scale: f32,
    /// Which hands stopped acting on purpose, and park at once.
    park_now: [bool; 2],
}

/// The drive of a hand that holds an object through a joint: a holding hand's pull and twist
/// ([`PickupConfig::hand_force_n`], [`PickupConfig::hand_torque_nm`]), no stronger than the hand's
/// own, and with both hands on the object, the twist at
/// [`PickupConfig::two_hand_angular_share`], so one hand lifts about what it carries and two hands
/// twice that, and the two grab points set the object's line.
///
/// [`PickupConfig::hand_force_n`]: crate::controllers::physical_hands::config::PickupConfig::hand_force_n
/// [`PickupConfig::hand_torque_nm`]: crate::controllers::physical_hands::config::PickupConfig::hand_torque_nm
/// [`PickupConfig::two_hand_angular_share`]: crate::controllers::physical_hands::config::PickupConfig::two_hand_angular_share
fn holding_tuning(tuning: &DriveTuning, config: &PhysicalHandsConfig, shared: bool) -> DriveTuning {
    let pickup = &config.pickup;
    let twist_share = if shared {
        pickup.two_hand_angular_share.clamp(0.0, 1.0)
    } else {
        1.0
    };
    DriveTuning {
        max_force_n: tuning.max_force_n.min(pickup.hand_force_n.max(0.0)),
        max_torque_nm: tuning
            .max_torque_nm
            .min(pickup.hand_torque_nm.max(0.0) * twist_share),
        ..*tuning
    }
}

/// Whether nothing solid lies between `from` and `to`, game space, for the aim's ray filter, which
/// never hits the player nor the hand bodies; `held` is an object a hand holds, also passed
/// through, or zero. A cast that fails counts as blocked.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
unsafe fn path_clear(from: Vec3, to: Vec3, held: usize) -> bool {
    let Some(ray) = Ray::new(from, to - from) else {
        return true;
    };
    let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
    let options = CastOptions {
        ignore: [player, held],
        see_through: false,
    };
    // SAFETY: forwarded from the caller; the ignored objects are the live player and a held
    // object the pickups checked this tick, or zero.
    matches!(
        unsafe { raycast::cast(&ray, from.distance(to), &options) },
        Ok(None)
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_joined_hand_holds_with_a_holding_hands_strength() {
        let config = PhysicalHandsConfig::new();
        let b = &config.bodies;
        let tuning = DriveTuning {
            mass_kg: b.mass_kg,
            max_force_n: b.force_n(),
            max_torque_nm: b.torque_nm(),
            linear_gain: b.linear_gain,
            angular_gain: b.angular_gain,
            max_speed_mps: b.max_speed_mps,
            max_angular_speed_rps: b.max_angular_speed_rps,
        };
        let one = holding_tuning(&tuning, &config, false);
        assert_eq!(one.max_force_n, config.pickup.hand_force_n);
        assert_eq!(one.max_torque_nm, config.pickup.hand_torque_nm);
        let two = holding_tuning(&tuning, &config, true);
        assert_eq!(two.max_force_n, config.pickup.hand_force_n);
        assert!(
            (two.max_torque_nm
                - config.pickup.hand_torque_nm * config.pickup.two_hand_angular_share)
                .abs()
                < 1e-5
        );
        // Never stronger than the hand itself.
        let weak = DriveTuning {
            max_force_n: 10.0,
            ..tuning
        };
        assert_eq!(holding_tuning(&weak, &config, false).max_force_n, 10.0);
    }
}
