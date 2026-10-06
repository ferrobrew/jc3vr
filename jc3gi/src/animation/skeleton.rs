#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A Havok dynamic array (`hkArray<T>`): a data pointer, the element count, and the capacity, whose
/// top bits carry the array's ownership flags.
pub struct HkArray<T> {
    /// The first element.
    pub data: *mut T,
    /// The number of live elements.
    pub size: i32,
    /// The allocated capacity in the low bits and the ownership flags in the top two bits.
    pub capacity_and_flags: i32,
}
impl<T> HkArray<T> {}
#[repr(C, align(8))]
/// A skeleton bone (`hkaBone`): its name and whether its translation is locked.
pub struct HkaBone {
    /// The bone's name (`hkStringPtr`). The lowest bit of the pointer is the string's ownership
    /// flag, so the characters start at the pointer with that bit cleared.
    pub m_Name: *const u8,
    /// Whether animation leaves the bone's translation at the reference pose's.
    pub m_LockTranslation: bool,
    _field_9: [u8; 7],
}
fn _HkaBone_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], HkaBone>([0u8; 0x10]);
    }
    unreachable!()
}
impl HkaBone {}
impl std::convert::AsRef<HkaBone> for HkaBone {
    fn as_ref(&self) -> &HkaBone {
        self
    }
}
impl std::convert::AsMut<HkaBone> for HkaBone {
    fn as_mut(&mut self) -> &mut HkaBone {
        self
    }
}
#[repr(C, align(8))]
/// A pose of a skeleton (`hkaPose`): the bones' transforms in local space (relative to the parent)
/// and in model space (relative to the character's root), kept in sync lazily through per-bone
/// flags. A read of a bone's model-space transform whose flag marks it dirty first recomputes it
/// from its ancestors' local transforms, and a model-space write recomputes the descendants it
/// propagates to.
pub struct HkaPose {
    /// The skeleton the pose is of.
    pub m_Skeleton: *const crate::animation::skeleton::HkaSkeleton,
    /// Each bone's transform relative to its parent.
    pub m_LocalPose: crate::animation::skeleton::HkArray<
        crate::animation::pose_producer::QsTransform,
    >,
    /// Each bone's transform relative to the model root.
    pub m_ModelPose: crate::animation::skeleton::HkArray<
        crate::animation::pose_producer::QsTransform,
    >,
    /// Each bone's sync flags, among them the flag that marks its model-space transform as stale.
    pub m_BoneFlags: crate::animation::skeleton::HkArray<u32>,
}
fn _HkaPose_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], HkaPose>([0u8; 0x38]);
    }
    unreachable!()
}
impl HkaPose {}
impl std::convert::AsRef<HkaPose> for HkaPose {
    fn as_ref(&self) -> &HkaPose {
        self
    }
}
impl std::convert::AsMut<HkaPose> for HkaPose {
    fn as_mut(&mut self) -> &mut HkaPose {
        self
    }
}
#[repr(C, align(8))]
/// A Havok animation skeleton (`hkaSkeleton`): the bone hierarchy and its reference pose. Bone `i`
/// of every array below is the same bone, and a pose of this skeleton indexes its transforms the
/// same way. Parents come before their children.
pub struct HkaSkeleton {
    _field_0: [u8; 16],
    /// The skeleton's name (`hkStringPtr`, with the ownership flag in its lowest bit).
    pub m_Name: *const u8,
    /// Each bone's parent index, `-1` for a root.
    pub m_ParentIndices: crate::animation::skeleton::HkArray<i16>,
    /// The bones, in hierarchy order.
    pub m_Bones: crate::animation::skeleton::HkArray<
        crate::animation::skeleton::HkaBone,
    >,
    /// The reference pose: each bone's transform relative to its parent. The pose job samples the
    /// first animation layer over it, so a bone no clip keys takes this transform.
    pub m_ReferencePose: crate::animation::skeleton::HkArray<
        crate::animation::pose_producer::QsTransform,
    >,
    /// The reference values of the skeleton's float slots.
    pub m_ReferenceFloats: crate::animation::skeleton::HkArray<f32>,
}
fn _HkaSkeleton_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], HkaSkeleton>([0u8; 0x58]);
    }
    unreachable!()
}
impl HkaSkeleton {}
impl std::convert::AsRef<HkaSkeleton> for HkaSkeleton {
    fn as_ref(&self) -> &HkaSkeleton {
        self
    }
}
impl std::convert::AsMut<HkaSkeleton> for HkaSkeleton {
    fn as_mut(&mut self) -> &mut HkaSkeleton {
        self
    }
}
#[repr(C, align(8))]
/// The engine's skeleton resource (`CSkeleton`): the Havok skeleton and a table of its bones'
/// hashed names, which `GetBoneIndexByHashedName` searches to resolve a bone by the hash of its
/// name.
pub struct Skeleton {
    /// The Havok skeleton.
    pub m_HavokSkeleton: *const crate::animation::skeleton::HkaSkeleton,
    _field_8: [u8; 16],
    /// Each bone's hashed name, indexed like the Havok skeleton's bones.
    pub m_BoneNameHashes: *const u32,
}
fn _Skeleton_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], Skeleton>([0u8; 0x20]);
    }
    unreachable!()
}
impl Skeleton {}
impl std::convert::AsRef<Skeleton> for Skeleton {
    fn as_ref(&self) -> &Skeleton {
        self
    }
}
impl std::convert::AsMut<Skeleton> for Skeleton {
    fn as_mut(&mut self) -> &mut Skeleton {
        self
    }
}
