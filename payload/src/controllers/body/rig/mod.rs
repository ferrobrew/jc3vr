//! The body rig's simulation fit, root ownership, collision response, and pose consumers.
//!
//! Tracking is copied at frame composition. The local solve gate commits one bounded body fit.
//! The procedural skeleton, torso follow, and the next movement task consume that result.

pub mod collision;
pub mod config;
pub(crate) mod estimate;
pub mod frames;
pub mod lean;
pub mod legs;
pub(crate) mod live_observation;
pub(crate) mod observation;
pub(crate) mod origin_provenance;
pub mod pelvis;
pub mod root;
pub(crate) mod runtime;
pub mod skeleton;
pub mod torso;

#[cfg(test)]
pub(crate) mod harness;

use glam::Quat;
use parking_lot::Mutex;

pub use crate::controllers::body::rig::{config::RigConfig, root::lean_allowance};
use crate::{
    config::Config,
    controllers::{
        body::rig::torso::{TorsoEstimate, wrap, yaw_forward, yaw_of},
        frame::{self, Pose},
    },
    headpose::{self, xr::FollowSettings},
    hooks,
};

/// Copy this frame's cockpit head and grips for the next simulation-time fit.
pub fn frame_tick(head: Pose) {
    let frame = frame::latest();
    runtime::frame_tick(head, frame.as_ref());
}

/// The latest committed torso estimate, when fresh and geometrically usable.
pub fn estimate() -> Option<TorsoEstimate> {
    runtime::torso()
}

/// Follow only a committed fit. A missing fit must not turn into a raw head-yaw request.
pub fn follow_settings() -> FollowSettings {
    let config = Config::lock_query(|c| c.controllers.body.rig.torso);
    if !config.follow || headpose::source() != headpose::Source::Vr || !hooks::in_gameplay() {
        return FollowSettings::OFF;
    }
    let Some(yaw) = runtime::follow_yaw() else {
        return FollowSettings::OFF;
    };
    FollowSettings {
        enabled: true,
        torso: Some(yaw),
        deadband_deg: config.follow_deadband_deg,
    }
}

/// Recentered tracking starts a new fit rather than applying old-frame inferred targets.
pub fn on_recenter(_turn: Quat) {
    runtime::invalidate();
}

/// The committed chest heading relative to the character's facing.
pub fn torso_turn(character_rotation: Quat) -> Option<f32> {
    if headpose::source() != headpose::Source::Vr {
        return None;
    }
    let estimate = estimate()?;
    let view = headpose::xr::body_rotation();
    yaw_of(character_rotation.inverse() * (view * yaw_forward(estimate.yaw)))
}

/// The committed chest turn, bounded for the remaining pre-solve IK consumers.
pub fn chest_turn(character_rotation: Quat) -> Option<f32> {
    let limit = Config::lock_query(|c| c.controllers.body.rig.torso.chest_limit_deg)
        .max(0.0)
        .to_radians();
    let turn = torso_turn(character_rotation).map(|turn| wrap(turn).clamp(-limit, limit));
    *CHEST_TURN.lock() = turn;
    turn
}

/// The committed torso heading for the hands' estimated shoulders.
pub fn shoulder_heading() -> Option<Quat> {
    estimate().map(|e| e.rotation())
}

/// What the torso did on its latest frames, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TorsoStatus {
    pub estimate: Option<TorsoEstimate>,
    pub following: bool,
    pub follow_target: f32,
    pub follow_offset: f32,
    pub chest_turn: Option<f32>,
}

pub fn torso_status() -> TorsoStatus {
    let (following, follow_target) = headpose::xr::follow_status();
    TorsoStatus {
        estimate: estimate(),
        following,
        follow_target,
        follow_offset: headpose::xr::follow_offset(true),
        chest_turn: *CHEST_TURN.lock(),
    }
}

static CHEST_TURN: Mutex<Option<f32>> = Mutex::new(None);
