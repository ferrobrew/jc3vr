//! A grip's joint: a fixed constraint between a hand body and the body of a loose object the hand
//! holds, made the way the magnet weapon makes its own (`docs/engine/physics/havok-bodies.md`,
//! "Joining bodies with a fixed constraint").
//!
//! The constraint's data comes from the calling thread's Havok heap ([`hkMemoryRouter::m_heap`]),
//! is constructed with one reference, the joint's, and is given the two bodies' frames: the
//! constraint frame is the hand body's own frame, and the same frame in the object's body space,
//! so the two bodies are held in the pose they had when the joint was made. The world's
//! constraint takes a reference of its own ([`hknpWorld::createConstraint`]), and the joint
//! releases its reference at once, so destroying the constraint ([`hknpWorld::destroyConstraints`])
//! frees the data. The joint keeps only the constraint's id, and checks that the world's slot for
//! it still holds its constraint (its two bodies and its data) before it destroys it, so a slot the
//! world has since reused is never touched.
//!
//! Every function here changes the physics world, so it runs only on the thread that steps it,
//! outside the step: from the pre-simulation callback, or from the shutdown cleanup on the game
//! thread. Both have a Havok memory router.

use std::{ffi::c_void, fmt, mem::MaybeUninit, num::NonZeroUsize};

use glam::{Affine3A, Mat3A, Vec3, Vec3A};
use jc3gi::{
    havok::{
        hkMemoryRouter, hkReferencedObject, hkTransformf, hknpConstraintCinfo,
        hkpFixedConstraintData,
    },
    physics::{PfxCharacterInstance, hknpBody, hknpConstraint, hknpConstraintManager, hknpWorld},
    types::vector_math::AlignedVector3,
};
use windows::Win32::System::Threading::TlsGetValue;

use crate::controllers::physical_hands::{
    bodies::{self, BodyError as ObjectError},
    physics::body::release,
    world::World,
};

/// Why a joint could not be made.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JointError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// The calling thread has no Havok memory router or heap.
    NoHeap,
    /// The Havok heap returned nothing for the constraint's data.
    NoMemory,
    /// A body's slot does not hold a body.
    NoBody,
    /// The object's body cannot be held.
    Object(ObjectError),
    /// The world made no constraint, or made it disabled.
    NoConstraint,
}

impl fmt::Display for JointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            JointError::NoPhysicsWorld => "the physics world does not exist",
            JointError::NoHeap => "the thread has no Havok heap",
            JointError::NoMemory => "the Havok heap gave no memory for the constraint's data",
            JointError::NoBody => "a body's slot is empty",
            JointError::Object(e) => return write!(f, "hand bodies: joint: {e}"),
            JointError::NoConstraint => "the world made no enabled constraint",
        };
        write!(f, "hand bodies: joint: {what}")
    }
}

impl std::error::Error for JointError {}

/// A fixed constraint between a hand body and an object's body.
#[derive(Debug, PartialEq, Eq)]
pub struct Joint {
    id: u32,
    hand_body: u32,
    object_body: u32,
    object: NonZeroUsize,
    /// The constraint's data, as an address, which the world's slot must still hold.
    data: usize,
}

impl Joint {
    /// Join the hand body `hand` to the body `object_body` of `object` in their present relative
    /// pose.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step; `hand` must be a live hand body
    /// in the simulation.
    pub unsafe fn create(
        hand: u32,
        object_body: u32,
        object: NonZeroUsize,
    ) -> Result<Self, JointError> {
        unsafe { bodies::read(object_body, object) }.map_err(JointError::Object)?;
        let mut world = unsafe { World::get() }.ok_or(JointError::NoPhysicsWorld)?;
        let hand_frame = affine(&unsafe { slot(world.world(), hand) }?.m_transform);
        let object_frame = affine(&unsafe { slot(world.world(), object_body) }?.m_transform);
        let pivot_a = transform(Affine3A::IDENTITY);
        let pivot_b = transform(pivot_in(object_frame, hand_frame));

        let data = unsafe { allocate() }?;
        // SAFETY: fresh heap memory of the data's size, constructed before any other use.
        let data = unsafe {
            (*data).Construct();
            (*data).setInBodySpace(&pivot_a, &pivot_b);
            &mut *data
        };
        let mut cinfo = unsafe { MaybeUninit::<hknpConstraintCinfo>::zeroed().assume_init() };
        cinfo.m_constraintData = (data as *mut hkpFixedConstraintData).cast::<hkReferencedObject>();
        cinfo.m_bodyA = hand;
        cinfo.m_bodyB = object_body;
        let mut id = hknpConstraintManager::INVALID_CONSTRAINT_ID;
        unsafe { world.world_mut().createConstraint(&mut id, &cinfo) };
        let joint = Self {
            id,
            hand_body: hand,
            object_body,
            object,
            data: data as *mut hkpFixedConstraintData as usize,
        };
        let made = unsafe { joint.entry(world.world()) }.map(|entry| entry.m_flags);
        // The world's constraint holds its own reference; this one was the joint's.
        unsafe { release(cinfo.m_constraintData) };
        match made {
            Some(flags) if flags & hknpConstraint::IS_DISABLED == 0 => Ok(joint),
            Some(_) => {
                unsafe { joint.destroy() };
                Err(JointError::NoConstraint)
            }
            None => Err(JointError::NoConstraint),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// The object and the body the joint holds.
    pub fn target(&self) -> (NonZeroUsize, u32) {
        (self.object, self.object_body)
    }

    /// Whether the world still holds the joint's constraint, and the object still owns its body.
    ///
    /// # Safety
    ///
    /// As [`Joint::create`].
    pub unsafe fn intact(&self) -> bool {
        let Some(world) = (unsafe { World::get() }) else {
            return false;
        };
        unsafe { self.entry(world.world()) }.is_some()
            && unsafe { bodies::read(self.object_body, self.object) }.is_ok()
    }

    /// Destroy the joint's constraint, if the world's slot still holds it. Returns whether it did.
    ///
    /// # Safety
    ///
    /// As [`Joint::create`].
    pub unsafe fn destroy(self) -> bool {
        let Some(mut world) = (unsafe { World::get() }) else {
            return false;
        };
        if unsafe { self.entry(world.world()) }.is_none() {
            return false;
        }
        unsafe { world.world_mut().destroyConstraints(&self.id, 1) };
        true
    }

    /// The world's slot for the joint's constraint, if it still holds it.
    unsafe fn entry<'w>(&self, world: &'w hknpWorld) -> Option<&'w hknpConstraint> {
        let manager = &world.m_constraintManager;
        if self.id == hknpConstraintManager::INVALID_CONSTRAINT_ID
            || self.id >= manager.m_capacity
            || manager.m_constraintBuffer.is_null()
        {
            return None;
        }
        let entry = unsafe { &*manager.m_constraintBuffer.add(self.id as usize) };
        (entry.m_bodyIdA == self.hand_body
            && entry.m_bodyIdB == self.object_body
            && entry.m_data as usize == self.data)
            .then_some(entry)
    }
}

/// The frame of the hand body `hand` in the body space of the object's body `object`: where the
/// constraint holds the hand's frame on the object.
pub fn pivot_in(object: Affine3A, hand: Affine3A) -> Affine3A {
    object.inverse() * hand
}

/// A Havok transform as an affine transform: its rotation's columns are the images of the local
/// axes.
pub fn affine(t: &hkTransformf) -> Affine3A {
    let [x, y, z] = t.m_rotation.map(|c| Vec3A::from_array(c.data));
    Affine3A {
        matrix3: Mat3A::from_cols(x, y, z),
        translation: Vec3A::from_array(t.m_translation.data),
    }
}

/// An affine transform as a Havok transform.
pub fn transform(a: Affine3A) -> hkTransformf {
    let column = |v: Vec3A| AlignedVector3 {
        data: Vec3::from(v).to_array(),
        _w: 0.0,
    };
    hkTransformf {
        m_rotation: [
            column(a.matrix3.x_axis),
            column(a.matrix3.y_axis),
            column(a.matrix3.z_axis),
        ],
        m_translation: column(a.translation),
    }
}

/// The world's slot for the body `body`, checked to hold a body.
unsafe fn slot(world: &hknpWorld, body: u32) -> Result<&hknpBody, JointError> {
    if body == PfxCharacterInstance::INVALID_BODY_ID
        || body > world.m_peakBodyIndex
        || world.m_bodies.is_null()
    {
        return Err(JointError::NoBody);
    }
    let slot = unsafe { &*world.m_bodies.add(body as usize) };
    if slot.m_motionId == hknpBody::INVALID_MOTION_ID {
        return Err(JointError::NoBody);
    }
    Ok(slot)
}

/// Fixed constraint data's memory, from the calling thread's Havok heap.
unsafe fn allocate() -> Result<*mut hkpFixedConstraintData, JointError> {
    // SAFETY: reads this thread's own slot of the engine's thread-local index.
    let router =
        unsafe { TlsGetValue(*hkMemoryRouter::get_s_memoryRouterSlot()) }.cast::<hkMemoryRouter>();
    let router = unsafe { router.as_mut() }.ok_or(JointError::NoHeap)?;
    let heap = unsafe { router.m_heap.as_mut() }.ok_or(JointError::NoHeap)?;
    let memory: *mut c_void =
        unsafe { heap.blockAlloc(size_of::<hkpFixedConstraintData>() as i32) };
    if memory.is_null() {
        return Err(JointError::NoMemory);
    }
    Ok(memory.cast())
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn frame(axis: Vec3, angle: f32, at: Vec3) -> Affine3A {
        Affine3A::from_rotation_translation(Quat::from_axis_angle(axis.normalize(), angle), at)
    }

    #[test]
    fn a_transform_survives_the_havok_layout() {
        let a = frame(Vec3::new(1.0, 2.0, 0.5), 0.7, Vec3::new(3.0, -1.0, 8.0));
        let back = affine(&transform(a));
        assert!(back.abs_diff_eq(a, 1e-6));
        // The columns are the images of the local axes.
        let t = transform(a);
        let x = Vec3::from_array(t.m_rotation[0].data);
        assert!(x.abs_diff_eq(a.transform_vector3(Vec3::X), 1e-6));
    }

    #[test]
    fn the_pivot_puts_the_hand_frame_on_the_object() {
        let object = frame(Vec3::Y, 1.2, Vec3::new(10.0, 2.0, -4.0));
        let hand = frame(Vec3::new(0.3, 1.0, 0.0), -0.4, Vec3::new(10.5, 2.3, -3.8));
        let pivot = pivot_in(object, hand);
        // The object's frame carried by the pivot is the hand's frame.
        assert!((object * pivot).abs_diff_eq(hand, 1e-5));
        // Moving both together leaves the pivot unchanged.
        let moved = frame(Vec3::Z, 0.9, Vec3::new(-2.0, 5.0, 1.0));
        assert!(pivot_in(moved * object, moved * hand).abs_diff_eq(pivot, 1e-5));
    }
}
