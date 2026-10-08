//! The rig's joints in the animation controller: the rig built for the skeleton the controller
//! holds, and the reads and writes of the driven bones through `AnimationController::GetJoint` and
//! `AnimationController::SetJoint`.
//!
//! [`RigCache`] keys the rig on the whole [`SkeletonKey`] (the controller, its skeleton resource,
//! the Havok skeleton, and the bone count), so a controller that holds another skeleton is rigged
//! afresh, and hands out a [`LiveRig`] only when every index the rig drives is below the
//! skeleton's bone count. `SetJoint` writes wherever its index points, so a [`LiveRig`] is the
//! only way the procedural body reaches it.

use glam::{Quat, Vec3};
use jc3gi::character::character::{AnimationController, Joint};

use crate::controllers::body::procedural::{
    rig::{Rig, RigBone, RigError},
    skeleton::{self, SkeletonKey, Transform},
    solver::pose::RigPose,
};

/// A rig checked against the skeleton its controller holds this tick.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveRig {
    pub rig: Rig,
}

impl LiveRig {
    /// The animated model-space pose of the rig's bones.
    ///
    /// # Safety
    ///
    /// `controller` must be the live controller the rig was checked against this tick, on the
    /// thread that owns its character.
    pub unsafe fn read(&self, controller: &AnimationController) -> RigPose {
        RigPose {
            bones: RigBone::ALL.map(|bone| {
                let mut joint = Joint::default();
                unsafe { controller.GetJoint(self.index(bone), &mut joint) };
                from_joint(&joint)
            }),
        }
    }

    /// Write the rig's bones, parents first, so each write carries the children that follow. A
    /// pose that is not finite is not written.
    ///
    /// # Safety
    ///
    /// As [`read`](Self::read).
    pub unsafe fn write(&self, controller: &mut AnimationController, pose: &RigPose) {
        if !pose.is_finite() {
            return;
        }
        for bone in RigBone::ALL {
            let mut joint = to_joint(pose.get(bone));
            unsafe { controller.SetJoint(self.index(bone), &mut joint) };
        }
    }

    fn index(&self, bone: RigBone) -> u32 {
        self.rig.skeleton_index(bone) as u32
    }
}

/// What [`RigCache::get`] found.
pub struct Lookup {
    /// The rig, when the skeleton is rigged and the rig fits it.
    pub rig: Option<LiveRig>,
    /// Whether this lookup rigged a skeleton it had not seen: what was measured on the last one
    /// is stale.
    pub rebuilt: bool,
    /// Why the skeleton could not be rigged, when this lookup tried and could not.
    pub error: Option<RigError>,
}

/// The rig of the skeleton an animation controller holds, built once per skeleton.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RigCache {
    /// The rig, for the skeleton it was built from; `None` beside the key when that skeleton could
    /// not be rigged.
    entry: Option<(SkeletonKey, Option<Rig>)>,
}

impl RigCache {
    pub const fn new() -> Self {
        Self { entry: None }
    }

    /// The rig of `controller`'s skeleton. A skeleton that cannot be rigged is reported once.
    ///
    /// # Safety
    ///
    /// `controller` must be a live controller, on the thread that owns its character.
    pub unsafe fn get(&mut self, controller: &AnimationController) -> Lookup {
        let Some(key) = (unsafe { SkeletonKey::of(controller) }) else {
            return Lookup {
                rig: None,
                rebuilt: false,
                error: None,
            };
        };
        let mut rebuilt = false;
        let mut error = None;
        if self.entry.as_ref().is_none_or(|(k, _)| *k != key) {
            rebuilt = true;
            let rig = match unsafe { skeleton::read(controller) } {
                Ok(snapshot) => match Rig::from_skeleton(&snapshot) {
                    Ok(rig) => {
                        tracing::info!(target: "body", "procedural body: rigged the skeleton");
                        Some(rig)
                    }
                    Err(e) => {
                        tracing::warn!(target: "body", "procedural body: {e}");
                        error = Some(e);
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!(target: "body", "procedural body: {e}");
                    None
                }
            };
            self.entry = Some((key, rig));
        }
        let rig = self
            .entry
            .as_ref()
            .and_then(|(key, rig)| rig.as_ref().filter(|rig| fits(rig, key.bones())))
            .map(|rig| LiveRig { rig: rig.clone() });
        Lookup {
            rig,
            rebuilt,
            error,
        }
    }
}

/// Whether every bone `rig` drives has an index below `bones`.
fn fits(rig: &Rig, bones: usize) -> bool {
    rig.bones.iter().all(|&index| index < bones)
}

pub(super) fn from_joint(joint: &Joint) -> Transform {
    let [x, y, z, w] = joint.m_Orientation.data;
    Transform {
        translation: Vec3::from_array(joint.m_Translation.data),
        rotation: Quat::from_xyzw(x, y, z, w),
        scale: Vec3::from_array(joint.m_Scale.data),
    }
}

pub(super) fn to_joint(transform: &Transform) -> Joint {
    let mut joint = Joint::default();
    joint.m_Translation.data = transform.translation.to_array();
    joint.m_Orientation.data = transform.rotation.to_array();
    joint.m_Scale.data = transform.scale.to_array();
    joint
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::rig::tests::skeleton;

    #[test]
    fn a_rig_fits_only_a_skeleton_with_all_its_bones() {
        let snapshot = skeleton();
        let rig = Rig::from_skeleton(&snapshot).unwrap();
        assert!(fits(&rig, snapshot.bones.len()));
        let highest = *rig.bones.iter().max().unwrap();
        assert!(!fits(&rig, highest));
    }
}
