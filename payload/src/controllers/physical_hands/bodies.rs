//! The engine side of holding and shoving objects: what a reached object is, a body's place,
//! motion, and mass properties, the velocities that drive it, and the impulses that shove it.
//!
//! A body is named by its Havok id, which is reused once the body is freed, so every read checks
//! that the id still belongs to the object it was taken from: the id has a slot, the slot has a
//! motion, the body is dynamic, and the body's engine instance
//! ([`hknpWorld::BODY_PROPERTY_PFX_INSTANCE`]) is still owned by the object. The positions the
//! Havok world holds are about its moving origin; they are moved to game space here with
//! [`PhysicsSystem::m_AccumulatedShift`].

use std::{fmt, num::NonZeroUsize, ptr};

use glam::{Quat, Vec3};
use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObject,
    physics::{
        PfxCharacterInstance, PfxInstance, PhysicsGameObject, RigidObject, hknpBody, hknpMotion,
        hknpWorld,
    },
    types::vector_math::AlignedVector3,
};

use crate::controllers::physical_hands::{
    grasp::{TargetKind, mass_of},
    rigid::{BodyPose, RigidBody, Velocities},
    world::World,
};

/// Why a body could not be read or driven.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BodyError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// The body id has no slot, or its slot was freed.
    NoBody,
    /// The body is not dynamic: static, fixed, or keyframed.
    NotDynamic,
    /// The body no longer belongs to the object it was taken from.
    Reowned,
    /// The body's motion holds a value that is not finite.
    BadMotion,
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            BodyError::NoPhysicsWorld => "the physics world does not exist",
            BodyError::NoBody => "the body has been freed",
            BodyError::NotDynamic => "the body is not dynamic",
            BodyError::Reowned => "the body belongs to another object",
            BodyError::BadMotion => "the body's motion is not finite",
        };
        write!(f, "physical hands: pickup: {what}")
    }
}

impl std::error::Error for BodyError {}

/// A body's state, game space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyState {
    pub pose: BodyPose,
    pub velocities: Velocities,
    /// The body's mass, or `None` for an infinite one.
    pub mass_kg: Option<f32>,
    /// The inverse inertia about the axes of the motion's frame.
    pub inverse_inertia: Vec3,
}

impl BodyState {
    pub fn rigid(&self) -> RigidBody {
        RigidBody {
            pose: self.pose,
            velocities: self.velocities,
            inverse_mass: self.mass_kg.map_or(0.0, |m| 1.0 / m),
            inverse_inertia: self.inverse_inertia,
        }
    }
}

/// What the object at `object`, reached on its body `body`, is to a hand: a weapon lying in the
/// world, a loose rigid object, or nothing a hand takes.
///
/// # Safety
///
/// Main thread, outside the physics step; `object` and `body` must come from a cast this tick.
pub(super) unsafe fn classify(object: NonZeroUsize, body: u32) -> Option<TargetKind> {
    let game_object =
        (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *const GameObject;
    if unsafe { (*game_object).IsType(WeaponBase::TYPE_ID()) } {
        let weapon = object.get() as *const WeaponBase;
        return (unsafe { (*weapon).m_State } == WeaponState::E_WS_DROPPED)
            .then_some(TargetKind::Weapon);
    }
    if !unsafe { (*game_object).IsType(RigidObject::TYPE_ID()) } {
        return None;
    }
    let state = unsafe { read(body, object) }.ok()?;
    Some(TargetKind::Prop {
        mass_kg: state.mass_kg?,
    })
}

/// The state of the body `body`, checked to still belong to `owner`.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn read(body: u32, owner: NonZeroUsize) -> Result<BodyState, BodyError> {
    let world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
    let motion = unsafe { owned_motion(world.world(), body, owner) }?;
    let centre = Vec3::from_array(motion.m_centerOfMass.data) + world.shift;
    let orientation = Quat::from_array(motion.m_orientation.data);
    let linear = Vec3::from_array(motion.m_linearVelocity.data);
    let local_angular = Vec3::from_array(motion.m_angularVelocity.data);
    if !(centre.is_finite()
        && orientation.is_finite()
        && linear.is_finite()
        && local_angular.is_finite())
    {
        return Err(BodyError::BadMotion);
    }
    let orientation = orientation.normalize();
    let [ix, iy, iz, inverse_mass] = motion.m_inverseInertia.map(half_to_f32);
    let inverse_inertia = Vec3::new(ix, iy, iz);
    if !inverse_inertia.is_finite() {
        return Err(BodyError::BadMotion);
    }
    Ok(BodyState {
        pose: BodyPose {
            centre,
            orientation,
        },
        velocities: Velocities {
            linear,
            angular: orientation * local_angular,
        },
        mass_kg: mass_of(inverse_mass),
        inverse_inertia,
    })
}

/// Set the body `body`'s velocities, world space, after checking it still belongs to `owner`.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn drive(
    body: u32,
    owner: NonZeroUsize,
    velocities: Velocities,
) -> Result<(), BodyError> {
    if !(velocities.linear.is_finite() && velocities.angular.is_finite()) {
        return Err(BodyError::BadMotion);
    }
    let mut world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
    unsafe { owned_motion(world.world(), body, owner) }?;
    let linear = aligned(velocities.linear);
    let angular = aligned(velocities.angular);
    unsafe { world.world_mut().setBodyVelocity(body, &linear, &angular) };
    Ok(())
}

/// Apply the linear impulse `impulse` to the body `body` at the game-space point `at`, after
/// checking it still belongs to `owner`.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn shove(
    body: u32,
    owner: NonZeroUsize,
    impulse: Vec3,
    at: Vec3,
) -> Result<(), BodyError> {
    if !(impulse.is_finite() && at.is_finite()) {
        return Err(BodyError::BadMotion);
    }
    let mut world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
    unsafe { owned_motion(world.world(), body, owner) }?;
    let impulse = aligned(impulse);
    let position = aligned(at - world.shift);
    unsafe {
        world
            .world_mut()
            .applyBodyImpulseAt(body, &impulse, &position)
    };
    Ok(())
}

/// The object that owns the body `body`, whatever its motion: its engine instance's owner, as a
/// physics query reports it. `None` for a freed body or one without an owner.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn owner_of(body: u32) -> Option<NonZeroUsize> {
    let world = unsafe { World::get() }?;
    let world = world.world();
    if body == PfxCharacterInstance::INVALID_BODY_ID
        || body > world.m_peakBodyIndex
        || world.m_bodies.is_null()
    {
        return None;
    }
    let slot = unsafe { &*world.m_bodies.add(body as usize) };
    if slot.m_motionId == hknpBody::INVALID_MOTION_ID {
        return None;
    }
    unsafe { body_owner(world, body) }
}

/// Whether the physics game object at `object` is of the run-time type `type_id`.
///
/// # Safety
///
/// `object` must be a live physics game object.
pub(super) unsafe fn is_type(object: NonZeroUsize, type_id: *const u32) -> bool {
    let game_object =
        (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *const GameObject;
    unsafe { (*game_object).IsType(type_id) }
}

/// The motion of the dynamic body `body`, checked to belong to `owner`.
unsafe fn owned_motion(
    world: &hknpWorld,
    body: u32,
    owner: NonZeroUsize,
) -> Result<&hknpMotion, BodyError> {
    if body == PfxCharacterInstance::INVALID_BODY_ID
        || body > world.m_peakBodyIndex
        || world.m_bodies.is_null()
        || world.m_motions.is_null()
    {
        return Err(BodyError::NoBody);
    }
    let slot = unsafe { &*world.m_bodies.add(body as usize) };
    if slot.m_motionId == hknpBody::INVALID_MOTION_ID {
        return Err(BodyError::NoBody);
    }
    if slot.m_flags & hknpBody::IS_DYNAMIC == 0 {
        return Err(BodyError::NotDynamic);
    }
    if unsafe { body_owner(world, body) } != Some(owner) {
        return Err(BodyError::Reowned);
    }
    Ok(unsafe { &*world.m_motions.add(slot.m_motionId as usize) })
}

/// The object that owns the body `body`: its engine instance's owner, as a physics query reports
/// it.
unsafe fn body_owner(world: &hknpWorld, body: u32) -> Option<NonZeroUsize> {
    let buffer = unsafe {
        world
            .m_bodyProperties
            .getWithDefault(hknpWorld::BODY_PROPERTY_PFX_INSTANCE, ptr::null_mut())
            .as_ref()
    }?;
    if body >= buffer.m_capacity || buffer.m_occupancy.is_null() || buffer.m_data.is_null() {
        return None;
    }
    let word = unsafe { *buffer.m_occupancy.add((body / 32) as usize) };
    if word & (1 << (body % 32)) == 0 {
        return None;
    }
    let instance = unsafe {
        *buffer
            .m_data
            .cast::<*const PfxInstance>()
            .add(body as usize)
    };
    let instance = unsafe { instance.as_ref() }?;
    NonZeroUsize::new(instance.m_GameObject as usize)
}

/// A Havok `hkHalf`: the upper 16 bits of an `f32`.
fn half_to_f32(half: u16) -> f32 {
    f32::from_bits(u32::from(half) << 16)
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
    fn a_half_is_the_top_of_a_float() {
        assert_eq!(half_to_f32((1.0f32.to_bits() >> 16) as u16), 1.0);
        assert_eq!(half_to_f32((0.25f32.to_bits() >> 16) as u16), 0.25);
        assert_eq!(half_to_f32(0), 0.0);
    }
}
