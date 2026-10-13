//! The body rig: who moves the character's root, the chase that keeps it under the player's head,
//! the lean the chase leaves alone, the push-back at walls, the player's torso, the pelvis, and the
//! legs (`docs/mod/body/rig.md`).
//!
//! - [`root`]: the root's owner per simulation tick, the chase, its ledger, the cockpit shift, and
//!   the push-back;
//! - [`lean`]: how far the neck pivot may be out over the feet without a step;
//! - [`collision`]: the head-through-geometry probe;
//! - [`torso`]: the torso's heading and joints from the head and the hands;
//! - [`pelvis`]: the pelvis's place, its hip hinge, and its lean into the body's motion;
//! - [`legs`]: where the feet stand and step, from the root's measured motion;
//! - [`frames`] and [`skeleton`]: relating the cockpit to the character's model space, and the
//!   character's skeleton after the solves;
//! - [`config`]: the settings, under `controllers.body.rig`.
//!
//! The torso is estimated once per rendered VR frame ([`frame_tick`]) from the cockpit-frame head
//! and the raw controller grips. The character turns under the view to face it on the input tick
//! ([`follow_settings`], [`crate::headpose::follow`]), and the chest turns by what the follow's
//! deadband leaves ([`chest_turn`]).

pub mod collision;
pub mod config;
pub mod frames;
pub mod lean;
pub mod legs;
pub mod pelvis;
pub mod root;
pub mod skeleton;
pub mod torso;

#[cfg(test)]
pub(crate) mod harness;

use std::time::{Duration, Instant};

use glam::Quat;
use parking_lot::Mutex;

pub use crate::controllers::body::rig::{
    config::RigConfig,
    root::{feet, lean_allowance},
};
use crate::{
    config::Config,
    controllers::{
        body::{
            self,
            rig::torso::{TorsoEstimate, TorsoEstimator, TorsoInputs, wrap, yaw_forward, yaw_of},
        },
        frame::{self, Hand, Pose},
    },
    headpose::{self, xr::FollowSettings},
    hooks,
};

/// Advance the torso estimate from this frame's cockpit-frame head pose and the latest controller
/// grips. Called once per rendered VR frame on the game thread, from the pose composition.
pub fn frame_tick(head: Pose) {
    let config = Config::lock_query(|c| c.controllers.body.rig.torso);
    let now = Instant::now();
    let hands = frame::with_latest(|frame| {
        Hand::BOTH.map(|hand| {
            let h = frame.hand(hand);
            (frame.focused && h.active && h.grip.position_valid).then_some(h.grip.pose.position)
        })
    })
    .unwrap_or([None; 2]);
    let dims = body::player();
    let inputs = TorsoInputs {
        head_position: head.position,
        head_orientation: head.orientation,
        hands,
    };
    let mut state = TORSO.lock();
    let dt = state.last_tick.map_or(0.0, |last| {
        now.saturating_duration_since(last).as_secs_f32()
    });
    state.last_tick = Some(now);
    if dt > ESTIMATE_GAP.as_secs_f32() {
        state.estimator.reset();
    }
    let estimate = state.estimator.update(&inputs, dt, &dims, &config);
    state.estimate = estimate.map(|estimate| (estimate, now));
}

/// The latest torso estimate, when it is fresh.
pub fn estimate() -> Option<TorsoEstimate> {
    fresh_estimate(&TORSO.lock())
}

/// How the body follows the torso on this input tick: on with the setting, in gameplay, while the
/// headset drives the view.
pub fn follow_settings() -> FollowSettings {
    let config = Config::lock_query(|c| c.controllers.body.rig.torso);
    if !config.follow || headpose::source() != headpose::Source::Vr || !hooks::in_gameplay() {
        return FollowSettings::OFF;
    }
    FollowSettings {
        enabled: true,
        torso: estimate().map(|e| e.yaw),
        deadband_deg: config.follow_deadband_deg,
    }
}

/// Turn the estimate with the cockpit frame, for a recenter that turned the frame by `turn` (new
/// frame from old), so that the torso stays where it is in the room.
pub fn on_recenter(turn: Quat) {
    let mut state = TORSO.lock();
    state.estimator.rotate(turn);
    state.estimate = None;
}

/// The torso's heading in the character's model space, relative to his facing, for a character
/// whose world rotation is `character_rotation`. `None` without a fresh estimate or under the
/// flatscreen source.
pub fn torso_turn(character_rotation: Quat) -> Option<f32> {
    if headpose::source() != headpose::Source::Vr {
        return None;
    }
    let estimate = estimate()?;
    let view = headpose::xr::body_rotation();
    yaw_of(character_rotation.inverse() * (view * yaw_forward(estimate.yaw)))
}

/// The chest's turn toward the player's torso, radians in model space, at most the chest limit;
/// `None` leaves the body IK to its head-yaw share.
pub fn chest_turn(character_rotation: Quat) -> Option<f32> {
    let limit = Config::lock_query(|c| c.controllers.body.rig.torso.chest_limit_deg)
        .max(0.0)
        .to_radians();
    let turn = torso_turn(character_rotation).map(|turn| wrap(turn).clamp(-limit, limit));
    TORSO.lock().chest_turn = turn;
    turn
}

/// The torso's heading for the hands' estimated shoulders.
pub fn shoulder_heading() -> Option<Quat> {
    estimate().map(|e| e.rotation())
}

/// What the torso did on its latest frames, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TorsoStatus {
    /// The latest torso estimate, when fresh.
    pub estimate: Option<TorsoEstimate>,
    /// Whether the body follows the torso, and its target heading relative to the view, radians.
    pub following: bool,
    pub follow_target: f32,
    /// The body's heading relative to the view, radians.
    pub follow_offset: f32,
    /// The chest turn the body IK took on its latest tick, radians.
    pub chest_turn: Option<f32>,
}

pub fn torso_status() -> TorsoStatus {
    let (following, follow_target) = headpose::xr::follow_status();
    let follow_offset = headpose::xr::follow_offset(true);
    let state = TORSO.lock();
    TorsoStatus {
        estimate: fresh_estimate(&state),
        following,
        follow_target,
        follow_offset,
        chest_turn: state.chest_turn,
    }
}

/// How old the estimate may be and still stand for the player's torso. The frame loop publishes
/// one every rendered frame.
const ESTIMATE_FRESH: Duration = Duration::from_millis(250);

/// A gap between frames longer than this restarts the estimate rather than easing across it.
const ESTIMATE_GAP: Duration = Duration::from_millis(500);

struct TorsoState {
    estimator: TorsoEstimator,
    last_tick: Option<Instant>,
    estimate: Option<(TorsoEstimate, Instant)>,
    chest_turn: Option<f32>,
}

static TORSO: Mutex<TorsoState> = Mutex::new(TorsoState {
    estimator: TorsoEstimator::new(),
    last_tick: None,
    estimate: None,
    chest_turn: None,
});

fn fresh_estimate(state: &TorsoState) -> Option<TorsoEstimate> {
    state
        .estimate
        .filter(|(_, at)| at.elapsed() <= ESTIMATE_FRESH)
        .map(|(estimate, _)| estimate)
}
