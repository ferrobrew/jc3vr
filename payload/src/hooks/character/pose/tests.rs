use jc3gi::animation::{
    pose_producer::{PoseProducer, QsTransform},
    skeleton::{HkaBone, HkaPose, HkaSkeleton, Skeleton},
};

use super::*;

fn array<T>(values: &mut [T]) -> HkArray<T> {
    HkArray {
        data: values.as_mut_ptr(),
        size: values.len() as i32,
        ..unsafe { std::mem::zeroed() }
    }
}

#[test]
fn failed_required_bone_capture_withdraws_current_anchor_authority() {
    crate::headpose::set_anchors(crate::headpose::Anchors {
        head: glam::Vec3::Y,
        head_prev: glam::Vec3::Y,
        neck: glam::Vec3::Y * 0.9,
        eye_arm: glam::Vec3::Z * 0.1,
    });
    assert!(crate::headpose::anchor().is_some());
    crate::hooks::character::body_ik::capture_unavailable();
    assert!(crate::headpose::anchor().is_none());
    assert!(crate::headpose::anchor_prev().is_none());
    assert_eq!(crate::headpose::neck_delta(), glam::Vec3::ZERO);
    assert_eq!(crate::headpose::eye_arm(), glam::Vec3::ZERO);
}

#[test]
fn native_admission_rejects_uninitialized_disabled_and_paused_characters() {
    let mut character: Character = unsafe { std::mem::zeroed() };
    assert!(!eligible(&character));
    character.m_InitializationFlags = CharacterInitializationFlags::INITIALIZED;
    assert!(!eligible(&character));
    character.m_DamageFlags = DamageStateFlags::ENABLED;
    assert!(eligible(&character));
    character.m_PauseFlags = CharacterPauseFlags::PAUSED;
    assert!(!eligible(&character));
}

#[test]
fn unknown_producer_buffers_and_indices_never_dispatch_native_joint_access() {
    let mut controller: AnimationController = unsafe { std::mem::zeroed() };
    assert!(unsafe { LivePose::new(&mut controller) }.is_none());
    let mut bones: [HkaBone; 2] = unsafe { std::mem::zeroed() };
    let mut parents = [-1, 0];
    let mut havok: HkaSkeleton = unsafe { std::mem::zeroed() };
    havok.m_Bones = array(&mut bones);
    havok.m_ParentIndices = array(&mut parents);
    let mut hashes = [1, 2];
    let mut skeleton: Skeleton = unsafe { std::mem::zeroed() };
    skeleton.m_HavokSkeleton = &mut havok;
    skeleton.m_BoneNameHashes = hashes.as_mut_ptr();
    controller.m_Skeleton = &mut skeleton;
    assert!(unsafe { LivePose::new(&mut controller) }.is_none());
    let mut producer: PoseProducer = unsafe { std::mem::zeroed() };
    controller.m_PoseProducer = &mut producer;
    assert!(unsafe { LivePose::new(&mut controller) }.is_none());
    let mut pose: HkaPose = unsafe { std::mem::zeroed() };
    producer.m_PoseT1 = &mut pose;
    pose.m_Skeleton = &havok;
    std::hint::black_box((&producer, &pose));
    assert!(unsafe { LivePose::new(&mut controller) }.is_none());
    let mut local: [QsTransform; 2] = unsafe { std::mem::zeroed() };
    let mut model: [QsTransform; 2] = unsafe { std::mem::zeroed() };
    let mut flags = [0; 2];
    pose.m_LocalPose = array(&mut local);
    pose.m_ModelPose = array(&mut model);
    pose.m_BoneFlags = array(&mut flags);
    std::hint::black_box(&pose);
    {
        let mut checked = unsafe { LivePose::new(&mut controller) }.unwrap();
        assert!(checked.contains(0));
        for missing_safe in [0, 2, u32::MAX] {
            assert_eq!(checked.safe_index(missing_safe), None);
            if let Some(index) = checked.safe_index(missing_safe) {
                let mut joint = checked.read(index).unwrap();
                checked.write(index, &mut joint);
                panic!("a missing SAFE bone reaches native joint access");
            }
        }
        assert!(checked.read(2).is_none());
        assert!(checked.read(u32::MAX).is_none());
        assert_eq!(checked.named_hash(3), None);
        assert_eq!(checked.named_hash(1), None);
        assert_eq!(checked.named_hash(2), Some(1));
    }
    pose.m_ModelPose.size = 1;
    std::hint::black_box(&pose);
    assert!(unsafe { LivePose::new(&mut controller) }.is_none());
    pose.m_ModelPose.size = 2;
    parents[1] = 1;
    std::hint::black_box((&pose, &parents));
    assert!(unsafe { LivePose::new(&mut controller) }.is_none());
}
