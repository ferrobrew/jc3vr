//! Geometry primitives: the nearest point of a segment, a point held within a sphere, and a cast
//! along a segment long enough to be worth casting.

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

/// `point`, or where the line from `centre` to it meets the sphere of `radius` about `centre` when
/// it lies outside. A point inside is returned exactly as it is, so the hold is continuous and a
/// point moving past the sphere slides along its surface. A radius that is not positive and finite,
/// or a point whose offset is not finite, leaves the point as it is.
pub fn within_sphere(point: Vec3, centre: Vec3, radius: f32) -> Vec3 {
    let offset = point - centre;
    let outside = offset.length_squared() > radius * radius;
    if radius.is_finite() && radius > 0.0 && offset.is_finite() && outside {
        centre + offset * (radius / offset.length())
    } else {
        point
    }
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
    fn a_point_outside_the_sphere_lands_on_its_surface() {
        let centre = Vec3::new(1.0, 0.0, 0.0);
        assert_eq!(
            within_sphere(Vec3::new(1.2, 0.0, 0.0), centre, 0.5),
            Vec3::new(1.2, 0.0, 0.0)
        );
        assert!(
            (within_sphere(Vec3::new(3.0, 0.0, 0.0), centre, 0.5) - Vec3::new(1.5, 0.0, 0.0))
                .length()
                < 1e-6
        );
        let far = Vec3::new(3.0, 0.0, 0.0);
        assert_eq!(within_sphere(far, centre, 0.0), far);
        assert_eq!(within_sphere(far, centre, f32::NAN), far);
        let lost = Vec3::new(f32::INFINITY, 0.0, 0.0);
        assert_eq!(within_sphere(lost, centre, 0.5), lost);
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
