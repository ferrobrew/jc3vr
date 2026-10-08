//! One hand body, or one held gun's body, in the game's physics world: its creation, its place in
//! the simulation, its state, and its teardown, as the engine's own free bodies do it.
//!
//! A hand body is built the way the grappling hook's hang proxy (`CGrapplingHangPhysicsProxy::Init`)
//! builds its free capsule: a capsule shape ([`hknpCapsuleShape::createCapsuleShape`]), a body and a
//! motion from their construction infos ([`hknpWorld::createDynamicBody`], not yet added), and the
//! engine's wrapper ([`PfxRigidBody`]) around the body, which registers it as an engine physics
//! instance so that every engine system that meets the body finds an instance (whose owner is
//! null, as the proxy's is). The wrapper then sets the body's collision layer and system group and
//! turns its gravity off. Deleting the wrapper ([`PfxRigidBody::Destruct`]) removes and destroys
//! the body.
//!
//! A held gun's body is made the same way from the shape of the gun's own body
//! ([`hknpBody::m_shape`]), which the new body takes a reference of its own to, so the gun's body
//! and the mod's share one shape and neither outlives the other's use of it; its gravity stays on.
//!
//! Every function here changes the physics world, so it runs only on the thread that steps it,
//! outside the step: from the pre-simulation callback, or from the shutdown cleanup on the game
//! thread.

use std::{
    fmt,
    mem::MaybeUninit,
    ptr::{self, NonNull},
    sync::atomic::{AtomicU32, Ordering},
};

use glam::{Mat3, Mat4, Quat, Vec3};
use jc3gi::{
    character::character::Character,
    havok::{hkReferencedObject, hknpBodyCinfo, hknpCapsuleShape, hknpMotionCinfo, hknpShape},
    memory::{MemoryFree, OperatorNew},
    physics::{
        CustomGroupCollisionFilter, PfxCharacterInstance, PfxInstance, PfxLayer, PfxRigidBody,
        hknpAdditionMode, hknpBody, hknpWorld,
    },
    types::{
        math::Matrix4,
        vector_math::{AlignedQuat, AlignedVector3},
    },
};

use crate::controllers::physical_hands::{
    physics::{
        contact::{self, ContactError, Source},
        drive::BodyMotion,
    },
    world::World,
};

/// Why a hand body could not be made, read, or driven.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BodyError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// The engine allocator returned nothing for the wrapper.
    NoWrapperMemory,
    /// The Havok heap returned nothing for the shape.
    NoShape,
    /// The world did not give the new body an id.
    NoBodyId,
    /// The body's slot no longer holds the body this wrapper made.
    Lost,
    /// The body's contacts could not be listened for.
    Contacts(ContactError),
    /// The body's state holds a value that is not finite.
    BadMotion,
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            BodyError::NoPhysicsWorld => "the physics world does not exist",
            BodyError::NoWrapperMemory => "the engine allocator gave no memory for the wrapper",
            BodyError::NoShape => "the Havok heap gave no capsule shape",
            BodyError::NoBodyId => "the world gave the new body no id",
            BodyError::Lost => "the body's slot no longer holds the hand's body",
            BodyError::BadMotion => "the body's state is not finite",
            BodyError::Contacts(e) => return fmt::Display::fmt(e, f),
        };
        write!(f, "hand bodies: {what}")
    }
}

impl std::error::Error for BodyError {}

/// The capsule a hand body is: a segment along the hand's fingers, centred on the palm.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Capsule {
    pub half_length_m: f32,
    pub radius_m: f32,
}

/// A body's shape.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum BodyShape {
    /// A capsule made for the body.
    Capsule(Capsule),
    /// Another body's shape, which the new body takes a reference of its own to. The address
    /// must be a live shape when the body is made.
    Shared(NonNull<hknpShape>),
}

/// What a hand body, or a held gun's body, is made with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodySpec {
    pub shape: BodyShape,
    pub mass_kg: f32,
    /// The collision system group it shares with the player's character proxy, so that it never
    /// collides with the player.
    pub system_group: u32,
    /// Whether the body's contacts are listened for ([`contact`]).
    pub contact_events: bool,
    /// Whether gravity acts on the body: a hand's is off, so the drive does not fight it, and a
    /// gun's is on, so its weight hangs from the hands.
    pub gravity: bool,
}

/// A hand body or a held gun's body: the engine wrapper this module allocated, which owns the
/// Havok body.
pub struct HandBody {
    wrapper: NonNull<PfxRigidBody>,
    body_id: u32,
    spec: BodySpec,
    source: Source,
}

// SAFETY: the wrapper is only touched on the thread that steps the physics world, outside the
// step (see the module docs); the handle moves between the callback and the cleanup, which run on
// that thread.
unsafe impl Send for HandBody {}

impl HandBody {
    /// Make a hand body at the game-space pose `position`, `orientation`, out of the simulation.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step.
    pub unsafe fn create(
        source: impl Into<Source>,
        spec: BodySpec,
        position: Vec3,
        orientation: Quat,
    ) -> Result<Self, BodyError> {
        let source = source.into();
        let mut world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
        // The wrapper's memory first: it is the one allocation a failure after the body is made
        // could not return without destroying the body. A failure before then frees it.
        let memory =
            unsafe { OperatorNew(size_of::<PfxRigidBody>() as u64) }.cast::<PfxRigidBody>();
        let memory = NonNull::new(memory).ok_or(BodyError::NoWrapperMemory)?;
        // SAFETY: the memory came from `OperatorNew` above and nothing else holds it.
        let free_memory = || unsafe { MemoryFree(memory.as_ptr().cast()) };

        // A made capsule comes with one reference, the caller's; a shared shape lends none.
        let (shape, owned) = match spec.shape {
            BodyShape::Capsule(capsule) => {
                let a = aligned(Vec3::NEG_Z * capsule.half_length_m);
                let b = aligned(Vec3::Z * capsule.half_length_m);
                let shape =
                    unsafe { hknpCapsuleShape::createCapsuleShape(&a, &b, capsule.radius_m) };
                (shape, true)
            }
            BodyShape::Shared(shape) => (shape.as_ptr(), false),
        };
        if shape.is_null() {
            free_memory();
            return Err(BodyError::NoShape);
        }

        let mut cinfo = MaybeUninit::<hknpBodyCinfo>::zeroed();
        // SAFETY: zeroed, then constructed; the constructor sets every member it reads.
        let cinfo = unsafe {
            (*cinfo.as_mut_ptr()).Construct();
            cinfo.assume_init_mut()
        };
        cinfo.m_shape = shape;
        cinfo.m_position = aligned(position - world.shift);
        cinfo.m_orientation = AlignedQuat {
            data: orientation.normalize().to_array(),
        };
        let mut motion = MaybeUninit::<hknpMotionCinfo>::zeroed();
        // SAFETY: as above.
        let motion = unsafe {
            (*motion.as_mut_ptr()).Construct();
            motion.assume_init_mut()
        };
        unsafe { motion.initializeWithMass(cinfo, 1, spec.mass_kg) };
        // A hand at rest on a table must still follow its controller.
        motion.m_enableDeactivation = false;

        let mut body_id = PfxCharacterInstance::INVALID_BODY_ID;
        unsafe {
            world.world_mut().createDynamicBody(
                &mut body_id,
                cinfo,
                motion,
                hknpAdditionMode::DO_NOT_ADD,
                0,
            )
        };
        // The body holds its own reference to the shape.
        if owned {
            unsafe { release(shape.cast::<hkReferencedObject>()) };
        }
        if body_id == PfxCharacterInstance::INVALID_BODY_ID {
            free_memory();
            return Err(BodyError::NoBodyId);
        }

        let no_data = [0u64; 2];
        let wrapper = unsafe { &mut *memory.as_ptr() };
        unsafe {
            wrapper.Construct(&body_id, &no_data, false);
            wrapper.SetCollisionLayer(PfxLayer::WEAPONS as u32);
            wrapper.SetCollisionSystemGroup(spec.system_group);
            if !spec.gravity {
                wrapper.SetGravityFactor(0.0);
            }
        }
        let body = Self {
            wrapper: memory,
            body_id,
            spec,
            source,
        };
        if spec.contact_events
            && let Err(e) = unsafe { contact::listen(memory, source) }
        {
            unsafe { body.destroy() };
            return Err(BodyError::Contacts(e));
        }
        Ok(body)
    }

    pub fn body_id(&self) -> u32 {
        self.body_id
    }

    pub fn spec(&self) -> BodySpec {
        self.spec
    }

    /// The body's collision-filter word, read back from the world.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn filter_info(&self) -> Result<u32, BodyError> {
        let world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
        let slot = unsafe { self.slot(world.world()) }?;
        Ok(slot.m_collisionFilterInfo)
    }

    /// Whether the body asks the solver for its contacts' impulse events.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn raises_contact_events(&self) -> Result<bool, BodyError> {
        let world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
        let slot = unsafe { self.slot(world.world()) }?;
        Ok(slot.m_flags & hknpBody::RAISE_CONTACT_IMPULSE_EVENTS != 0)
    }

    /// Whether the body is in the simulation.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn is_added(&self) -> bool {
        unsafe { self.wrapper.as_ref().IsAddedToWorld() }
    }

    /// Add the body to the simulation.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn add(&mut self) {
        unsafe { self.wrapper.as_mut().AddToWorld() };
    }

    /// Take the body out of the simulation; it keeps existing.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn remove(&mut self) {
        unsafe { self.wrapper.as_mut().RemoveFromWorld(0) };
    }

    /// The body's state, game space, after checking that its slot still holds it.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn motion(&self) -> Result<BodyMotion, BodyError> {
        let world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
        let world = world.world();
        let slot = unsafe { self.slot(world) }?;
        let mut transform = Matrix4::IDENTITY;
        unsafe { self.wrapper.as_ref().GetTransform(&mut transform) };
        let transform = Mat4::from(transform);
        let motion = unsafe { &*world.m_motions.add(slot.m_motionId as usize) };
        let frame = Quat::from_array(motion.m_orientation.data);
        let linear = Vec3::from_array(motion.m_linearVelocity.data);
        let local_angular = Vec3::from_array(motion.m_angularVelocity.data);
        let [ix, iy, iz, _] = motion.m_inverseInertia.map(half_to_f32);
        let position = transform.w_axis.truncate();
        let orientation = Quat::from_mat4(&transform);
        if !(position.is_finite()
            && orientation.is_finite()
            && frame.is_finite()
            && linear.is_finite()
            && local_angular.is_finite()
            && ix.is_finite()
            && iy.is_finite()
            && iz.is_finite())
        {
            return Err(BodyError::BadMotion);
        }
        let frame = frame.normalize();
        let rotation = Mat3::from_quat(frame);
        Ok(BodyMotion {
            position,
            orientation: orientation.normalize(),
            linear,
            angular: frame * local_angular,
            inverse_inertia: rotation
                * Mat3::from_diagonal(Vec3::new(ix, iy, iz))
                * rotation.transpose(),
        })
    }

    /// Give the body the velocities `linear` and `angular`, world space.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn set_velocities(&mut self, linear: Vec3, angular: Vec3) -> Result<(), BodyError> {
        if !(linear.is_finite() && angular.is_finite()) {
            return Err(BodyError::BadMotion);
        }
        let mut world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
        unsafe { self.slot(world.world()) }?;
        unsafe {
            world
                .world_mut()
                .setBodyVelocity(self.body_id, &aligned(linear), &aligned(angular))
        };
        Ok(())
    }

    /// Put the body at the game-space pose `position`, `orientation`, at rest.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn place(&mut self, position: Vec3, orientation: Quat) -> Result<(), BodyError> {
        if !(position.is_finite() && orientation.is_finite()) {
            return Err(BodyError::BadMotion);
        }
        let transform = Matrix4::from(Mat4::from_rotation_translation(
            orientation.normalize(),
            position,
        ));
        unsafe { self.wrapper.as_mut().SetTransform(&transform) };
        unsafe { self.set_velocities(Vec3::ZERO, Vec3::ZERO) }
    }

    /// Delete the wrapper, which removes the body from the simulation and destroys it, when the
    /// body's slot still holds it; otherwise forget the wrapper without touching the engine
    /// ([`HandBody::abandon`]). Returns whether it deleted the wrapper.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn destroy(self) -> bool {
        let held = unsafe { World::get() }
            .is_some_and(|world| unsafe { self.slot(world.world()) }.is_ok());
        if !held {
            self.abandon();
            return false;
        }
        contact::forget(self.source);
        unsafe { (*self.wrapper.as_ptr()).Destruct(PfxRigidBody::DESTRUCT_FREE) };
        true
    }

    /// Forget the wrapper without touching the engine: for a body whose slot no longer holds it.
    pub fn abandon(self) {
        contact::abandon(self.source);
    }

    /// Give the body an impulse `impulse` at the game-space point `at`, world space.
    ///
    /// # Safety
    ///
    /// As [`HandBody::create`].
    pub unsafe fn apply_impulse(&mut self, impulse: Vec3, at: Vec3) -> Result<(), BodyError> {
        if !(impulse.is_finite() && at.is_finite()) {
            return Err(BodyError::BadMotion);
        }
        let mut world = unsafe { World::get() }.ok_or(BodyError::NoPhysicsWorld)?;
        unsafe { self.slot(world.world()) }?;
        let at = aligned(at - world.shift);
        unsafe {
            world
                .world_mut()
                .applyBodyImpulseAt(self.body_id, &aligned(impulse), &at)
        };
        Ok(())
    }

    /// The body's slot, checked to still hold this wrapper's body: the id has a slot with a
    /// motion, and the slot's engine instance is this wrapper.
    unsafe fn slot<'w>(&self, world: &'w hknpWorld) -> Result<&'w hknpBody, BodyError> {
        if self.body_id > world.m_peakBodyIndex
            || world.m_bodies.is_null()
            || world.m_motions.is_null()
        {
            return Err(BodyError::Lost);
        }
        let slot = unsafe { &*world.m_bodies.add(self.body_id as usize) };
        if slot.m_motionId == hknpBody::INVALID_MOTION_ID {
            return Err(BodyError::Lost);
        }
        if unsafe { instance_of(world, self.body_id) } != Some(self.wrapper.as_ptr() as usize) {
            return Err(BodyError::Lost);
        }
        Ok(slot)
    }
}

/// The local player's collision system group, or `None` without a player or a body, or when the
/// player's body is in system group zero, where a hand body would collide with it.
///
/// # Safety
///
/// As [`HandBody::create`].
pub unsafe fn player_system_group() -> Option<u32> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetCharacterRigidBodyId(&mut body) };
    let world = unsafe { World::get() }?;
    let world = world.world();
    if body == PfxCharacterInstance::INVALID_BODY_ID
        || body > world.m_peakBodyIndex
        || world.m_bodies.is_null()
    {
        return None;
    }
    let slot = unsafe { &*world.m_bodies.add(body as usize) };
    let group = slot.m_collisionFilterInfo >> CustomGroupCollisionFilter::SYSTEM_GROUP_SHIFT;
    (group != 0).then_some(group)
}

/// The engine instance of the body `body`, as an address.
pub(super) unsafe fn instance_of(world: &hknpWorld, body: u32) -> Option<usize> {
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

/// Drop one reference to `object`, destroying it when it was the last, as the engine's inline
/// release does: an object whose allocation-size half is zero is not counted.
pub(super) unsafe fn release(object: *mut hkReferencedObject) {
    let Some(object) = (unsafe { object.as_mut() }) else {
        return;
    };
    // SAFETY: the word is a naturally aligned `u32` the engine changes only atomically.
    let word = unsafe { AtomicU32::from_ptr(&raw mut object.m_memSizeAndRefCount) };
    if word.load(Ordering::Relaxed) >> hkReferencedObject::MEMORY_SIZE_SHIFT == 0 {
        return;
    }
    let count = hkReferencedObject::REFERENCE_COUNT_MASK;
    let previous = word
        .try_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            Some((v & !count) | (v.wrapping_sub(1) & count))
        })
        .unwrap_or_else(|v| v);
    if previous & count == 1 {
        unsafe { object.deleteThisReferencedObject() };
    }
}

/// A Havok `hkHalf`: the upper 16 bits of an `f32`.
pub(super) fn half_to_f32(half: u16) -> f32 {
    f32::from_bits(u32::from(half) << 16)
}

fn aligned(v: Vec3) -> AlignedVector3 {
    AlignedVector3 {
        data: v.to_array(),
        _w: 0.0,
    }
}
