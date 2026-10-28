//! The headpose abstraction: a pure pose source of truth.
//!
//! In flatscreen, the [`sim`] module publishes a headpose (a latching mouse-look scheme). In VR,
//! the OpenXR pose will replace the sim and publish directly here. The headpose owns no latch
//! logic, no mode detection, and no body-yaw target — those are all sim concerns. Consumers (the
//! head bone override, the camera hook, the debug UI) read the world-space pose via [`query`].
//!
//! Two auxiliary pieces of state support the consumers:
//!
//! - The **anchor**: the animated (pre-override) head bone world position, published by the
//!   character hook each frame before it overrides the bone. The pose position derives from the
//!   anchor rather than being read back from the bone — the bone reflects last frame's override,
//!   so reading it back closes a feedback loop that freezes the position at whatever world point
//!   the loop first latched onto (and drags the head bone toward it as the body walks away).
//! - The **previous pose**: the pose as of the previous input tick, rotated by [`snapshot_prev`].
//!   The engine polls input on its fixed-rate sim tick, not per rendered frame, and its camera
//!   smooths that cadence by interpolating `m_TransformT0 → m_TransformT1` with the sub-frame
//!   fraction `dtf`. Handing it the previous/current pose pair lets it smooth the headpose exactly
//!   as it smooths its own camera.
//!
//! Recentering is a source concern: the sim's pose is body-relative, so it recenters by zeroing
//! its accumulated angles ([`recenter`] delegates to [`sim::reset`]). The VR source re-bases
//! against the HMD's absolute pose instead ([`crate::vr::recenter`]).

use std::sync::{
    OnceLock,
    atomic::{AtomicU8, Ordering},
};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

pub(crate) mod anchors;
pub mod config;
pub mod follow;
pub mod sim;
pub mod xr;

pub use config::HeadPoseConfig;

use crate::config::Config;

/// Which source currently owns the published headpose. The flatscreen [`sim`] and the VR [`xr`]
/// source are mutually exclusive publishers; the arbiter is a plain atomic so the sim's per-tick
/// gate and [`set_anchors`] can read it without taking the runtime lock (the VR frame loop holds the
/// OpenXR runtime lock across the frame, so reading it there would deadlock).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// The flatscreen mouse-look simulation ([`sim`]) publishes the pose.
    Sim,
    /// The OpenXR HMD source ([`xr`]) publishes the pose.
    Vr,
}

/// The active headpose source. Set once per frame by the VR frame loop (`Vr` while an OpenXR session
/// is running, else `Sim`).
static SOURCE: AtomicU8 = AtomicU8::new(Source::Sim as u8);

/// Set the active headpose source (see [`Source`]). Called once per frame by the VR frame loop.
pub fn set_source(source: Source) {
    SOURCE.store(source as u8, Ordering::Relaxed);
}

/// The active headpose source.
pub fn source() -> Source {
    if SOURCE.load(Ordering::Relaxed) == Source::Vr as u8 {
        Source::Vr
    } else {
        Source::Sim
    }
}

/// A head pose: world-space position and orientation (quaternion).
#[derive(Copy, Clone, Default)]
pub struct HeadPose {
    pub position: Vec3,
    pub orientation: Quat,
}

/// The current headpose (position + orientation), as published by the active source.
pub fn query() -> HeadPose {
    state().lock().pose
}

/// The headpose as of the previous input tick, for the engine's T0 → T1 interpolation.
pub fn query_prev() -> HeadPose {
    state().lock().prev_pose
}

/// The head's rotation *relative to the body frame*: how far the head is turned from neutral, with
/// the body's own facing (and, under the sim source, its animation-posture swing) divided out. The
/// identity means the head faces exactly where the body does.
///
/// Both sources compose their published world orientation as `body × (…) × this`, so this is the
/// factor each contributes — the sim's accumulated look angles, or the HMD's orientation relative to
/// the recenter baseline. Consumers that place the head *bone* want this rather than
/// [`query`]`().orientation`: the bone is posed in the character's model space, where the body frame
/// is already the identity, and this is the only part of the pose that is not the body's own
/// rotation. It is also rest-frame-free — neutral is the identity regardless of how the head bone's
/// rest orientation relates to the model axes — so it can be compared against zero, unlike a
/// difference taken against an animated bone orientation.
///
/// The identity until the VR source has published its first frame.
pub fn body_relative_rotation() -> Quat {
    match source() {
        Source::Sim => {
            let (yaw, pitch, roll) = sim::euler_angles();
            Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, roll)
        }
        // The view frame is the body frame turned by `xr::follow_turn` while the body follows
        // the torso, so the cockpit orientation is relative to the view, and the turn carries it
        // onto the body.
        Source::Vr => xr::cockpit_pose().map_or(Quat::IDENTITY, |pose| {
            xr::follow_turn(true) * pose.orientation
        }),
    }
}

/// Rotate the pose pair: the current pose becomes the previous pose. Called by the source at the
/// start of each input tick, so the previous/current pair spans exactly one tick.
pub fn snapshot_prev() {
    let mut s = state().lock();
    s.prev_pose = s.pose;
}

/// Called by the sim (or VR) to publish the headpose.
pub fn set_pose(pose: HeadPose) {
    state().lock().pose = pose;
}

/// Publish a tick-spaced pose pair: `prev` as of the previous sim tick, `cur` as of the current one,
/// so the engine's `dtf` lerp interpolates the camera between them across sub-tick frames. The VR
/// source uses this for the sim-driven part of the pose (the body frame and the animated head
/// anchor, which advance at the tick rate). Its HMD cockpit delta is identical on both sides, so it
/// still passes through with zero added latency — publishing the same pose to both sides instead
/// would freeze the tick-rate anchor/body motion too, which stepped the camera in vehicles and while
/// parachuting.
pub fn set_pose_pair(prev: HeadPose, cur: HeadPose) {
    let mut s = state().lock();
    s.pose = cur;
    s.prev_pose = prev;
}

/// The current geometry passed through the root-stabilization adapter before publication.
#[derive(Copy, Clone)]
pub struct Anchors {
    /// The head bone world position (this sim tick, from `m_WorldMatrixT1`).
    pub head: Vec3,
    /// The root-stabilization adapter's previous head position. Published history retains the
    /// complete preceding capture instead of reconstructing it from this field.
    pub head_prev: Vec3,
    /// The neck bone world position (the camera's pitch pivot).
    pub neck: Vec3,
    /// The neck-to-eye-midpoint arm in the body frame. The camera hangs the eye point from the neck
    /// by it: swung by the head orientation under the sim source, held at rest in the body frame
    /// under the VR source (whose tracked translation already carries the neck pivot).
    pub eye_arm: Vec3,
}

/// Publish a collapsed anchor sample for non-historical callers.
#[cfg(test)]
pub fn set_anchors(anchors: Anchors) {
    let geometry = anchors::Geometry::from_anchors(anchors, glam::Mat4::IDENTITY);
    set_anchor_pair([geometry; 2]);
}

/// Publish the complete pre-override endpoints captured on distinct simulation ticks.
/// Invalid geometry withdraws the whole pair rather than retaining individual old components.
pub(crate) fn set_anchor_pair(pair: [anchors::Geometry; 2]) {
    if !pair.into_iter().all(anchors::Geometry::valid) {
        withdraw_anchors();
        return;
    }
    let offset = Config::lock_query(|c| c.headpose.position_offset);
    let sim_orientation = (source() == Source::Sim).then(|| sim::orientation_at(pair[1].root));
    state()
        .lock()
        .publish_anchors(pair, offset, sim_orientation);
}

/// Read both complete endpoints from the same publication.
pub(crate) fn anchor_pair() -> Option<[anchors::Geometry; 2]> {
    state().lock().anchor_pair
}

/// Withdraw simulation anchor authority while leaving the separately published display pose alone.
pub(crate) fn withdraw_anchors() {
    let mut state = state().lock();
    state.anchor = None;
    state.anchor_prev = None;
    state.anchor_pair = None;
    state.neck_delta = Vec3::ZERO;
    state.eye_arm = Vec3::ZERO;
}

/// The animated head bone world position, or `None` until the character hook has published a valid
/// one (no local character yet, or only garbage bone data so far).
pub fn anchor() -> Option<Vec3> {
    state().lock().anchor
}

/// The animated head bone world position as of the previous sim tick (see [`Anchors::head_prev`]),
/// or `None` until the character hook has published a valid one. The VR source composes the
/// previous-tick side of its pose pair from this.
pub fn anchor_prev() -> Option<Vec3> {
    state().lock().anchor_prev
}

/// The world-space vector from the head bone anchor to the neck bone: the camera hangs the eye arm
/// from the neck rather than the head bone, so under the sim source pitching the head swings the
/// eyes forward over the chest instead of rotating in place at the skull base. Zero until the
/// character hook has published anchors. Camera consumers read the matching endpoint from
/// [`anchor_pair`] instead of sharing this current value across ticks.
pub fn neck_delta() -> Vec3 {
    state().lock().neck_delta
}

/// The neck-to-eye-midpoint arm in the body frame (see [`Anchors::eye_arm`]). Zero until the
/// character hook has published anchors.
pub fn eye_arm() -> Vec3 {
    state().lock().eye_arm
}

/// Recenter the headpose. The sim is body-relative and recenters by zeroing its accumulated angles;
/// the VR source re-bases the cockpit baseline against the HMD's latest located pose. Both are
/// re-based so one action recenters whichever source is live (and the other stays consistent for a
/// later switch). The sim recenters at once; the VR re-base runs at the start of the next frame
/// ([`crate::vr::recenter`]), so this is safe from any thread.
pub fn recenter() {
    sim::reset();
    crate::vr::recenter();
}

/// The desired body forward on the ground plane the locomotion hook should steer the body toward,
/// or `None` to leave the body untouched. Dispatched by the active source: the VR source turns the
/// body with the look input ([`xr::body_yaw_target`]), the flatscreen sim with its head-follow latch
/// ([`sim::body_yaw_target`]).
pub fn body_yaw_target() -> Option<Vec3> {
    match source() {
        Source::Vr => xr::body_yaw_target(),
        Source::Sim => sim::body_yaw_target(),
    }
}

/// Whether headpose-driven head control is enabled.
pub fn is_active() -> bool {
    Config::lock_query(|c| c.headpose.enabled)
}

#[derive(Default)]
pub(crate) struct HeadPoseState {
    /// The current headpose, written by the active source (sim or VR).
    pose: HeadPose,
    /// The headpose as of the previous input tick, for engine interpolation.
    prev_pose: HeadPose,
    /// The animated head bone world position, published by the character hook.
    anchor: Option<Vec3>,
    /// The previous sim tick's animated head bone world position (see [`Anchors::head_prev`]).
    anchor_prev: Option<Vec3>,
    anchor_pair: Option<[anchors::Geometry; 2]>,
    /// The world-space head-to-neck vector, published alongside the anchor (see [`neck_delta`]).
    neck_delta: Vec3,
    /// The body-frame neck-to-eye-midpoint arm (see [`Anchors::eye_arm`]).
    eye_arm: Vec3,
}

impl HeadPoseState {
    pub(crate) fn publish_anchors(
        &mut self,
        pair: [anchors::Geometry; 2],
        offset: Vec3,
        sim_orientation: Option<Quat>,
    ) {
        let [previous, current] = pair;
        let previous_anchor = self.anchor;
        self.anchor = Some(current.head);
        self.anchor_prev = Some(previous.head);
        self.neck_delta = current.neck_delta;
        self.eye_arm = current.eye_arm;
        self.anchor_pair = Some(pair);
        if let Some(orientation) = sim_orientation {
            self.pose = HeadPose {
                position: current.head + orientation * offset,
                orientation,
            };
            if current.collapsed {
                self.prev_pose = self.pose;
            }
        } else if let Some(previous_anchor) = previous_anchor {
            self.pose.position += current.head - previous_anchor;
        }
    }

    #[cfg(test)]
    pub(crate) fn test_poses(&self) -> [HeadPose; 2] {
        [self.prev_pose, self.pose]
    }

    #[cfg(test)]
    pub(crate) fn test_set_poses(&mut self, poses: [HeadPose; 2]) {
        [self.prev_pose, self.pose] = poses;
    }
}

static STATE: OnceLock<Mutex<HeadPoseState>> = OnceLock::new();

fn state() -> &'static Mutex<HeadPoseState> {
    STATE.get_or_init(|| Mutex::new(HeadPoseState::default()))
}
