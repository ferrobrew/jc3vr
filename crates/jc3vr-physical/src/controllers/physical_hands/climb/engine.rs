//! The climbing's engine side: the identity and motion of the bodies a hand grips, the ownership
//! context the native gate copies, and whether the player is in the air. The player's proxy, its
//! support, and its velocity are the physics substrate's
//! ([`jc3vr_body::controllers::physics::proxy`];
//! `docs/engine/character/root-motion-and-proxy.md`, "The proxy's support and the fall").
//!
//! The game decides whether the player is on the ground from the proxy's support
//! ([`jc3gi::physics::PfxCharacterInstance::m_GroundInformation`]), which the proxy's update
//! ([`jc3gi::physics::PfxCharacterInstance::UpdateContext`]) finds afresh before each physics step
//! and which the state machines read before the next proxy update. The physics system's
//! pre-simulation callbacks run after every character's proxy update, so a support written there
//! is the one the state machines, the fall bookkeeping, and the ground raycasts see until the next
//! proxy update: the player hanging from the hands stays on foot rather than falling. The proxy's
//! update has already set the proxy's velocity for the step by then, so a velocity set there is
//! the one the step integrates. The climb holds the player up from that callback for both reasons.
//!
//! Every function here runs outside the physics step, on the thread its safety section names.

use std::ptr;

use glam::{Quat, Vec3, Vec3A};
use jc3gi::{
    character::character::Character,
    physics::{PfxInstance, hknpBody, hknpWorld},
};
use jc3vr_body::controllers::{
    facts::climb::{BodyFrame, BodyIdentity},
    physics::{bodies::slot, havok::affine as havok_affine},
};

use crate::controllers::physical_hands::world::World;

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

/// The local player and the world the native gate's claims are taken in, or `None` without a
/// player, a proxy, or a world.
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
        epoch: jc3vr_view::headpose::teleport::epoch(),
    })
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

/// Whether a hand climbs the body `frame`: the static world, a keyframed body, or a dynamic one at
/// least `min_mass_kg`.
pub(crate) fn climbable(frame: &BodyFrame, min_mass_kg: f32) -> bool {
    frame.is_static || frame.is_keyframed || frame.mass_kg.is_none_or(|m| m >= min_mass_kg.max(0.0))
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

#[cfg(test)]
mod tests {
    use glam::Affine3A;

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
