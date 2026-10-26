//! Current-step terrain queries used by contact transitions, never by a render update.

use std::time::Instant;

use glam::{Mat4, Vec3};

use crate::controllers::body::rig::{estimate::Support, legs::swing::FootPose};

/// A copied support frame observed after physics, without a prediction from its velocity.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct SupportSample {
    pub identity: u64,
    pub generation: u64,
    pub transform: Mat4,
    pub at: Instant,
}

/// Each accepted point is in the named support's current local frame.
/// A miss, an unsupported body, or a different support returns no foothold.
pub(crate) trait Terrain {
    fn observed(&self) -> bool {
        true
    }

    fn floor_height(&self, support: Support) -> f32 {
        support.current.w_axis.y
    }

    fn foothold(
        &mut self,
        support: Support,
        point: Vec3,
        heading: f32,
        ankle_height: f32,
    ) -> Option<FootPose>;
}

pub(crate) struct MissingTerrain;

impl Terrain for MissingTerrain {
    fn foothold(
        &mut self,
        _support: Support,
        _point: Vec3,
        _heading: f32,
        _ankle_height: f32,
    ) -> Option<FootPose> {
        None
    }
}

/// Finite analytic patches are retained for deterministic estimator fixtures.
#[cfg(test)]
pub(crate) struct PatchTerrain;

#[cfg(test)]
impl Terrain for PatchTerrain {
    fn observed(&self) -> bool {
        false
    }

    fn foothold(
        &mut self,
        support: Support,
        point: Vec3,
        heading: f32,
        ankle_height: f32,
    ) -> Option<FootPose> {
        support.foothold(point, heading, ankle_height)
    }
}

/// The same geometric tolerance applies to a retained plant and a completed swing.
pub(crate) fn matches_surface(
    terrain: &mut dyn Terrain,
    support: Support,
    pose: FootPose,
    ankle_height: f32,
) -> bool {
    terrain
        .foothold(
            support,
            pose.position,
            crate::controllers::body::rig::estimate::contacts::yaw(pose.rotation),
            ankle_height,
        )
        .is_some_and(|local| support.world(local).position.distance(pose.position) <= 0.035)
}
