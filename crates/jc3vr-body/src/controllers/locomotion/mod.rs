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
//! - [`view_follow`] has that held view follow Rico's animated head through a ragdoll, a get-up, a
//!   death, and a landing that carries the head well away from the root.
//!
//! The legs still play the game's clips; only where the body goes, and where the view sits, no
//! longer come from them.
//!
//! In water, [`strokes`] swims the body by the arms alongside the stick.

pub mod accel;
pub mod acts;
pub mod anchor;
pub mod axis_output;
pub mod config;
pub mod speeds;
pub mod strokes;
pub mod tuning;
pub mod view;
pub mod view_follow;

use std::time::Duration;

use glam::{Vec2, Vec3};
use jc3gi::{
    blackboard::ObjectBlackboard,
    character::character::Character,
    input::input_action_map::{Action, LocalPlayerActionMap},
    types::math::Vector3,
};
use jc3vr_core::published::Stamp;
use jc3vr_view::headpose;
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::{procedural::state::animation_moves_body, rig::grounded},
        body_frame::{self, LOCOMOTION_FRAME},
        locomotion::{config::LOCOMOTION_CONFIG, speeds::SpeedLog, tuning::LOCOMOTION},
    },
    tuning::FRESHNESS,
};

/// What the stick locomotion did on its last tick: the locomotion's section of the body's handoff
/// data ([`crate::controllers::body_frame`]).
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LocomotionFrame {
    /// When the movement task last ran for the local player.
    pub at: Option<Stamp>,
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

/// The locomotion's readout, which is the section itself.
pub type LocomotionStatus = LocomotionFrame;

impl LocomotionFrame {
    /// Whether the movement task ran within the last few ticks before `now`: Rico is on foot on
    /// the ground.
    pub fn fresh(&self, now: Stamp) -> bool {
        FRESHNESS
            .get()
            .locomotion
            .movement_task_ms
            .window()
            .holds(self.at, now)
    }

    /// Whether Rico is standing still: no stick, and no motion from the task or from the stick.
    pub fn standing_still(&self) -> bool {
        let still = &LOCOMOTION.get().stick;
        self.stick.length() < still.still_stick
            && self.task_velocity.length() < still.still_speed_mps
            && self
                .velocity
                .is_none_or(|v| v.length() < still.still_speed_mps)
    }
}

/// The last tick's status, for the debug UI. Code acting on the locomotion reads
/// [`body_frame::locomotion`].
pub fn status() -> LocomotionStatus {
    body_frame::locomotion()
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
    let config = LOCOMOTION_CONFIG.lock().stick;
    let tuning = &LOCOMOTION.get().stick;
    let now = Stamp::now();
    // A committed step of the body rig walks the game's animation through the move stick; its
    // speed is the rig's drive, so its share of the stick moves nothing here.
    let stick = player_stick(unsafe { stick_vector() }, grounded::step_stick());
    let game_speed = unsafe { game_target_speed(character) };
    let root_speed = unsafe { root_speed(character) };
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    let [task_x, task_y, task_z] = wanted.data;
    let task_velocity = Vec2::new(task_x, task_z);

    let mut state = STATE.lock();
    // The movement task is the section's only writer, and it runs under the state lock.
    let previous = body_frame::locomotion();
    let resumed = !previous.fresh(now);
    state
        .speeds
        .observe(now, stick.length(), game_speed, root_speed);
    let mut status = LocomotionFrame {
        at: Some(now),
        stick,
        game_speed,
        root_speed,
        task_velocity,
        ..previous
    };
    // While the animation carries the body (an authored one-off, a hit reaction), its root motion
    // is the motion, and the task's velocity passes through; the integrator picks up from the
    // body's real velocity when the stick drives it again.
    if !config.enabled || unsafe { animation_moves_body(character, now) } {
        status.target = Vec2::ZERO;
        status.velocity = None;
        LOCOMOTION_FRAME.publish_at(status, now);
        return;
    }

    // A gap means the task stopped (a jump, a fall, a vehicle) and the body kept whatever velocity
    // the other states gave it, so the integrator starts from the body's real velocity: a landing
    // carries its speed instead of stopping dead.
    let body = unsafe { proxy_velocity(character) };
    let start = match previous.velocity {
        Some(velocity) if !resumed => velocity,
        _ => body.unwrap_or(task_velocity),
    };
    let forward = unsafe { view_forward(character) };
    let target = accel::target(stick, forward, game_speed);
    let start = accel::coast_limited(start, target, body);
    let velocity = accel::step(start, target, dt, tuning.rates);
    let world = Vector3 {
        data: [velocity.x, task_y, velocity.y],
    };
    unsafe { character.SetWantedWorldVelocity(&world) };
    status.target = target;
    status.velocity = Some(velocity);

    if state
        .log_at
        .is_none_or(|at| now.saturating_duration_since(at) >= LOG_INTERVAL)
        && (velocity != Vec2::ZERO || task_velocity.length() > tuning.still_speed_mps)
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
    LOCOMOTION_FRAME.publish_at(status, now);
}

/// How often the commanded velocity is logged while moving.
const LOG_INTERVAL: Duration = Duration::from_secs(1);

struct State {
    speeds: SpeedLog,
    log_at: Option<Stamp>,
}

static STATE: Mutex<State> = Mutex::new(State {
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

/// The player's own move stick: what the game reads less the walk the body rig added for a
/// committed step.
fn player_stick(read: Vec2, step: Vec2) -> Vec2 {
    let stick = read - step;
    if stick.length() < LOCOMOTION.get().stick.still_stick || !stick.is_finite() {
        Vec2::ZERO
    } else {
        stick.clamp_length_max(1.0)
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
    let min_forward = LOCOMOTION.get().stick.min_forward;
    if flat.length_squared() > min_forward * min_forward && flat.is_finite() {
        return flat.normalize();
    }
    let body = -glam::Mat4::from(character.m_WorldMatrixT1)
        .z_axis
        .truncate();
    Vec2::new(body.x, body.z)
        .try_normalize()
        .unwrap_or(Vec2::NEG_Y)
}
