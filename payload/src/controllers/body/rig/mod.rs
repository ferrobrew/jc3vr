//! The body rig: the grounded rig that keeps Rico under the player's head, root ownership, the
//! collision response, and the readouts of the player's torso (`docs/mod/body/rig.md`).
//!
//! - [`grounded`]: the pure core (`body-rig`) advanced once per rendered frame: the pelvis solver,
//!   the step commit, the yaw follow, and the standing height;
//! - [`root`]: who moves the root, the step drive through the game's movement, the cockpit shift,
//!   and the push-back at walls;
//! - [`collision`]: the head-through-geometry probe;
//! - [`skeleton`] and [`frames`]: Rico's solved skeleton for the body's volumes, and its divergence
//!   from the player's torso;
//! - [`visual`]: the grounded rig's in-world diagnostics.

pub mod collision;
pub mod config;
pub mod frames;
pub mod grounded;
pub(crate) mod live_observation;
pub(crate) mod origin_provenance;
pub mod root;
pub mod skeleton;
pub mod torso;
pub(crate) mod visual;

use body_rig::angle::wrap;
use glam::Quat;
use parking_lot::Mutex;

pub use crate::controllers::body::rig::config::RigConfig;
use crate::{
    config::Config,
    controllers::body::rig::torso::{CHEST_LIMIT_DEG, TorsoEstimate, yaw_forward, yaw_of},
    headpose::{self, xr::FollowSettings},
    hooks,
};

/// The player's torso as the grounded rig places it, while it is fresh.
pub fn estimate() -> Option<TorsoEstimate> {
    grounded::torso()
}

/// How Rico's yaw follows the player on this input tick: toward the grounded rig's body heading
/// (the legs'), whose own rules and slew already decide when the body turns, so the follow adds no
/// deadband. With no fresh rig (off the ground, or before the first frame) the body does not
/// follow.
pub fn follow_settings() -> FollowSettings {
    let follow = Config::lock_query(|c| c.controllers.body.rig.follow.enabled);
    if !follow || headpose::source() != headpose::Source::Vr || !hooks::in_gameplay() {
        return FollowSettings::OFF;
    }
    let Some(yaw) = grounded::body_yaw() else {
        return FollowSettings::OFF;
    };
    FollowSettings {
        enabled: true,
        torso: Some(yaw),
        deadband_deg: 0.0,
    }
}

/// The player's torso heading relative to the character's facing.
pub fn torso_turn(character_rotation: Quat) -> Option<f32> {
    if headpose::source() != headpose::Source::Vr {
        return None;
    }
    let estimate = estimate()?;
    let view = headpose::xr::body_rotation();
    yaw_of(character_rotation.inverse() * (view * yaw_forward(estimate.yaw)))
}

/// The torso's turn, bounded for the remaining pre-solve IK consumers.
pub fn chest_turn(character_rotation: Quat) -> Option<f32> {
    let limit = CHEST_LIMIT_DEG.to_radians();
    let turn = torso_turn(character_rotation).map(|turn| wrap(turn).clamp(-limit, limit));
    *CHEST_TURN.lock() = turn;
    turn
}

/// The torso's heading for the hands' estimated shoulders.
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
