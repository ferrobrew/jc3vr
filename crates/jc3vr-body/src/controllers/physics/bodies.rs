//! The engine side of holding and shoving objects: a body's place, motion, and mass properties,
//! its owner, the velocities that drive it, and the impulses that shove it.
//!
//! A placed rigid object rests with a body that is not dynamic until something turns it loose; it
//! still has the mass it was placed with ([`placed_mass`]), and [`loosen`] turns it dynamic.
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
    game::GameObject,
    havok::hkAabb,
    physics::{
        PfxCharacterInstance, PfxInstance, PhysicsGameObject, RigidObject, RigidObjectType,
        hknpBody, hknpMotion, hknpWorld,
    },
};
use jc3vr_core::hash::hashlittle;

use crate::controllers::{
    physics::{
        havok::vector as havok_vector,
        rigid::{BodyPose, RigidBody, Velocities},
        world::World,
    },
    telemetry::{Applied, BodyBefore, note_force},
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
    /// A fixed gameplay fixture is not activated or carried.
    Fixture,
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            BodyError::NoPhysicsWorld => "the physics world does not exist",
            BodyError::NoBody => "the body has been freed",
            BodyError::NotDynamic => "the body is not dynamic",
            BodyError::Reowned => "the body belongs to another object",
            BodyError::BadMotion => "the body's motion is not finite",
            BodyError::Fixture => "the object is a fixed gameplay fixture",
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
    #[inline]
    pub fn rigid(&self) -> RigidBody {
        RigidBody {
            pose: self.pose,
            velocities: self.velocities,
            inverse_mass: self.mass_kg.map_or(0.0, |m| 1.0 / m),
            inverse_inertia: self.inverse_inertia,
        }
    }
}

/// The mass of a body whose inverse mass is `inverse_mass`, or `None` for an infinite mass.
pub fn mass_of(inverse_mass: f32) -> Option<f32> {
    (inverse_mass.is_finite() && inverse_mass > 0.0).then(|| 1.0 / inverse_mass)
}

/// The mass of the placed rigid object `object` whose body `body` is still at rest where it was
/// placed: a [`RigidObject`] whose type is fixed or keyframed ([`RigidObjectType`]), whose body
/// wrapper's body is `body` and not yet dynamic, and whose wrapper keeps the motion the body had
/// before it was placed
/// ([`PfxRigidBody::m_SavedMotionId`](jc3gi::physics::PfxRigidBody::m_SavedMotionId)), from which
/// this reads the mass. `None` for anything else, a static rigid object, which is part of the
/// world, among them.
///
/// # Safety
///
/// Main thread, outside the physics step; `object` and `body` must come from a cast this tick or be
/// held alive.
pub unsafe fn placed_mass(object: NonZeroUsize, body: u32) -> Option<f32> {
    let saved = unsafe { placed_motion(object, body) }?;
    let world = unsafe { World::get() }?;
    let world = world.world();
    if world.m_motions.is_null() {
        return None;
    }
    // SAFETY: a saved motion is one of the world's, which the wrapper owns until the body turns
    // dynamic.
    let motion = unsafe { &*world.m_motions.add(saved as usize) };
    mass_of(half_to_f32(motion.m_inverseInertia[3])).filter(|m| m.is_finite())
}

/// Turn the placed rigid object `object` loose, as the game does when its data or a script makes it
/// dynamic ([`RigidObject::MakeDynamic`]): its body `body` keeps its id and takes back the mass and
/// inertia it was placed with, at once. Returns whether it changed anything: a body that is
/// already dynamic is left as it is. Fails for a body that is neither dynamic nor a placed object's
/// ([`placed_mass`]), or one the change left not dynamic.
///
/// # Safety
///
/// Main thread, outside the physics step; `object` and `body` must come from a cast this tick or be
/// held alive.
pub unsafe fn loosen(object: NonZeroUsize, body: u32) -> Result<bool, BodyError> {
    if unsafe { owner_of(body) } != Some(object) {
        return Err(BodyError::Reowned);
    }
    if unsafe { fixed_fixture(object) } {
        return Err(BodyError::Fixture);
    }
    {
        let world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
        let slot = unsafe { slot(world.world(), body) }.ok_or(BodyError::NoBody)?;
        if slot.m_flags & hknpBody::IS_DYNAMIC != 0 {
            return Ok(false);
        }
    }
    if unsafe { placed_mass(object, body) }.is_none() {
        return Err(BodyError::NotDynamic);
    }
    let rigid = object.get() as *mut RigidObject;
    // SAFETY: checked a live rigid object above. Every call the game makes passes false, which
    // keeps the body in its collision system group.
    unsafe { (*rigid).MakeDynamic(false) };
    unsafe { read(body, object) }?;
    Ok(true)
}

/// Where the body `body` of `owner` is, world space: its centre of mass while it is dynamic, or for
/// a placed object at rest ([`placed_mass`]), the origin of its transform.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub unsafe fn position(body: u32, owner: NonZeroUsize) -> Result<Vec3, BodyError> {
    match unsafe { read(body, owner) } {
        Ok(state) => Ok(state.pose.centre),
        Err(BodyError::NotDynamic) if unsafe { placed_motion(owner, body) }.is_some() => {
            let world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
            let slot = unsafe { slot(world.world(), body) }.ok_or(BodyError::NoBody)?;
            let at = Vec3::from_array(slot.m_transform.m_translation.data) + world.shift;
            if at.is_finite() {
                Ok(at)
            } else {
                Err(BodyError::BadMotion)
            }
        }
        Err(e) => Err(e),
    }
}

/// The state of the body `body`, checked to still belong to `owner`.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub unsafe fn read(body: u32, owner: NonZeroUsize) -> Result<BodyState, BodyError> {
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
#[track_caller]
pub unsafe fn drive(
    body: u32,
    owner: NonZeroUsize,
    velocities: Velocities,
) -> Result<(), BodyError> {
    if !(velocities.linear.is_finite() && velocities.angular.is_finite()) {
        return Err(BodyError::BadMotion);
    }
    let mut world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
    let motion = unsafe { owned_motion(world.world(), body, owner) }?;
    note_force(
        body,
        Applied::Velocity {
            linear: velocities.linear,
            angular: velocities.angular,
        },
        Some(before(motion, owner)),
    );
    let linear = havok_vector(velocities.linear);
    let angular = havok_vector(velocities.angular);
    unsafe { world.world_mut().setBodyVelocity(body, &linear, &angular) };
    Ok(())
}

/// Apply the linear impulse `impulse` to the body `body` at the game-space point `at`, after
/// checking it still belongs to `owner`.
///
/// # Safety
///
/// Main thread, outside the physics step.
#[track_caller]
pub unsafe fn shove(
    body: u32,
    owner: NonZeroUsize,
    impulse: Vec3,
    at: Vec3,
) -> Result<(), BodyError> {
    if !(impulse.is_finite() && at.is_finite()) {
        return Err(BodyError::BadMotion);
    }
    let mut world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
    let motion = unsafe { owned_motion(world.world(), body, owner) }?;
    note_force(
        body,
        Applied::Impulse { impulse, at },
        Some(before(motion, owner)),
    );
    let impulse = havok_vector(impulse);
    let position = havok_vector(at - world.shift);
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
pub unsafe fn owner_of(body: u32) -> Option<NonZeroUsize> {
    let world = unsafe { World::get() }?;
    let world = world.world();
    unsafe { slot(world, body) }?;
    unsafe { body_owner(world, body) }
}

/// The world's slot for the body `body`, while it holds a body: the id is a valid one within the
/// world's bodies, and its slot has a motion.
///
/// # Safety
///
/// `world` must be the live world, read where its bodies do not change under the read: the thread
/// that steps it, or the native physics phase.
pub unsafe fn slot(world: &hknpWorld, body: u32) -> Option<&hknpBody> {
    if body == PfxCharacterInstance::INVALID_BODY_ID
        || body > world.m_peakBodyIndex
        || world.m_bodies.is_null()
    {
        return None;
    }
    // SAFETY: every id up to the peak has a slot.
    let slot = unsafe { &*world.m_bodies.add(body as usize) };
    (slot.m_motionId != hknpBody::INVALID_MOTION_ID).then_some(slot)
}

/// The object that owns the body `body`: its engine instance's owner ([`instance_of`]), as a
/// physics query reports it.
///
/// # Safety
///
/// As [`slot`].
pub unsafe fn body_owner(world: &hknpWorld, body: u32) -> Option<NonZeroUsize> {
    let instance = unsafe { (instance_of(world, body)? as *const PfxInstance).as_ref() }?;
    NonZeroUsize::new(instance.m_GameObject as usize)
}

/// The engine instance of the body `body` ([`hknpWorld::BODY_PROPERTY_PFX_INSTANCE`]), as an
/// address.
///
/// # Safety
///
/// As [`slot`].
pub unsafe fn instance_of(world: &hknpWorld, body: u32) -> Option<usize> {
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
    (!instance.is_null()).then_some(instance as usize)
}

/// The box of the body `body` as the broad phase holds it, game space: its least and greatest
/// corners. `None` for a freed body, without a world, or for a box that is not finite.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub unsafe fn aabb(body: u32) -> Option<(Vec3, Vec3)> {
    let world = unsafe { World::get() }?;
    unsafe { slot(world.world(), body) }?;
    let mut aabb = hkAabb::default();
    unsafe { world.world().getBodyAabb(body, &mut aabb) };
    let min = Vec3::from_array(aabb.m_min.data) + world.shift;
    let max = Vec3::from_array(aabb.m_max.data) + world.shift;
    (min.is_finite() && max.is_finite() && min.cmple(max).all()).then_some((min, max))
}

/// The mass of the dynamic body `body`, whatever owns it: `None` for a freed body, one that is
/// not dynamic, or an infinite mass.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(crate) unsafe fn mass_kg(body: u32) -> Option<f32> {
    let world = unsafe { World::get() }?;
    let world = world.world();
    let slot = unsafe { slot(world, body) }?;
    if world.m_motions.is_null() || slot.m_flags & hknpBody::IS_DYNAMIC == 0 {
        return None;
    }
    let motion = unsafe { &*world.m_motions.add(slot.m_motionId as usize) };
    mass_of(half_to_f32(motion.m_inverseInertia[3])).filter(|m| m.is_finite())
}

/// Whether the physics game object at `object` is of the run-time type `type_id`.
///
/// # Safety
///
/// `object` must be a live physics game object.
#[inline]
pub unsafe fn is_type(object: NonZeroUsize, type_id: *const u32) -> bool {
    unsafe { (*game_object_of(object)).IsType(type_id) }
}

/// The `CGameObject` subobject of the physics game object at `object`, as a physics query reports
/// it.
pub fn game_object_of(object: NonZeroUsize) -> *mut GameObject {
    (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *mut GameObject
}

/// The physics game object a `CGameObject` subobject belongs to, when the object has one.
///
/// # Safety
///
/// `object` must be a live game object.
pub unsafe fn physics_object_of(object: NonZeroUsize) -> Option<NonZeroUsize> {
    let game_object = object.get() as *const GameObject;
    if !unsafe { (*game_object).IsType(PhysicsGameObject::TYPE_ID()) } {
        return None;
    }
    NonZeroUsize::new(object.get() - PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize)
}

/// A body's owner, mass, and velocities before a force, for the recording.
fn before(motion: &hknpMotion, owner: NonZeroUsize) -> BodyBefore {
    let frame = Quat::from_array(motion.m_orientation.data);
    let local_angular = Vec3::from_array(motion.m_angularVelocity.data);
    BodyBefore {
        object: Some(owner.get()),
        mass_kg: mass_of(f32::from_bits(u32::from(motion.m_inverseInertia[3]) << 16)),
        linear: Vec3::from_array(motion.m_linearVelocity.data),
        angular: if frame.is_finite() {
            frame.normalize() * local_angular
        } else {
            local_angular
        },
    }
}

/// The motion of the dynamic body `body`, checked to belong to `owner`.
unsafe fn owned_motion(
    world: &hknpWorld,
    body: u32,
    owner: NonZeroUsize,
) -> Result<&hknpMotion, BodyError> {
    let slot = unsafe { slot(world, body) }.ok_or(BodyError::NoBody)?;
    if world.m_motions.is_null() {
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

/// The saved motion of the placed rigid object `object`, when its body `body` is still at rest
/// where it was placed ([`placed_mass`]).
unsafe fn placed_motion(object: NonZeroUsize, body: u32) -> Option<u32> {
    if unsafe { fixed_fixture(object) } || !unsafe { is_type(object, RigidObject::TYPE_ID()) } {
        return None;
    }
    let rigid = unsafe { &*(object.get() as *const RigidObject) };
    let kind = rigid.m_Flags & RigidObject::TYPE_MASK;
    if kind != RigidObjectType::FIXED_RIGIDOBJECT as u16
        && kind != RigidObjectType::KEYFRAMED_RIGIDOBJECT as u16
    {
        return None;
    }
    let pfx = unsafe { rigid.m_PfxInstance.px.as_ref() }?;
    let saved = pfx.m_SavedMotionId;
    if pfx.m_BodyId != body || saved == hknpBody::INVALID_MOTION_ID || saved == 0 {
        return None;
    }
    let world = unsafe { World::get() }?;
    let slot = unsafe { slot(world.world(), body) }?;
    (slot.m_flags & hknpBody::IS_DYNAMIC == 0
        && unsafe { body_owner(world.world(), body) } == Some(object))
    .then_some(saved)
}

/// The military weapon cabinet is a rigid fixture; its displayed weapons are separate actors.
///
/// # Safety
///
/// `object` must be a live game object.
pub unsafe fn fixed_fixture(object: NonZeroUsize) -> bool {
    if !unsafe { is_type(object, RigidObject::TYPE_ID()) } {
        return false;
    }
    let rigid = unsafe { &*(object.get() as *const RigidObject) };
    let model = unsafe { rigid.m_Model.px.as_ref() };
    model.is_some_and(|model| fixture_model(model.m_Id.m_Hash))
}

fn fixture_model(model: u32) -> bool {
    model
        == hashlittle(
            b"models/jc_environments/structures/military/weapon_rack/weapon_rack_greybox.lod",
        )
}

/// A Havok `hkHalf`: the upper 16 bits of an `f32`.
fn half_to_f32(half: u16) -> f32 {
    f32::from_bits(u32::from(half) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weapon_cabinet_model_does_not_exclude_other_rigid_props_or_its_weapons() {
        assert!(fixture_model(hashlittle(
            b"models/jc_environments/structures/military/weapon_rack/weapon_rack_greybox.lod"
        )));
        for model in [
            b"models/jc_environments/props/barrel.lod".as_slice(),
            b"models/jc_weapons/03_thrown/w301_grenade_frag/w301_grenade_frag_body.lod".as_slice(),
            b"models/jc_environments/structures/military/weapon_rack/weapon_rack_greybox_col.pfxc"
                .as_slice(),
        ] {
            assert!(!fixture_model(hashlittle(model)));
        }
    }

    #[test]
    fn a_mass_is_the_inverse_of_a_finite_positive_inverse_mass() {
        assert_eq!(mass_of(0.5), Some(2.0));
        assert_eq!(mass_of(0.0), None);
    }

    #[test]
    fn a_half_is_the_top_of_a_float() {
        assert_eq!(half_to_f32((1.0f32.to_bits() >> 16) as u16), 1.0);
        assert_eq!(half_to_f32((0.25f32.to_bits() >> 16) as u16), 0.25);
        assert_eq!(half_to_f32(0), 0.0);
    }
}
