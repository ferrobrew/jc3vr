//! Geometry primitives: the nearest point of a segment, and a cast along a segment long enough to
//! be worth casting.

use glam::Vec3;

/// The point of the segment from `a` to `b` nearest `point`; `a` for a segment of no length.
pub fn closest_on_segment(point: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let along = b - a;
    let length_sq = along.length_squared();
    if length_sq < MIN_SEGMENT_LENGTH_SQ {
        return a;
    }
    a + along * ((point - a).dot(along) / length_sq).clamp(0.0, 1.0)
}

/// The distance from `point` to the segment from `a` to `b`.
pub fn segment_distance(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    point.distance(closest_on_segment(point, a, b))
}

/// `cast` along the segment from `from` to `to`, or `None` without casting when the segment is
/// shorter than [`MIN_CAST_M`].
pub fn cast_segment<T>(
    from: Vec3,
    to: Vec3,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<T>,
) -> Option<T> {
    if (to - from).length() < MIN_CAST_M {
        None
    } else {
        cast(from, to)
    }
}

/// The shortest segment worth casting, metres.
pub const MIN_CAST_M: f32 = 1e-3;

/// The squared length below which a segment is a point.
const MIN_SEGMENT_LENGTH_SQ: f32 = 1e-10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_closest_point_is_clamped_to_the_segment() {
        let (a, b) = (Vec3::ZERO, Vec3::X);
        assert_eq!(
            closest_on_segment(Vec3::new(0.5, 0.2, 0.0), a, b),
            Vec3::new(0.5, 0.0, 0.0)
        );
        assert_eq!(closest_on_segment(Vec3::new(-0.3, 0.0, 0.0), a, b), a);
        assert_eq!(closest_on_segment(Vec3::new(1.4, 0.3, 0.0), a, b), b);
        assert!((segment_distance(Vec3::new(1.4, 0.3, 0.0), a, b) - 0.5).abs() < 1e-6);
        assert!((segment_distance(Vec3::Y, a, a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_segment_too_short_is_not_cast() {
        let mut casts = 0;
        let mut cast = |_: Vec3, to: Vec3| {
            casts += 1;
            Some(to)
        };
        assert_eq!(cast_segment(Vec3::ZERO, Vec3::splat(1e-4), &mut cast), None);
        assert_eq!(cast_segment(Vec3::ZERO, Vec3::X, &mut cast), Some(Vec3::X));
        assert_eq!(casts, 1);
    }
}
