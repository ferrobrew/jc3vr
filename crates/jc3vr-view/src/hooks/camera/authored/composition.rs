//! The pure transform math: the marker's motion since entry, and the view it composes into.

use glam::{Mat4, Quat, Vec3};

use crate::hooks::camera::authored::Refusal;

/// Position, orientation, and scale are retained together until matrix composition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Trs {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Trs {
    #[cfg(test)]
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn matrix(self) -> Result<Mat4, Refusal> {
        if !self.translation.is_finite()
            || !self.rotation.is_finite()
            || !self.scale.is_finite()
            || self.scale.abs().min_element() <= f32::EPSILON
            || (self.rotation.length_squared() - 1.0).abs() > 0.001
        {
            return Err(Refusal::InvalidTransform);
        }
        let matrix =
            Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation);
        if !matrix.is_finite() {
            return Err(Refusal::InvalidTransform);
        }
        Ok(matrix)
    }
}

/// The marker's model-space motion since the run's entry, `inverse(L_b) * L`, for both sides of a
/// pose pair. The native blend is already in `L`, so nothing here weights it again.
pub(super) fn deltas(baseline: Trs, pair: [Trs; 2]) -> Result<[Mat4; 2], Refusal> {
    let inverse = baseline.matrix()?.inverse();
    let deltas = [inverse * pair[0].matrix()?, inverse * pair[1].matrix()?];
    if deltas.iter().any(|matrix| !matrix.is_finite()) {
        return Err(Refusal::InvalidTransform);
    }
    Ok(deltas)
}

/// The view base excludes the tracked head; the head transform is applied exactly once here.
pub(super) fn compose_view(
    view_base: Trs,
    authored_delta: Mat4,
    tracked_head: Trs,
) -> Result<Mat4, Refusal> {
    let result = view_base.matrix()? * authored_delta * tracked_head.matrix()?;
    if !result.is_finite() {
        return Err(Refusal::InvalidTransform);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delta_is_relative_to_the_full_entry_transform() {
        let baseline = Trs {
            translation: Vec3::Y,
            rotation: Quat::from_rotation_x(0.3),
            scale: Vec3::new(0.9, 1.1, 1.0),
        };
        let current = Trs {
            translation: Vec3::X * 0.2,
            rotation: Quat::from_rotation_y(0.9),
            scale: Vec3::new(1.0, 0.8, 1.1),
        };
        let [entry, delta] = deltas(baseline, [baseline, current]).unwrap();
        assert!(entry.abs_diff_eq(Mat4::IDENTITY, 0.0001));
        let expected = baseline.matrix().unwrap().inverse() * current.matrix().unwrap();
        assert!(delta.abs_diff_eq(expected, 0.0001));
    }

    #[test]
    fn invalid_transforms_are_refused() {
        for invalid in [
            Trs {
                translation: Vec3::NAN,
                ..Trs::IDENTITY
            },
            Trs {
                scale: Vec3::ZERO,
                ..Trs::IDENTITY
            },
            Trs {
                rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 2.0),
                ..Trs::IDENTITY
            },
        ] {
            assert_eq!(invalid.matrix(), Err(Refusal::InvalidTransform));
            assert!(deltas(invalid, [Trs::IDENTITY; 2]).is_err());
        }
    }
}
