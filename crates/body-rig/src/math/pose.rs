//! A rigid transform and its algebra.

use glam::{Mat4, Quat, Vec3};

/// A rigid transform: a rotation, then a translation. As a placement, it is where a frame sits and
/// how it is turned within its parent.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Pose {
    /// Metres.
    pub position: Vec3,
    pub orientation: Quat,
}

impl Pose {
    pub const IDENTITY: Pose = Pose {
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
    };

    /// `self` then `local`: the pose `local`, given in `self`'s frame, in the frame `self` is given
    /// in. The orientation is renormalized, so that a long chain does not drift off the unit
    /// sphere.
    pub fn compose(self, local: Pose) -> Pose {
        Pose {
            position: self.transform_point(local.position),
            orientation: (self.orientation * local.orientation).normalize(),
        }
    }

    /// The transform that undoes `self`: `self.compose(self.inverse())` is the identity.
    pub fn inverse(self) -> Pose {
        let orientation = self.orientation.inverse();
        Pose {
            position: orientation * -self.position,
            orientation,
        }
    }

    /// The point `point`, given in `self`'s frame, in the frame `self` is given in.
    pub fn transform_point(self, point: Vec3) -> Vec3 {
        self.position + self.orientation * point
    }

    /// The matrix of the transform.
    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_rotation_translation(self.orientation, self.position)
    }

    /// The rotation and translation of `matrix`, with any scale it carries dropped.
    pub fn from_mat4(matrix: Mat4) -> Pose {
        let (_, orientation, position) = matrix.to_scale_rotation_translation();
        Pose {
            position,
            orientation,
        }
    }
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec4;
    use proptest::prelude::*;

    use super::*;

    fn pose() -> impl Strategy<Value = Pose> {
        (
            proptest::array::uniform3(-10.0f32..10.0),
            proptest::array::uniform4(-1.0f32..1.0),
        )
            .prop_filter_map("a rotation needs a nonzero quaternion", |(p, q)| {
                let q = Vec4::from(q);
                (q.length() > 0.1).then(|| Pose {
                    position: Vec3::from(p),
                    orientation: Quat::from_vec4(q.normalize()),
                })
            })
    }

    fn close(a: Pose, b: Pose) -> bool {
        a.position.abs_diff_eq(b.position, 1e-3)
            && a.orientation.dot(b.orientation).abs() > 1.0 - 1e-5
    }

    proptest! {
        #[test]
        fn composing_matches_the_matrices(a in pose(), b in pose(), point in proptest::array::uniform3(-5.0f32..5.0)) {
            let point = Vec3::from(point);
            let composed = a.compose(b);
            prop_assert!(composed.to_mat4().abs_diff_eq(a.to_mat4() * b.to_mat4(), 1e-3));
            prop_assert!(composed.transform_point(point).abs_diff_eq(a.transform_point(b.transform_point(point)), 1e-3));
        }

        #[test]
        fn the_inverse_undoes_the_pose(a in pose()) {
            prop_assert!(close(a.compose(a.inverse()), Pose::IDENTITY));
            prop_assert!(close(a.inverse().compose(a), Pose::IDENTITY));
            prop_assert!(close(a.inverse().inverse(), a));
        }

        #[test]
        fn composition_is_associative(a in pose(), b in pose(), c in pose()) {
            prop_assert!(close(a.compose(b).compose(c), a.compose(b.compose(c))));
        }

        #[test]
        fn the_matrix_round_trips(a in pose(), scale in 0.1f32..10.0) {
            prop_assert!(close(Pose::from_mat4(a.to_mat4()), a));
            let scaled = a.to_mat4() * Mat4::from_scale(Vec3::splat(scale));
            prop_assert!(close(Pose::from_mat4(scaled), a));
        }
    }
}
