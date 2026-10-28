//! Held lower-body targets shared by the solve math and its retained mathematical regressions.

use glam::Vec3;

use crate::controllers::body::rig::legs::swing::FootPose;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LowerTargets {
    pub weight: f32,
    pub hips: Vec3,
    pub feet: [FootPose; 2],
    pub poles: [Vec3; 2],
}
