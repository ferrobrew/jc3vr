//! Native leaf inventory and status-checked GSK queries for authored-shape restoration.

use std::{mem::MaybeUninit, ptr::NonNull};

use glam::{Mat3, Vec3};
use jc3gi::{
    havok::{
        getSubshape, hkAabb, hkTransformf, hkVector4f, hkcdGskCache, hkcdGskGetClosestPoint,
        hkcdGskGetClosestPointInput, hkcdGskGetClosestPointOutput, hkcdGskGetClosestPointStatus,
        hknpGetShapeKeysConfig, hknpInplaceTriangleShape, hknpShape, hknpShapeCollector,
        hknpShapeType,
    },
    types::vector_math::AlignedVector3,
};

use crate::controllers::physical_hands::human_hull::{
    ClearanceUnavailableReason,
    geometry::{self, Bounds},
};

#[derive(Debug, Copy, Clone)]
pub(super) struct Contact {
    gap: f32,
    normal: Vec3,
}

pub(super) struct Budget {
    remaining: usize,
}

#[derive(Copy, Clone)]
struct QueryHull<'a> {
    proposed: NonNull<hknpShape>,
    current: NonNull<hknpShape>,
    transform: &'a hkTransformf,
}

impl Budget {
    pub(super) fn new() -> Self {
        Self {
            remaining: MAXIMUM_LEAF_WORK,
        }
    }

    fn take(&mut self) -> Result<(), ClearanceUnavailableReason> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(ClearanceUnavailableReason::BudgetExhausted)?;
        Ok(())
    }
}

/// Both native shapes and transforms stay alive under the pre-simulation world authority.
/// Every leaf is checked; no collector-hit count is interpreted as successful clearance.
pub(super) unsafe fn check_target(
    query: NonNull<hknpShape>,
    current: NonNull<hknpShape>,
    query_transform: &hkTransformf,
    target: NonNull<hknpShape>,
    target_transform: &hkTransformf,
    budget: &mut Budget,
) -> Result<(), ClearanceUnavailableReason> {
    validate_transform(query_transform)?;
    validate_transform(target_transform)?;
    let hull = QueryHull {
        proposed: query,
        current,
        transform: query_transform,
    };
    if unsafe { target.as_ref() }.m_flags & hknpShape::IS_CONVEX_SHAPE != 0 {
        return unsafe { check_leaf(hull, target, target_transform, [1.0; 3], budget) };
    }
    let kind = unsafe { target.as_ref().getType() };
    require_composite_kind(kind)?;
    let mut config: hknpGetShapeKeysConfig = unsafe { std::mem::zeroed() };
    config.m_parentKey = hknpShape::INVALID_SHAPE_KEY;
    config.m_parentSize = 0;
    config.m_mask = std::ptr::null();
    config.m_levelOfDetail = hknpGetShapeKeysConfig::MOST_DETAILED;
    let mut previous = hknpShape::INVALID_SHAPE_KEY;
    let mut seen = std::collections::HashSet::new();
    let mut triangle = MaybeUninit::<hknpInplaceTriangleShape>::zeroed();
    unsafe { (*triangle.as_mut_ptr()).Construct(false) };
    let mut collector = MaybeUninit::<hknpShapeCollector>::zeroed();
    let collector = unsafe { collector.assume_init_mut() };
    collector.m_triangleShapePrototype = triangle.as_mut_ptr().cast();
    loop {
        budget.take()?;
        let mut keys = [hknpShape::INVALID_SHAPE_KEY; KEY_PAGE];
        let count = unsafe {
            target
                .as_ref()
                .getShapeKeys(keys.as_mut_ptr(), KEY_PAGE as i32, previous, &config)
        };
        let count = accepted_page(count, keys.len(), seen.is_empty())?;
        if count == 0 {
            return Ok(());
        }
        for &key in &keys[..count] {
            budget.take()?;
            if key == hknpShape::INVALID_SHAPE_KEY || !seen.insert(key) {
                return Err(ClearanceUnavailableReason::QueryIncomplete);
            }
            unsafe { collector.reset(target_transform, 1) };
            let leaf =
                NonNull::new(unsafe { getSubshape(target.as_ptr(), key, collector) }.cast_mut())
                    .ok_or(ClearanceUnavailableReason::QueryIncomplete)?;
            if collector.m_shapeOut != leaf.as_ptr() {
                return Err(ClearanceUnavailableReason::QueryIncomplete);
            }
            unsafe {
                check_leaf(
                    hull,
                    leaf,
                    &collector.m_transformOut,
                    collector.m_scaleOut.data[..3].try_into().unwrap(),
                    budget,
                )?
            };
        }
        previous = keys[count - 1];
    }
}

pub(super) unsafe fn world_bounds(
    shape: NonNull<hknpShape>,
    transform: &hkTransformf,
) -> Result<Bounds, ClearanceUnavailableReason> {
    validate_transform(transform)?;
    let mut aabb = hkAabb::default();
    unsafe { shape.as_ref().calcAabb(transform, &mut aabb) };
    let bounds = Bounds {
        min: aabb.m_min.data,
        max: aabb.m_max.data,
    };
    if valid_bounds(bounds) {
        Ok(bounds)
    } else {
        Err(ClearanceUnavailableReason::InvalidBounds)
    }
}

pub(super) fn valid_bounds(bounds: Bounds) -> bool {
    (0..3).all(|axis| {
        bounds.min[axis].is_finite()
            && bounds.max[axis].is_finite()
            && bounds.min[axis] <= bounds.max[axis]
    })
}

pub(super) fn intersects(a: Bounds, b: Bounds) -> bool {
    (0..3).all(|axis| {
        a.min[axis] <= b.max[axis] + CLEARANCE_MARGIN_M
            && b.min[axis] <= a.max[axis] + CLEARANCE_MARGIN_M
    })
}

unsafe fn check_leaf(
    hull: QueryHull<'_>,
    leaf: NonNull<hknpShape>,
    leaf_transform: &hkTransformf,
    scale: [f32; 3],
    budget: &mut Budget,
) -> Result<(), ClearanceUnavailableReason> {
    budget.take()?;
    // Scaled collector leaves require their scale-mode contract, not unscaled GSK geometry.
    if scale
        .iter()
        .any(|value| !value.is_finite() || (*value - 1.0).abs() > SCALE_TOLERANCE)
    {
        return Err(ClearanceUnavailableReason::UnsupportedShape);
    }
    let proposed_bounds = unsafe { world_bounds(hull.proposed, hull.transform)? };
    let leaf_bounds = unsafe { world_bounds(leaf, leaf_transform)? };
    if !intersects(proposed_bounds, leaf_bounds) {
        return Ok(());
    }
    let proposed_contact =
        unsafe { distance(hull.proposed, hull.transform, leaf, leaf_transform)? };
    if proposed_contact.gap > CLEARANCE_MARGIN_M {
        return Ok(());
    }
    let existing_contact = unsafe { distance(hull.current, hull.transform, leaf, leaf_transform)? };
    compare_contacts(proposed_contact, existing_contact)
}

fn accepted_page(
    count: i32,
    capacity: usize,
    first: bool,
) -> Result<usize, ClearanceUnavailableReason> {
    let count = usize::try_from(count).map_err(|_| ClearanceUnavailableReason::QueryIncomplete)?;
    if count > capacity {
        Err(ClearanceUnavailableReason::QueryIncomplete)
    } else if first && count == 0 {
        Err(ClearanceUnavailableReason::UnsupportedShape)
    } else {
        Ok(count)
    }
}

fn require_composite_kind(kind: u32) -> Result<(), ClearanceUnavailableReason> {
    if kind == hknpShapeType::COMPRESSED_MESH as u32
        || kind == hknpShapeType::COMPRESSED_HEIGHT_FIELD as u32
    {
        Ok(())
    } else {
        Err(ClearanceUnavailableReason::UnsupportedShape)
    }
}

fn compare_contacts(
    proposed: Contact,
    existing: Contact,
) -> Result<(), ClearanceUnavailableReason> {
    // Width changes cannot justify new support penetration, nor excuse a wall contact.
    let unchanged_support = proposed.normal.y >= MINIMUM_SUPPORT_NORMAL_Y
        && existing.normal.y >= MINIMUM_SUPPORT_NORMAL_Y
        && proposed.normal.dot(existing.normal) >= MINIMUM_NORMAL_AGREEMENT
        && proposed.gap >= -MAXIMUM_EXISTING_SUPPORT_DEPTH_M
        && existing.gap >= -MAXIMUM_EXISTING_SUPPORT_DEPTH_M
        && existing.gap <= CLEARANCE_MARGIN_M
        && (proposed.gap - existing.gap).abs() <= CONTACT_DISTANCE_TOLERANCE_M;
    if unchanged_support {
        Ok(())
    } else {
        Err(ClearanceUnavailableReason::Obstructed)
    }
}

unsafe fn distance(
    a: NonNull<hknpShape>,
    transform_a: &hkTransformf,
    b: NonNull<hknpShape>,
    transform_b: &hkTransformf,
) -> Result<Contact, ClearanceUnavailableReason> {
    let vertices_a = unsafe { geometry::vertices(a) }
        .map_err(|_| ClearanceUnavailableReason::UnsupportedShape)?;
    let vertices_b = unsafe { geometry::vertices(b) }
        .map_err(|_| ClearanceUnavailableReason::UnsupportedShape)?;
    let rotation_a = rotation(transform_a);
    let rotation_b = rotation(transform_b);
    let a_from_b = rotation_a.transpose() * rotation_b;
    let translation =
        rotation_a.transpose() * (translation(transform_b) - translation(transform_a));
    let mut input: hkcdGskGetClosestPointInput = unsafe { std::mem::zeroed() };
    input.m_aTb = hkTransformf {
        m_rotation: a_from_b
            .to_cols_array_2d()
            .map(|data| AlignedVector3 { data, _w: 0.0 }),
        m_translation: AlignedVector3 {
            data: translation.to_array(),
            _w: 0.0,
        },
    };
    input.m_handlePenetration = hkcdGskGetClosestPointInput::HANDLE_PENETRATION;
    input.m_coreCollisionTolerance = hkVector4f {
        data: [f32::MAX; 4],
    };
    input.m_terminationGrowthRate = hkcdGskGetClosestPointInput::DEFAULT_TERMINATION_GROWTH_RATE;
    invoke_distance(
        &input,
        rotation_a,
        unsafe { a.as_ref() }.m_convexRadius,
        unsafe { b.as_ref() }.m_convexRadius,
        |cache, output| {
            Ok(unsafe {
                hkcdGskGetClosestPoint(
                    vertices_a.as_ptr(),
                    vertices_a.len() as i32,
                    vertices_b.as_ptr(),
                    vertices_b.len() as i32,
                    &input,
                    cache,
                    output,
                )
            })
        },
    )
}

fn invoke_distance(
    input: &hkcdGskGetClosestPointInput,
    rotation_a: Mat3,
    radius_a: f32,
    radius_b: f32,
    call: impl FnOnce(
        &mut hkcdGskCache,
        &mut hkcdGskGetClosestPointOutput,
    ) -> Result<i32, ClearanceUnavailableReason>,
) -> Result<Contact, ClearanceUnavailableReason> {
    if input.m_handlePenetration != hkcdGskGetClosestPointInput::HANDLE_PENETRATION {
        return Err(ClearanceUnavailableReason::QueryRejected);
    }
    let mut cache = hkcdGskCache {
        m_vertexIds: [0; 4],
        m_dimAb: hkcdGskCache::INITIAL_DIMENSION,
    };
    let mut output = hkcdGskGetClosestPointOutput {
        m_distance: hkVector4f {
            data: [f32::NAN; 4],
        },
        m_normalInA: hkVector4f {
            data: [f32::NAN; 4],
        },
        m_pointAinA: hkVector4f {
            data: [f32::NAN; 4],
        },
    };
    let status = call(&mut cache, &mut output)?;
    accepted_output(status, &output, rotation_a, radius_a, radius_b)
}

fn accepted_output(
    status: i32,
    output: &hkcdGskGetClosestPointOutput,
    rotation_a: Mat3,
    radius_a: f32,
    radius_b: f32,
) -> Result<Contact, ClearanceUnavailableReason> {
    if status != hkcdGskGetClosestPointStatus::STATUS_OK_NOT_PENTRATING as i32
        && status != hkcdGskGetClosestPointStatus::STATUS_OK_PENETRATING as i32
    {
        return Err(ClearanceUnavailableReason::QueryRejected);
    }
    let distance = output.m_distance.data[0];
    let normal = rotation_a * Vec3::from_array(output.m_normalInA.data[..3].try_into().unwrap());
    if !distance.is_finite()
        || !normal.is_finite()
        || (normal.length_squared() - 1.0).abs() > 1.0e-3
        || !output.m_pointAinA.data[..3]
            .iter()
            .all(|value| value.is_finite())
        || !radius_a.is_finite()
        || !radius_b.is_finite()
        || radius_a < 0.0
        || radius_b < 0.0
    {
        return Err(ClearanceUnavailableReason::QueryRejected);
    }
    let gap = distance - radius_a - radius_b;
    if gap.is_finite() {
        Ok(Contact { gap, normal })
    } else {
        Err(ClearanceUnavailableReason::QueryRejected)
    }
}

fn rotation(transform: &hkTransformf) -> Mat3 {
    Mat3::from_cols_array_2d(&transform.m_rotation.each_ref().map(|column| column.data))
}

fn translation(transform: &hkTransformf) -> Vec3 {
    Vec3::from_array(transform.m_translation.data)
}

fn validate_transform(transform: &hkTransformf) -> Result<(), ClearanceUnavailableReason> {
    let rotation = rotation(transform);
    if !translation(transform).is_finite()
        || !rotation.is_finite()
        || (rotation.determinant() - 1.0).abs() > 1.0e-3
        || !(rotation.transpose() * rotation).abs_diff_eq(Mat3::IDENTITY, 1.0e-3)
    {
        Err(ClearanceUnavailableReason::InvalidBounds)
    } else {
        Ok(())
    }
}

const KEY_PAGE: usize = 64;
const MAXIMUM_LEAF_WORK: usize = 1_000_000;
const CLEARANCE_MARGIN_M: f32 = 1.0e-3;
const CONTACT_DISTANCE_TOLERANCE_M: f32 = 2.0e-4;
const MAXIMUM_EXISTING_SUPPORT_DEPTH_M: f32 = 0.02;
const MINIMUM_SUPPORT_NORMAL_Y: f32 = 0.999;
const MINIMUM_NORMAL_AGREEMENT: f32 = 0.9999;
const SCALE_TOLERANCE: f32 = 1.0e-5;

#[cfg(test)]
mod tests;
