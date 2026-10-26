//! The collider view's read of the physics world: every body slot up to the world's peak body id,
//! the bodies in the simulation whose boxes come within the radius, and for the nearest of them
//! their frame, kind, mass, and shape; the mod's joints; and the player's ground.
//!
//! A body is in the simulation when its slot holds a static or dynamic body
//! ([`hknpBody::IS_STATIC`], [`hknpBody::IS_DYNAMIC`]) with a motion and a broad-phase entry
//! ([`hknpBody::m_broadPhaseId`]). Its box is the broad phase's ([`hknpBody::m_aabb`]), restored
//! through the world's integer space ([`hknpWorld::m_intSpaceUtil`]), which is cheap enough to read
//! for every slot. Its shape's kind is the shape's own answer ([`hknpShape::getType`]); the convex
//! kinds are read from their relative arrays ([`hknpConvexShape::m_vertices`],
//! [`hknpConvexPolytopeShape::m_faces`]), and every other kind is drawn by its box.

use std::fmt;

use glam::{Vec3, Vec3A};
use jc3gi::{
    character::character::Character,
    havok::{
        hkRelArray, hknpCapsuleShape, hknpConvexPolytopeFace, hknpConvexPolytopeShape,
        hknpConvexShape, hknpShape, hknpShapeType,
    },
    physics::{
        CustomGroupCollisionFilter, PfxCharacterInstance, PfxLayer, hknpBody,
        hknpCharacterSurfaceInfo, hknpConstraint, hknpWorld,
    },
    types::vector_math::AlignedVector3,
};

use crate::controllers::physical_hands::{
    collider_view::{
        config::ColliderViewConfig,
        model::{
            Aabb, Body, Geometry, Ground, Joint, JointKind, KindInputs, Layer, ModBody, MotionKind,
            Snapshot, capsule_radius, classify, hull_edges, nearest, restore_aabb,
        },
    },
    physics::havok_affine,
    world::World,
};

/// The mod's own bodies and joints, by id, as their readouts publish them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ModIds {
    pub(super) hands: Vec<u32>,
    pub(super) guns: Vec<u32>,
    /// Each joint's kind and constraint id.
    pub(super) joints: Vec<(JointKind, u32)>,
}

/// Why the world could not be read.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScanError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// The world has no body array.
    NoBodies,
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanError::NoPhysicsWorld => {
                write!(f, "collider view: the physics world does not exist")
            }
            ScanError::NoBodies => write!(f, "collider view: the world has no body array"),
        }
    }
}

impl std::error::Error for ScanError {}

/// Read the bodies within the configured radius of `centre` (game space).
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(super) unsafe fn scan(
    centre: Vec3,
    config: &ColliderViewConfig,
    mods: &ModIds,
) -> Result<Snapshot, ScanError> {
    let world = unsafe { World::get() }.ok_or(ScanError::NoPhysicsWorld)?;
    let shift = world.shift;
    let w = world.world();
    if w.m_bodies.is_null() {
        return Err(ScanError::NoBodies);
    }
    let space = Space::of(w);
    let player = unsafe { player_proxy() };
    let ground = player.and_then(|p| p.ground);
    let radius = config.radius_m.max(0.0);

    let slots = w.m_peakBodyIndex.saturating_add(1);
    let mut candidates = Vec::new();
    for id in 0..slots {
        // SAFETY: every id up to the peak has a slot.
        let slot = unsafe { &*w.m_bodies.add(id as usize) };
        let Some(motion) = motion_kind(slot) else {
            continue;
        };
        if motion == MotionKind::Static
            && !config.static_bodies
            && ground.is_none_or(|g| g.body != id)
        {
            continue;
        }
        let aabb = space.restore(slot, shift);
        candidates.push((aabb.distance(centre), id));
    }
    let (kept, in_range) = nearest(candidates, radius, config.max_bodies as usize);

    let bodies = kept
        .into_iter()
        .filter_map(|(distance_m, id)| {
            // SAFETY: as above; the slot held a body a moment ago on this thread.
            let slot = unsafe { &*w.m_bodies.add(id as usize) };
            let motion = motion_kind(slot)?;
            let mod_body = if mods.hands.contains(&id) {
                Some(ModBody::Hand)
            } else if mods.guns.contains(&id) {
                Some(ModBody::Gun)
            } else {
                None
            };
            let kind = classify(KindInputs {
                motion,
                layer: layer(slot.m_collisionFilterInfo),
                mod_body,
                player: player.is_some_and(|p| p.body == id),
            });
            let mut transform = havok_affine(&slot.m_transform);
            transform.translation += Vec3A::from(shift);
            Some(Body {
                id,
                shape_key: slot.m_shape as usize,
                kind,
                motion,
                filter_info: slot.m_collisionFilterInfo,
                transform,
                aabb: space.restore(slot, shift),
                // SAFETY: the motion array holds the motion of every body in the simulation.
                mass_kg: unsafe { mass(w, slot, motion) },
                // SAFETY: the body holds a reference to its shape.
                geometry: unsafe { geometry(slot.m_shape) },
                distance_m,
            })
        })
        .collect();

    // SAFETY: forwarded from the caller.
    let joints = mods
        .joints
        .iter()
        .filter_map(|&(kind, constraint)| unsafe { joint(w, kind, constraint) })
        .collect();

    Ok(Snapshot {
        bodies,
        joints,
        ground,
        scanned: slots,
        in_range,
    })
}

/// Refresh retained bodies at render time without re-reading their shape geometry.
/// Reused or removed body slots are discarded instead of displaying the old body's outline.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(super) unsafe fn refresh(snapshot: &mut Snapshot, centre: Vec3) -> Result<(), ScanError> {
    let world = unsafe { World::get() }.ok_or(ScanError::NoPhysicsWorld)?;
    let w = world.world();
    if w.m_bodies.is_null() {
        return Err(ScanError::NoBodies);
    }
    let space = Space::of(w);
    snapshot.bodies.retain_mut(|body| {
        if body.id > w.m_peakBodyIndex {
            return false;
        }
        // SAFETY: the id is within the world's body array on this thread.
        let slot = unsafe { &*w.m_bodies.add(body.id as usize) };
        if motion_kind(slot).is_none() || slot.m_shape as usize != body.shape_key {
            return false;
        }
        let mut transform = havok_affine(&slot.m_transform);
        transform.translation += Vec3A::from(world.shift);
        body.transform = transform;
        body.aabb = space.restore(slot, world.shift);
        body.distance_m = body.aabb.distance(centre);
        true
    });
    snapshot.ground = unsafe { player_proxy() }.and_then(|player| player.ground);
    Ok(())
}

/// The world's integer space.
struct Space {
    scale_inv: Vec3,
    offset_low: Vec3,
}

impl Space {
    fn of(world: &hknpWorld) -> Self {
        let space = &world.m_intSpaceUtil;
        Self {
            scale_inv: Vec3::from_array(space.m_bitScaleInv.data),
            offset_low: Vec3::from_array(space.m_bitOffsetLow.data),
        }
    }

    /// A body's box, game space.
    fn restore(&self, body: &hknpBody, shift: Vec3) -> Aabb {
        restore_aabb(
            body.m_aabb.m_min,
            body.m_aabb.m_max,
            self.scale_inv,
            self.offset_low,
        )
        .translated(shift)
    }
}

/// The player's proxy body and what it stands on.
#[derive(Debug, Copy, Clone)]
struct PlayerProxy {
    body: u32,
    ground: Option<Ground>,
}

/// The local player's proxy, as its last update left it.
///
/// # Safety
///
/// The game thread, outside the physics step.
unsafe fn player_proxy() -> Option<PlayerProxy> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetCharacterRigidBodyId(&mut body) };
    let info = &proxy.m_GroundInformation;
    let ground = (proxy.m_GroundBodyId != PfxCharacterInstance::INVALID_BODY_ID
        && info.m_supportedState != hknpCharacterSurfaceInfo::UNSUPPORTED)
        .then(|| Ground {
            body: proxy.m_GroundBodyId,
            normal: Vec3::from_array(info.m_surfaceNormal.data),
        });
    Some(PlayerProxy { body, ground })
}

/// The motion of a slot holding a body in the simulation, or `None` for a freed slot, an empty
/// one, or a body out of the simulation.
fn motion_kind(body: &hknpBody) -> Option<MotionKind> {
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

/// The layer of a collision-filter word, as far as the kinds tell layers apart.
fn layer(filter_info: u32) -> Layer {
    let value = filter_info & CustomGroupCollisionFilter::LAYER_MASK;
    let is = |layers: &[u32]| layers.contains(&value);
    if is(&[
        PfxLayer::CHARACTER_COLLISION as u32,
        PfxLayer::CHARACTER_GHOST_MODE as u32,
    ]) {
        Layer::Character
    } else if is(&[
        PfxLayer::CHARACTER_RAGDOLL as u32,
        PfxLayer::CHARACTER_RAGDOLL_KEYFRAMED as u32,
        PfxLayer::CHARACTER_RAGDOLL_PROXIES as u32,
    ]) {
        Layer::Ragdoll
    } else if is(&[
        PfxLayer::VEHICLE_COLLISION_WHEEL as u32,
        PfxLayer::VEHICLE_COLLISION_DYNAMIC_ONLY as u32,
        PfxLayer::VEHICLE_COLLISION as u32,
    ]) {
        Layer::Vehicle
    } else {
        Layer::Other(value)
    }
}

/// The mass of a moving body's motion, kilograms: the inverse of its inverse mass, the last of
/// [`hknpMotion::m_inverseInertia`]'s half-floats. `None` for a static body and an infinite mass.
///
/// [`hknpMotion::m_inverseInertia`]: jc3gi::physics::hknpMotion::m_inverseInertia
unsafe fn mass(world: &hknpWorld, body: &hknpBody, motion: MotionKind) -> Option<f32> {
    if motion == MotionKind::Static || world.m_motions.is_null() {
        return None;
    }
    let motion = unsafe { &*world.m_motions.add(body.m_motionId as usize) };
    let inverse = f32::from_bits(u32::from(motion.m_inverseInertia[3]) << 16);
    (inverse.is_finite() && inverse > 0.0).then(|| 1.0 / inverse)
}

/// A shape's geometry, body space.
///
/// # Safety
///
/// `shape` must be null or a live shape.
unsafe fn geometry(shape: *const hknpShape) -> Geometry {
    let Some(shape) = (unsafe { shape.as_ref() }) else {
        return Geometry::Other(u32::MAX);
    };
    let kind = unsafe { shape.getType() };
    let convex = shape.m_flags & hknpShape::IS_CONVEX_SHAPE != 0;
    let polytope = shape.m_flags & hknpShape::IS_CONVEX_POLYTOPE_SHAPE != 0;
    let as_convex = || unsafe { &*(shape as *const hknpShape).cast::<hknpConvexShape>() };
    let vertices = || -> Option<Vec<Vec3>> {
        let vertices = unsafe { rel::<AlignedVector3>(&as_convex().m_vertices, MAX_VERTICES) }?;
        Some(vertices.iter().map(|v| Vec3::from_array(v.data)).collect())
    };
    match kind {
        k if k == hknpShapeType::SPHERE as u32 && convex => match vertices() {
            Some(v) if !v.is_empty() => Geometry::Sphere {
                centre: v[0],
                radius: shape.m_convexRadius,
            },
            _ => Geometry::Other(kind),
        },
        k if k == hknpShapeType::CAPSULE as u32 && polytope => {
            let capsule = unsafe { &*(shape as *const hknpShape).cast::<hknpCapsuleShape>() };
            Geometry::Capsule {
                a: Vec3::from_array(capsule.m_a.data),
                b: Vec3::from_array(capsule.m_b.data),
                radius: capsule_radius(shape.m_convexRadius, hknpCapsuleShape::RADIUS_IN_CORE),
            }
        }
        k if k == hknpShapeType::CONVEX_POLYTOPE as u32 && polytope => {
            let polytope =
                unsafe { &*(shape as *const hknpShape).cast::<hknpConvexPolytopeShape>() };
            let faces = unsafe { rel::<hknpConvexPolytopeFace>(&polytope.m_faces, MAX_FACES) };
            let indices = unsafe { rel::<u8>(&polytope.m_indices, MAX_INDICES) };
            match (vertices(), faces, indices) {
                (Some(vertices), Some(faces), Some(indices)) => {
                    let runs: Vec<(u16, u8)> = faces
                        .iter()
                        .map(|f| (f.m_firstIndex, f.m_numIndices))
                        .collect();
                    let edges = hull_edges(&runs, indices, vertices.len());
                    Geometry::Hull { vertices, edges }
                }
                _ => Geometry::Other(kind),
            }
        }
        k if k == hknpShapeType::CONVEX as u32 && convex => {
            vertices().map_or(Geometry::Other(kind), Geometry::Points)
        }
        _ => Geometry::Other(kind),
    }
}

/// The elements of a relative array, or `None` when it holds more than `max` or they are
/// misaligned.
///
/// # Safety
///
/// `array` must be a relative array of `T`s inside a live object.
unsafe fn rel<T>(array: &hkRelArray, max: usize) -> Option<&[T]> {
    let count = usize::from(array.m_size);
    if count > max {
        return None;
    }
    if count == 0 {
        return Some(&[]);
    }
    let start = (array as *const hkRelArray)
        .cast::<u8>()
        .wrapping_add(usize::from(array.m_offset))
        .cast::<T>();
    start
        .is_aligned()
        .then(|| unsafe { std::slice::from_raw_parts(start, count) })
}

/// The mod's joint `constraint`, while the world's slot for it holds an enabled constraint.
///
/// # Safety
///
/// The game thread, outside the physics step.
unsafe fn joint(world: &hknpWorld, kind: JointKind, constraint: u32) -> Option<Joint> {
    let manager = &world.m_constraintManager;
    if manager.m_constraintBuffer.is_null() || constraint >= manager.m_capacity {
        return None;
    }
    let slot = unsafe { &*manager.m_constraintBuffer.add(constraint as usize) };
    (slot.m_bodyIdA != hknpConstraint::INVALID_BODY_ID
        && slot.m_flags & hknpConstraint::IS_DISABLED == 0)
        .then_some(Joint {
            kind,
            body_a: slot.m_bodyIdA,
            body_b: slot.m_bodyIdB,
        })
}

/// Bounds past which a relative array is taken as unreadable rather than read.
const MAX_VERTICES: usize = 256;
const MAX_FACES: usize = 256;
const MAX_INDICES: usize = 2048;
