//! The narrowed hull's shape: a copy of a human proxy's convex hull half as wide across the ground,
//! built through Havok's own convex factory, and the step profile of a hull's bottom cone.

use std::{mem::MaybeUninit, ptr::NonNull};

use jc3gi::havok::{
    hkStridedVertices, hkVector4f, hknpConvexShape, hknpConvexShapeBuildConfig, hknpShape,
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Bounds {
    pub(super) min: [f32; 3],
    pub(super) max: [f32; 3],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum GeometryError {
    UnsupportedShape,
    InvalidGeometry,
    Allocation,
    ChangedHeightOrCenter,
}

/// A copy of the convex hull `source` half as wide across the ground, its height and centre kept
/// ([`narrow_points`]), built by [`hknpConvexShape::createFromVertices`], with the source's bounds.
///
/// # Safety
///
/// `source` is a retained native convex shape, and the calling thread has a Havok router.
pub(super) unsafe fn narrow(
    source: NonNull<hknpShape>,
) -> Result<(NonNull<hknpShape>, Bounds), GeometryError> {
    let original = unsafe { bounds(source)? };
    let shape = unsafe { source.as_ref() };
    let radius = shape.m_convexRadius;
    let center =
        std::array::from_fn::<_, 3, _>(|axis| (original.min[axis] + original.max[axis]) * 0.5);
    let mut scale = [1.0; 3];
    for axis in [0, 2] {
        let half = (original.max[axis] - original.min[axis]) * 0.5;
        let core_half = half - radius;
        let target_core_half = half * 0.5 - radius;
        if core_half <= 0.0 || target_core_half <= 0.0 {
            return Err(GeometryError::InvalidGeometry);
        }
        scale[axis] = target_core_half / core_half;
    }
    let mut vertices = unsafe { vertices(source)? }.to_vec();
    let mut points: Vec<[f32; 3]> = vertices
        .iter()
        .map(|vertex| [vertex.data[0], vertex.data[1], vertex.data[2]])
        .collect();
    narrow_points(&mut points, center, scale);
    for (vertex, point) in vertices.iter_mut().zip(&points) {
        vertex.data = [point[0], point[1], point[2], 0.0];
    }
    let input = hkStridedVertices {
        m_vertices: vertices.as_ptr().cast(),
        m_numVertices: vertices.len() as i32,
        m_striding: size_of::<hkVector4f>() as i32,
    };
    let mut config = MaybeUninit::<hknpConvexShapeBuildConfig>::zeroed();
    let config = unsafe {
        (*config.as_mut_ptr()).Construct();
        config.assume_init_mut()
    };
    // The input consists of the source's core vertices, not its radius-expanded surface.
    config.m_shrinkByRadius = false;
    config.m_simplificationTolerance = 0.0;
    let replacement =
        NonNull::new(unsafe { hknpConvexShape::createFromVertices(&input, radius, config) })
            .ok_or(GeometryError::Allocation)?;
    unsafe { (*replacement.as_ptr()).m_userData = shape.m_userData };
    let actual = match unsafe { bounds(replacement) } {
        Ok(bounds) => bounds,
        Err(error) => {
            unsafe { crate::controllers::physical_hands::human_hull::native::release(replacement) };
            return Err(error);
        }
    };
    let valid = (actual.min[1] - original.min[1]).abs() <= GEOMETRY_TOLERANCE_M
        && (actual.max[1] - original.max[1]).abs() <= GEOMETRY_TOLERANCE_M
        && [0, 2].into_iter().all(|axis| {
            let actual_center = (actual.min[axis] + actual.max[axis]) * 0.5;
            let expected_half = (original.max[axis] - original.min[axis]) * 0.25;
            (actual_center - center[axis]).abs() <= GEOMETRY_TOLERANCE_M
                && ((actual.max[axis] - actual.min[axis]) * 0.5 - expected_half).abs()
                    <= GEOMETRY_TOLERANCE_M
        });
    if !valid {
        unsafe { crate::controllers::physical_hands::human_hull::native::release(replacement) };
        return Err(GeometryError::ChangedHeightOrCenter);
    }
    Ok((replacement, original))
}

/// Narrow a hull's core vertices `points` about `center` by `scale` across the ground (x and z),
/// keeping its height, and keep the slope of its bottom cap.
///
/// The game's human proxy (`CSharedPfxCharacterSettings::CreateQuadrupedShape`) is a heptagonal
/// prism whose bottom is a cone: an apex at the feet and a ring as wide as the hull, as high above
/// the apex as it is wide. The character's rigid body treats a contact whose normal leans more than
/// its maximum slope from the vertical as a wall, and the cone's faces lie just under the slope the
/// game gives it, so a kerb's or a step's edge pressed into them steps the character up. Scaled
/// across the ground alone, the cone keeps its height at half the width and its faces stand far
/// steeper than that slope: every step's edge meets a wall.
/// So the vertices of the bottom cap (no higher above the lowest vertex than the hull's widest
/// reach from its axis) are also scaled toward the lowest vertex by the same factor, which keeps
/// the cone's angles; the narrower cone steps up edges as high as it now is, half the authored
/// cone's height. The rest of the hull keeps its height, and the lowest and highest vertices stay.
pub(super) fn narrow_points(points: &mut [[f32; 3]], center: [f32; 3], scale: [f32; 3]) {
    let Some(bottom) = points.iter().map(|p| p[1]).min_by(f32::total_cmp) else {
        return;
    };
    let reach = points
        .iter()
        .map(|p| (p[0] - center[0]).hypot(p[2] - center[2]))
        .fold(0.0_f32, f32::max);
    let cap_scale = (scale[0] + scale[2]) * 0.5;
    for point in points.iter_mut() {
        for axis in [0, 2] {
            point[axis] = center[axis] + (point[axis] - center[axis]) * scale[axis];
        }
        let rise = point[1] - bottom;
        if rise <= reach + GEOMETRY_TOLERANCE_M {
            point[1] = bottom + rise * cap_scale;
        }
    }
}

/// What a hull's bottom steps up: the height of its bottom cone above its lowest point, and how far
/// the hull reaches from its axis across the ground, metres, both including the convex radius
/// `radius`. The cone's top is the lowest of the vertices at the hull's widest reach.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct StepProfile {
    pub(crate) cone_height_m: f32,
    pub(crate) half_width_m: f32,
}

/// The [`StepProfile`] of a hull of core vertices `points` with convex radius `radius`, its axis
/// through the lowest vertex, the bottom cone's apex. `None` for no vertices.
pub(super) fn step_profile(points: &[[f32; 3]], radius: f32) -> Option<StepProfile> {
    let lowest = points.iter().min_by(|a, b| a[1].total_cmp(&b[1]))?;
    let (bottom, cx, cz) = (lowest[1], lowest[0], lowest[2]);
    let reach_of = |p: &[f32; 3]| (p[0] - cx).hypot(p[2] - cz);
    let reach = points.iter().map(reach_of).fold(0.0_f32, f32::max);
    let top = points
        .iter()
        .filter(|p| reach_of(p) >= reach - REACH_TOLERANCE_M)
        .map(|p| p[1])
        .min_by(f32::total_cmp)?;
    let radius = if radius.is_finite() {
        radius.max(0.0)
    } else {
        0.0
    };
    Some(StepProfile {
        cone_height_m: top - bottom,
        half_width_m: reach + radius,
    })
}

/// The [`StepProfile`] of a live convex shape.
///
/// # Safety
///
/// `shape` must be a retained native convex shape.
pub(super) unsafe fn shape_step_profile(shape: NonNull<hknpShape>) -> Option<StepProfile> {
    let radius = unsafe { shape.as_ref() }.m_convexRadius;
    let points: Vec<[f32; 3]> = unsafe { vertices(shape) }
        .ok()?
        .iter()
        .map(|v| [v.data[0], v.data[1], v.data[2]])
        .collect();
    step_profile(&points, radius)
}

pub(super) unsafe fn bounds(shape: NonNull<hknpShape>) -> Result<Bounds, GeometryError> {
    let radius = unsafe { shape.as_ref() }.m_convexRadius;
    if !radius.is_finite() || radius < 0.0 {
        return Err(GeometryError::InvalidGeometry);
    }
    let mut bounds = Bounds {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };
    for vertex in unsafe { vertices(shape)? } {
        for axis in 0..3 {
            let coordinate = vertex.data[axis];
            if !coordinate.is_finite() {
                return Err(GeometryError::InvalidGeometry);
            }
            bounds.min[axis] = bounds.min[axis].min(coordinate - radius);
            bounds.max[axis] = bounds.max[axis].max(coordinate + radius);
        }
    }
    Ok(bounds)
}

pub(super) unsafe fn vertices<'a>(
    shape: NonNull<hknpShape>,
) -> Result<&'a [hkVector4f], GeometryError> {
    let shape_ref = unsafe { shape.as_ref() };
    if !shape_ref.m_convexRadius.is_finite() || shape_ref.m_convexRadius < 0.0 {
        return Err(GeometryError::InvalidGeometry);
    }
    if shape_ref.m_flags & hknpShape::IS_CONVEX_SHAPE == 0 {
        return Err(GeometryError::UnsupportedShape);
    }
    let convex = unsafe { &*shape.as_ptr().cast::<hknpConvexShape>() };
    let array = &convex.m_vertices;
    if array.m_size == 0 || array.m_size > MAXIMUM_VERTICES || array.m_offset == 0 {
        return Err(GeometryError::InvalidGeometry);
    }
    let start = (array as *const _ as *const u8)
        .wrapping_add(usize::from(array.m_offset))
        .cast::<hkVector4f>();
    let vertices = unsafe { std::slice::from_raw_parts(start, usize::from(array.m_size)) };
    if vertices
        .iter()
        .any(|vertex| !vertex.data[..3].iter().all(|value| value.is_finite()))
    {
        return Err(GeometryError::InvalidGeometry);
    }
    Ok(vertices)
}

const MAXIMUM_VERTICES: u16 = 252;
const GEOMETRY_TOLERANCE_M: f32 = 1.0e-4;
/// How far short of the hull's widest reach a vertex may be and still count as at it: the
/// heptagon's corners all sit at the same reach, to rounding.
const REACH_TOLERANCE_M: f32 = 1.0e-3;

#[cfg(test)]
mod tests {
    use super::*;

    /// The game's player standing hull, as `CreateQuadrupedShape` builds it for that state.
    fn player_hull() -> Vec<[f32; 3]> {
        let (height, radius) = (1.7_f32, 0.45_f32);
        let mut points = vec![[0.0, height, 0.0]];
        for k in 0..7 {
            let angle = k as f32 * std::f32::consts::TAU / 7.0;
            let (sin, cos) = angle.sin_cos();
            points.push([radius * cos, height - radius, -radius * sin]);
            points.push([radius * cos, radius, -radius * sin]);
        }
        points.push([0.0, 0.0, 0.0]);
        points
    }

    /// The steepest lean of a bottom face's normal from the vertical, degrees: a face of the cone
    /// rises from the apex to the ring by the ring's height over its inradius.
    fn bottom_face_lean_deg(points: &[[f32; 3]]) -> f32 {
        let ring = points
            .iter()
            .filter(|p| p[1] > 0.0)
            .min_by(|a, b| a[1].total_cmp(&b[1]))
            .unwrap();
        let reach = ring[0].hypot(ring[2]);
        let inradius = reach * (std::f32::consts::PI / 7.0).cos();
        (ring[1] / inradius).atan().to_degrees()
    }

    #[test]
    fn a_narrowed_hull_keeps_its_bottom_cones_slope_its_height_and_its_feet() {
        let authored = player_hull();
        let mut narrowed = authored.clone();
        narrow_points(&mut narrowed, [0.0; 3], [0.5, 1.0, 0.5]);
        let authored_lean = bottom_face_lean_deg(&authored);
        assert!(authored_lean < 50.0, "{authored_lean}");
        assert!((bottom_face_lean_deg(&narrowed) - authored_lean).abs() < 1e-3);
        // The feet, the top, and the head's ring stay where they were; the hull is half as wide.
        assert_eq!(narrowed[0], [0.0, 1.7, 0.0]);
        assert_eq!(*narrowed.last().unwrap(), [0.0, 0.0, 0.0]);
        assert!((narrowed[1][1] - 1.25).abs() < 1e-6);
        assert!((narrowed[1][0] - 0.225).abs() < 1e-6);
        // The bottom ring comes down to half its height.
        assert!((narrowed[2][1] - 0.225).abs() < 1e-6);
        let authored_steps = step_profile(&authored, 0.01).unwrap();
        let narrowed_steps = step_profile(&narrowed, 0.01).unwrap();
        assert!((authored_steps.cone_height_m - 0.45).abs() < 1e-5);
        assert!((authored_steps.half_width_m - 0.46).abs() < 1e-5);
        assert!((narrowed_steps.cone_height_m - 0.225).abs() < 1e-5);
        assert!((narrowed_steps.half_width_m - 0.235).abs() < 1e-5);
    }

    #[test]
    fn scaled_across_the_ground_alone_the_bottom_cone_is_a_wall() {
        let mut stretched = player_hull();
        for p in &mut stretched {
            p[0] *= 0.5;
            p[2] *= 0.5;
        }
        assert!(bottom_face_lean_deg(&stretched) > 60.0);
    }
}
