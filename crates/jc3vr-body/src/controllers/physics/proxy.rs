//! The local player's character proxy as the physics world holds it: its body's slot, the velocity
//! its motion holds, the support its last update found, and the velocity it is given for the coming
//! step, the way its own update gives it one.
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
    bodies::slot, error::PhysicsError, havok::vector as havok_vector, world::World,
};

/// The local player character's proxy, or [`PhysicsError::NoPlayer`] without a local player or a
/// proxy.
///
/// # Safety
///
/// The game thread, outside the physics step. The proxy is the character's, live while it is; the
/// caller holds the reference no longer than the call that took it.
pub unsafe fn local_proxy() -> Result<&'static mut PfxCharacterInstance, PhysicsError> {
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
pub unsafe fn proxy_slot<'w>(
    world: &'w hknpWorld,
    proxy: &PfxCharacterInstance,
) -> Result<&'w hknpBody, PhysicsError> {
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetCharacterRigidBodyId(&mut body) };
    unsafe { slot(world, body) }.ok_or(PhysicsError::NoPlayer)
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
