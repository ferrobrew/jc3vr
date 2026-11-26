//! The anchor fixtures other crates' tests share, behind the `test-support` feature.

use jc3gi::{
    animation::skeleton::{HkaSkeleton, Skeleton},
    character::character::AnimationController,
};

use crate::headpose::{anchors::Identity, skeleton::SkeletonKey};

/// An anchor identity over a zeroed five-bone skeleton.
pub fn identity_fixture() -> Identity {
    let mut havok: HkaSkeleton = unsafe { std::mem::zeroed() };
    havok.m_Bones.size = 5;
    let mut skeleton: Skeleton = unsafe { std::mem::zeroed() };
    skeleton.m_HavokSkeleton = &mut havok;
    let mut controller: AnimationController = unsafe { std::mem::zeroed() };
    controller.m_Skeleton = &mut skeleton;
    Identity {
        character: 1,
        skeleton: unsafe { SkeletonKey::of(&controller) }.unwrap(),
        producer: 2,
        bones: [1, 2, 3, 4],
    }
}
