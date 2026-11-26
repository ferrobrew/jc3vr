//! Which skeleton an animation controller holds, the identity the head anchors and the procedural
//! body key their per-skeleton state by.

use jc3gi::character::character::AnimationController;

/// Which skeleton an animation controller holds: the controller, its skeleton resource, the Havok
/// skeleton under it, and its bone count. A rig built for one key is only valid while the
/// controller still holds that skeleton: a controller can be reused for another character, and a
/// skeleton can be swapped under a live controller.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SkeletonKey {
    controller: usize,
    skeleton: usize,
    havok: usize,
    bones: usize,
}

impl SkeletonKey {
    /// The key of the skeleton `controller` holds now, or `None` without a plausible one.
    ///
    /// # Safety
    ///
    /// `controller` must be a live animation controller, read on the thread that owns its character
    /// for the duration of the call.
    pub unsafe fn of(controller: &AnimationController) -> Option<Self> {
        let skeleton = unsafe { controller.m_Skeleton.as_ref() }?;
        let havok = unsafe { skeleton.m_HavokSkeleton.as_ref() }?;
        let count = havok.m_Bones.size;
        (1..=MAX_BONES).contains(&count).then_some(Self {
            controller: controller as *const AnimationController as usize,
            skeleton: skeleton as *const _ as usize,
            havok: havok as *const _ as usize,
            bones: count as usize,
        })
    }

    /// The skeleton's bone count: every joint index read or written must be below it, since
    /// `AnimationController::SetJoint` does not check its index.
    pub fn bones(&self) -> usize {
        self.bones
    }
}

/// Bone counts beyond this are garbage data, not a character skeleton.
pub(crate) const MAX_BONES: i32 = 1024;
