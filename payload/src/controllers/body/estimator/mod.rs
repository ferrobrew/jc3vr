//! The learned body estimator in the game: EgoPoser (`body-estimator`) run on the game's own
//! Direct3D 11 device, and its latest estimate published for any part of the mod to read
//! (`docs/mod/body/body-estimator.md`).
//!
//! Producing the estimate is separate from consuming it. The producer runs whenever the controller
//! scheme is in effect and the head is tracked, in every mode, and publishes one estimate through
//! [`latest`]; the grounded rig ([`for_rig`]) and the IK body's FK pelvis
//! ([`crate::controllers::body::ik_body::fk`]) consume it.
//!
//! - [`producer`]: on the game thread, once per rendered frame from the pose composition, the
//!   tracked head and grips go into a live 60 Hz window in a room frame no recenter turns, and at
//!   the submit rate the window is posted for the render thread.
//! - [`executor`]: on the render thread, once per frame under the engine's context lock, the GPU
//!   executor takes the posted window, records the pass, and collects any finished estimate without
//!   waiting, which it publishes.
//!
//! The two threads meet only in two latest-wins mailboxes, each a mutex around one value: the
//! window to run, and the estimate read back. A window the render thread has not taken by the next
//! submission is replaced, and so is an estimate nobody has read.

pub(crate) mod config;
pub(crate) mod executor;
pub mod producer;
pub(crate) mod visual;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use body_estimator::{
    EgoPoserOutput,
    smpl::{AMASS_FROM_ROOM, JOINT_COUNT, PELVIS, SPINE3, room_yaw},
};
use body_rig::{BodyEstimate, EstimateTuning, angle::wrap};
use glam::{DMat3, DQuat, DVec3, Quat};
use parking_lot::Mutex;

pub use crate::controllers::body::estimator::{
    config::EstimatorConfig,
    executor::{render_tick, status as executor_status},
    producer::{Observation, observe, on_recenter},
};
use crate::controllers::roomscale::posture::EffectivePosture;

/// The estimator's latest reading of the body, in the cockpit frame as it stands now.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Estimate {
    /// The pelvis's heading, in `body-rig`'s yaw convention.
    pub pelvis_yaw: f32,
    /// The upper chest's (SMPL spine3's) heading.
    pub chest_yaw: f32,
    /// Every SMPL body joint's global rotation, its joint frame (+X the body's left, +Y up, +Z
    /// forward at rest) in the cockpit frame's axes, in SMPL joint order.
    pub joints: [Quat; JOINT_COUNT],
    /// How old the window's last pose is.
    pub age: Duration,
    /// The posture whose head-height remap the window's poses were in: seated, the head was lifted
    /// to the standing height as the rig lifts it.
    pub posture: EffectivePosture,
    /// How long the GPU took from the window's submission to its readback, frames included.
    pub readback: Duration,
}

impl Estimate {
    /// Whether the estimate is too old to stand for the body.
    pub fn stale(&self) -> bool {
        self.age > STALE_AFTER
    }
}

/// An estimate older than this is stale for any consumer.
pub const STALE_AFTER: Duration = Duration::from_millis(500);

/// The latest estimate, whatever its age, while the estimator runs.
pub fn latest() -> Option<Estimate> {
    let reading = (*LATEST.lock())?;
    let room_from_cockpit = producer::room_from_cockpit();
    Some(reading.estimate(room_from_cockpit, Instant::now()))
}

/// The rig's view of `estimate`: its headings with a confidence that falls with its age under
/// `tuning`, or `None` once it counts for nothing.
pub fn for_rig(estimate: Option<&Estimate>, tuning: &EstimateTuning) -> Option<BodyEstimate> {
    let estimate = estimate?;
    let confidence = tuning.confidence(estimate.age.as_secs_f32());
    (confidence > 0.0).then_some(BodyEstimate {
        pelvis_yaw: estimate.pelvis_yaw,
        chest_yaw: estimate.chest_yaw,
        confidence,
    })
}

/// One estimate as read back, in the room frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Reading {
    pub output: EgoPoserOutput,
    /// When the window's last pose was taken.
    pub window_end: Instant,
    pub posture: EffectivePosture,
    pub submitted_at: Instant,
    pub read_at: Instant,
}

impl Reading {
    /// The reading in the cockpit frame `room_from_cockpit` maps into the room frame, at `now`.
    pub(crate) fn estimate(&self, room_from_cockpit: DQuat, now: Instant) -> Estimate {
        let global = self.output.global_rotations();
        let cockpit_from_room = DMat3::from_quat(room_from_cockpit.inverse());
        let turn = room_from_cockpit_yaw(room_from_cockpit);
        let yaw = |joint: usize| wrap((room_yaw(global[joint]) - turn) as f32);
        let joints = global.map(|rotation| {
            let in_room = AMASS_FROM_ROOM.transpose() * rotation;
            Quat::from_mat3(&(cockpit_from_room * in_room).as_mat3()).normalize()
        });
        Estimate {
            pelvis_yaw: yaw(PELVIS),
            chest_yaw: yaw(SPINE3),
            joints,
            age: now.saturating_duration_since(self.window_end),
            posture: self.posture,
            readback: self.read_at.saturating_duration_since(self.submitted_at),
        }
    }
}

/// The yaw a rotation about +Y turns a heading by, in `body-rig`'s convention.
fn room_from_cockpit_yaw(rotation: DQuat) -> f64 {
    let forward = rotation * DVec3::NEG_Z;
    (-forward.x).atan2(-forward.z)
}

/// The latest estimate read back, until a newer one replaces it or the estimator stops.
static LATEST: Mutex<Option<Reading>> = Mutex::new(None);

/// Publish `reading` as the latest estimate.
fn publish(reading: Reading) {
    *LATEST.lock() = Some(reading);
}

/// Withdraw the latest estimate, as the estimator stops.
fn withdraw() {
    *LATEST.lock() = None;
}
