//! The climbing's engine side: the player's character proxy, its support, and its velocity
//! (`docs/engine/character/hands-locomotion-and-state.md`, "The proxy's support and the fall").
//!
//! The game decides whether the player is on the ground from the proxy's support
//! ([`PfxCharacterInstance::m_GroundInformation`]), which the proxy's update
//! ([`PfxCharacterInstance::UpdateContext`]) finds afresh before each physics step and which the
//! state machines read before the next proxy update. The physics system's pre-simulation
//! callbacks run after every character's proxy update, so a support written there is the one the
//! state machines, the fall bookkeeping, and the ground raycasts see until the next proxy update:
//! the player hanging from the hands stays on foot rather than falling. The proxy's update has
//! already set the proxy's velocity for the step by then, so a velocity set there is the one the
//! step integrates.
//!
//! Every function here runs on the thread that steps the physics world, outside the step, except
//! the reads marked otherwise.

use std::{fmt, ptr};

use glam::{Affine3A, Mat4, Quat, Vec3, Vec3A};
use jc3gi::{
    character::character::Character,
    havok::hkSimdFloat32,
    physics::{PfxCharacterInstance, PfxInstance, hknpBody, hknpCharacterSurfaceInfo, hknpWorld},
    types::vector_math::AlignedVector3,
};

use crate::controllers::physical_hands::{
    physics::{self, havok_affine},
    world::World,
};

/// Why the player could not be climbed with.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ClimbEngineError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// There is no local player character, or it has no proxy or proxy body.
    NoPlayer,
    /// The velocity to set is not finite.
    BadVelocity,
}

impl fmt::Display for ClimbEngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            ClimbEngineError::NoPhysicsWorld => "the physics world does not exist",
            ClimbEngineError::NoPlayer => "the player has no character proxy",
            ClimbEngineError::BadVelocity => "the climbing velocity is not finite",
        };
        write!(f, "hand climbing: {what}")
    }
}

impl std::error::Error for ClimbEngineError {}

/// The player as the climb sees it: the proxy body's centre and the character's root, game space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PlayerPlace {
    pub body: Vec3,
    pub feet: Vec3,
}

/// Where the local player's proxy body and root are.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn player_place() -> Result<PlayerPlace, ClimbEngineError> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .ok_or(ClimbEngineError::NoPlayer)?;
    let proxy =
        unsafe { character.m_PfxCharacter.px.as_ref() }.ok_or(ClimbEngineError::NoPlayer)?;
    let world = unsafe { World::get() }.ok_or(ClimbEngineError::NoPhysicsWorld)?;
    let slot = unsafe { proxy_slot(world.world(), proxy) }?;
    let body = Vec3::from_array(slot.m_transform.m_translation.data) + world.shift;
    let feet = Mat4::from(character.m_WorldMatrixT1).w_axis.truncate();
    if !(body.is_finite() && feet.is_finite()) {
        return Err(ClimbEngineError::NoPlayer);
    }
    Ok(PlayerPlace { body, feet })
}

/// The live metadata of a reached body. Static world geometry may have no engine owner.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct BodyIdentity {
    pub(crate) world: usize,
    pub(crate) bodies: usize,
    pub(crate) instance: usize,
    pub(crate) owner: usize,
    pub(crate) shape: usize,
    pub(crate) motion: u32,
    pub(crate) static_body: bool,
    pub(crate) lifetime: u64,
}

/// The current identity of an added body, checked before reading its engine instance.
///
/// # Safety
///
/// Outside the physics step, while body creation and destruction are not running.
pub unsafe fn body_identity(body: u32) -> Option<BodyIdentity> {
    let world = unsafe { World::get() }?;
    let world = world.world();
    let slot = unsafe { slot(world, body) }?;
    if slot.m_broadPhaseId == hknpBody::INVALID_BROAD_PHASE_ID || slot.m_shape.is_null() {
        return None;
    }
    let buffer = unsafe {
        world
            .m_bodyProperties
            .getWithDefault(hknpWorld::BODY_PROPERTY_PFX_INSTANCE, ptr::null_mut())
            .as_ref()
    }?;
    if body >= buffer.m_capacity || buffer.m_occupancy.is_null() || buffer.m_data.is_null() {
        return None;
    }
    let occupied =
        unsafe { *buffer.m_occupancy.add((body / 32) as usize) } & (1 << (body % 32)) != 0;
    if !occupied {
        return None;
    }
    let instance = unsafe {
        *buffer
            .m_data
            .cast::<*const PfxInstance>()
            .add(body as usize)
    };
    let owner = unsafe { instance.as_ref() }?.m_GameObject as usize;
    let static_body = slot.m_flags & hknpBody::IS_STATIC != 0;
    if owner == 0 && !static_body {
        return None;
    }
    Some(BodyIdentity {
        world: ptr::from_ref(world) as usize,
        bodies: world.m_bodies as usize,
        instance: instance as usize,
        owner,
        shape: slot.m_shape as usize,
        motion: slot.m_motionId,
        static_body,
        lifetime: crate::controllers::physical_hands::climb::lifetime::stamp(
            ptr::from_ref(world) as usize,
            body,
        ),
    })
}

/// Copy ownership only from the serialized input/pre-simulation boundary.
///
/// # Safety
///
/// The game thread outside the world update, or its pre-simulation callback before the step.
pub(crate) unsafe fn gate_context()
-> Option<crate::controllers::physical_hands::climb::native::Context> {
    let character = unsafe { Character::GetLocalPlayerCharacter() };
    let local = unsafe { character.as_ref() }?;
    if !local.m_IsLocalCharacter || local.m_PfxCharacter.px.is_null() {
        return None;
    }
    let world = unsafe { World::get() }?;
    let world = world.world();
    if world.m_bodies.is_null() {
        return None;
    }
    Some(crate::controllers::physical_hands::climb::native::Context {
        character: character as usize,
        proxy: local.m_PfxCharacter.px as usize,
        world: ptr::from_ref(world) as usize,
        bodies: world.m_bodies as usize,
        epoch: crate::controllers::roomscale::teleport::epoch(),
    })
}

/// A body's place and motion, game space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    /// The body's transform.
    pub transform: Affine3A,
    /// Whether the body is static, which never moves.
    pub is_static: bool,
    /// Whether the body is keyframed, moved along a set path.
    pub is_keyframed: bool,
    /// The body's mass, or `None` for an infinite one.
    pub mass_kg: Option<f32>,
    pub centre: Vec3,
    pub linear: Vec3,
    pub angular: Vec3,
}

impl BodyFrame {
    /// The velocity of the body's point at `at`, world space.
    pub fn point_velocity(&self, at: Vec3) -> Vec3 {
        if self.is_static {
            return Vec3::ZERO;
        }
        self.linear + self.angular.cross(at - self.centre)
    }
}

/// The place and motion of the body `body`, or `None` for a freed slot or a motion that is not
/// finite.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn body_frame(body: u32) -> Option<BodyFrame> {
    let world = unsafe { World::get() }?;
    let shift = world.shift;
    let world = world.world();
    let slot = unsafe { slot(world, body) }?;
    let mut transform = havok_affine(&slot.m_transform);
    transform.translation += Vec3A::from(shift);
    let is_static = slot.m_flags & hknpBody::IS_STATIC != 0;
    let is_keyframed = slot.m_flags & hknpBody::IS_KEYFRAMED != 0;
    let (centre, linear, angular, mass_kg) = if is_static || world.m_motions.is_null() {
        (
            Vec3::from(transform.translation),
            Vec3::ZERO,
            Vec3::ZERO,
            None,
        )
    } else {
        let motion = unsafe { &*world.m_motions.add(slot.m_motionId as usize) };
        let orientation = Quat::from_array(motion.m_orientation.data).normalize();
        let inverse_mass = f32::from_bits(u32::from(motion.m_inverseInertia[3]) << 16);
        (
            Vec3::from_array(motion.m_centerOfMass.data) + shift,
            Vec3::from_array(motion.m_linearVelocity.data),
            orientation * Vec3::from_array(motion.m_angularVelocity.data),
            (inverse_mass.is_finite() && inverse_mass > 0.0).then(|| 1.0 / inverse_mass),
        )
    };
    let frame = BodyFrame {
        transform,
        is_static,
        is_keyframed,
        mass_kg,
        centre,
        linear,
        angular,
    };
    (transform.is_finite() && centre.is_finite() && linear.is_finite() && angular.is_finite())
        .then_some(frame)
}

/// Whether a hand climbs the body `frame`: the static world, and with `moving`, a keyframed body
/// or a dynamic one at least `min_mass_kg`.
pub fn climbable(frame: &BodyFrame, moving: bool, min_mass_kg: f32) -> bool {
    frame.is_static
        || (moving
            && (frame.is_keyframed || frame.mass_kg.is_none_or(|m| m >= min_mass_kg.max(0.0))))
}

/// The support the proxy's update found for this step, before anything else wrote it: the upward
/// share of the normal of the surface the proxy stands on, or `None` when it stands on nothing.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the proxy's update.
pub unsafe fn support() -> Option<f32> {
    let proxy = unsafe { proxy() }.ok()?;
    let info = &proxy.m_GroundInformation;
    (info.m_supportedState == hknpCharacterSurfaceInfo::SUPPORTED)
        .then_some(info.m_surfaceNormal.data[1])
        .filter(|y| y.is_finite())
}

/// Hold the player up: give the proxy `velocity` for the coming step, as its own update does, and
/// write it as supported on the body `ground`, level, moving at `surface_velocity`: at rest on the
/// static world, carried with a moving body.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the proxy's update; `ground`
/// must be a body of the world.
pub unsafe fn hold(
    velocity: Vec3,
    ground: u32,
    surface_velocity: Vec3,
) -> Result<(), ClimbEngineError> {
    if !surface_velocity.is_finite() {
        return Err(ClimbEngineError::BadVelocity);
    }
    unsafe { set_velocity(velocity) }?;
    let proxy = unsafe { proxy() }?;
    let info = &mut proxy.m_GroundInformation;
    info.m_isSurfaceDynamic = surface_velocity != Vec3::ZERO;
    info.m_supportedState = hknpCharacterSurfaceInfo::SUPPORTED;
    info.m_surfaceDistanceExcess = 0.0;
    info.m_surfaceNormal = aligned(Vec3::Y);
    info.m_surfaceVelocity = aligned(surface_velocity);
    proxy.m_GroundBodyId = ground;
    proxy.m_LastValidGroundBodyId = ground;
    Ok(())
}

/// Give the proxy `velocity` for the coming step, as its own update does.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the proxy's update.
pub unsafe fn set_velocity(velocity: Vec3) -> Result<(), ClimbEngineError> {
    if !velocity.is_finite() {
        return Err(ClimbEngineError::BadVelocity);
    }
    let proxy = unsafe { proxy() }?;
    let world = unsafe { World::get() }.ok_or(ClimbEngineError::NoPhysicsWorld)?;
    unsafe { proxy_slot(world.world(), proxy) }?;
    let body = unsafe { proxy.m_CharacterRigidBody.as_mut() }.ok_or(ClimbEngineError::NoPlayer)?;
    let timestep = hkSimdFloat32 {
        m_real: [physics::step_seconds(); 4],
    };
    unsafe { body.setLinearVelocity(&aligned(velocity), &timestep, false) };
    Ok(())
}

/// The velocity of the local player's proxy body, as its motion holds it.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub unsafe fn proxy_velocity() -> Result<Vec3, ClimbEngineError> {
    let proxy = unsafe { proxy() }?;
    let world = unsafe { World::get() }.ok_or(ClimbEngineError::NoPhysicsWorld)?;
    let slot = unsafe { proxy_slot(world.world(), proxy) }?;
    let motions = world.world().m_motions;
    if motions.is_null() {
        return Err(ClimbEngineError::NoPlayer);
    }
    let motion = unsafe { &*motions.add(slot.m_motionId as usize) };
    let velocity = Vec3::from_array(motion.m_linearVelocity.data);
    velocity
        .is_finite()
        .then_some(velocity)
        .ok_or(ClimbEngineError::BadVelocity)
}

/// Whether the local player is in the air as the game judges it ([`Character::IsInAir`]).
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn in_air() -> Option<bool> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    unsafe { character.m_PfxCharacter.px.as_ref() }?;
    Some(unsafe { character.IsInAir() })
}

unsafe fn proxy() -> Result<&'static mut PfxCharacterInstance, ClimbEngineError> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .ok_or(ClimbEngineError::NoPlayer)?;
    unsafe { character.m_PfxCharacter.px.as_mut() }.ok_or(ClimbEngineError::NoPlayer)
}

/// The world's slot of the proxy's body, checked to hold a body.
unsafe fn proxy_slot<'w>(
    world: &'w hknpWorld,
    proxy: &PfxCharacterInstance,
) -> Result<&'w hknpBody, ClimbEngineError> {
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetCharacterRigidBodyId(&mut body) };
    unsafe { slot(world, body) }.ok_or(ClimbEngineError::NoPlayer)
}

/// The world's slot for the body `body`, if it holds one.
unsafe fn slot(world: &hknpWorld, body: u32) -> Option<&hknpBody> {
    if body == PfxCharacterInstance::INVALID_BODY_ID
        || body > world.m_peakBodyIndex
        || world.m_bodies.is_null()
    {
        return None;
    }
    let slot = unsafe { &*world.m_bodies.add(body as usize) };
    (slot.m_motionId != hknpBody::INVALID_MOTION_ID).then_some(slot)
}

fn aligned(v: Vec3) -> AlignedVector3 {
    AlignedVector3 {
        data: v.to_array(),
        _w: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_motion_uses_the_rotating_gripped_point() {
        let frame = BodyFrame {
            transform: Affine3A::IDENTITY,
            is_static: false,
            is_keyframed: false,
            mass_kg: Some(1000.0),
            centre: Vec3::ZERO,
            linear: Vec3::X * 30.0,
            angular: Vec3::Y * 2.0,
        };
        assert_eq!(frame.point_velocity(Vec3::Z * 3.0), Vec3::X * 36.0);
        assert_eq!(
            BodyFrame {
                is_static: true,
                ..frame
            }
            .point_velocity(Vec3::Z * 3.0),
            Vec3::ZERO
        );
    }
}
