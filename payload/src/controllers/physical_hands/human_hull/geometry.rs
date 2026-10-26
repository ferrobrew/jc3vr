//! Builds a horizontally narrowed copy through Havok's own convex factory.

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

/// The source is a retained native convex shape, and the calling thread has a Havok router.
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
    for vertex in &mut vertices {
        for axis in [0, 2] {
            vertex.data[axis] = center[axis] + (vertex.data[axis] - center[axis]) * scale[axis];
        }
        vertex.data[3] = 0.0;
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
