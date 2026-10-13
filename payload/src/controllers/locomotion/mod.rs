//! Stick-driven on-foot locomotion, and the view that does not move with the animation. See
//! `docs/mod/body/locomotion.md`.
//!
//! The game moves Rico on foot by his clips' root velocity: the movement task scales its direction
//! by the animation's raw root speed, so the starts, the stops, the turning starts, and the run's
//! per-step surges are all in the body's motion, and through the head bone in the view. This module
//! takes both away from the animation:
//!
//! - [`after_task`] replaces the horizontal velocity the movement task wrote with the stick's,
//!   reached at a fixed acceleration ([`accel`]). It runs from the post-call detour on the on-foot
//!   locomotion task ([`crate::controllers::body::rig::root`]), before the palms' push and the
//!   chase add their own.
//! - [`acts`] keeps the turn-in-place and turning start and stop acts off the body.
//! - [`anchor`] anchors the on-foot view to Rico's root and a neutral standing head instead of his
//!   animated head ([`view`]).
//!
//! The legs still play the game's clips; only where the body goes, and where the view sits, no
//! longer come from them.

pub mod accel;
pub mod acts;
pub mod anchor;
pub mod config;
pub mod speeds;
pub mod view;

use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use jc3gi::{
    blackboard::ObjectBlackboard,
    character::character::Character,
    input::input_action_map::{Action, LocalPlayerActionMap},
    types::math::Vector3,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::procedural::{sat_on_ground, state::animation_moves_body},
        locomotion::{accel::Rates, speeds::SpeedLog},
    },
    headpose,
};

/// What the stick locomotion did on its last tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LocomotionStatus {
    /// When the movement task last ran for the local player.
    pub at: Option<Instant>,
    /// The move stick, `(right, forward)`.
    pub stick: Vec2,
    /// The game's target speed for the stick, metres per second, when it has one.
    pub game_speed: Option<f32>,
    /// The horizontal speed of the animation's root motion, metres per second.
    pub root_speed: f32,
    /// The horizontal velocity the movement task wrote, world `(x, z)`.
    pub task_velocity: Vec2,
    /// The stick's target velocity, world `(x, z)`.
    pub target: Vec2,
    /// The velocity written in the task's place, world `(x, z)`, while the stick drives the body.
    pub velocity: Option<Vec2>,
}

impl LocomotionStatus {
    /// Whether the movement task ran within the last few ticks: Rico is on foot on the ground.
    pub fn fresh(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }

    /// Whether Rico is standing still: no stick, and no motion from the task or from the stick.
    pub fn standing_still(&self) -> bool {
        self.stick.length() < STILL_STICK
            && self.task_velocity.length() < STILL_SPEED_MPS
            && self.velocity.is_none_or(|v| v.length() < STILL_SPEED_MPS)
    }
}

/// The last tick's status.
pub fn status() -> LocomotionStatus {
    STATE.lock().status
}

/// Replace the movement task's horizontal velocity with the stick's, for the local player after the
/// on-foot locomotion task wrote it. The caller runs it only on a tick where the task wrote the
/// velocity and the headset drives the view.
///
/// # Safety
///
/// `character` must be the live local character, on the thread running its state machines' serial
/// pass.
pub(crate) unsafe fn after_task(character: &mut Character, dt: f32) {
    let config = Config::lock_query(|c| c.controllers.locomotion.stick);
    let now = Instant::now();
    let stick = unsafe { stick_vector() };
    let game_speed = unsafe { game_target_speed(character) };
    let root_speed = unsafe { root_speed(character) };
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    let [task_x, task_y, task_z] = wanted.data;
    let task_velocity = Vec2::new(task_x, task_z);

    let mut state = STATE.lock();
    let resumed = !state.status.fresh(now);
    state
        .speeds
        .observe(now, stick.length(), game_speed, root_speed);
    state.status.at = Some(now);
    state.status.stick = stick;
    state.status.game_speed = game_speed;
    state.status.root_speed = root_speed;
    state.status.task_velocity = task_velocity;
    // While the animation carries the body (an authored one-off, a hit reaction), its root motion
    // is the motion, and the task's velocity passes through; the integrator picks up from the
    // body's real velocity when the stick drives it again.
    if !config.enabled || unsafe { animation_moves_body(character, now) } {
        state.status.target = Vec2::ZERO;
        state.status.velocity = None;
        return;
    }

    // A gap means the task stopped (a jump, a fall, a vehicle) and the body kept whatever velocity
    // the other states gave it, so the integrator starts from the body's real velocity: a landing
    // carries its speed instead of stopping dead.
    let start = match state.status.velocity {
        Some(velocity) if !resumed => velocity,
        _ => unsafe { proxy_velocity(character) }.unwrap_or(task_velocity),
    };
    let forward = unsafe { view_forward(character) };
    // Sat on the ground, the stick stands the body up rather than walking it: the velocity stays
    // at rest until the procedural body is up.
    let target = if sat_on_ground() {
        Vec2::ZERO
    } else {
        accel::target(stick, forward, game_speed, &config)
    };
    let velocity = accel::step(start, target, dt, Rates::of(&config));
    let world = Vector3 {
        data: [velocity.x, task_y, velocity.y],
    };
    unsafe { character.SetWantedWorldVelocity(&world) };
    state.status.target = target;
    state.status.velocity = Some(velocity);

    if state
        .log_at
        .is_none_or(|at| now.saturating_duration_since(at) >= LOG_INTERVAL)
        && (velocity != Vec2::ZERO || task_velocity.length() > STILL_SPEED_MPS)
    {
        state.log_at = Some(now);
        let proxy = unsafe { proxy_velocity(character) };
        tracing::debug!(
            target: "controllers",
            stick = ?stick,
            game_speed = ?game_speed,
            root_speed,
            task = ?task_velocity,
            target = ?target,
            commanded = ?velocity,
            proxy = ?proxy,
            "locomotion: stick velocity",
        );
    }
}

/// Ticks of the movement task further apart than this are separate runs of it.
const STALE_AFTER: Duration = Duration::from_millis(200);

/// Below these, Rico is standing still.
const STILL_STICK: f32 = 0.05;
const STILL_SPEED_MPS: f32 = 0.05;

/// How often the commanded velocity is logged while moving.
const LOG_INTERVAL: Duration = Duration::from_secs(1);

struct State {
    status: LocomotionStatus,
    speeds: SpeedLog,
    log_at: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: LocomotionStatus {
        at: None,
        stick: Vec2::ZERO,
        game_speed: None,
        root_speed: 0.0,
        task_velocity: Vec2::ZERO,
        target: Vec2::ZERO,
        velocity: None,
    },
    speeds: SpeedLog::new(),
    log_at: None,
});

/// The move stick as the game reads it, `(right, forward)`, from the player action map's move
/// half-axes, at most one long.
unsafe fn stick_vector() -> Vec2 {
    let Some(map) = (unsafe { LocalPlayerActionMap::get() }) else {
        return Vec2::ZERO;
    };
    let mut value =
        |action| unsafe { map.GetActionEffector(action).as_ref() }.map_or(0.0, |e| e.m_Value);
    let stick = Vec2::new(
        value(Action::MOVE_RIGHT) - value(Action::MOVE_LEFT),
        value(Action::MOVE_FORWARD) - value(Action::MOVE_BACKWARD),
    );
    if stick.is_finite() {
        stick.clamp_length_max(1.0)
    } else {
        Vec2::ZERO
    }
}

/// The game's target speed for the stick (`ObjectBlackboard::SPEED_ID`), which the input tasks
/// write for the movement task.
unsafe fn game_target_speed(character: &mut Character) -> Option<f32> {
    let mut speed = 0.0f32;
    unsafe {
        character
            .m_Blackboard
            .GetFloat(ObjectBlackboard::SPEED_ID, &mut speed)
    }
    .then_some(speed)
    .filter(|s| s.is_finite())
}

/// The horizontal speed of the animation's root motion
/// (`AnimationController::GetRawRootVelocity`), which the movement task takes as Rico's speed.
unsafe fn root_speed(character: &Character) -> f32 {
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() })
    else {
        return 0.0;
    };
    let Some(velocity) = (unsafe { controller.GetRawRootVelocity().as_ref() }) else {
        return 0.0;
    };
    let [x, _, z] = velocity.data;
    let speed = Vec2::new(x, z).length();
    if speed.is_finite() { speed } else { 0.0 }
}

/// The character proxy's horizontal velocity, world `(x, z)`.
unsafe fn proxy_velocity(character: &Character) -> Option<Vec2> {
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
    let mut out = Vector3 { data: [0.0; 3] };
    unsafe { proxy.GetVelocity(&mut out) };
    let [x, _, z] = out.data;
    Some(Vec2::new(x, z)).filter(|v| v.is_finite())
}

/// The stick's frame: the view's forward on the ground plane. The bindings already turn the stick
/// into the off hand's frame when the player moves that way, so the view is the frame either way.
/// Rico's own forward stands in while the head looks straight up or down.
unsafe fn view_forward(character: &Character) -> Vec2 {
    let head = headpose::query().orientation * Vec3::NEG_Z;
    let flat = Vec2::new(head.x, head.z);
    if flat.length_squared() > MIN_FORWARD * MIN_FORWARD && flat.is_finite() {
        return flat.normalize();
    }
    let body = -glam::Mat4::from(character.m_WorldMatrixT1)
        .z_axis
        .truncate();
    Vec2::new(body.x, body.z)
        .try_normalize()
        .unwrap_or(Vec2::NEG_Y)
}

/// The shortest horizontal head forward that still has a heading.
const MIN_FORWARD: f32 = 0.1;
