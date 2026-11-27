//! The local player's character proxy as the physics world holds it: its body's slot and place, the
//! velocity its motion holds, the support its last update found, and the velocity and support it is
//! given for the coming step, the way its own update gives them.
//!
//! Every function here reaches the local player character itself, on the thread that steps the
//! physics world, outside the step.

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    havok::hkSimdFloat32,
    physics::{PfxCharacterInstance, hknpBody, hknpCharacterSurfaceInfo, hknpWorld},
};

use crate::controllers::physics::{
    bodies::slot, characters::character_position, error::PhysicsError,
    havok::vector as havok_vector, world::World,
};

/// The local player as the physics world places it: the proxy body's centre and the character's
/// root, game space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ProxyPlace {
    pub body: Vec3,
    pub feet: Vec3,
}

/// The local player character's proxy, or [`PhysicsError::NoPlayer`] without a local player or a
/// proxy.
///
/// # Safety
///
/// The game thread, outside the physics step. The proxy is the character's, live while it is; the
/// caller holds the reference no longer than the call that took it.
pub(crate) unsafe fn local_proxy() -> Result<&'static mut PfxCharacterInstance, PhysicsError> {
    let character =
        unsafe { Character::GetLocalPlayerCharacter().as_ref() }.ok_or(PhysicsError::NoPlayer)?;
    unsafe { character.m_PfxCharacter.px.as_mut() }.ok_or(PhysicsError::NoPlayer)
}

/// The world's slot of `proxy`'s body ([`PfxCharacterInstance::GetCharacterRigidBodyId`]), checked
/// to hold a body, or [`PhysicsError::NoPlayer`].
///
/// # Safety
///
/// `world` must be the live world and `proxy` a live proxy, read where the world's bodies do not
/// change under the read.
pub(crate) unsafe fn proxy_slot<'w>(
    world: &'w hknpWorld,
    proxy: &PfxCharacterInstance,
) -> Result<&'w hknpBody, PhysicsError> {
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetCharacterRigidBodyId(&mut body) };
    unsafe { slot(world, body) }.ok_or(PhysicsError::NoPlayer)
}

/// Where the local player's proxy body and root are: [`PhysicsError::NoPlayer`] without a local
/// player, a proxy, or a proxy body, or for a place that is not finite.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn place() -> Result<ProxyPlace, PhysicsError> {
    let character =
        unsafe { Character::GetLocalPlayerCharacter().as_ref() }.ok_or(PhysicsError::NoPlayer)?;
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }.ok_or(PhysicsError::NoPlayer)?;
    let world = unsafe { World::require() }?;
    let slot = unsafe { proxy_slot(world.world(), proxy) }?;
    let body = Vec3::from_array(slot.m_transform.m_translation.data) + world.shift;
    let feet = character_position(character);
    if !(body.is_finite() && feet.is_finite()) {
        return Err(PhysicsError::NoPlayer);
    }
    Ok(ProxyPlace { body, feet })
}

/// The velocity of the local player's proxy body, as its motion holds it: [`PhysicsError::NoPlayer`]
/// while the world has no motions, and [`PhysicsError::BadMotion`] for one that is not finite.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub unsafe fn proxy_velocity() -> Result<Vec3, PhysicsError> {
    let proxy = unsafe { local_proxy() }?;
    let world = unsafe { World::require() }?;
    let slot = unsafe { proxy_slot(world.world(), proxy) }?;
    let motion = world.motion(slot).ok_or(PhysicsError::NoPlayer)?;
    let velocity = Vec3::from_array(motion.m_linearVelocity.data);
    velocity
        .is_finite()
        .then_some(velocity)
        .ok_or(PhysicsError::BadMotion)
}

/// Give the local player's proxy body `velocity` for the coming step of `step_s` seconds, as the
/// proxy's own update does. Fails for a velocity that is not finite ([`PhysicsError::BadMotion`])
/// before reaching the proxy.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the proxy's update.
pub unsafe fn set_proxy_velocity(velocity: Vec3, step_s: f32) -> Result<(), PhysicsError> {
    if !velocity.is_finite() {
        return Err(PhysicsError::BadMotion);
    }
    let proxy = unsafe { local_proxy() }?;
    let world = unsafe { World::require() }?;
    unsafe { proxy_slot(world.world(), proxy) }?;
    let body = unsafe { proxy.m_CharacterRigidBody.as_mut() }.ok_or(PhysicsError::NoPlayer)?;
    let timestep = hkSimdFloat32 {
        m_real: [step_s; 4],
    };
    unsafe { body.setLinearVelocity(&havok_vector(velocity), &timestep, false) };
    Ok(())
}

/// Hold the local player up: give the proxy body `velocity` for the coming step of `step_s` seconds
/// ([`set_proxy_velocity`]), and write the proxy as supported on the body `ground`, level, moving at
/// `surface_velocity`: at rest on the static world, carried with a moving body. Fails for a surface
/// velocity that is not finite ([`PhysicsError::BadMotion`]) before reaching the proxy.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the proxy's update; `ground`
/// must be a body of the world.
pub unsafe fn hold(
    velocity: Vec3,
    step_s: f32,
    ground: u32,
    surface_velocity: Vec3,
) -> Result<(), PhysicsError> {
    if !surface_velocity.is_finite() {
        return Err(PhysicsError::BadMotion);
    }
    unsafe { set_proxy_velocity(velocity, step_s) }?;
    let proxy = unsafe { local_proxy() }?;
    let info = &mut proxy.m_GroundInformation;
    info.m_isSurfaceDynamic = surface_velocity != Vec3::ZERO;
    info.m_supportedState = hknpCharacterSurfaceInfo::SUPPORTED;
    info.m_surfaceDistanceExcess = 0.0;
    info.m_surfaceNormal = havok_vector(Vec3::Y);
    info.m_surfaceVelocity = havok_vector(surface_velocity);
    proxy.m_GroundBodyId = ground;
    proxy.m_LastValidGroundBodyId = ground;
    Ok(())
}

/// The support the local player's proxy's update found for this step: the upward share of the
/// normal of the surface it stands on, or `None` when it stands on nothing.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the proxy's update.
pub unsafe fn support() -> Option<f32> {
    let proxy = unsafe { local_proxy() }.ok()?;
    let info = &proxy.m_GroundInformation;
    (info.m_supportedState == hknpCharacterSurfaceInfo::SUPPORTED)
        .then_some(info.m_surfaceNormal.data[1])
        .filter(|y| y.is_finite())
}
