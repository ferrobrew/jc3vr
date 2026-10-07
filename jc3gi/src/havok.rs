#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A Havok reference-counted object (`hkReferencedObject`), the base of Havok shapes, constraint
/// data, and most Havok heap objects. Only its reference count is bound.
pub struct hkReferencedObject {
    vftable: *const crate::havok::hkReferencedObjectVftable,
    /// The allocation size and the reference count in one word: the low half
    /// ([`REFERENCE_COUNT_MASK`](crate::havok::hkReferencedObject::REFERENCE_COUNT_MASK)) counts the references, and the high
    /// half (from [`MEMORY_SIZE_SHIFT`](crate::havok::hkReferencedObject::MEMORY_SIZE_SHIFT)) is the size of the object's Havok
    /// heap allocation. An object whose size half is zero is not counted: it was not allocated from
    /// the heap, and a release leaves it alone. A release atomically decrements the low half
    /// without touching the high half, and an object whose count was one is destroyed through
    /// [`deleteThisReferencedObject`](crate::havok::hkReferencedObject::deleteThisReferencedObject). A new heap object starts
    /// with a count of one.
    pub m_memSizeAndRefCount: u32,
    _field_c: [u8; 4],
}
fn _hkReferencedObject_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], hkReferencedObject>([0u8; 0x10]);
    }
    unreachable!()
}
impl hkReferencedObject {
    pub fn vftable(&self) -> *const crate::havok::hkReferencedObjectVftable {
        self.vftable as *const crate::havok::hkReferencedObjectVftable
    }
    /// Destroys the object and returns its memory to the Havok heap it came from. The engine
    /// calls it when a release drops the last reference.
    pub unsafe fn deleteThisReferencedObject(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).deleteThisReferencedObject).read();
            f(self as *mut Self as _)
        }
    }
}
impl hkReferencedObject {
    /// Where the allocation-size half of [`m_memSizeAndRefCount`](crate::havok::hkReferencedObject::m_memSizeAndRefCount) begins.
    pub const MEMORY_SIZE_SHIFT: u32 = 16;
    /// The reference-count half of [`m_memSizeAndRefCount`](crate::havok::hkReferencedObject::m_memSizeAndRefCount).
    pub const REFERENCE_COUNT_MASK: u32 = 65535;
}
impl std::convert::AsRef<hkReferencedObject> for hkReferencedObject {
    fn as_ref(&self) -> &hkReferencedObject {
        self
    }
}
impl std::convert::AsMut<hkReferencedObject> for hkReferencedObject {
    fn as_mut(&mut self) -> &mut hkReferencedObject {
        self
    }
}
#[repr(C, align(8))]
pub struct hkReferencedObjectVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::havok::hkReferencedObject),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::havok::hkReferencedObject),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::havok::hkReferencedObject),
    /// Destroys the object and returns its memory to the Havok heap it came from. The engine
    /// calls it when a release drops the last reference.
    pub deleteThisReferencedObject: unsafe extern "system" fn(
        this: *mut crate::havok::hkReferencedObject,
    ),
}
fn _hkReferencedObjectVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], hkReferencedObjectVftable>([0u8; 0x20]);
    }
    unreachable!()
}
impl hkReferencedObjectVftable {}
impl std::convert::AsRef<hkReferencedObjectVftable> for hkReferencedObjectVftable {
    fn as_ref(&self) -> &hkReferencedObjectVftable {
        self
    }
}
impl std::convert::AsMut<hkReferencedObjectVftable> for hkReferencedObjectVftable {
    fn as_mut(&mut self) -> &mut hkReferencedObjectVftable {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(16))]
/// A Havok transform (`hkTransformf`): a rotation as three column vectors, the images of the local
/// axes, and a translation.
pub struct hkTransformf {
    pub m_rotation: [crate::types::vector_math::AlignedVector3; 3],
    pub m_translation: crate::types::vector_math::AlignedVector3,
}
fn _hkTransformf_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], hkTransformf>([0u8; 0x40]);
    }
    unreachable!()
}
impl hkTransformf {}
impl std::convert::AsRef<hkTransformf> for hkTransformf {
    fn as_ref(&self) -> &hkTransformf {
        self
    }
}
impl std::convert::AsMut<hkTransformf> for hkTransformf {
    fn as_mut(&mut self) -> &mut hkTransformf {
        self
    }
}
#[repr(C, align(16))]
/// The construction info of a Havok body (`hknpBodyCinfo`), read by
/// [`hknpWorld::createDynamicBody`](crate::physics::hknpWorld::createDynamicBody). Positions are Havok
/// space: the game-space position less
/// [`PhysicsSystem::m_AccumulatedShift`](crate::physics::PhysicsSystem::m_AccumulatedShift). Only some
/// members are bound; [`Construct`](crate::havok::hknpBodyCinfo::Construct) sets the others to their defaults: no reserved
/// body id, no motion, the default quality and material, a zero collision-filter word, no name, no
/// local frame, and the identity orientation.
pub struct hknpBodyCinfo {
    /// The body's shape. Creating the body takes a reference of its own.
    pub m_shape: *mut crate::havok::hknpShape,
    _field_8: [u8; 40],
    /// The body's position.
    pub m_position: crate::types::vector_math::AlignedVector3,
    /// The body's orientation, a unit quaternion `(x, y, z, w)`.
    pub m_orientation: crate::types::vector_math::AlignedQuat,
    _field_50: [u8; 16],
}
fn _hknpBodyCinfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], hknpBodyCinfo>([0u8; 0x60]);
    }
    unreachable!()
}
impl hknpBodyCinfo {
    pub const Construct_ADDRESS: usize = 0x141500080;
    /// `hknpBodyCinfo::hknpBodyCinfo`: constructs the info with its defaults.
    pub unsafe fn Construct(&mut self) -> *mut crate::havok::hknpBodyCinfo {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::havok::hknpBodyCinfo = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<hknpBodyCinfo> for hknpBodyCinfo {
    fn as_ref(&self) -> &hknpBodyCinfo {
        self
    }
}
impl std::convert::AsMut<hknpBodyCinfo> for hknpBodyCinfo {
    fn as_mut(&mut self) -> &mut hknpBodyCinfo {
        self
    }
}
#[repr(C, align(8))]
/// A capsule shape (`hknpCapsuleShape`): the points within a radius of a segment, built as a convex
/// polytope with that radius as its convex radius.
pub struct hknpCapsuleShape {}
impl hknpCapsuleShape {
    pub const createCapsuleShape_ADDRESS: usize = 0x141537F70;
    /// Allocates a capsule from the calling thread's Havok heap, with the segment from `a` to `b`
    /// (body space) and `radius`, and returns it with one reference, the caller's. The grappling
    /// hook's hang proxy (`CGrapplingHangPhysicsProxy::Init`) builds its body's capsule with it and
    /// releases its own reference once the body is created.
    pub unsafe fn createCapsuleShape(
        a: *const crate::types::vector_math::AlignedVector3,
        b: *const crate::types::vector_math::AlignedVector3,
        radius: f32,
    ) -> *mut crate::havok::hknpShape {
        unsafe {
            let f: unsafe extern "system" fn(
                a: *const crate::types::vector_math::AlignedVector3,
                b: *const crate::types::vector_math::AlignedVector3,
                radius: f32,
            ) -> *mut crate::havok::hknpShape = ::std::mem::transmute(
                Self::createCapsuleShape_ADDRESS,
            );
            f(a, b, radius)
        }
    }
}
impl std::convert::AsRef<hknpCapsuleShape> for hknpCapsuleShape {
    fn as_ref(&self) -> &hknpCapsuleShape {
        self
    }
}
impl std::convert::AsMut<hknpCapsuleShape> for hknpCapsuleShape {
    fn as_mut(&mut self) -> &mut hknpCapsuleShape {
        self
    }
}
#[repr(C, align(8))]
/// The construction info of a constraint (`hknpConstraintCinfo`), read by
/// [`hknpWorld::createConstraint`](crate::physics::hknpWorld::createConstraint).
pub struct hknpConstraintCinfo {
    /// The constraint's data, such as a [`hkpFixedConstraintData`](crate::havok::hkpFixedConstraintData) (`hkRefPtr<hkpConstraintData>`).
    pub m_constraintData: *mut crate::havok::hkReferencedObject,
    /// The constraint's first body.
    pub m_bodyA: u32,
    /// The constraint's second body.
    pub m_bodyB: u32,
    /// The constraint's flags; the engine's callers pass zero.
    pub m_flags: u8,
    _field_11: [u8; 7],
}
fn _hknpConstraintCinfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], hknpConstraintCinfo>([0u8; 0x18]);
    }
    unreachable!()
}
impl hknpConstraintCinfo {}
impl std::convert::AsRef<hknpConstraintCinfo> for hknpConstraintCinfo {
    fn as_ref(&self) -> &hknpConstraintCinfo {
        self
    }
}
impl std::convert::AsMut<hknpConstraintCinfo> for hknpConstraintCinfo {
    fn as_mut(&mut self) -> &mut hknpConstraintCinfo {
        self
    }
}
#[repr(C, align(16))]
/// A contact-impulse event (`hknpContactImpulseEvent`), raised by the solver for a contact between
/// two bodies at least one of which carries
/// [`RAISE_CONTACT_IMPULSE_EVENTS`](crate::physics::hknpBody::RAISE_CONTACT_IMPULSE_EVENTS). The contact
/// impulse event creator (`hknpContactImpulseEventCreator::postContactSolve`) writes one into the
/// solving thread's command stream after it solves the contact, and the world dispatches the
/// streams in its post-solve phase (`hknpWorldEx::postSolve`), the last task
/// [`hknpWorld::stepSolve`](crate::physics::hknpWorld::stepSolve) generates, which it runs alone on its
/// calling thread. For a contact that keeps a collision cache, the creator raises
/// [`STATUS_STARTED`](crate::havok::hknpContactImpulseEvent::STATUS_STARTED) on the first step the contact's impulse is positive,
/// and raises [`STATUS_CONTINUED`](crate::havok::hknpContactImpulseEvent::STATUS_CONTINUED) and
/// [`STATUS_FINISHED`](crate::havok::hknpContactImpulseEvent::STATUS_FINISHED) only for a contact a listener asked them of; a
/// contact without a cache raises [`STATUS_IMMEDIATE`](crate::havok::hknpContactImpulseEvent::STATUS_IMMEDIATE) each step its
/// impulse is positive.
pub struct hknpContactImpulseEvent {
    _field_0: [u8; 8],
    /// The two bodies, the first and the second.
    pub m_bodyIds: [u32; 2],
    /// The contact's solver Jacobian (`hknpContactJacobian`), null for a finished contact whose
    /// manifold is gone; see [`hknpContactJacobianUtil::getNumContactPoints`](crate::havok::hknpContactJacobianUtil::getNumContactPoints).
    pub m_contactJacobian: *const ::std::ffi::c_void,
    /// The index of the contact's manifold in its Jacobian.
    pub m_manifoldIndex: u8,
    /// The world's contact solver type (`hknpContactSolverType`), which the Jacobian's layout
    /// depends on.
    pub m_contactSolverType: u8,
    _field_1a: [u8; 6],
    /// The event's status, one of the `STATUS_` constants.
    pub m_status: u8,
    _field_21: [u8; 3],
    /// The contact's friction factor.
    pub m_frictionFactor: f32,
    _field_28: [u8; 8],
    /// The impulse the solver applied at each of the contact's points over the step, newton
    /// seconds; only the first [`hknpContactJacobianUtil::getNumContactPoints`](crate::havok::hknpContactJacobianUtil::getNumContactPoints) are the contact's.
    pub m_contactImpulses: [f32; 4],
}
fn _hknpContactImpulseEvent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], hknpContactImpulseEvent>([0u8; 0x40]);
    }
    unreachable!()
}
impl hknpContactImpulseEvent {}
impl hknpContactImpulseEvent {
    /// [`m_status`](crate::havok::hknpContactImpulseEvent::m_status): the contact's impulse stayed positive this step.
    pub const STATUS_CONTINUED: u8 = 3;
    /// [`m_status`](crate::havok::hknpContactImpulseEvent::m_status): the contact's impulse fell to zero this step.
    pub const STATUS_FINISHED: u8 = 2;
    /// [`m_status`](crate::havok::hknpContactImpulseEvent::m_status): a positive impulse on a contact without a collision cache.
    pub const STATUS_IMMEDIATE: u8 = 4;
    /// [`m_status`](crate::havok::hknpContactImpulseEvent::m_status): the contact's impulse became positive this step.
    pub const STATUS_STARTED: u8 = 1;
}
impl std::convert::AsRef<hknpContactImpulseEvent> for hknpContactImpulseEvent {
    fn as_ref(&self) -> &hknpContactImpulseEvent {
        self
    }
}
impl std::convert::AsMut<hknpContactImpulseEvent> for hknpContactImpulseEvent {
    fn as_mut(&mut self) -> &mut hknpContactImpulseEvent {
        self
    }
}
#[repr(C, align(8))]
/// Helpers that read a contact's solver Jacobian (`hknpContactJacobianUtil`).
pub struct hknpContactJacobianUtil {}
impl hknpContactJacobianUtil {
    pub const getNumContactPoints_ADDRESS: usize = 0x14148C090;
    /// The number of contact points, at most four, of the contact whose Jacobian is `jacobian`
    /// under the world's contact solver type `solver_type`. The contact impulse event creator
    /// counts a contact's impulses with it.
    pub unsafe fn getNumContactPoints(
        solver_type: u32,
        jacobian: *const ::std::ffi::c_void,
    ) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(
                solver_type: u32,
                jacobian: *const ::std::ffi::c_void,
            ) -> i32 = ::std::mem::transmute(Self::getNumContactPoints_ADDRESS);
            f(solver_type, jacobian)
        }
    }
}
impl std::convert::AsRef<hknpContactJacobianUtil> for hknpContactJacobianUtil {
    fn as_ref(&self) -> &hknpContactJacobianUtil {
        self
    }
}
impl std::convert::AsMut<hknpContactJacobianUtil> for hknpContactJacobianUtil {
    fn as_mut(&mut self) -> &mut hknpContactJacobianUtil {
        self
    }
}
#[repr(C, align(16))]
/// The construction info of a Havok motion (`hknpMotionCinfo`), the mass properties and velocities
/// of a dynamic body, read by
/// [`hknpWorld::createDynamicBody`](crate::physics::hknpWorld::createDynamicBody). Only some members are
/// bound; [`Construct`](crate::havok::hknpMotionCinfo::Construct) sets the default motion properties, unit mass, the
/// identity orientation, and zero velocities, with deactivation enabled.
pub struct hknpMotionCinfo {
    _field_0: [u8; 2],
    /// Whether the motion may be deactivated: a body at rest for a while is put to sleep, and is
    /// not simulated until something wakes it.
    pub m_enableDeactivation: bool,
    _field_3: [u8; 109],
}
fn _hknpMotionCinfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], hknpMotionCinfo>([0u8; 0x70]);
    }
    unreachable!()
}
impl hknpMotionCinfo {
    pub const Construct_ADDRESS: usize = 0x141505D20;
    /// `hknpMotionCinfo::hknpMotionCinfo`: constructs the info with its defaults.
    pub unsafe fn Construct(&mut self) -> *mut crate::havok::hknpMotionCinfo {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::havok::hknpMotionCinfo = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const initializeWithMass_ADDRESS: usize = 0x141506480;
    /// Fills the mass properties for the `num_bodies` bodies at `bodies` sharing one motion, with a
    /// total of `mass` kilograms: the inverse mass, the inverse inertia from the bodies' shapes, and
    /// the centre of mass and orientation from the bodies' poses. The bodies' shapes, positions,
    /// and orientations must already be set.
    pub unsafe fn initializeWithMass(
        &mut self,
        bodies: *const crate::havok::hknpBodyCinfo,
        num_bodies: i32,
        mass: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                bodies: *const crate::havok::hknpBodyCinfo,
                num_bodies: i32,
                mass: f32,
            ) = ::std::mem::transmute(Self::initializeWithMass_ADDRESS);
            f(self as *mut Self as _, bodies, num_bodies, mass)
        }
    }
}
impl std::convert::AsRef<hknpMotionCinfo> for hknpMotionCinfo {
    fn as_ref(&self) -> &hknpMotionCinfo {
        self
    }
}
impl std::convert::AsMut<hknpMotionCinfo> for hknpMotionCinfo {
    fn as_mut(&mut self) -> &mut hknpMotionCinfo {
        self
    }
}
#[repr(C, align(8))]
/// A Havok collision shape (`hknpShape`), a [`hkReferencedObject`](crate::havok::hkReferencedObject) allocated from the Havok heap of
/// the thread that creates it. A body holds its own reference to its shape. Only the reference
/// count is bound.
pub struct hknpShape {
    pub base: crate::havok::hkReferencedObject,
}
fn _hknpShape_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], hknpShape>([0u8; 0x10]);
    }
    unreachable!()
}
impl hknpShape {
    pub fn vftable(&self) -> *const crate::havok::hkReferencedObjectVftable {
        self.base.vftable() as *const crate::havok::hkReferencedObjectVftable
    }
    /// Destroys the object and returns its memory to the Havok heap it came from. The engine
    /// calls it when a release drops the last reference.
    pub unsafe fn deleteThisReferencedObject(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).deleteThisReferencedObject).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<crate::havok::hkReferencedObject> for hknpShape {
    fn as_ref(&self) -> &crate::havok::hkReferencedObject {
        &self.base
    }
}
impl std::convert::AsMut<crate::havok::hkReferencedObject> for hknpShape {
    fn as_mut(&mut self) -> &mut crate::havok::hkReferencedObject {
        &mut self.base
    }
}
impl std::convert::AsRef<hknpShape> for hknpShape {
    fn as_ref(&self) -> &hknpShape {
        self
    }
}
impl std::convert::AsMut<hknpShape> for hknpShape {
    fn as_mut(&mut self) -> &mut hknpShape {
        self
    }
}
#[repr(C, align(16))]
/// The data of a fixed constraint (`hkpFixedConstraintData`), which holds two bodies in a fixed
/// pose relative to each other. The magnet weapon
/// (`CMagnetWeaponComponent::ConstrainBodiesToGravityField`) allocates one from the Havok heap of
/// the calling thread, constructs it, sets its frames, and creates a constraint from it. The
/// disassembler names that function `CPartInstance::AttachPartWithConstraint`. Only some members
/// are bound.
pub struct hkpFixedConstraintData {
    pub base: crate::havok::hkReferencedObject,
    _field_10: [u8; 208],
}
fn _hkpFixedConstraintData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE0], hkpFixedConstraintData>([0u8; 0xE0]);
    }
    unreachable!()
}
impl hkpFixedConstraintData {
    pub fn vftable(&self) -> *const crate::havok::hkReferencedObjectVftable {
        self.base.vftable() as *const crate::havok::hkReferencedObjectVftable
    }
    pub const Construct_ADDRESS: usize = 0x141354DB0;
    /// `hkpFixedConstraintData::hkpFixedConstraintData`: constructs the data with one reference
    /// and the identity frames.
    pub unsafe fn Construct(&mut self) -> *mut crate::havok::hkpFixedConstraintData {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::havok::hkpFixedConstraintData = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const setInBodySpace_ADDRESS: usize = 0x141355090;
    /// Sets the constraint's frame in each body's space: `pivot_a` in the first body's, `pivot_b`
    /// in the second's. The constraint holds the two frames together.
    pub unsafe fn setInBodySpace(
        &mut self,
        pivot_a: *const crate::havok::hkTransformf,
        pivot_b: *const crate::havok::hkTransformf,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pivot_a: *const crate::havok::hkTransformf,
                pivot_b: *const crate::havok::hkTransformf,
            ) = ::std::mem::transmute(Self::setInBodySpace_ADDRESS);
            f(self as *mut Self as _, pivot_a, pivot_b)
        }
    }
    /// Destroys the object and returns its memory to the Havok heap it came from. The engine
    /// calls it when a release drops the last reference.
    pub unsafe fn deleteThisReferencedObject(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).deleteThisReferencedObject).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<crate::havok::hkReferencedObject> for hkpFixedConstraintData {
    fn as_ref(&self) -> &crate::havok::hkReferencedObject {
        &self.base
    }
}
impl std::convert::AsMut<crate::havok::hkReferencedObject> for hkpFixedConstraintData {
    fn as_mut(&mut self) -> &mut crate::havok::hkReferencedObject {
        &mut self.base
    }
}
impl std::convert::AsRef<hkpFixedConstraintData> for hkpFixedConstraintData {
    fn as_ref(&self) -> &hkpFixedConstraintData {
        self
    }
}
impl std::convert::AsMut<hkpFixedConstraintData> for hkpFixedConstraintData {
    fn as_mut(&mut self) -> &mut hkpFixedConstraintData {
        self
    }
}
