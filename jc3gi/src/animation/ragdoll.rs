#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A character's ragdoll instance (`NAnimationSystem::CRagdollInstanceImpl`): the Havok ragdoll
/// built from the character's ragdoll resource, its bodies in the physics world, and the
/// controllers that drive them toward the animated pose. While its owner controls its displacement
/// the bodies follow the animation; otherwise they simulate freely and the character follows them.
/// Only some members are bound.
pub struct RagdollInstance {
    _field_0: [u8; 8],
    /// The physics world the ragdoll's bodies live in.
    pub m_World: *mut crate::physics::hknpWorld,
    /// The Havok ragdoll: the ragdoll's bodies and constraints, and its skeleton.
    pub m_Ragdoll: *mut crate::physics::hknpRagdoll,
}
fn _RagdollInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], RagdollInstance>([0u8; 0x18]);
    }
    unreachable!()
}
impl RagdollInstance {}
impl std::convert::AsRef<RagdollInstance> for RagdollInstance {
    fn as_ref(&self) -> &RagdollInstance {
        self
    }
}
impl std::convert::AsMut<RagdollInstance> for RagdollInstance {
    fn as_mut(&mut self) -> &mut RagdollInstance {
        self
    }
}
