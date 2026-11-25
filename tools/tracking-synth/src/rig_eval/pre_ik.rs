//! The floor point under the hips of the game's own animated pose (the pre-IK `Pose` samples, from
//! version 8), carried into the room frame, for replaying the rig against the game's animation.
//!
//! A pose is in the character's model space, and its `character` transform places it in the world.
//! The controller state's view (`ViewState`) maps the cockpit frame into the world, so its inverse
//! carries the hips back into the cockpit frame. The room frame is the cockpit frame less the
//! floor's height, which leaves the floor point `(x, z)` as it is.
//!
//! The pose is read during the game frame's world update and the view after its render update, so
//! a pose takes the view of the first controller state after it: the same game frame's.

use glam::{Vec2, Vec3};
use tracking_format::{
    Recording,
    event::{EventKind, PoseSample, PoseStage, ViewState},
    model::Pose,
};

/// The hips' floor points of a recording's pre-IK poses, in time order.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PreIkTrack {
    /// `(time_ns, room (x, z))`, at each pose's time.
    pub(crate) points: Vec<(u64, Vec2)>,
    /// Pre-IK poses with hips that no view placed.
    pub(crate) unplaced: usize,
}

impl PreIkTrack {
    /// The latest point at or before `time_ns`, while it is younger than [`POSE_FRESH_NS`].
    pub(crate) fn at(&self, time_ns: u64) -> Option<Vec2> {
        let index = self.points.partition_point(|(at, _)| *at <= time_ns);
        let (at, point) = *self.points.get(index.checked_sub(1)?)?;
        (time_ns - at <= POSE_FRESH_NS).then_some(point)
    }
}

/// Read the pre-IK poses' hips out of `recording`.
pub(crate) fn pre_ik_track(recording: &Recording) -> PreIkTrack {
    let mut views: Vec<(usize, u64, &ViewState)> = Vec::new();
    let mut poses: Vec<(usize, u64, &PoseSample)> = Vec::new();
    for (index, event) in recording.events.iter().enumerate() {
        match &event.kind {
            EventKind::ControllerState(state) => {
                if let Some(view) = state.view.as_ref() {
                    views.push((index, event.time_ns, view));
                }
            }
            EventKind::Pose(pose) if pose.stage == PoseStage::PreIk => {
                poses.push((index, event.time_ns, pose));
            }
            _ => {}
        }
    }
    let mut track = PreIkTrack::default();
    for (index, time_ns, pose) in poses {
        let Some(hips) = hips(pose) else {
            continue;
        };
        let after = views.partition_point(|(at, _, _)| *at < index);
        let view = views
            .get(after)
            .and_then(|entry| near(entry, time_ns))
            .or_else(|| near(views.get(after.checked_sub(1)?)?, time_ns));
        match view.and_then(|view| hips_in_cockpit(&pose.character, &hips, view)) {
            Some(cockpit) => track
                .points
                .push((time_ns, Vec2::new(cockpit.x, cockpit.z))),
            None => track.unplaced += 1,
        }
    }
    track.points.sort_by_key(|(at, _)| *at);
    track
}

/// The hips at `hips` in the model space of a character placed by `character`, carried through
/// `view` into the cockpit frame: `head_cockpit + view⁻¹ * (world - head_world) / world_scale`.
pub(crate) fn hips_in_cockpit(character: &Pose, hips: &Pose, view: &ViewState) -> Option<Vec3> {
    if !(view.world_scale.is_finite() && view.world_scale > 0.0) {
        return None;
    }
    let world = character.position + character.orientation * hips.position;
    let cockpit = view.head_cockpit
        + view.view.inverse() * (world - view.head_world.position) / view.world_scale;
    cockpit.is_finite().then_some(cockpit)
}

/// A pre-IK pose older than this no longer places the pelvis, nanoseconds: about the age past
/// which the payload stops trusting Rico's skeleton.
const POSE_FRESH_NS: u64 = 250_000_000;

/// A pose takes a view at most this far from it in time, nanoseconds.
const VIEW_PAIR_NS: u64 = 100_000_000;

/// The skeleton's name for the hips bone.
const HIPS: &str = "Hips";

/// The view of `entry`, if it is within [`VIEW_PAIR_NS`] of `time_ns`.
fn near<'a>(entry: &(usize, u64, &'a ViewState), time_ns: u64) -> Option<&'a ViewState> {
    let (_, at, view) = *entry;
    (at.abs_diff(time_ns) <= VIEW_PAIR_NS).then_some(view)
}

fn hips(pose: &PoseSample) -> Option<Pose> {
    let index = pose.set.names().iter().position(|name| *name == HIPS)?;
    *pose.bones.get(index)?
}
