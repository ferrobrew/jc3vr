//! Scans of the physics world's bodies by their broad-phase boxes: a body's motion kind from its
//! slot, its box restored to game space, and the walks over the slots that find the dynamic bodies
//! whose boxes a test accepts.
//!
//! A body is in the simulation when its slot holds a static or dynamic body
//! ([`hknpBody::IS_STATIC`], [`hknpBody::IS_DYNAMIC`]) with a motion and a broad-phase entry
//! ([`hknpBody::m_broadPhaseId`]). Its box is the broad phase's ([`hknpBody::m_aabb`]), restored
//! through the world's integer space ([`hknpWorld::m_intSpaceUtil`]), which is cheap enough to read
//! for every slot. A walk of every slot up to the world's peak streams the whole body array, so a
//! caller that scans every step walks a slice per step ([`walk_dynamic`]).

use glam::Vec3;
use jc3gi::physics::{hknpBody, hknpWorld};

use crate::controllers::physics::{error::PhysicsError, world::World};

/// A box with its least and greatest corners.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn centre(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn translated(&self, by: Vec3) -> Aabb {
        Aabb {
            min: self.min + by,
            max: self.max + by,
        }
    }

    /// How far `point` is from the box: zero inside it.
    pub fn distance(&self, point: Vec3) -> f32 {
        (self.min - point)
            .max(point - self.max)
            .max(Vec3::ZERO)
            .length()
    }

    /// The box's volume, cubic metres.
    pub fn volume(&self) -> f32 {
        (self.max - self.min).max(Vec3::ZERO).element_product()
    }

    /// Where the ray from `origin` along the unit `direction` enters the box, metres along it,
    /// within `max_m`: zero when it starts inside, `None` when it misses.
    pub fn ray_entry(&self, origin: Vec3, direction: Vec3, max_m: f32) -> Option<f32> {
        let mut near = 0.0f32;
        let mut far = max_m;
        for axis in 0..3 {
            let (o, d) = (origin[axis], direction[axis]);
            let (lo, hi) = (self.min[axis], self.max[axis]);
            if d.abs() < 1e-9 {
                if o < lo || o > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo - o) / d, (hi - o) / d);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
        Some(near)
    }

    /// The box's twelve edges.
    pub fn edges(&self) -> [(Vec3, Vec3); 12] {
        let corner = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            )
        };
        BOX_EDGES.map(|(a, b)| (corner(a), corner(b)))
    }
}

/// A box quantized into the world's integer space, restored as the engine restores it
/// ([`hknpWorld::getBodyAabb`]): each corner times the inverse scale, less the low offset.
pub fn restore_aabb(min: [u16; 3], max: [u16; 3], scale_inv: Vec3, offset_low: Vec3) -> Aabb {
    let restore = |q: [u16; 3]| Vec3::from(q.map(f32::from)) * scale_inv - offset_low;
    Aabb {
        min: restore(min),
        max: restore(max),
    }
}

/// A body's motion, from its flags.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MotionKind {
    Static,
    Dynamic,
    Keyframed,
}

impl MotionKind {
    pub const fn label(self) -> &'static str {
        match self {
            MotionKind::Static => "static",
            MotionKind::Dynamic => "dynamic",
            MotionKind::Keyframed => "keyframed",
        }
    }
}

/// The motion of a slot holding a body in the simulation, or `None` for a freed slot, an empty
/// one, or a body out of the simulation.
pub fn motion_kind(body: &hknpBody) -> Option<MotionKind> {
    let flags = body.m_flags;
    if body.m_motionId == hknpBody::INVALID_MOTION_ID
        || flags & (hknpBody::IS_STATIC | hknpBody::IS_DYNAMIC) == 0
        || body.m_broadPhaseId == hknpBody::INVALID_BROAD_PHASE_ID
    {
        return None;
    }
    Some(if flags & hknpBody::IS_STATIC != 0 {
        MotionKind::Static
    } else if flags & hknpBody::IS_KEYFRAMED != 0 {
        MotionKind::Keyframed
    } else {
        MotionKind::Dynamic
    })
}

/// The world's integer space, which the broad phase's boxes are quantized into.
pub struct Space {
    scale_inv: Vec3,
    offset_low: Vec3,
}

impl Space {
    pub fn of(world: &hknpWorld) -> Self {
        let space = &world.m_intSpaceUtil;
        Self {
            scale_inv: Vec3::from_array(space.m_bitScaleInv.data),
            offset_low: Vec3::from_array(space.m_bitOffsetLow.data),
        }
    }

    /// A body's box, game space, for the world's shift `shift`.
    pub fn restore(&self, body: &hknpBody, shift: Vec3) -> Aabb {
        restore_aabb(
            body.m_aabb.m_min,
            body.m_aabb.m_max,
            self.scale_inv,
            self.offset_low,
        )
        .translated(shift)
    }
}

/// Where a slice of a walk ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Slice {
    /// The slots the slice went through.
    pub walked: u32,
    /// The world's peak slot.
    pub peak: u32,
    /// The slot the next slice starts at.
    pub next: u32,
}

/// The ids and boxes of the dynamic bodies in the simulation whose boxes `keep` accepts, at most
/// `max_bodies` of them in id order, without reading their shapes. [`PhysicsError::NoBody`] while
/// the world has no body array.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn dynamic_boxes(
    mut keep: impl FnMut(&Aabb) -> bool,
    max_bodies: usize,
) -> Result<Vec<(u32, Aabb)>, PhysicsError> {
    let world = unsafe { World::require() }?;
    let w = world.world();
    if w.m_bodies.is_null() {
        return Err(PhysicsError::NoBody);
    }
    let space = Space::of(w);
    let mut kept = Vec::new();
    for id in 0..w.m_peakBodyIndex.saturating_add(1) {
        if kept.len() >= max_bodies {
            break;
        }
        // SAFETY: every id up to the peak has a slot.
        let slot = unsafe { &*w.m_bodies.add(id as usize) };
        if motion_kind(slot) != Some(MotionKind::Dynamic) {
            continue;
        }
        let aabb = space.restore(slot, world.shift);
        if keep(&aabb) {
            kept.push((id, aabb));
        }
    }
    Ok(kept)
}

/// Walk up to `count` slots from `start`, wrapping at the world's peak, and hand `keep` each
/// dynamic body in the simulation with its box, game space. [`PhysicsError::NoBody`] while the
/// world has no body array.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn walk_dynamic(
    start: u32,
    count: u32,
    mut keep: impl FnMut(u32, &Aabb),
) -> Result<Slice, PhysicsError> {
    let world = unsafe { World::require() }?;
    let w = world.world();
    if w.m_bodies.is_null() {
        return Err(PhysicsError::NoBody);
    }
    let space = Space::of(w);
    let slots = w.m_peakBodyIndex.saturating_add(1);
    let count = count.min(slots);
    let mut id = if start < slots { start } else { 0 };
    for _ in 0..count {
        // SAFETY: every id up to the peak has a slot.
        let slot = unsafe { &*w.m_bodies.add(id as usize) };
        if motion_kind(slot) == Some(MotionKind::Dynamic) {
            keep(id, &space.restore(slot, world.shift));
        }
        id += 1;
        if id >= slots {
            id = 0;
        }
    }
    Ok(Slice {
        walked: count,
        peak: w.m_peakBodyIndex,
        next: id,
    })
}

/// The corner pairs of a box's edges, corners numbered by their max-corner bits (x 1, y 2, z 4).
const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quantized_box_is_restored_by_the_inverse_scale_less_the_low_offset() {
        let aabb = restore_aabb(
            [100, 200, 300],
            [110, 220, 330],
            Vec3::splat(0.5),
            Vec3::new(10.0, 20.0, 30.0),
        );
        assert_eq!(aabb.min, Vec3::new(40.0, 80.0, 120.0));
        assert_eq!(aabb.max, Vec3::new(45.0, 90.0, 135.0));
    }

    #[test]
    fn a_point_is_as_far_from_a_box_as_from_its_nearest_point() {
        let aabb = Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        assert_eq!(aabb.distance(Vec3::splat(0.5)), 0.0);
        assert_eq!(aabb.distance(Vec3::new(3.0, 0.5, 0.5)), 2.0);
        assert!((aabb.distance(Vec3::new(2.0, 2.0, 0.5)) - 2f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn a_ray_enters_a_box_where_it_crosses_its_nearest_face() {
        let aabb = Aabb {
            min: Vec3::new(-1.0, -1.0, 4.0),
            max: Vec3::new(1.0, 1.0, 6.0),
        };
        assert_eq!(aabb.ray_entry(Vec3::ZERO, Vec3::Z, 10.0), Some(4.0));
        assert_eq!(aabb.ray_entry(Vec3::ZERO, Vec3::Z, 3.0), None);
        assert_eq!(aabb.ray_entry(Vec3::ZERO, Vec3::NEG_Z, 10.0), None);
        assert_eq!(
            aabb.ray_entry(Vec3::new(2.0, 0.0, 0.0), Vec3::Z, 10.0),
            None
        );
        assert_eq!(
            aabb.ray_entry(Vec3::new(0.0, 0.0, 5.0), Vec3::X, 10.0),
            Some(0.0)
        );
    }

    #[test]
    fn a_box_has_twelve_distinct_edges_along_the_axes() {
        let aabb = Aabb {
            min: Vec3::ZERO,
            max: Vec3::new(1.0, 2.0, 3.0),
        };
        let edges = aabb.edges();
        for (a, b) in edges {
            let d = (b - a).abs();
            assert_eq!(
                [d.x, d.y, d.z].iter().filter(|&&c| c > 0.0).count(),
                1,
                "{a} {b}"
            );
        }
        let total: f32 = edges.iter().map(|(a, b)| a.distance(*b)).sum();
        assert_eq!(total, 4.0 * (1.0 + 2.0 + 3.0));
    }
}
