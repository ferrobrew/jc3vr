use jc3gi::{
    animation::{
        pose_producer::{PoseProducer, QsTransform},
        skeleton::{HkArray, HkaBone, HkaPose, HkaSkeleton, Skeleton},
    },
    character::character::{CharacterInitializationFlags, DamageStateFlags},
};

use super::*;
use crate::{
    controllers::config::ControllersConfig,
    headpose::Anchors,
    thread_role::{self, ThreadRole},
};

fn array<T>(values: &mut [T]) -> HkArray<T> {
    HkArray {
        data: values.as_mut_ptr(),
        size: values.len() as i32,
        ..unsafe { std::mem::zeroed() }
    }
}

struct FixtureState;

impl FixtureState {
    fn new() -> Self {
        Self::clear();
        Self
    }

    fn clear() {
        crate::controllers::frame::clear();
        rig::invalidate();
        DEFERRED.replace(None);
        ownership::clear();
        headpose::withdraw_anchors();
    }
}

impl Drop for FixtureState {
    fn drop(&mut self) {
        Self::clear();
    }
}

#[test]
fn replacement_before_measure_cannot_seed_new_or_deferred_plans_with_old_arms() {
    let _config = crate::config::test_fixture();
    // Plan admission is under test, not the live controller and world-rotation adapters.
    crate::config::CONFIG.lock().controllers.enabled = false;
    let _role = thread_role::enter(ThreadRole::Unmarked);
    let _state = FixtureState::new();
    let mut bones: [HkaBone; 2] = unsafe { std::mem::zeroed() };
    let mut parents = [-1, 0];
    let mut havok: HkaSkeleton = unsafe { std::mem::zeroed() };
    havok.m_Bones = array(&mut bones);
    havok.m_ParentIndices = array(&mut parents);
    let mut skeleton: Skeleton = unsafe { std::mem::zeroed() };
    skeleton.m_HavokSkeleton = &mut havok;
    let mut replacement: Skeleton = unsafe { std::mem::zeroed() };
    replacement.m_HavokSkeleton = &mut havok;
    let mut local: [QsTransform; 2] = unsafe { std::mem::zeroed() };
    let mut model: [QsTransform; 2] = unsafe { std::mem::zeroed() };
    let mut flags = [0; 2];
    let mut pose: HkaPose = unsafe { std::mem::zeroed() };
    pose.m_Skeleton = &havok;
    pose.m_LocalPose = array(&mut local);
    pose.m_ModelPose = array(&mut model);
    pose.m_BoneFlags = array(&mut flags);
    let mut producer: PoseProducer = unsafe { std::mem::zeroed() };
    producer.m_PoseT1 = &mut pose;
    let mut replacement_producer: PoseProducer = unsafe { std::mem::zeroed() };
    replacement_producer.m_PoseT1 = &mut pose;
    let mut controller: AnimationController = unsafe { std::mem::zeroed() };
    controller.m_Skeleton = &mut skeleton;
    controller.m_PoseProducer = &mut producer;
    let mut character: Box<Character> = Box::new(unsafe { std::mem::zeroed() });
    character.m_IsLocalCharacter = true;
    character.m_InitializationFlags = CharacterInitializationFlags::INITIALIZED;
    character.m_DamageFlags = DamageStateFlags::ENABLED;
    character.m_WorldMatrixT1 = Mat4::IDENTITY.into();
    character.m_AnimatedModel.m_AnimationController = &mut controller;
    for replace_producer in [false, true] {
        controller.m_Skeleton = &mut skeleton;
        controller.m_PoseProducer = &mut producer;
        unsafe { rig::prepare(&character) };
        rig::seed_test_measurement();
        let arms = rig::test_arms(Quat::IDENTITY);
        assert!(arms.iter().all(Option::is_some));
        let view = Quat::from_rotation_y(0.6);
        for (original, transported) in arms.into_iter().zip(rig::test_arms(view)) {
            assert!(
                transported
                    .unwrap()
                    .eye_to_shoulder
                    .abs_diff_eq(view.inverse() * original.unwrap().eye_to_shoulder, 1.0e-6)
            );
        }
        let mut retained = unsafe { plan(&character) };
        assert!(unsafe { retained.current_for(&character) });
        let old_identity = retained.arm_identity;
        let hand = CockpitHand {
            wrist: Pose::IDENTITY,
            grip: Pose::IDENTITY,
            aim: Pose::IDENTITY,
        };
        retained.active = Some(ActiveHands {
            config: ControllersConfig::default(),
            head: Pose::IDENTITY,
            hands: [Some(hand); 2],
            world_scale: 1.0,
        });
        retained.weights = [1.0; 2];
        DEFERRED.replace(Some(retained.clone()));
        if replace_producer {
            controller.m_PoseProducer = &mut replacement_producer;
        } else {
            controller.m_Skeleton = &mut replacement;
        }
        let fresh = unsafe { plan(&character) };
        assert!(fresh.arm_identity.is_some());
        assert_ne!(fresh.arm_identity, old_identity);
        assert!(rig::test_arms(view).iter().all(Option::is_none));
        assert!(!unsafe { retained.current_for(&character) });
        assert!(unsafe { fresh.current_for(&character) });
        headpose::set_anchors(Anchors {
            head: Vec3::Y,
            head_prev: Vec3::Y,
            neck: Vec3::Y,
            eye_arm: Vec3::ZERO,
        });
        assert_eq!(unsafe { deferred_wrist_targets(&character) }, [None; 2]);
        unsafe { queue_deferred(character.as_mut()) };
        assert!(DEFERRED.with_borrow(Option::is_none));
        DEFERRED.replace(Some(retained.clone()));
        unsafe { queue_or_defer(character.as_mut(), retained) };
        assert!(DEFERRED.with_borrow(Option::is_none));
        assert_eq!(
            placement_weights(
                character.as_ref() as *const Character as usize,
                &controller as *const AnimationController as usize
            ),
            [0.0; 2]
        );
    }
}
