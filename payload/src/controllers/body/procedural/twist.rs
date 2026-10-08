//! The limbs' twist shared out over their twist bones once the pose is solved
//! ([`solver::twist`](crate::controllers::body::procedural::solver::twist)).
//!
//! It runs at the end of the pose-finalization pass (the post-call `UpdatePropEffects` detour),
//! after the HumanIK solve has turned the hands to the controllers and the head override and the
//! finger curls have written theirs, with `AnimationController::SetJoint` over the finalized
//! model-space pose, as those do. The arms' twist bones take the arms' twist while the procedural
//! body poses the torso, and the legs' while it poses the legs, each by that region's weight, so
//! the animation's own twist bones come back as the body hands back.

use std::time::Instant;

use glam::Quat;
use jc3gi::character::character::{AnimationController, Character, Joint};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::body::procedural::{
        config::ProceduralMode,
        joints::{from_joint, to_joint},
        published_status,
        skeleton::{self, SkeletonKey},
        solver::twist::{Region, TwistRig},
    },
};

/// What the twist distribution did on its last pass.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TwistStatus {
    pub at: Option<Instant>,
    /// How many twist bones the skeleton has, on how many segments; `None` before a skeleton is
    /// read.
    pub bones: Option<(usize, usize)>,
    /// The largest twist a segment carried on the last pass, radians.
    pub largest_twist: f32,
    /// Whether the last pass wrote the twist bones.
    pub wrote: bool,
}

/// The last pass's status. A leaf lock, so any thread may read it.
pub fn status() -> TwistStatus {
    *STATUS.lock()
}

/// Share the twist out over the local player's twist bones.
///
/// # Safety
///
/// `character` must be null or a live character, called from the post-call `UpdatePropEffects`
/// detour on the thread running its pose finalization.
pub(crate) unsafe fn after_solve(character: *mut Character) {
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let (enabled, mode) = Config::lock_query(|c| {
        (
            c.controllers.body.procedural.twist_bones,
            c.controllers.body.procedural.mode,
        )
    });
    let body = published_status();
    let weights = [body.weight, body.legs_weight];
    let mut status = TwistStatus {
        at: Some(Instant::now()),
        ..*STATUS.lock()
    };
    status.wrote = false;
    status.largest_twist = 0.0;
    if enabled
        && mode == ProceduralMode::Full
        && weights.iter().any(|w| *w > 0.0)
        && let Some(controller) =
            unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() }
    {
        let mut cache = CACHE.lock();
        if let Some(rig) = unsafe { cache.get(controller) } {
            status.bones = Some((
                rig.segments.iter().map(|s| s.rolls.len()).sum(),
                rig.segments.len(),
            ));
            let largest = unsafe { distribute(controller, rig, weights) };
            status.largest_twist = largest;
            status.wrote = !rig.segments.is_empty();
        }
    }
    *STATUS.lock() = status;
}

/// The twist rig of the skeleton a controller holds, built once per skeleton.
struct TwistCache {
    entry: Option<(SkeletonKey, Option<TwistRig>)>,
}

impl TwistCache {
    /// The twist rig of `controller`'s skeleton, when every bone it reads and writes is below the
    /// skeleton's bone count. A new skeleton is read and its twist bones logged once.
    ///
    /// # Safety
    ///
    /// `controller` must be a live controller, on the thread that owns its character.
    unsafe fn get(&mut self, controller: &AnimationController) -> Option<&TwistRig> {
        let key = unsafe { SkeletonKey::of(controller) }?;
        if self.entry.as_ref().is_none_or(|(k, _)| *k != key) {
            let rig = match unsafe { skeleton::read(controller) } {
                Ok(snapshot) => {
                    let rig = TwistRig::from_skeleton(&snapshot);
                    let names: Vec<&str> = rig
                        .segments
                        .iter()
                        .flat_map(|s| s.rolls.iter().map(|r| r.name.as_str()))
                        .collect();
                    tracing::info!(
                        target: "body",
                        bones = ?names,
                        segments = rig.segments.len(),
                        "procedural body: the twist bones the limbs' twist is shared over",
                    );
                    Some(rig)
                }
                Err(e) => {
                    tracing::warn!(target: "body", "procedural body: twist bones: {e}");
                    None
                }
            };
            self.entry = Some((key, rig));
        }
        self.entry
            .as_ref()
            .and_then(|(key, rig)| rig.as_ref().filter(|r| r.highest_bone() < key.bones()))
    }
}

/// Turn each segment's twist bones by their share of its twist, by the region's weight, and
/// return the largest twist, radians.
///
/// # Safety
///
/// As [`TwistCache::get`], with `rig` checked against the skeleton `controller` holds.
unsafe fn distribute(
    controller: &mut AnimationController,
    rig: &TwistRig,
    weights: [f32; 2],
) -> f32 {
    let read = |controller: &AnimationController, bone: usize| {
        let mut joint = Joint::default();
        unsafe { controller.GetJoint(bone as u32, &mut joint) };
        from_joint(&joint)
    };
    let mut largest: f32 = 0.0;
    for segment in &rig.segments {
        let weight = match segment.region {
            Region::Arms => weights[0],
            Region::Legs => weights[1],
        };
        if weight <= 0.0 {
            continue;
        }
        let [parent, bone, end] =
            [segment.parent, segment.bone, segment.end].map(|b| read(controller, b));
        let twist = segment.twist(parent.rotation, bone.rotation, end.rotation);
        largest = largest.max(twist.abs());
        let current: Vec<_> = segment
            .rolls
            .iter()
            .map(|r| read(controller, r.bone))
            .collect();
        let rotations: Vec<Quat> = current.iter().map(|t| t.rotation).collect();
        let turned = segment.rolls(bone.rotation, twist, &rotations, weight);
        for ((roll, now), rotation) in segment.rolls.iter().zip(&current).zip(turned) {
            let mut written = *now;
            written.rotation = rotation;
            let mut joint = to_joint(&written);
            unsafe { controller.SetJoint(roll.bone as u32, &mut joint) };
        }
    }
    largest
}

static CACHE: Mutex<TwistCache> = Mutex::new(TwistCache { entry: None });

static STATUS: Mutex<TwistStatus> = Mutex::new(TwistStatus {
    at: None,
    bones: None,
    largest_twist: 0.0,
    wrote: false,
});
