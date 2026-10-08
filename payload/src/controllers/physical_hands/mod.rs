//! Physical hands: each empty hand's palm collides with the world. The design is in
//! `docs/mod/input/physical-hands.md`, the engine side in `docs/engine/gameplay/melee-and-damage.md`.
//!
//! - [`palm`]: the palm box at the tracked wrist, and the points its contacts are probed from;
//! - [`contact`]: whether each probe point has passed into a surface, and how deep;
//! - [`push`]: the body's push away from what the palms press into;
//! - [`shove`]: a palm's soft contact with a loose body, and how much the body gives;
//! - [`pickup`]: taking hold of loose objects and picking up weapons with the grip, with the pure
//!   grip in [`grasp`] and the solver that carries what the hands hold in [`hold`];
//! - [`rigid`]: a rigid body's response to impulses;
//! - [`touch`]: what the contacts do to the world: knocks, shoves, and pressure pulses;
//! - [`physics`]: the hand bodies, which replace the palms' probes for the hands they drive;
//! - [`characters`]: taking hold of characters, and pulling a seated one out through an open door;
//! - [`climb`]: climbing by gripping the static world with a hand its body drives;
//! - [`vault`]: pushing the body up with the hands pressed down on a surface;

//! - [`doors`]: opening vehicle doors by hand, a latched one gripped at its handle area;
//! - [`world`]: the Havok world, lent out for one call at a time;
//! - [`config`]: the settings.
//!
//! A hand its body drives ([`physics::drives`]) meets the world through its body. Otherwise, the
//! palms are probed rather than simulated: no body is added to the physics world. Each tick,
//! from the player aim control's post-simulation update ([`after_aim`], the simulation-phase update
//! where the engine casts its own aim ray), each probe point of each acting palm is cast from where
//! it last was in the open to where it is now, through the player aim's ray filter, which never
//! hits the player. Characters are passed through. What the casts find drives these:
//!
//! 1. A press into a solid surface pushes the body away from it ([`push`]), as movement added to
//!    the on-foot locomotion task's own ([`push_body`], from the roomscale simulation half), so
//!    that the view moves with the body as it does for stick movement.
//! 2. The rendered hand is held at the surface ([`apply`], from [`crate::controllers::hands`]).
//! 3. A press into a dynamic object shoves it every tick, within the hand's strength ([`shove`]);
//!    the share it does not give pushes the body away, as a wall does. A fast hit also knocks it.
//! 4. An object within reach of a palm can be taken with the grip ([`pickup`]), held at the point
//!    the hand touched ([`grasp`]) and carried within the hands' strength ([`hold`]); the rendered
//!    hand stays on the grab point, and what the hand cannot move pushes the body.
//!
//! A palm acts only on foot, on the ground or falling ([`mode_acts`]), for a tracked hand the
//! controller drives that holds nothing: not a gun, not the foregrip, not a grenade or a charge
//! being thrown.
//!
//! The knocks, the shoves, and the holds change engine state that is the main thread's, so they
//! are made only when the aim update runs on the thread the input tick runs on ([`input_tick`]
//! notes it); otherwise the palms stand down and the readout says why.

mod bodies;
pub mod characters;
pub mod climb;
pub mod config;
pub mod contact;
pub mod doors;
mod draw;
mod engine;
pub mod grasp;
pub mod hold;
pub mod palm;
pub mod physics;
pub mod pickup;
pub mod push;
pub mod rigid;
pub mod shove;
mod touch;
pub mod vault;
mod world;

use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

pub use bodies::BodyError;
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
        body,
        frame::{Hand, Pose},
        hands::{self, CockpitHand, pose as hand_pose, reach, tiers},
        mode::{self, AirborneVariant, Detection, Mode, OnFootVariant},
        motion,
        physical_hands::{
            contact::{HandContact, PointContact, PointProbe, Surface, SurfaceHit},
            grasp::{AngularTracker, PalmMotion, Reach, VelocityTracker},
            hold::{BodySpace, PlayerKeepOut},
            palm::{PROBE_POINTS, PalmBox},
            pickup::{HeldReport, JointInputs, PickupState, PickupStatus},
            shove::{Give, ShoveTuning},
            touch::{ShoveInput, Touch, pressure_pulses},
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
    /// Not on foot on the ground or falling.
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
            Idle::Mode => "not on foot on the ground or falling",
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
    /// What the hand could take with its grip.
    pub reach: Option<Reach>,
    /// What the hand holds.
    pub held: Option<HeldReport>,
    /// The palm's shove of the object it presses into.
    pub shove: Option<ShoveReport>,
    /// Whether the hand grips a latched door.
    pub latched: bool,
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

/// Hold each pressed hand at the surface it presses into, in the cockpit frame. Called by
/// [`crate::controllers::hands::active_hands`].
pub fn apply(cockpit: &mut [Option<CockpitHand>; 2]) {
    let (enabled, palm_offset) = Config::lock_query(|c| {
        let p = &c.controllers.physical_hands;
        (p.enabled && p.clamp_hand, p.palm_offset_m)
    });
    physics::apply(cockpit, palm_offset);
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
/// Give the dynamic body `body` the velocities `linear` and `angular`, world space, after checking
/// it still belongs to `owner`: a dropped weapon leaving the hand with the hand's motion.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn set_body_velocities(
    body: u32,
    owner: NonZeroUsize,
    linear: Vec3,
    angular: Vec3,
) -> Result<(), BodyError> {
    unsafe { bodies::drive(body, owner, rigid::Velocities { linear, angular }) }
}

/// The dynamic body `body`'s centre of mass and orientation, game space, after checking it still
/// belongs to `owner`: a live grenade or a dropped magazine near the hands, which a body still
/// owned by it proves alive.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn body_pose(body: u32, owner: NonZeroUsize) -> Result<(Vec3, Quat), BodyError> {
    let state = unsafe { bodies::read(body, owner) }?;
    Ok((state.pose.centre, state.pose.orientation))
}

pub(crate) unsafe fn push_body(character: &mut Character) {
    let enabled = Config::lock_query(|c| {
        c.controllers.physical_hands.enabled && c.controllers.physical_hands.push
    });
    let palms = if enabled {
        let state = STATE.lock();
        if state.fresh(Instant::now()) {
            state.status.push
        } else {
            Vec2::ZERO
        }
    } else {
        Vec2::ZERO
    };
    let push = palms + physics::push();
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
    physics::observe(dt);
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let idle = if !config.enabled || !scheme::is_controllers() || !hooks::in_gameplay() {
        Some(Idle::Disabled)
    } else if MAIN_THREAD.load(Ordering::Relaxed) != unsafe { GetCurrentThreadId() } {
        Some(Idle::OffThread)
    } else {
        (!mode_acts(mode::current())).then_some(Idle::Mode)
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
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    let space = (config.pickup.keep_out_radius_m > 0.0).then(|| {
        body::procedural::torso_keep_out(to_world).map_or_else(
            || {
                PlayerKeepOut::Cylinder(BodySpace {
                    base: to_world.w_axis.truncate(),
                    height_m: body::player().height * active.world_scale,
                    radius_m: config.pickup.keep_out_radius_m * active.world_scale,
                    forward: body_rotation * Vec3::NEG_Z,
                })
            },
            PlayerKeepOut::Torso,
        )
    });
    let shove_tuning = ShoveTuning::of(&config.shove);
    // SAFETY: forwarded from the caller.
    let melee_impulse = unsafe { engine::melee_impulse(owner) };
    // SAFETY: forwarded from the caller.
    let standing_on = unsafe { engine::ground_body(owner) };

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

    let reach_tuning = pickup::ReachTuning {
        world_scale: active.world_scale,
        pickup: &config.pickup,
        doors: &config.doors,
        characters: &config.characters,
    };
    let climb_on = climb::enabled(&config);
    let mut climb_palms = [None; 2];
    let mut climb_pressing = [false; 2];
    let mut climb_reaches = [None; 2];
    let mut touches = Vec::new();
    let mut hand_status = [HandStatus::default(); 2];
    let mut clamp = [Vec3::ZERO; 2];
    let mut palms = [None; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let velocity = state.hand_velocity(hand);
        let speed = velocity.length();
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
            // A holding hand neither probes nor pushes: its palm is around the object.
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            hand_status[i] = HandStatus {
                acting: true,
                palm: Some(palm),
                speed,
                ..HandStatus::default()
            };
            continue;
        }
        if physics::drives(hand) {
            // A hand its body drives does not probe, push, or shove: the body meets the world
            // itself. It still reaches for what its grip would take, from where its body is
            // rather than the controller, so that it reaches what the body touches: a body held
            // at a wall is drawn there while the controller is through it.
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            let body_palm =
                physics::body_pose(hand).map_or(palm, |(centre, orientation)| PalmBox {
                    centre,
                    orientation,
                    half_extents: palm.half_extents,
                });
            let reach = if pickup_on {
                // SAFETY: the simulation phase outside the physics step.
                unsafe {
                    pickup::reach(
                        &body_palm,
                        None,
                        standing_on,
                        &reach_tuning,
                        &mut |from, to| cast(from, to, &reach_options),
                    )
                }
            } else {
                None
            };
            if climb_on {
                // The palm in the rig: its tracking-space place turned and scaled as the world
                // palm is, but not carried by the head's world position, which the climb moves.
                let rig = PalmBox::from_wrist(cockpit.wrist, 1.0, &config).centre;
                climb_palms[i] = Some(body_rotation * rig * active.world_scale);
                climb_pressing[i] = physics::pressing(hand);
                // A loose object in reach wins over the world it rests on.
                climb_reaches[i] = if reach.is_none() {
                    // SAFETY: the simulation phase outside the physics step.
                    unsafe {
                        climb::reach(
                            &body_palm,
                            config.climb.reach_m * active.world_scale,
                            &config.climb,
                            &mut |from, to| cast(from, to, &probe_options),
                        )
                    }
                } else {
                    None
                };
            }
            hand_status[i] = HandStatus {
                palm: Some(palm),
                speed,
                reach,
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
        let pressed = contact.press.is_some();
        if let Some((hit, _)) = contact.press
            && !state.pressed[i]
        {
            touches.push(Touch {
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
                    contact.press.map(|(hit, _)| hit),
                    standing_on,
                    &reach_tuning,
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
            reach,
            held: None,
            shove,
            latched: false,
        };
    }

    let mut joined = [None; 2];
    let mut pins = [None; 2];
    if pickup_on {
        let inputs = pickup::TickInputs {
            palms,
            space,
            joints: joint_inputs(&config),
            dt,
            unlatch_pull_m: config.doors.unlatch_pull_m * active.world_scale,
            world_scale: active.world_scale,
            characters: config.characters,
            reaches: hand_status.map(|h| h.reach),
        };
        // SAFETY: the simulation phase outside the physics step.
        let outcome = unsafe { pickup::tick(&mut state.pickup, inputs, &config.pickup) };
        joined = outcome.joints;
        pins = outcome.pins;
        state.record_pickup(outcome, &mut hand_status, &config);
        for hand in Hand::BOTH {
            let i = hand.index();
            hand_status[i].latched = state.pickup.latched(hand);
            // A hand on a latched door stays at its grab point; a driven hand's body is held
            // there, and a probed one is drawn there.
            if let (Some(pin), Some(palm)) = (pins[i], palms[i])
                && !physics::drives(hand)
            {
                let offset =
                    (pin - palm.pose.position).clamp_length_max(config.pickup.break_distance_m);
                clamp[i] = (body_rotation.inverse() * offset) / active.world_scale.max(1e-3);
            }
        }
        for hand in Hand::BOTH {
            let i = hand.index();
            let Some(held) = hand_status[i].held.filter(|h| !h.joined) else {
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
    for touch in touches {
        // SAFETY: forwarded from the caller; the touches' objects were found by this tick's casts.
        unsafe { state.touch(touch, melee_impulse, &config) };
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
    physics::publish_holds(joined);
    physics::publish_pins(pins);
    let snapshot = state.status.hands;
    drop(state);
    climb::observe(
        climb::Observed {
            palms: climb_palms,
            reaches: climb_reaches,
            pressing: climb_pressing,
            body_rotation,
        },
        &config,
    );
    draw::submit(&snapshot, &config);
}

/// Whether the hands act in the mode `detection`: on foot on the ground, or falling, so that a
/// hand flung or jumped at a ledge can catch it; not swimming, in a vehicle, in the wingsuit or
/// under the parachute, nor while the grapple carries the player.
pub(crate) fn mode_acts(detection: Detection) -> bool {
    matches!(
        detection.mode,
        Mode::OnFoot(OnFootVariant::Ground) | Mode::Airborne(AirborneVariant::Falling)
    ) && detection.traversal.is_none()
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
        for hand in Hand::BOTH {
            if outcome.unlatched[hand.index()] {
                status.unlatches += 1;
                pulse(
                    hand,
                    config.doors.unlatch_pulse,
                    1.0,
                    config,
                    PulseSource::Pickup,
                );
            }
        }
        status.door_grips += outcome.door_grips;
        status.character_grips += outcome.character_grips;
        status.hijacks += outcome.hijacks;
        status.yanks += outcome.yanks;
        status.regrips += outcome.regrips;
        if outcome.door.is_some() {
            status.last_door = outcome.door;
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
            reach: None,
            held: None,
            shove: None,
            latched: false,
        }; 2],
        push: Vec2::ZERO,
        casts: 0,
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
            door_grips: 0,
            unlatches: 0,
            last_door: None,
            character_grips: 0,
            hijacks: 0,
            yanks: 0,
            regrips: 0,
            last_error: None,
        },
    },
    probes: [[PointProbe::new(); PROBE_POINTS]; 2],
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
    physics::publish_holds([None; 2]);
    physics::publish_pins([None; 2]);

    state.last_sample = [None; 2];
    state.clamp = [Vec3::ZERO; 2];
    state.at = None;
    state.status.idle = Some(idle);
    state.status.hands = [HandStatus::default(); 2];
    state.status.push = Vec2::ZERO;
    state.status.casts = 0;
    drop(state);
    climb::stand_down();
    draw::clear();
}

/// What the hand bodies tell the pickups about the joints: which hands take what they grab through
/// a joint, which lost theirs, and each body's load.
fn joint_inputs(config: &PhysicalHandsConfig) -> JointInputs {
    let lost = physics::take_joint_losses();
    let joining = physics::enabled(config) && config.bodies.joint_grabs;
    let bodies = physics::status();
    JointInputs {
        join: Hand::BOTH.map(|hand| joining && physics::drives(hand)),
        lost,
        loads: bodies.hands.map(|h| (h.force_share, h.torque_share)),
    }
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
