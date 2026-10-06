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
