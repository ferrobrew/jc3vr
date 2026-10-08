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

use std::fmt;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    havok::hkSimdFloat32,
    physics::{PfxCharacterInstance, PhysicsSystem, hknpBody, hknpCharacterSurfaceInfo, hknpWorld},
    types::vector_math::AlignedVector3,
};

use crate::controllers::physical_hands::world::World;

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

/// Whether the body `body` is a static body of the world: one of the static world, which never
/// moves.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn is_static(body: u32) -> bool {
    let Some(world) = (unsafe { World::get() }) else {
        return false;
    };
    unsafe { slot(world.world(), body) }.is_some_and(|slot| slot.m_flags & hknpBody::IS_STATIC != 0)
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
/// write it as supported on the static body `ground`, level, at rest.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the proxy's update; `ground`
/// must be a static body of the world.
pub unsafe fn hold(velocity: Vec3, ground: u32) -> Result<(), ClimbEngineError> {
    unsafe { set_velocity(velocity) }?;
    let proxy = unsafe { proxy() }?;
    let info = &mut proxy.m_GroundInformation;
    info.m_isSurfaceDynamic = false;
    info.m_supportedState = hknpCharacterSurfaceInfo::SUPPORTED;
    info.m_surfaceDistanceExcess = 0.0;
    info.m_surfaceNormal = aligned(Vec3::Y);
    info.m_surfaceVelocity = aligned(Vec3::ZERO);
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
        m_real: [PhysicsSystem::STEP_SECONDS; 4],
    };
    unsafe { body.setLinearVelocity(&aligned(velocity), &timestep, false) };
    Ok(())
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
