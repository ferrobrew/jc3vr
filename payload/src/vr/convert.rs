//! Conversions between OpenXR's pose types and the payload's: `glam` vectors and quaternions, and
//! the controllers' [`Pose`]. Both use the same axes (+X right, +Y up, -Z forward, metres), so each
//! is a field-by-field copy.

use glam::{Quat, Vec3};
use openxr as xr;

use crate::controllers::frame::Pose;

/// An OpenXR vector as a [`Vec3`].
pub(crate) fn vec3(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// An OpenXR quaternion as a [`Quat`].
pub(crate) fn quat(q: xr::Quaternionf) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

/// An OpenXR pose as a [`Pose`].
pub(crate) fn pose(pose: xr::Posef) -> Pose {
    Pose {
        position: vec3(pose.position),
        orientation: quat(pose.orientation),
    }
}

/// An OpenXR pose from a position and an orientation.
pub(crate) fn posef(position: Vec3, orientation: Quat) -> xr::Posef {
    xr::Posef {
        orientation: xr::Quaternionf {
            x: orientation.x,
            y: orientation.y,
            z: orientation.z,
            w: orientation.w,
        },
        position: xr::Vector3f {
            x: position.x,
            y: position.y,
            z: position.z,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pose_survives_the_round_trip() {
        let back = pose(posef(Vec3::new(0.1, 1.6, -0.2), Quat::from_rotation_y(0.4)));
        assert_eq!(back.position, Vec3::new(0.1, 1.6, -0.2));
        assert_eq!(back.orientation, Quat::from_rotation_y(0.4));
    }
}
