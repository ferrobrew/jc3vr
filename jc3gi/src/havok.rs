#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone, Default)]
#[repr(C, align(16))]
/// An axis-aligned box (`hkAabb`): its least and greatest corners.
pub struct hkAabb {
    pub m_min: crate::types::vector_math::AlignedVector3,
    pub m_max: crate::types::vector_math::AlignedVector3,
}
fn _hkAabb_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], hkAabb>([0u8; 0x20]);
    }
    unreachable!()
}
impl hkAabb {}
impl std::convert::AsRef<hkAabb> for hkAabb {
    fn as_ref(&self) -> &hkAabb {
        self
    }
}
impl std::convert::AsMut<hkAabb> for hkAabb {
    fn as_mut(&mut self) -> &mut hkAabb {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(16))]
/// A box quantized into a world's integer space (`hkAabb16`): each corner's coordinates as 16-bit
/// integers, which the world's [`hkIntSpaceUtil`](crate::havok::hkIntSpaceUtil) turns back into Havok-space positions. The two
/// key halves are not coordinates.
pub struct hkAabb16 {
    pub m_min: [u16; 3],
    pub m_key: u16,
    pub m_max: [u16; 3],
    pub m_key1: u16,
}
fn _hkAabb16_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], hkAabb16>([0u8; 0x10]);
    }
    unreachable!()
}
impl hkAabb16 {}
impl std::convert::AsRef<hkAabb16> for hkAabb16 {
    fn as_ref(&self) -> &hkAabb16 {
        self
    }
}
impl std::convert::AsMut<hkAabb16> for hkAabb16 {
    fn as_mut(&mut self) -> &mut hkAabb16 {
        self
    }
}
#[repr(C, align(16))]
/// The mapping between Havok space and a world's integer space (`hkIntSpaceUtil`), in which the
/// broad phase keeps every body's box ([`hknpBody::m_aabb`](crate::physics::hknpBody::m_aabb)). A
/// position is quantized as `(position + offset) * scale`, clamped to the 16-bit range, with
/// [`m_bitOffsetLow`](crate::havok::hkIntSpaceUtil::m_bitOffsetLow) for a box's least corner and
/// [`m_bitOffsetHigh`](crate::havok::hkIntSpaceUtil::m_bitOffsetHigh) for its greatest, and restored as
/// `integer * scale_inv - low_offset` for both corners
/// ([`hknpWorld::getBodyAabb`](crate::physics::hknpWorld::getBodyAabb)). Only the mapping is bound.
pub struct hkIntSpaceUtil {
    pub m_bitOffsetLow: crate::types::vector_math::AlignedVector3,
    pub m_bitOffsetHigh: crate::types::vector_math::AlignedVector3,
    pub m_bitScale: crate::types::vector_math::AlignedVector3,
    pub m_bitScaleInv: crate::types::vector_math::AlignedVector3,
}
fn _hkIntSpaceUtil_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], hkIntSpaceUtil>([0u8; 0x40]);
    }
    unreachable!()
}
impl hkIntSpaceUtil {}
impl std::convert::AsRef<hkIntSpaceUtil> for hkIntSpaceUtil {
    fn as_ref(&self) -> &hkIntSpaceUtil {
        self
    }
}
impl std::convert::AsMut<hkIntSpaceUtil> for hkIntSpaceUtil {
    fn as_mut(&mut self) -> &mut hkIntSpaceUtil {
        self
    }
}
#[repr(C, align(8))]
/// A Havok memory allocator (`hkMemoryAllocator`). The engine's Havok heap objects come from a
/// router's [`m_heap`](crate::havok::hkMemoryRouter::m_heap) through [`blockAlloc`](crate::havok::hkMemoryAllocator::blockAlloc) with the
/// object's size, and go back through [`blockFree`](crate::havok::hkMemoryAllocator::blockFree) with the same size, as a
/// [`hkReferencedObject`](crate::havok::hkReferencedObject)'s deletion does. Only some members are bound.
pub struct hkMemoryAllocator {
    vftable: *const crate::havok::hkMemoryAllocatorVftable,
}
fn _hkMemoryAllocator_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], hkMemoryAllocator>([0u8; 0x8]);
    }
    unreachable!()
}
impl hkMemoryAllocator {
    pub fn vftable(&self) -> *const crate::havok::hkMemoryAllocatorVftable {
        self.vftable as *const crate::havok::hkMemoryAllocatorVftable
    }
    /// Allocates `num_bytes` bytes, or returns null.
    pub unsafe fn blockAlloc(&mut self, num_bytes: i32) -> *mut ::std::ffi::c_void {
        unsafe {
            let f = (&raw const (*self.vftable()).blockAlloc).read();
            f(self as *mut Self as _, num_bytes)
        }
    }
    /// Frees the block `p` of `num_bytes` bytes.
    pub unsafe fn blockFree(&mut self, p: *mut ::std::ffi::c_void, num_bytes: i32) {
        unsafe {
            let f = (&raw const (*self.vftable()).blockFree).read();
            f(self as *mut Self as _, p, num_bytes)
        }
    }
}
impl std::convert::AsRef<hkMemoryAllocator> for hkMemoryAllocator {
    fn as_ref(&self) -> &hkMemoryAllocator {
        self
    }
}
impl std::convert::AsMut<hkMemoryAllocator> for hkMemoryAllocator {
    fn as_mut(&mut self) -> &mut hkMemoryAllocator {
        self
    }
}
#[repr(C, align(8))]
pub struct hkMemoryAllocatorVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::havok::hkMemoryAllocator),
    /// Allocates `num_bytes` bytes, or returns null.
    pub blockAlloc: unsafe extern "system" fn(
        this: *mut crate::havok::hkMemoryAllocator,
        num_bytes: i32,
    ) -> *mut ::std::ffi::c_void,
    /// Frees the block `p` of `num_bytes` bytes.
    pub blockFree: unsafe extern "system" fn(
        this: *mut crate::havok::hkMemoryAllocator,
        p: *mut ::std::ffi::c_void,
        num_bytes: i32,
    ),
}
fn _hkMemoryAllocatorVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], hkMemoryAllocatorVftable>([0u8; 0x18]);
    }
    unreachable!()
}
impl hkMemoryAllocatorVftable {}
impl std::convert::AsRef<hkMemoryAllocatorVftable> for hkMemoryAllocatorVftable {
    fn as_ref(&self) -> &hkMemoryAllocatorVftable {
        self
    }
}
impl std::convert::AsMut<hkMemoryAllocatorVftable> for hkMemoryAllocatorVftable {
    fn as_mut(&mut self) -> &mut hkMemoryAllocatorVftable {
        self
    }
}
#[repr(C, align(8))]
/// A thread's Havok memory router (`hkMemoryRouter`), held in the thread-local storage slot
/// [`s_memoryRouterSlot`](hkMemoryRouter::get_s_memoryRouterSlot). The game thread and the physics workers have
/// one; Havok objects are allocated from and freed to the router of the thread that makes or
/// deletes them. Only some members are bound.
pub struct hkMemoryRouter {
    _field_0: [u8; 88],
    /// The heap allocator, which Havok objects and their buffers come from.
    pub m_heap: *mut crate::havok::hkMemoryAllocator,
}
fn _hkMemoryRouter_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], hkMemoryRouter>([0u8; 0x60]);
    }
    unreachable!()
}
impl hkMemoryRouter {}
impl hkMemoryRouter {
    /// The thread-local storage slot (`hkMemoryRouter::s_memoryRouter`) that holds each thread's
    /// router, read with `TlsGetValue`.
    pub unsafe fn get_s_memoryRouterSlot() -> &'static mut u32 {
        unsafe { &mut *(0x142F450C8 as *mut u32) }
    }
}
impl std::convert::AsRef<hkMemoryRouter> for hkMemoryRouter {
    fn as_ref(&self) -> &hkMemoryRouter {
        self
    }
}
impl std::convert::AsMut<hkMemoryRouter> for hkMemoryRouter {
    fn as_mut(&mut self) -> &mut hkMemoryRouter {
        self
    }
}
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
#[repr(C, align(2))]
/// An array stored inside its owner at a relative position (`hkRelArray`): its elements start
/// [`m_offset`](crate::havok::hkRelArray::m_offset) bytes after the array's own address. The convex shapes keep their
/// vertices, planes, faces, and indices in such arrays after their fixed members.
pub struct hkRelArray {
    /// The number of elements.
    pub m_size: u16,
    /// Where the elements start, in bytes from the array's own address.
    pub m_offset: u16,
}
fn _hkRelArray_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], hkRelArray>([0u8; 0x4]);
    }
    unreachable!()
}
impl hkRelArray {}
impl std::convert::AsRef<hkRelArray> for hkRelArray {
    fn as_ref(&self) -> &hkRelArray {
        self
    }
}
impl std::convert::AsMut<hkRelArray> for hkRelArray {
    fn as_mut(&mut self) -> &mut hkRelArray {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(16))]
/// A Havok SIMD scalar (`hkSimdFloat32`): one value held in all four lanes of a vector register.
pub struct hkSimdFloat32 {
    pub m_real: [f32; 4],
}
fn _hkSimdFloat32_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], hkSimdFloat32>([0u8; 0x10]);
    }
    unreachable!()
}
impl hkSimdFloat32 {}
impl std::convert::AsRef<hkSimdFloat32> for hkSimdFloat32 {
    fn as_ref(&self) -> &hkSimdFloat32 {
        self
    }
}
impl std::convert::AsMut<hkSimdFloat32> for hkSimdFloat32 {
    fn as_mut(&mut self) -> &mut hkSimdFloat32 {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// A strided array of three-dimensional vertices (`hkStridedVertices`).
pub struct hkStridedVertices {
    /// The first vertex, whose coordinates are stored as floats.
    pub m_vertices: *const f32,
    /// The number of vertices.
    pub m_numVertices: i32,
    /// The distance in bytes between consecutive vertices.
    pub m_striding: i32,
}
fn _hkStridedVertices_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], hkStridedVertices>([0u8; 0x10]);
    }
    unreachable!()
}
impl hkStridedVertices {}
impl std::convert::AsRef<hkStridedVertices> for hkStridedVertices {
    fn as_ref(&self) -> &hkStridedVertices {
        self
    }
}
impl std::convert::AsMut<hkStridedVertices> for hkStridedVertices {
    fn as_mut(&mut self) -> &mut hkStridedVertices {
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
#[derive(Copy, Clone)]
#[repr(C, align(16))]
/// A four-lane floating-point vector (`hkVector4f`).
pub struct hkVector4f {
    pub data: [f32; 4],
}
fn _hkVector4f_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], hkVector4f>([0u8; 0x10]);
    }
    unreachable!()
}
impl hkVector4f {}
impl std::convert::AsRef<hkVector4f> for hkVector4f {
    fn as_ref(&self) -> &hkVector4f {
        self
    }
}
impl std::convert::AsMut<hkVector4f> for hkVector4f {
    fn as_mut(&mut self) -> &mut hkVector4f {
        self
    }
}
#[repr(C, align(1))]
/// GSK's simplex cache.
pub struct hkcdGskCache {
    pub m_vertexIds: [u8; 4],
    pub m_dimAb: u8,
}
fn _hkcdGskCache_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x5], hkcdGskCache>([0u8; 0x5]);
    }
    unreachable!()
}
impl hkcdGskCache {}
impl hkcdGskCache {
    pub const INITIAL_DIMENSION: u8 = 65;
}
impl std::convert::AsRef<hkcdGskCache> for hkcdGskCache {
    fn as_ref(&self) -> &hkcdGskCache {
        self
    }
}
impl std::convert::AsMut<hkcdGskCache> for hkcdGskCache {
    fn as_mut(&mut self) -> &mut hkcdGskCache {
        self
    }
}
#[repr(C, align(16))]
/// Input for the ordinary vertex-array GSK distance query. The transform maps B into A.
pub struct hkcdGskGetClosestPointInput {
    pub m_aTb: crate::havok::hkTransformf,
    pub m_handlePenetration: u32,
    _field_44: [u8; 12],
    pub m_coreCollisionTolerance: crate::havok::hkVector4f,
    pub m_terminationGrowthRate: f32,
    _field_64: [u8; 12],
}
fn _hkcdGskGetClosestPointInput_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], hkcdGskGetClosestPointInput>([0u8; 0x70]);
    }
    unreachable!()
}
impl hkcdGskGetClosestPointInput {}
impl hkcdGskGetClosestPointInput {
    /// The ordinary-distance termination growth rate used by native callers.
    pub const DEFAULT_TERMINATION_GROWTH_RATE: f32 = 1.4;
    /// Enables the penetration-depth query.
    pub const HANDLE_PENETRATION: u32 = 1;
}
impl std::convert::AsRef<hkcdGskGetClosestPointInput> for hkcdGskGetClosestPointInput {
    fn as_ref(&self) -> &hkcdGskGetClosestPointInput {
        self
    }
}
impl std::convert::AsMut<hkcdGskGetClosestPointInput> for hkcdGskGetClosestPointInput {
    fn as_mut(&mut self) -> &mut hkcdGskGetClosestPointInput {
        self
    }
}
#[repr(C, align(16))]
/// Distance between the core shapes, normal in A, and a closest point in A. Shape convex radii
/// are not subtracted from the distance.
pub struct hkcdGskGetClosestPointOutput {
    pub m_distance: crate::havok::hkVector4f,
    pub m_normalInA: crate::havok::hkVector4f,
    pub m_pointAinA: crate::havok::hkVector4f,
}
fn _hkcdGskGetClosestPointOutput_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], hkcdGskGetClosestPointOutput>([0u8; 0x30]);
    }
    unreachable!()
}
impl hkcdGskGetClosestPointOutput {}
impl std::convert::AsRef<hkcdGskGetClosestPointOutput> for hkcdGskGetClosestPointOutput {
    fn as_ref(&self) -> &hkcdGskGetClosestPointOutput {
        self
    }
}
impl std::convert::AsMut<hkcdGskGetClosestPointOutput> for hkcdGskGetClosestPointOutput {
    fn as_mut(&mut self) -> &mut hkcdGskGetClosestPointOutput {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// Status returned by an ordinary GSK closest-point query.
pub enum hkcdGskGetClosestPointStatus {
    STATUS_OK_NOT_PENTRATING = 0isize as _,
    STATUS_OK_PENETRATING = 1isize as _,
    STATUS_PD_HEURISTIC_SAMPLING = 2isize as _,
    STATUS_PD_OUT_OF_MEMORY = 3isize as _,
    STATUS_OK_FLAG = 4isize as _,
    STATUS_PD_DISTANCE_TOO_BIG = 5isize as _,
    STATUS_PENETRATING = 6isize as _,
    STATUS_PD_UNSOLVABLE = 7isize as _,
}
fn _hkcdGskGetClosestPointStatus_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], hkcdGskGetClosestPointStatus>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(16))]
/// A contact manifold (`hkcdManifold4`) with the normal and world-space contact positions.
pub struct hkcdManifold4 {
    pub m_numPoints: i32,
    pub m_minimumDistance: f32,
    _field_8: [u8; 8],
    pub m_normal: crate::havok::hkVector4f,
    pub m_weldNormal: crate::havok::hkVector4f,
    pub m_distances: [f32; 4],
    pub m_positions: [crate::havok::hkVector4f; 4],
}
fn _hkcdManifold4_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x80], hkcdManifold4>([0u8; 0x80]);
    }
    unreachable!()
}
impl hkcdManifold4 {}
impl std::convert::AsRef<hkcdManifold4> for hkcdManifold4 {
    fn as_ref(&self) -> &hkcdManifold4 {
        self
    }
}
impl std::convert::AsMut<hkcdManifold4> for hkcdManifold4 {
    fn as_mut(&mut self) -> &mut hkcdManifold4 {
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
#[repr(C, align(16))]
/// A capsule shape (`hknpCapsuleShape`): the points within a radius of a segment, built as an
/// [`hknpConvexPolytopeShape`](crate::havok::hknpConvexPolytopeShape): a box of eight vertices along the segment, reaching
/// [`RADIUS_IN_CORE`](crate::havok::hknpCapsuleShape::RADIUS_IN_CORE) of the radius past it on every side, rounded by the
/// convex radius ([`hknpShape::m_convexRadius`](crate::havok::hknpShape::m_convexRadius)), the rest of the radius.
pub struct hknpCapsuleShape {
    pub base: crate::havok::hknpConvexPolytopeShape,
    /// The segment's first end, body space.
    pub m_a: crate::types::vector_math::AlignedVector3,
    /// The segment's second end, body space.
    pub m_b: crate::types::vector_math::AlignedVector3,
}
fn _hknpCapsuleShape_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], hknpCapsuleShape>([0u8; 0x70]);
    }
    unreachable!()
}
impl hknpCapsuleShape {
    pub fn vftable(&self) -> *const crate::havok::hknpShapeVftable {
        self.base.vftable() as *const crate::havok::hknpShapeVftable
    }
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
    /// As [`hkReferencedObject::deleteThisReferencedObject`](crate::havok::hkReferencedObject::deleteThisReferencedObject).
    pub unsafe fn deleteThisReferencedObject(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).deleteThisReferencedObject).read();
            f(self as *mut Self as _)
        }
    }
    /// The shape's kind, an [`hknpShapeType`](crate::havok::hknpShapeType) value; each concrete shape class returns its own
    /// constant.
    pub unsafe fn getType(&self) -> u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getType).read();
            f(self as *const Self as _)
        }
    }
    /// Computes the world-space axis-aligned bounds of the shape.
    pub unsafe fn calcAabb(
        &self,
        transform: *const crate::havok::hkTransformf,
        out: *mut crate::havok::hkAabb,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).calcAabb).read();
            f(self as *const Self as _, transform, out)
        }
    }
    /// Returns shape keys following `previous`, up to the provided capacity.
    pub unsafe fn getShapeKeys(
        &self,
        keys: *mut u32,
        capacity: i32,
        previous: u32,
        config: *const crate::havok::hknpGetShapeKeysConfig,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getShapeKeys).read();
            f(self as *const Self as _, keys, capacity, previous, config)
        }
    }
    /// Collects the leaf shapes identified by the provided keys.
    pub unsafe fn getLeafShapes(
        &self,
        keys: *const u32,
        count: i32,
        collector: *mut crate::havok::hknpShapeCollector,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getLeafShapes).read();
            f(self as *const Self as _, keys, count, collector)
        }
    }
}
impl hknpCapsuleShape {
    /// The share of the capsule's radius its core box reaches past the segment;
    /// [`hknpShape::m_convexRadius`](crate::havok::hknpShape::m_convexRadius) is the capsule's radius less that share.
    pub const RADIUS_IN_CORE: f32 = 0.01;
}
impl std::convert::AsRef<crate::havok::hknpConvexPolytopeShape> for hknpCapsuleShape {
    fn as_ref(&self) -> &crate::havok::hknpConvexPolytopeShape {
        &self.base
    }
}
impl std::convert::AsMut<crate::havok::hknpConvexPolytopeShape> for hknpCapsuleShape {
    fn as_mut(&mut self) -> &mut crate::havok::hknpConvexPolytopeShape {
        &mut self.base
    }
}
impl std::convert::AsRef<crate::havok::hknpConvexShape> for hknpCapsuleShape {
    fn as_ref(&self) -> &crate::havok::hknpConvexShape {
        &self.base.base
    }
}
impl std::convert::AsMut<crate::havok::hknpConvexShape> for hknpCapsuleShape {
    fn as_mut(&mut self) -> &mut crate::havok::hknpConvexShape {
        &mut self.base.base
    }
}
impl std::convert::AsRef<crate::havok::hknpShape> for hknpCapsuleShape {
    fn as_ref(&self) -> &crate::havok::hknpShape {
        &self.base.base.base
    }
}
impl std::convert::AsMut<crate::havok::hknpShape> for hknpCapsuleShape {
    fn as_mut(&mut self) -> &mut crate::havok::hknpShape {
        &mut self.base.base.base
    }
}
impl std::convert::AsRef<crate::havok::hkReferencedObject> for hknpCapsuleShape {
    fn as_ref(&self) -> &crate::havok::hkReferencedObject {
        &self.base.base.base.base
    }
}
impl std::convert::AsMut<crate::havok::hkReferencedObject> for hknpCapsuleShape {
    fn as_mut(&mut self) -> &mut crate::havok::hkReferencedObject {
        &mut self.base.base.base.base
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
impl hknpContactImpulseEvent {
    pub const calculateContactPointPositions_ADDRESS: usize = 0x1415192C0;
    /// Reconstructs the contact positions from the Jacobian and the world's body motions.
    /// The disassembler labels this function `hknpBodyTracker_getBodyIdsFromInput`.
    pub unsafe fn calculateContactPointPositions(
        &self,
        world: *const crate::physics::hknpWorld,
        manifold: *mut crate::havok::hkcdManifold4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                world: *const crate::physics::hknpWorld,
                manifold: *mut crate::havok::hkcdManifold4,
            ) = ::std::mem::transmute(Self::calculateContactPointPositions_ADDRESS);
            f(self as *const Self as _, world, manifold)
        }
    }
}
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
#[derive(Copy, Clone)]
#[repr(C, align(2))]
/// One face of an [`hknpConvexPolytopeShape`](crate::havok::hknpConvexPolytopeShape) (`hknpConvexPolytopeShape::Face`).
pub struct hknpConvexPolytopeFace {
    /// The face's first index in [`hknpConvexPolytopeShape::m_indices`](crate::havok::hknpConvexPolytopeShape::m_indices).
    pub m_firstIndex: u16,
    /// How many indices the face has.
    pub m_numIndices: u8,
    /// The face's minimum half-angle as the collision detection keeps it, one byte; its encoding
    /// is not established.
    pub m_minHalfAngle: u8,
}
fn _hknpConvexPolytopeFace_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], hknpConvexPolytopeFace>([0u8; 0x4]);
    }
    unreachable!()
}
impl hknpConvexPolytopeFace {}
impl std::convert::AsRef<hknpConvexPolytopeFace> for hknpConvexPolytopeFace {
    fn as_ref(&self) -> &hknpConvexPolytopeFace {
        self
    }
}
impl std::convert::AsMut<hknpConvexPolytopeFace> for hknpConvexPolytopeFace {
    fn as_mut(&mut self) -> &mut hknpConvexPolytopeFace {
        self
    }
}
#[repr(C, align(16))]
/// A convex polytope shape (`hknpConvexPolytopeShape`): an [`hknpConvexShape`](crate::havok::hknpConvexShape) with its faces. Each
/// face ([`hknpConvexPolytopeFace`](crate::havok::hknpConvexPolytopeFace)) names a run of [`m_indices`](crate::havok::hknpConvexPolytopeShape::m_indices), each a vertex
/// index, which go round the face in order, so consecutive indices and the last and first are the
/// face's edges. The plane array may hold more planes than there are faces.
pub struct hknpConvexPolytopeShape {
    pub base: crate::havok::hknpConvexShape,
    /// The face planes, as four-lane vectors.
    pub m_planes: crate::havok::hkRelArray,
    /// The faces, [`hknpConvexPolytopeFace`](crate::havok::hknpConvexPolytopeFace)s.
    pub m_faces: crate::havok::hkRelArray,
    /// The faces' vertex indices, one byte each.
    pub m_indices: crate::havok::hkRelArray,
    _field_4c: [u8; 4],
}
fn _hknpConvexPolytopeShape_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], hknpConvexPolytopeShape>([0u8; 0x50]);
    }
    unreachable!()
}
impl hknpConvexPolytopeShape {
    pub fn vftable(&self) -> *const crate::havok::hknpShapeVftable {
        self.base.vftable() as *const crate::havok::hknpShapeVftable
    }
    /// As [`hkReferencedObject::deleteThisReferencedObject`](crate::havok::hkReferencedObject::deleteThisReferencedObject).
    pub unsafe fn deleteThisReferencedObject(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).deleteThisReferencedObject).read();
            f(self as *mut Self as _)
        }
    }
    /// The shape's kind, an [`hknpShapeType`](crate::havok::hknpShapeType) value; each concrete shape class returns its own
    /// constant.
    pub unsafe fn getType(&self) -> u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getType).read();
            f(self as *const Self as _)
        }
    }
    /// Computes the world-space axis-aligned bounds of the shape.
    pub unsafe fn calcAabb(
        &self,
        transform: *const crate::havok::hkTransformf,
        out: *mut crate::havok::hkAabb,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).calcAabb).read();
            f(self as *const Self as _, transform, out)
        }
    }
    /// Returns shape keys following `previous`, up to the provided capacity.
    pub unsafe fn getShapeKeys(
        &self,
        keys: *mut u32,
        capacity: i32,
        previous: u32,
        config: *const crate::havok::hknpGetShapeKeysConfig,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getShapeKeys).read();
            f(self as *const Self as _, keys, capacity, previous, config)
        }
    }
    /// Collects the leaf shapes identified by the provided keys.
    pub unsafe fn getLeafShapes(
        &self,
        keys: *const u32,
        count: i32,
        collector: *mut crate::havok::hknpShapeCollector,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getLeafShapes).read();
            f(self as *const Self as _, keys, count, collector)
        }
    }
}
impl std::convert::AsRef<crate::havok::hknpConvexShape> for hknpConvexPolytopeShape {
    fn as_ref(&self) -> &crate::havok::hknpConvexShape {
        &self.base
    }
}
impl std::convert::AsMut<crate::havok::hknpConvexShape> for hknpConvexPolytopeShape {
    fn as_mut(&mut self) -> &mut crate::havok::hknpConvexShape {
        &mut self.base
    }
}
impl std::convert::AsRef<crate::havok::hknpShape> for hknpConvexPolytopeShape {
    fn as_ref(&self) -> &crate::havok::hknpShape {
        &self.base.base
    }
}
impl std::convert::AsMut<crate::havok::hknpShape> for hknpConvexPolytopeShape {
    fn as_mut(&mut self) -> &mut crate::havok::hknpShape {
        &mut self.base.base
    }
}
impl std::convert::AsRef<crate::havok::hkReferencedObject> for hknpConvexPolytopeShape {
    fn as_ref(&self) -> &crate::havok::hkReferencedObject {
        &self.base.base.base
    }
}
impl std::convert::AsMut<crate::havok::hkReferencedObject> for hknpConvexPolytopeShape {
    fn as_mut(&mut self) -> &mut crate::havok::hkReferencedObject {
        &mut self.base.base.base
    }
}
impl std::convert::AsRef<hknpConvexPolytopeShape> for hknpConvexPolytopeShape {
    fn as_ref(&self) -> &hknpConvexPolytopeShape {
        self
    }
}
impl std::convert::AsMut<hknpConvexPolytopeShape> for hknpConvexPolytopeShape {
    fn as_mut(&mut self) -> &mut hknpConvexPolytopeShape {
        self
    }
}
#[repr(C, align(16))]
/// A convex shape (`hknpConvexShape`): the hull of its vertices, rounded by its convex radius
/// ([`hknpShape::m_convexRadius`](crate::havok::hknpShape::m_convexRadius)). Each vertex is a four-lane vector (`hkcdVertex`) whose last
/// lane holds the vertex's id bits, not a coordinate. Only the vertices are bound.
pub struct hknpConvexShape {
    pub base: crate::havok::hknpShape,
    /// The vertices, body space, as four-lane vectors.
    pub m_vertices: crate::havok::hkRelArray,
    _field_34: [u8; 12],
}
fn _hknpConvexShape_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], hknpConvexShape>([0u8; 0x40]);
    }
    unreachable!()
}
impl hknpConvexShape {
    pub fn vftable(&self) -> *const crate::havok::hknpShapeVftable {
        self.base.vftable() as *const crate::havok::hknpShapeVftable
    }
    pub const createFromVertices_ADDRESS: usize = 0x1414E0B10;
    /// Builds a convex shape from strided vertices, applying `radius` and `config`.
    pub unsafe fn createFromVertices(
        vertices: *const crate::havok::hkStridedVertices,
        radius: f32,
        config: *const crate::havok::hknpConvexShapeBuildConfig,
    ) -> *mut crate::havok::hknpShape {
        unsafe {
            let f: unsafe extern "system" fn(
                vertices: *const crate::havok::hkStridedVertices,
                radius: f32,
                config: *const crate::havok::hknpConvexShapeBuildConfig,
            ) -> *mut crate::havok::hknpShape = ::std::mem::transmute(
                Self::createFromVertices_ADDRESS,
            );
            f(vertices, radius, config)
        }
    }
    /// As [`hkReferencedObject::deleteThisReferencedObject`](crate::havok::hkReferencedObject::deleteThisReferencedObject).
    pub unsafe fn deleteThisReferencedObject(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).deleteThisReferencedObject).read();
            f(self as *mut Self as _)
        }
    }
    /// The shape's kind, an [`hknpShapeType`](crate::havok::hknpShapeType) value; each concrete shape class returns its own
    /// constant.
    pub unsafe fn getType(&self) -> u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getType).read();
            f(self as *const Self as _)
        }
    }
    /// Computes the world-space axis-aligned bounds of the shape.
    pub unsafe fn calcAabb(
        &self,
        transform: *const crate::havok::hkTransformf,
        out: *mut crate::havok::hkAabb,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).calcAabb).read();
            f(self as *const Self as _, transform, out)
        }
    }
    /// Returns shape keys following `previous`, up to the provided capacity.
    pub unsafe fn getShapeKeys(
        &self,
        keys: *mut u32,
        capacity: i32,
        previous: u32,
        config: *const crate::havok::hknpGetShapeKeysConfig,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getShapeKeys).read();
            f(self as *const Self as _, keys, capacity, previous, config)
        }
    }
    /// Collects the leaf shapes identified by the provided keys.
    pub unsafe fn getLeafShapes(
        &self,
        keys: *const u32,
        count: i32,
        collector: *mut crate::havok::hknpShapeCollector,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getLeafShapes).read();
            f(self as *const Self as _, keys, count, collector)
        }
    }
}
impl std::convert::AsRef<crate::havok::hknpShape> for hknpConvexShape {
    fn as_ref(&self) -> &crate::havok::hknpShape {
        &self.base
    }
}
impl std::convert::AsMut<crate::havok::hknpShape> for hknpConvexShape {
    fn as_mut(&mut self) -> &mut crate::havok::hknpShape {
        &mut self.base
    }
}
impl std::convert::AsRef<crate::havok::hkReferencedObject> for hknpConvexShape {
    fn as_ref(&self) -> &crate::havok::hkReferencedObject {
        &self.base.base
    }
}
impl std::convert::AsMut<crate::havok::hkReferencedObject> for hknpConvexShape {
    fn as_mut(&mut self) -> &mut crate::havok::hkReferencedObject {
        &mut self.base.base
    }
}
impl std::convert::AsRef<hknpConvexShape> for hknpConvexShape {
    fn as_ref(&self) -> &hknpConvexShape {
        self
    }
}
impl std::convert::AsMut<hknpConvexShape> for hknpConvexShape {
    fn as_mut(&mut self) -> &mut hknpConvexShape {
        self
    }
}
#[repr(C, align(8))]
/// The options used to build a convex shape (`hknpConvexShapeBuildConfig`). Only the options read
/// by the vertex factory are mapped.
pub struct hknpConvexShapeBuildConfig {
    /// The tolerance used when simplifying the convex shape.
    pub m_simplificationTolerance: f32,
    /// Whether the input vertices are shrunk by the shape radius during construction.
    pub m_shrinkByRadius: bool,
    _field_5: [u8; 51],
}
fn _hknpConvexShapeBuildConfig_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], hknpConvexShapeBuildConfig>([0u8; 0x38]);
    }
    unreachable!()
}
impl hknpConvexShapeBuildConfig {
    pub const Construct_ADDRESS: usize = 0x1414DCF60;
    /// `hknpConvexShapeBuildConfig::hknpConvexShapeBuildConfig`: initializes the build options.
    pub unsafe fn Construct(&mut self) -> *mut crate::havok::hknpConvexShapeBuildConfig {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::havok::hknpConvexShapeBuildConfig = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<hknpConvexShapeBuildConfig> for hknpConvexShapeBuildConfig {
    fn as_ref(&self) -> &hknpConvexShapeBuildConfig {
        self
    }
}
impl std::convert::AsMut<hknpConvexShapeBuildConfig> for hknpConvexShapeBuildConfig {
    fn as_mut(&mut self) -> &mut hknpConvexShapeBuildConfig {
        self
    }
}
#[repr(C, align(8))]
/// The configuration for paged shape-key enumeration. The parent path starts empty, with no mask
/// and the most detailed geometry.
pub struct hknpGetShapeKeysConfig {
    pub m_parentKey: u32,
    pub m_parentSize: i32,
    pub m_mask: *const ::std::ffi::c_void,
    pub m_levelOfDetail: i32,
    _field_14: [u8; 4],
}
fn _hknpGetShapeKeysConfig_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], hknpGetShapeKeysConfig>([0u8; 0x18]);
    }
    unreachable!()
}
impl hknpGetShapeKeysConfig {}
impl hknpGetShapeKeysConfig {
    /// Selects the most detailed geometry.
    pub const MOST_DETAILED: i32 = 0;
}
impl std::convert::AsRef<hknpGetShapeKeysConfig> for hknpGetShapeKeysConfig {
    fn as_ref(&self) -> &hknpGetShapeKeysConfig {
        self
    }
}
impl std::convert::AsMut<hknpGetShapeKeysConfig> for hknpGetShapeKeysConfig {
    fn as_mut(&mut self) -> &mut hknpGetShapeKeysConfig {
        self
    }
}
#[repr(C, align(16))]
/// An in-place triangle, including its relative vertex, plane, face, and index arrays.
pub struct hknpInplaceTriangleShape {
    _field_0: [u8; 240],
}
fn _hknpInplaceTriangleShape_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xF0], hknpInplaceTriangleShape>([0u8; 0xF0]);
    }
    unreachable!()
}
impl hknpInplaceTriangleShape {
    pub const Construct_ADDRESS: usize = 0x1414DC020;
    /// Builds the in-place triangle with its native default radius. The object's storage remains
    /// owned by its caller.
    pub unsafe fn Construct(
        &mut self,
        finish_loaded: bool,
    ) -> *mut crate::havok::hknpInplaceTriangleShape {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                finish_loaded: bool,
            ) -> *mut crate::havok::hknpInplaceTriangleShape = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _, finish_loaded)
        }
    }
}
impl std::convert::AsRef<hknpInplaceTriangleShape> for hknpInplaceTriangleShape {
    fn as_ref(&self) -> &hknpInplaceTriangleShape {
        self
    }
}
impl std::convert::AsMut<hknpInplaceTriangleShape> for hknpInplaceTriangleShape {
    fn as_mut(&mut self) -> &mut hknpInplaceTriangleShape {
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
#[repr(C, align(16))]
/// A Havok collision shape (`hknpShape`), a [`hkReferencedObject`](crate::havok::hkReferencedObject) allocated from the Havok heap of
/// the thread that creates it. A body holds its own reference to its shape. Its kind is what
/// [`getType`](crate::havok::hknpShape::getType) returns; the convex kinds are [`hknpConvexShape`](crate::havok::hknpConvexShape) and the types
/// built on it. Only some members are bound.
pub struct hknpShape {
    pub base: crate::havok::hkReferencedObject,
    /// The shape's flags (`hknpShape::FlagsEnum`): [`IS_CONVEX_SHAPE`](crate::havok::hknpShape::IS_CONVEX_SHAPE) and
    /// [`IS_CONVEX_POLYTOPE_SHAPE`](crate::havok::hknpShape::IS_CONVEX_POLYTOPE_SHAPE) mark the convex classes.
    pub m_flags: u16,
    _field_12: [u8; 2],
    /// The radius the shape's core is rounded by: every point within it of the core is inside the
    /// shape. A sphere is a point with its radius here, and a capsule's core is a box around its
    /// segment with most of its radius here.
    pub m_convexRadius: f32,
    /// Opaque user data carried by the shape.
    pub m_userData: u64,
    _field_20: [u8; 16],
}
fn _hknpShape_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], hknpShape>([0u8; 0x30]);
    }
    unreachable!()
}
impl hknpShape {
    pub fn vftable(&self) -> *const crate::havok::hknpShapeVftable {
        self.base.vftable() as *const crate::havok::hknpShapeVftable
    }
    /// As [`hkReferencedObject::deleteThisReferencedObject`](crate::havok::hkReferencedObject::deleteThisReferencedObject).
    pub unsafe fn deleteThisReferencedObject(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).deleteThisReferencedObject).read();
            f(self as *mut Self as _)
        }
    }
    /// The shape's kind, an [`hknpShapeType`](crate::havok::hknpShapeType) value; each concrete shape class returns its own
    /// constant.
    pub unsafe fn getType(&self) -> u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getType).read();
            f(self as *const Self as _)
        }
    }
    /// Computes the world-space axis-aligned bounds of the shape.
    pub unsafe fn calcAabb(
        &self,
        transform: *const crate::havok::hkTransformf,
        out: *mut crate::havok::hkAabb,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).calcAabb).read();
            f(self as *const Self as _, transform, out)
        }
    }
    /// Returns shape keys following `previous`, up to the provided capacity.
    pub unsafe fn getShapeKeys(
        &self,
        keys: *mut u32,
        capacity: i32,
        previous: u32,
        config: *const crate::havok::hknpGetShapeKeysConfig,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getShapeKeys).read();
            f(self as *const Self as _, keys, capacity, previous, config)
        }
    }
    /// Collects the leaf shapes identified by the provided keys.
    pub unsafe fn getLeafShapes(
        &self,
        keys: *const u32,
        count: i32,
        collector: *mut crate::havok::hknpShapeCollector,
    ) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).getLeafShapes).read();
            f(self as *const Self as _, keys, count, collector)
        }
    }
}
impl hknpShape {
    /// The collector flag indicating that a leaf contains stored triangles.
    pub const COLLECTOR_STORES_TRIANGLES: u32 = 16;
    /// The invalid shape key returned when shape-key enumeration has no further key.
    pub const INVALID_SHAPE_KEY: u32 = 4294967295;
    /// The [`m_flags`](crate::havok::hknpShape::m_flags) bit of a shape that is an [`hknpConvexPolytopeShape`](crate::havok::hknpConvexPolytopeShape).
    pub const IS_CONVEX_POLYTOPE_SHAPE: u16 = 2;
    /// The [`m_flags`](crate::havok::hknpShape::m_flags) bit of a shape that is an [`hknpConvexShape`](crate::havok::hknpConvexShape).
    pub const IS_CONVEX_SHAPE: u16 = 1;
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
/// Storage used to collect leaf shapes. The triangle prototype is externally supplied. A reset
/// installs the root transform and clears the prior result; leaf extraction can modify the
/// transform, scale, and shape. Triangle batches are materialized by [`getSubshape`](crate::havok::getSubshape).
pub struct hknpShapeCollector {
    pub m_transformOut: crate::havok::hkTransformf,
    pub m_scaleOut: crate::havok::hkVector4f,
    pub m_flags: u32,
    _field_54: [u8; 4],
    pub m_shapeOut: *const crate::havok::hknpShape,
    _field_60: [u8; 8],
    pub m_triangleShapePrototype: *mut crate::havok::hknpShape,
    _field_70: [u8; 3040],
}
fn _hknpShapeCollector_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC50], hknpShapeCollector>([0u8; 0xC50]);
    }
    unreachable!()
}
impl hknpShapeCollector {
    pub const reset_ADDRESS: usize = 0x1401D9400;
    /// Resets the collector with a root transform and room for shape tags.
    pub unsafe fn reset(
        &mut self,
        transform: *const crate::havok::hkTransformf,
        num_shapes: i32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                transform: *const crate::havok::hkTransformf,
                num_shapes: i32,
            ) = ::std::mem::transmute(Self::reset_ADDRESS);
            f(self as *mut Self as _, transform, num_shapes)
        }
    }
}
impl std::convert::AsRef<hknpShapeCollector> for hknpShapeCollector {
    fn as_ref(&self) -> &hknpShapeCollector {
        self
    }
}
impl std::convert::AsMut<hknpShapeCollector> for hknpShapeCollector {
    fn as_mut(&mut self) -> &mut hknpShapeCollector {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// A shape's kind (`hknpShapeType::Enum`), as [`hknpShape::getType`](crate::havok::hknpShape::getType) returns it. The bound kinds
/// include convex shapes and the compressed mesh and height-field shapes.
pub enum hknpShapeType {
    CONVEX = 0isize as _,
    CONVEX_POLYTOPE = 1isize as _,
    SPHERE = 2isize as _,
    CAPSULE = 3isize as _,
    COMPRESSED_MESH = 5isize as _,
    COMPRESSED_HEIGHT_FIELD = 10isize as _,
}
fn _hknpShapeType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], hknpShapeType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
pub struct hknpShapeVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    /// As [`hkReferencedObject::deleteThisReferencedObject`](crate::havok::hkReferencedObject::deleteThisReferencedObject).
    pub deleteThisReferencedObject: unsafe extern "system" fn(
        this: *mut crate::havok::hknpShape,
    ),
    /// The shape's kind, an [`hknpShapeType`](crate::havok::hknpShapeType) value; each concrete shape class returns its own
    /// constant.
    pub getType: unsafe extern "system" fn(this: *const crate::havok::hknpShape) -> u32,
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    /// Computes the world-space axis-aligned bounds of the shape.
    pub calcAabb: unsafe extern "system" fn(
        this: *const crate::havok::hknpShape,
        transform: *const crate::havok::hkTransformf,
        out: *mut crate::havok::hkAabb,
    ),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::havok::hknpShape),
    /// Returns shape keys following `previous`, up to the provided capacity.
    pub getShapeKeys: unsafe extern "system" fn(
        this: *const crate::havok::hknpShape,
        keys: *mut u32,
        capacity: i32,
        previous: u32,
        config: *const crate::havok::hknpGetShapeKeysConfig,
    ) -> i32,
    /// Collects the leaf shapes identified by the provided keys.
    pub getLeafShapes: unsafe extern "system" fn(
        this: *const crate::havok::hknpShape,
        keys: *const u32,
        count: i32,
        collector: *mut crate::havok::hknpShapeCollector,
    ) -> i32,
}
fn _hknpShapeVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xB0], hknpShapeVftable>([0u8; 0xB0]);
    }
    unreachable!()
}
impl hknpShapeVftable {}
impl std::convert::AsRef<hknpShapeVftable> for hknpShapeVftable {
    fn as_ref(&self) -> &hknpShapeVftable {
        self
    }
}
impl std::convert::AsMut<hknpShapeVftable> for hknpShapeVftable {
    fn as_mut(&mut self) -> &mut hknpShapeVftable {
        self
    }
}
#[repr(C, align(16))]
/// The data of a fixed constraint (`hkpFixedConstraintData`), which holds two bodies in a fixed
/// pose relative to each other. The magnet weapon
/// (`CMagnetWeaponComponent::ConstrainBodiesToGravityField`) allocates one of this type's size from
/// the calling thread's Havok heap ([`hkMemoryAllocator::blockAlloc`](crate::havok::hkMemoryAllocator::blockAlloc)), constructs it, sets its
/// frames and the stabilized solving method, which the constructor already sets, and creates a
/// constraint from it. The disassembler names that function
/// `CPartInstance::AttachPartWithConstraint`. Only some members are bound.
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
pub const getSubshape_ADDRESS: usize = 0x140F883B0;
/// Extracts one leaf, materializes any triangle batch into the supplied prototype, and returns
/// the resulting shape pointer from the collector. The collector must have been reset.
pub unsafe fn getSubshape(
    shape: *const crate::havok::hknpShape,
    key: u32,
    collector: *mut crate::havok::hknpShapeCollector,
) -> *const crate::havok::hknpShape {
    unsafe {
        let f: unsafe extern "system" fn(
            shape: *const crate::havok::hknpShape,
            key: u32,
            collector: *mut crate::havok::hknpShapeCollector,
        ) -> *const crate::havok::hknpShape = ::std::mem::transmute(getSubshape_ADDRESS);
        f(shape, key, collector)
    }
}
pub const hkcdGskGetClosestPoint_ADDRESS: usize = 0x1412FC430;
/// Computes the closest core points of the two vertex arrays, updating the simplex cache and
/// returning a status independently of the output. The arrays contain aligned Havok vertices.
pub unsafe fn hkcdGskGetClosestPoint(
    vertices_a: *const crate::havok::hkVector4f,
    count_a: i32,
    vertices_b: *const crate::havok::hkVector4f,
    count_b: i32,
    input: *const crate::havok::hkcdGskGetClosestPointInput,
    cache: *mut crate::havok::hkcdGskCache,
    output: *mut crate::havok::hkcdGskGetClosestPointOutput,
) -> i32 {
    unsafe {
        let f: unsafe extern "system" fn(
            vertices_a: *const crate::havok::hkVector4f,
            count_a: i32,
            vertices_b: *const crate::havok::hkVector4f,
            count_b: i32,
            input: *const crate::havok::hkcdGskGetClosestPointInput,
            cache: *mut crate::havok::hkcdGskCache,
            output: *mut crate::havok::hkcdGskGetClosestPointOutput,
        ) -> i32 = ::std::mem::transmute(hkcdGskGetClosestPoint_ADDRESS);
        f(vertices_a, count_a, vertices_b, count_b, input, cache, output)
    }
}
