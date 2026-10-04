//! A scene raycast that ignores the player: the engine's closest-hit ray query
//! ([`PhysicsSystem::CastRay`]) with the player aim's own filter ([`PlayerAimRaycastFilter`]),
//! built the way the player aim's raycast builds it.
//!
//! The filter rejects the ignored objects' every physics instance (the character's capsule, its
//! ragdoll bodies and hit proxies, and the weapons the character uses), which an instance-only
//! filter would not. It accepts the same instance kinds the game's aim ray does.
//!
//! Call it only from the simulation phase outside the physics step: the query reads the live Havok
//! world. The player's post-camera aim update ([`crate::controllers::aim`]) qualifies.

use std::{fmt, mem::MaybeUninit, num::NonZeroUsize, ptr};

use glam::Vec3;
use jc3gi::{
    aim::aim::PlayerAimRaycastFilter,
    physics::{
        CastRayInfo, CastRayInfoBase, MaterialPassFlags, PhysicsSystem, Ray3, RaycastFilter,
    },
    types::math::Vector3,
};

use crate::controllers::aim::ray::Ray;

/// What a cast should pass through.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CastOptions {
    /// Game objects never hit (primary object pointers, such as a `CCharacter`), or zero.
    pub ignore: [usize; 2],
    /// Pass through the materials bullets and rays pass through (foliage, fences, glass), as the
    /// game's aim ray does.
    pub see_through: bool,
}

/// The closest hit of a cast.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RayHit {
    pub position: Vec3,
    pub normal: Vec3,
    /// The distance from the ray's origin, metres.
    pub distance: f32,
    /// The hit game object's address, or `None` for world geometry.
    pub object: Option<NonZeroUsize>,
    /// The Havok body id of the hit body.
    pub body: u32,
}

/// Why a cast could not run.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RaycastError {
    /// The physics system does not exist (outside a loaded world).
    NoPhysicsSystem,
    /// The requested length is not a positive, finite distance.
    InvalidDistance,
}

impl fmt::Display for RaycastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RaycastError::NoPhysicsSystem => {
                write!(f, "raycast: the physics system does not exist")
            }
            RaycastError::InvalidDistance => {
                write!(f, "raycast: the ray length is not a positive distance")
            }
        }
    }
}

impl std::error::Error for RaycastError {}

/// Cast `ray` out to `max_distance` metres and return the closest hit that `options` does not
/// exclude, or `None` for a miss.
///
/// # Safety
///
/// Must run on the simulation thread outside the physics step, and every object in
/// `options.ignore` must be zero or a live game object.
pub unsafe fn cast(
    ray: &Ray,
    max_distance: f32,
    options: &CastOptions,
) -> Result<Option<RayHit>, RaycastError> {
    if !(max_distance.is_finite() && max_distance > 0.0) {
        return Err(RaycastError::InvalidDistance);
    }
    let physics = unsafe { PhysicsSystem::get() }.ok_or(RaycastError::NoPhysicsSystem)?;

    // Both are plain stack objects in the engine too: nothing they hold is heap-owned, so neither
    // needs a destructor call. Every field is a pointer, an integer, or a float, so all-zero is a
    // valid value to construct over.
    let mut filter = unsafe { MaybeUninit::<PlayerAimRaycastFilter>::zeroed().assume_init() };
    unsafe {
        filter.ConstructPlayerAim(
            MaterialPassFlags::empty(),
            MaterialPassFlags::empty(),
            ptr::null_mut(),
        );
    }
    filter.m_IgnoreObject = options.ignore.map(|object| object as *mut _);
    if options.see_through {
        unsafe {
            filter.base.SetIgnoreBulletPass(true);
            filter.base.SetIgnoreRayPass(true);
        }
    }

    let mut info = unsafe { MaybeUninit::<CastRayInfo>::zeroed().assume_init() };
    info.base.__vftable = CastRayInfoBase::VFTABLE;
    info.base.m_HitObjectId = 0;
    unsafe { info.base.Reset() };
    info.base.__vftable = CastRayInfo::VFTABLE;
    info.m_GameObject = ptr::null_mut();

    let query = Ray3 {
        Origin: vector(ray.origin),
        Direction: vector(ray.direction),
    };
    let hit = unsafe {
        physics.CastRay(
            &query,
            0.0,
            max_distance,
            &mut info.base,
            (&raw mut filter).cast::<RaycastFilter>(),
            0,
            PhysicsSystem::QUERY_LAYER,
        )
    };
    let fraction = info.base.m_HitDistance;
    if !hit || !(0.0..1.0).contains(&fraction) {
        return Ok(None);
    }
    // With a zero start distance the fraction is of the whole length.
    let distance = fraction * max_distance;
    Ok(Some(RayHit {
        position: ray.at(distance),
        normal: Vec3::from_array(info.base.m_Normal.data),
        distance,
        object: NonZeroUsize::new(info.m_GameObject as usize),
        body: info.base.m_BodyId,
    }))
}

/// Whether the line from `origin` to `end` is clear, allowing a hit within `tolerance` metres
/// short of `end`: the line to a target's body position ends inside the target's own body. A failed
/// cast counts as blocked.
///
/// # Safety
///
/// As [`cast`].
pub unsafe fn line_of_sight(
    origin: Vec3,
    end: Vec3,
    tolerance: f32,
    options: &CastOptions,
) -> bool {
    let offset = end - origin;
    let distance = offset.length();
    let Some(line) = Ray::new(origin, offset) else {
        return false;
    };
    match unsafe { cast(&line, distance, options) } {
        Ok(Some(hit)) => hit.distance >= distance - tolerance.max(0.0),
        Ok(None) => true,
        Err(_) => false,
    }
}

fn vector(v: Vec3) -> Vector3 {
    Vector3 { data: v.to_array() }
}
