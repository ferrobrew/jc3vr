//! Checked joint access during the character-owned pose-finalization phases.

use jc3gi::{
    animation::skeleton::HkArray,
    character::character::{
        AnimationController, Character, CharacterInitializationFlags, CharacterPauseFlags,
        DamageStateFlags, Joint,
    },
};

use crate::{controllers::body::procedural::skeleton::SkeletonKey, hash::hashlittle};

#[cfg(test)]
mod tests;

/// The native character update-pass admission, before any pre-finalize pose access.
pub(crate) fn eligible(character: &Character) -> bool {
    character
        .m_InitializationFlags
        .contains(CharacterInitializationFlags::INITIALIZED)
        && character.m_DamageFlags.contains(DamageStateFlags::ENABLED)
        && !character.m_PauseFlags.contains(CharacterPauseFlags::PAUSED)
}

/// Check the allocation identity without traversing the pose buffers or bone hierarchy.
///
/// # Safety
///
/// The controller and native identity allocations must be live on their serialized owning thread.
pub(crate) unsafe fn identity(controller: &AnimationController) -> Option<(SkeletonKey, usize)> {
    let key = unsafe { SkeletonKey::of(controller) }?;
    let producer = unsafe { controller.m_PoseProducer.as_ref() }?;
    let pose = unsafe { producer.m_PoseT1.as_ref() }?;
    let skeleton = unsafe { controller.m_Skeleton.as_ref() }?;
    if pose.m_Skeleton != skeleton.m_HavokSkeleton {
        return None;
    }
    Some((key, controller.m_PoseProducer as usize))
}

/// A live pose whose buffers cover the skeleton and its parent traversal.
/// The owning character thread must serialize native reads and writes for the borrow.
pub(crate) struct LivePose<'a> {
    controller: &'a mut AnimationController,
    key: SkeletonKey,
}

impl<'a> LivePose<'a> {
    /// Check the current producer pose at a character-owned pre-finalize or post-solve seam.
    ///
    /// # Safety
    ///
    /// The controller and its native allocations must be live and exclusively owned by this thread.
    pub(crate) unsafe fn new(controller: &'a mut AnimationController) -> Option<Self> {
        let key = unsafe { SkeletonKey::of(controller) }?;
        let producer = unsafe { controller.m_PoseProducer.as_ref() }?;
        let pose = unsafe { producer.m_PoseT1.as_ref() }?;
        let skeleton = unsafe { controller.m_Skeleton.as_ref() }?;
        let havok = unsafe { skeleton.m_HavokSkeleton.as_ref() }?;
        if !std::ptr::eq(pose.m_Skeleton, havok)
            || !covers(&pose.m_LocalPose, key.bones())
            || !covers(&pose.m_ModelPose, key.bones())
            || !covers(&pose.m_BoneFlags, key.bones())
            || !covers(&havok.m_ParentIndices, key.bones())
        {
            return None;
        }
        let parents =
            unsafe { std::slice::from_raw_parts(havok.m_ParentIndices.data, key.bones()) };
        if parents
            .iter()
            .enumerate()
            .any(|(bone, &parent)| parent != -1 && (parent < 0 || parent as usize >= bone))
        {
            return None;
        }
        Some(Self { controller, key })
    }

    pub(crate) fn key(&self) -> SkeletonKey {
        self.key
    }

    pub(crate) fn producer(&self) -> usize {
        self.controller.m_PoseProducer as usize
    }

    pub(crate) fn contains(&self, index: u32) -> bool {
        (index as usize) < self.key.bones()
    }

    /// Resolve a SAFE-bone lookup without accepting its missing-bone root fallback.
    pub(crate) fn safe_index(&self, index: u32) -> Option<u32> {
        (index != 0 && self.contains(index)).then_some(index)
    }

    /// Resolve an optional named bone without accepting the native missing-name root fallback.
    pub(crate) fn named(&self, name: &[u8]) -> Option<u32> {
        self.named_hash(hashlittle(name))
    }

    fn named_hash(&self, hash: u32) -> Option<u32> {
        let skeleton = unsafe { self.controller.m_Skeleton.as_ref() }?;
        if skeleton.m_BoneNameHashes.is_null() {
            return None;
        }
        let hashes =
            unsafe { std::slice::from_raw_parts(skeleton.m_BoneNameHashes, self.key.bones()) };
        hashes
            .iter()
            .position(|&entry| entry == hash)
            .filter(|&index| index != 0)
            .map(|index| index as u32)
    }

    pub(crate) fn write(&mut self, index: u32, joint: &mut Joint) {
        if self.contains(index) {
            unsafe { self.controller.SetJoint(index, joint) };
        }
    }

    pub(crate) fn read(&self, index: u32) -> Option<Joint> {
        if !self.contains(index) {
            return None;
        }
        let mut joint = Joint::default();
        unsafe { self.controller.GetJoint(index, &mut joint) };
        Some(joint)
    }
}

fn covers<T>(array: &HkArray<T>, bones: usize) -> bool {
    !array.data.is_null()
        && (array.data as usize).is_multiple_of(std::mem::align_of::<T>())
        && array.size >= 0
        && array.size as usize >= bones
}
