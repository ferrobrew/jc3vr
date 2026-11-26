//! A grip's joint: a fixed constraint between a hand body and the body of a loose object the hand
//! holds, or of the gun it holds, made the way the magnet weapon makes its own
//! (`docs/engine/physics/havok-bodies.md`, "Joining bodies with a fixed constraint"), or a point
//! (ball-and-socket) constraint at the palm, which holds only the contact point and leaves the
//! rotation to the object's own constraints ([`JointKind`]).
//!
//! The constraint's data comes from the calling thread's Havok heap ([`hkMemoryRouter::m_heap`]),
//! is constructed with one reference, the joint's, and is given the two bodies' frames: the
//! constraint frame is the hand body's own frame, and the same frame in the object's body space,
//! so the two bodies are held in the pose they had when the joint was made, or at a supplied palm
//! frame on the object for an anatomical regrip. The world's
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

use glam::{Affine3A, Vec3};
use jc3gi::{
    havok::{
        hkMemoryRouter, hkReferencedObject, hknpConstraintCinfo, hkpBallAndSocketConstraintData,
        hkpFixedConstraintData,
    },
    physics::{hknpConstraint, hknpConstraintManager, hknpWorld},
};
use jc3vr_body::controllers::physics::{
    bodies,
    bodies::BodyError as ObjectError,
    havok::{affine, transform},
};
use jc3vr_input::frame::Pose;
use windows::Win32::System::Threading::TlsGetValue;

use crate::controllers::physical_hands::{physics::body::release, world::World};

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
    /// The desired palm frame is not finite or has no rotation.
    InvalidPalm,
}

impl fmt::Display for JointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            JointError::NoPhysicsWorld => "the physics world does not exist",
            JointError::NoHeap => "the thread has no Havok heap",
            JointError::NoMemory => "the Havok heap gave no memory for the constraint's data",
            JointError::NoBody => "a body's slot is empty",
            JointError::Object(e) => return write!(f, "hand bodies: joint: {e}"),
            JointError::InvalidPalm => "the desired palm frame is invalid",
            JointError::NoConstraint => "the world made no enabled constraint",
        };
        write!(f, "hand bodies: joint: {what}")
    }
}

impl std::error::Error for JointError {}

/// Which constraint joins a hand to what it holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JointKind {
    /// The two bodies held in a fixed pose: the hand turns the object.
    Fixed,
    /// The contact point held on the palm, the rotation free: for an object that hangs on
    /// constraints of its own, two hands on one ragdoll, where each hand's fixed pose fought the
    /// other's and the ragdoll's joint limits and the body rang between them.
    Point,
}

/// A constraint between a hand body and an object's body.
#[derive(Debug, PartialEq, Eq)]
pub struct Joint {
    id: u32,
    hand_body: u32,
    object_body: u32,
    /// The object that owns the other body, which must still own it; `None` for a body of the
    /// mod's own, a held gun's.
    object: Option<NonZeroUsize>,
    /// The constraint's data, as an address, which the world's slot must still hold.
    data: usize,
    kind: JointKind,
}

impl Joint {
    /// Join at a common world-space frame without introducing an initial constraint error: the
    /// constraint frame is `palm` in both bodies, so an anatomical regrip holds the hand's original
    /// contact on the object without a snap.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step; `hand` must be a live hand body
    /// in the simulation.
    pub unsafe fn create_at(
        hand: u32,
        object_body: u32,
        object: NonZeroUsize,
        palm: Pose,
        kind: JointKind,
    ) -> Result<Self, JointError> {
        unsafe { bodies::read(object_body, object) }.map_err(JointError::Object)?;
        unsafe { Self::join(hand, object_body, Some(object), Some(palm), kind) }
    }

    /// Join the hand body `hand` to the body `body`, one of the mod's own, in their present
    /// relative pose.
    ///
    /// # Safety
    ///
    /// As [`Joint::create_at`]; `body` must be a live body in the simulation.
    pub unsafe fn create_between(hand: u32, body: u32) -> Result<Self, JointError> {
        unsafe { Self::join(hand, body, None, None, JointKind::Fixed) }
    }

    unsafe fn join(
        hand: u32,
        object_body: u32,
        object: Option<NonZeroUsize>,
        palm: Option<Pose>,
        kind: JointKind,
    ) -> Result<Self, JointError> {
        let mut world = unsafe { World::get() }.ok_or(JointError::NoPhysicsWorld)?;
        let slot = |body| unsafe { bodies::slot(world.world(), body) }.ok_or(JointError::NoBody);
        let actual_hand_frame = affine(&slot(hand)?.m_transform);
        let hand_frame = match palm {
            Some(palm) => palm_frame(palm, world.shift).ok_or(JointError::InvalidPalm)?,
            None => actual_hand_frame,
        };
        let object_frame = affine(&slot(object_body)?.m_transform);
        let (pivot_a, pivot_b) = coincident_pivots(actual_hand_frame, object_frame, hand_frame);
        let pivot_a = transform(pivot_a);
        let pivot_b = transform(pivot_b);

        let data: *mut hkReferencedObject = match kind {
            JointKind::Fixed => {
                let data = unsafe { allocate(size_of::<hkpFixedConstraintData>()) }?
                    .cast::<hkpFixedConstraintData>();
                // SAFETY: fresh heap memory of the data's size, constructed before any other use.
                unsafe {
                    (*data).Construct();
                    (*data).setInBodySpace(&pivot_a, &pivot_b);
                }
                data.cast()
            }
            JointKind::Point => {
                let data = unsafe { allocate(size_of::<hkpBallAndSocketConstraintData>()) }?
                    .cast::<hkpBallAndSocketConstraintData>();
                // SAFETY: as above. The pivots are the contact frame's origin in each body; the
                // stabilized solving method matches the fixed joint's.
                unsafe {
                    (*data).Construct();
                    (*data).m_translationA = pivot_a.m_translation;
                    (*data).m_translationB = pivot_b.m_translation;
                    (*data).m_setupStabilizationEnabled = true;
                    (*data).m_ballSocketSolvingMethod = 0;
                }
                data.cast()
            }
        };
        let mut cinfo = unsafe { MaybeUninit::<hknpConstraintCinfo>::zeroed().assume_init() };
        cinfo.m_constraintData = data;
        cinfo.m_bodyA = hand;
        cinfo.m_bodyB = object_body;
        let mut id = hknpConstraintManager::INVALID_CONSTRAINT_ID;
        unsafe { world.world_mut().createConstraint(&mut id, &cinfo) };
        let joint = Self {
            id,
            hand_body: hand,
            object_body,
            object,
            data: data as usize,
            kind,
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

    /// Which constraint the joint is.
    pub fn kind(&self) -> JointKind {
        self.kind
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// The object and the body the joint holds; no object for a body of the mod's own.
    pub fn target(&self) -> (Option<NonZeroUsize>, u32) {
        (self.object, self.object_body)
    }

    /// Whether the world still holds the joint's constraint, and the object still owns its body.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step; `hand` must be a live hand body
    /// in the simulation.
    pub unsafe fn intact(&self) -> bool {
        let Some(world) = (unsafe { World::get() }) else {
            return false;
        };
        unsafe { self.entry(world.world()) }.is_some()
            && self
                .object
                .is_none_or(|object| unsafe { bodies::read(self.object_body, object) }.is_ok())
    }

    /// Destroy the joint's constraint, if the world's slot still holds it. Returns whether it did.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step; `hand` must be a live hand body
    /// in the simulation.
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

fn coincident_pivots(hand: Affine3A, object: Affine3A, contact: Affine3A) -> (Affine3A, Affine3A) {
    (pivot_in(hand, contact), pivot_in(object, contact))
}

fn palm_frame(palm: Pose, shift: Vec3) -> Option<Affine3A> {
    let position = palm.position - shift;
    let length = palm.orientation.length_squared();
    (position.is_finite() && length.is_finite() && length > f32::EPSILON)
        .then(|| Affine3A::from_rotation_translation(palm.orientation.normalize(), position))
}

/// Memory of `size` bytes for constraint data, from the calling thread's Havok heap.
unsafe fn allocate(size: usize) -> Result<*mut c_void, JointError> {
    // SAFETY: reads this thread's own slot of the engine's thread-local index.
    let router =
        unsafe { TlsGetValue(*hkMemoryRouter::get_s_memoryRouterSlot()) }.cast::<hkMemoryRouter>();
    let router = unsafe { router.as_mut() }.ok_or(JointError::NoHeap)?;
    let heap = unsafe { router.m_heap.as_mut() }.ok_or(JointError::NoHeap)?;
    let memory: *mut c_void = unsafe { heap.blockAlloc(size as i32) };
    if memory.is_null() {
        return Err(JointError::NoMemory);
    }
    Ok(memory)
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
    fn a_regrip_uses_the_original_contact_frame_not_the_lagging_hand() {
        let shift = Vec3::new(1000.0, 2000.0, -3000.0);
        let object = frame(Vec3::Y, 0.6, Vec3::new(2.0, 3.0, 4.0));
        let desired = frame(Vec3::X, -0.3, Vec3::new(2.2, 3.4, 4.1));
        let palm = Pose {
            position: Vec3::from(desired.translation) + shift,
            orientation: Quat::from_mat3a(&desired.matrix3),
        };
        let pivot = pivot_in(object, palm_frame(palm, shift).unwrap());
        assert!((object * pivot).abs_diff_eq(desired, 2e-4));
        let lagging = frame(Vec3::Z, 0.2, Vec3::new(2.0, 5.0, 4.0));
        let (on_hand, on_object) =
            coincident_pivots(lagging, object, palm_frame(palm, shift).unwrap());
        assert!((lagging * on_hand).abs_diff_eq(object * on_object, 2e-4));
        assert!(!on_hand.abs_diff_eq(Affine3A::IDENTITY, 0.01));
        assert!(!pivot.abs_diff_eq(pivot_in(object, lagging), 0.01));
        assert!(
            palm_frame(
                Pose {
                    position: Vec3::NAN,
                    ..palm
                },
                shift
            )
            .is_none()
        );
        assert!(
            palm_frame(
                Pose {
                    orientation: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
                    ..palm
                },
                shift
            )
            .is_none()
        );
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
