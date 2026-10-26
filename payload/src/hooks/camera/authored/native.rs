//! Native reads at the camera-tree seam, before the tracked contexts replace the engine output.

use glam::Mat4;
use jc3gi::{
    animation::{
        pose_producer::{AnimLoopMode, AnimationInstance},
        skeleton::HkArray,
    },
    character::character::{AnimationController, Character},
    hash::hashlittle,
    types::{math::Matrix4, shared_ptr::SharedPtr},
};

use crate::hooks::camera::authored::{
    Refusal,
    composition::Trs,
    evidence::{self, Animation, Clip, ClipSource},
    policy::{GraphState, HangingState, LandingState, RecoveryState},
};

/// All native pointers are used only during capture; publication contains owned values.
pub(super) struct NativeSample {
    pub character: u64,
    pub source: Source,
    pub pose_buffer: u64,
    pub finalized: u64,
    pub animation: Animation,
    pub state: Result<GraphState, Refusal>,
    pub root: Trs,
    pub local: Trs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Source {
    pub controller: u64,
    pub producer: u64,
    pub producer_owner: u64,
    pub skeleton_resource: u64,
    pub skeleton: u64,
    pub driver: u64,
    pub rule: u64,
    pub blender: u64,
    pub camera_bone: u32,
}

/// The tree's native attachment modifiers read these same model-space bones before this call.
pub(super) unsafe fn read(character: &Character) -> Result<NativeSample, Refusal> {
    unsafe {
        let controller = character
            .m_AnimatedModel
            .m_AnimationController
            .as_ref()
            .ok_or(Refusal::MissingChannel)?;
        let producer = controller
            .m_PoseProducer
            .as_ref()
            .ok_or(Refusal::MissingChannel)?;
        let pose = producer.m_PoseT1.as_ref().ok_or(Refusal::MissingChannel)?;
        let skeleton = controller
            .m_Skeleton
            .as_ref()
            .ok_or(Refusal::MissingChannel)?;
        let havok = skeleton
            .m_HavokSkeleton
            .as_ref()
            .ok_or(Refusal::MissingChannel)?;
        if controller.m_SkeletonResourceHandle.is_null()
            || !std::ptr::eq(pose.m_Skeleton, skeleton.m_HavokSkeleton)
            || havok.m_Bones.size <= 0
            || havok.m_Bones.data.is_null()
            || skeleton.m_BoneNameHashes.is_null()
        {
            return Err(Refusal::MissingChannel);
        }
        // The safe CAMERA slot names an absent unnumbered bone on the checked skeleton.
        // Resolve the actual named channel and reject GetBoneIndex's root fallback.
        let hash = hashlittle(b"CharacterCam1") as u32;
        let index = controller.GetBoneIndex(hash);
        let count = havok.m_Bones.size as u32;
        if index == 0
            || index >= count
            || index >= pose.m_ModelPose.size.max(0) as u32
            || index >= pose.m_BoneFlags.size.max(0) as u32
            || pose.m_ModelPose.data.is_null()
            || pose.m_BoneFlags.data.is_null()
            || *skeleton.m_BoneNameHashes.add(index as usize) != hash
        {
            return Err(Refusal::MissingChannel);
        }
        let identity = source(controller, index)?;
        let body_rule = character
            .m_AnimatedModel
            .m_RuleSystems
            .as_slice()
            .first()
            .and_then(|rule| rule.as_ref())
            .ok_or(Refusal::MissingState)?;
        if body_rule as *const _ as u64 != identity.rule {
            return Err(Refusal::IncoherentAnimation);
        }
        let observed = animation(controller, index)?;
        let finalized = evidence::matching(identity, producer.m_PoseT1 as u64, &observed)?;
        let mut matrix = Matrix4::default();
        controller.GetBoneMatrix(index, &mut matrix);
        let after = animation(controller, index)?;
        let checked =
            evidence::matching(source(controller, index)?, producer.m_PoseT1 as u64, &after)?;
        if checked.serial != finalized.serial || checked.animation != finalized.animation {
            return Err(Refusal::IncoherentAnimation);
        }
        Ok(NativeSample {
            character: character as *const Character as u64,
            source: identity,
            pose_buffer: producer.m_PoseT1 as u64,
            finalized: finalized.serial,
            state: classify(observed.state),
            animation: observed,
            root: trs(Mat4::from(character.m_WorldMatrixT1))?,
            local: trs(Mat4::from(matrix))?,
        })
    }
}

/// The selected range and producer buffers immediately before a synchronous update.
pub(crate) struct AnimationEvaluation {
    controller: u64,
    num_of_bones: i32,
    before: ProducerBuffers,
}

/// Snapshots typed producer state without retaining native pointers across the call.
pub(crate) unsafe fn animation_evaluation(
    controller: &AnimationController,
    num_of_bones: i32,
) -> Option<AnimationEvaluation> {
    Some(AnimationEvaluation {
        controller: controller as *const _ as u64,
        num_of_bones,
        before: unsafe { producer_buffers(controller)? },
    })
}

/// Observe only the controller supplied by the serialized animation hook.
/// Time-only, skipped, and camera-excluding updates invalidate the previous evidence.
pub(crate) unsafe fn animation_update(
    controller: &AnimationController,
    evaluation: Option<AnimationEvaluation>,
) {
    let address = controller as *const _ as u64;
    let result = unsafe {
        (|| {
            let evaluation = evaluation.ok_or(Refusal::MissingFinalizedAnimation)?;
            let after = producer_buffers(controller).ok_or(Refusal::MissingChannel)?;
            if evaluation.controller != address || !evaluation.before.exchanged_with(after) {
                return Err(Refusal::MissingFinalizedAnimation);
            }
            if controller.m_SkeletonResourceHandle.is_null() || controller.m_Skeleton.is_null() {
                return Err(Refusal::MissingChannel);
            }
            let index = controller.GetBoneIndex(hashlittle(b"CharacterCam1") as u32);
            let skeleton = controller
                .m_Skeleton
                .as_ref()
                .ok_or(Refusal::MissingChannel)?;
            let havok = skeleton
                .m_HavokSkeleton
                .as_ref()
                .ok_or(Refusal::MissingChannel)?;
            if index == 0
                || index >= havok.m_Bones.size.max(0) as u32
                || skeleton.m_BoneNameHashes.is_null()
                || *skeleton.m_BoneNameHashes.add(index as usize)
                    != hashlittle(b"CharacterCam1") as u32
            {
                return Err(Refusal::MissingChannel);
            }
            let source = source(controller, index)?;
            evaluation.complete(after, source.camera_bone)?;
            let animation = animation(controller, index)?;
            Ok((source, after.current, animation))
        })()
    };
    publish_evaluation(address, result);
}

#[derive(Clone, Copy)]
struct ProducerBuffers {
    producer: u64,
    owner: u64,
    previous: u64,
    current: u64,
}

impl ProducerBuffers {
    fn exchanged_with(self, after: Self) -> bool {
        // PoseProducer::Update exchanges the buffers only after the controller's native guard.
        self.producer == after.producer
            && self.owner == after.owner
            && self.previous != self.current
            && self.previous == after.current
            && self.current == after.previous
    }
}

impl AnimationEvaluation {
    fn complete(&self, after: ProducerBuffers, camera: u32) -> Result<(), Refusal> {
        if !self.before.exchanged_with(after)
            || !u32::try_from(self.num_of_bones).is_ok_and(|count| camera < count)
        {
            return Err(Refusal::MissingFinalizedAnimation);
        }
        Ok(())
    }
}

unsafe fn producer_buffers(controller: &AnimationController) -> Option<ProducerBuffers> {
    let producer = unsafe { controller.m_PoseProducer.as_ref()? };
    if controller.m_PoseProducerControlBlock.is_null()
        || producer.m_PoseT0.is_null()
        || producer.m_PoseT1.is_null()
    {
        return None;
    }
    Some(ProducerBuffers {
        producer: controller.m_PoseProducer as u64,
        owner: controller.m_PoseProducerControlBlock as u64,
        previous: producer.m_PoseT0 as u64,
        current: producer.m_PoseT1 as u64,
    })
}

fn publish_evaluation(controller: u64, result: Result<(Source, u64, Animation), Refusal>) {
    match result {
        Ok((source, buffer, animation)) => evidence::publish(source, buffer, animation),
        Err(_) => evidence::invalidate(controller),
    }
}

unsafe fn source(controller: &AnimationController, index: u32) -> Result<Source, Refusal> {
    unsafe {
        let producer = controller
            .m_PoseProducer
            .as_ref()
            .ok_or(Refusal::MissingChannel)?;
        let skeleton = controller
            .m_Skeleton
            .as_ref()
            .ok_or(Refusal::MissingChannel)?;
        if index == 0
            || controller.m_PoseProducerControlBlock.is_null()
            || controller.m_SkeletonResourceHandle.is_null()
            || producer.m_PoseT1.is_null()
        {
            return Err(Refusal::MissingChannel);
        }
        let part = controller
            .m_BodyParts
            .as_slice()
            .first()
            .and_then(|part| part.as_ref())
            .ok_or(Refusal::MissingClip)?;
        let driver = part
            .m_AnimationDriver
            .as_ref()
            .ok_or(Refusal::MissingClip)?;
        let rule = driver.m_RuleSystem.as_ref().ok_or(Refusal::MissingState)?;
        let blender = driver.m_Blender.as_ref().ok_or(Refusal::MissingClip)?;
        Ok(Source {
            controller: controller as *const _ as u64,
            producer: controller.m_PoseProducer as u64,
            producer_owner: controller.m_PoseProducerControlBlock as u64,
            skeleton_resource: controller.m_SkeletonResourceHandle as u64,
            skeleton: skeleton.m_HavokSkeleton as u64,
            driver: driver as *const _ as u64,
            rule: rule as *const _ as u64,
            blender: blender as *const _ as u64,
            camera_bone: index,
        })
    }
}

unsafe fn animation(controller: &AnimationController, camera: u32) -> Result<Animation, Refusal> {
    unsafe {
        let part = controller
            .m_BodyParts
            .as_slice()
            .first()
            .and_then(|part| part.as_ref())
            .ok_or(Refusal::MissingClip)?;
        let driver = part
            .m_AnimationDriver
            .as_ref()
            .ok_or(Refusal::MissingClip)?;
        let rule = driver.m_RuleSystem.as_ref().ok_or(Refusal::MissingState)?;
        let machine = rule
            .m_StateMachineInstance
            .as_ref()
            .ok_or(Refusal::MissingState)?;
        let blender = driver.m_Blender.as_ref().ok_or(Refusal::MissingClip)?;
        let samplers = hk_slice(&blender.m_Samplers)?;
        let playing = driver.m_PlayingAnimInstances.as_slice();
        if playing.len() > MAX_NATIVE_ITEMS {
            return Err(Refusal::IncoherentAnimation);
        }
        let observation = Animation {
            state: machine
                .m_CurrentState
                .as_ref()
                .map(|state| state.m_HashID.m_Hash),
            playing: playing
                .iter()
                .map(|instance| clip(instance, camera))
                .collect::<Result<_, _>>()?,
            samplers: samplers
                .iter()
                .map(|instance| clip(instance, camera))
                .collect::<Result<_, _>>()?,
        };
        observation.validate()?;
        Ok(observation)
    }
}

unsafe fn clip(owner: &SharedPtr<AnimationInstance>, camera: u32) -> Result<Clip, Refusal> {
    unsafe {
        let instance = owner.as_ref().ok_or(Refusal::MissingClip)?;
        if instance.m_Animation.is_null() || owner.pn.is_null() {
            return Err(Refusal::MissingClip);
        }
        let binding = instance.m_Binding.as_ref().ok_or(Refusal::MissingClip)?;
        let map = hk_slice(&binding.m_TransformTrackToBoneIndices)?;
        let masks = hk_slice(&instance.m_TransformTrackWeights)?;
        let camera_track_weight = if map.is_empty() {
            masks
                .get(camera as usize)
                .copied()
                .map_or(0.0, |mask| f32::from(mask) / f32::from(u8::MAX))
        } else {
            let mut weight = 0.0_f32;
            for (track, bone) in map.iter().enumerate() {
                if i32::from(*bone) == camera as i32 {
                    let mask = if masks.is_empty() {
                        u8::MAX
                    } else {
                        masks
                            .get(track)
                            .copied()
                            .ok_or(Refusal::IncoherentAnimation)?
                    };
                    weight = weight.max(f32::from(mask) / f32::from(u8::MAX));
                }
            }
            weight
        };
        let clip = Clip {
            source: ClipSource {
                instance: owner.px as u64,
                owner: owner.pn as u64,
                animation: instance.m_Animation as u64,
                binding: instance.m_Binding as u64,
            },
            time: instance.GetTime(),
            duration: instance.GetDuration(),
            looping: instance.m_LoopMode == AnimLoopMode::Loop,
            overflows: instance.m_OverflowCount,
            underflows: instance.m_UnderflowCount,
            weight: instance.GetTotalWeight(),
            camera_track_weight,
        };
        clip.validate()?;
        Ok(clip)
    }
}

unsafe fn hk_slice<T>(array: &HkArray<T>) -> Result<&[T], Refusal> {
    if array.size < 0
        || array.size as usize > MAX_NATIVE_ITEMS
        || (array.size > 0 && array.data.is_null())
    {
        return Err(Refusal::IncoherentAnimation);
    }
    if array.size == 0 {
        return Ok(&[]);
    }
    Ok(unsafe { std::slice::from_raw_parts(array.data, array.size as usize) })
}

// A bounded diagnostic read, not an engine allocation limit.
const MAX_NATIVE_ITEMS: usize = 4096;

fn classify(hash: Option<u32>) -> Result<GraphState, Refusal> {
    classify_with(hash, |name| hashlittle(name) as u32)
}

fn classify_with(hash: Option<u32>, name_id: impl Fn(&[u8]) -> u32) -> Result<GraphState, Refusal> {
    let hash = hash.ok_or(Refusal::MissingState)?;
    if hash == name_id(b"S_JC_WINGSUIT_IDLE") {
        return Err(Refusal::IdleTailWeightNotBound);
    }
    EXACT_STATES
        .iter()
        .find_map(|(name, state)| (hash == name_id(name)).then_some(*state))
        .ok_or(Refusal::OrdinaryOrUnsupportedState)
}

pub(super) fn trs(matrix: Mat4) -> Result<Trs, Refusal> {
    if !matrix.is_finite() || matrix.determinant().abs() <= f32::EPSILON {
        return Err(Refusal::InvalidTransform);
    }
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let value = Trs {
        translation,
        rotation,
        scale,
    };
    let rebuilt = value.matrix().map_err(|_| Refusal::InvalidTransform)?;
    if matrix
        .to_cols_array()
        .into_iter()
        .zip(rebuilt.to_cols_array())
        .any(|(left, right)| (left - right).abs() > 0.001)
    {
        return Err(Refusal::InvalidTransform);
    }
    Ok(value)
}

// The shipped AFSM and its explicit ASB state map supply these names.
const EXACT_STATES: &[(&[u8], GraphState)] = &[
    (
        b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO",
        GraphState::GroundInto,
    ),
    (
        b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND",
        GraphState::GroundFollowOn,
    ),
    (
        b"S_LAND_FAST_FALL",
        GraphState::Landing(LandingState::FastFall),
    ),
    (b"S_LAND_HARD", GraphState::Landing(LandingState::Hard)),
    (b"S_LAND_IDLE_0", GraphState::Landing(LandingState::Idle)),
    (
        b"S_LAND_IDLE_90L",
        GraphState::Landing(LandingState::IdleLeft),
    ),
    (
        b"S_LAND_IDLE_90R",
        GraphState::Landing(LandingState::IdleRight),
    ),
    (
        b"S_LAND_IDLE_180L",
        GraphState::Landing(LandingState::IdleBackLeft),
    ),
    (
        b"S_LAND_IDLE_180R",
        GraphState::Landing(LandingState::IdleBackRight),
    ),
    (b"S_LAND_RUN_0", GraphState::Landing(LandingState::Run)),
    (
        b"S_GETUP_POSEMATCHING",
        GraphState::Recovery(RecoveryState::PoseMatching),
    ),
    (
        b"S_GETUP_BLENDIN",
        GraphState::Recovery(RecoveryState::BlendIn),
    ),
    (b"S_GETUP", GraphState::Recovery(RecoveryState::GetUp)),
    (b"S_IDLE_PICKUP", GraphState::FloorPickup),
    (
        b"S_GRAPPLE_ATTACH_0",
        GraphState::Hanging(HangingState::GrappleAttached),
    ),
    (
        b"S_IDLE_HANG_STUNT",
        GraphState::Hanging(HangingState::StuntIdle),
    ),
    (
        b"S_IDLE_HANG_STUNT_120L",
        GraphState::Hanging(HangingState::StuntIdleLeft),
    ),
    (
        b"S_IDLE_HANG_STUNT_120R",
        GraphState::Hanging(HangingState::StuntIdleRight),
    ),
    (
        b"S_IDLE_HANG_STUNT2",
        GraphState::Hanging(HangingState::StuntIdleAlternate),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_adapter_invalidates_a_range_excluding_the_camera() {
        for count in [-1, 0, 7, 8] {
            let (evaluation, after, source, animation) = evaluation_fixture(910, count);
            evidence::publish(source, after.current, animation.clone());
            let result = evaluation
                .complete(after, source.camera_bone)
                .map(|()| (source, after.current, animation.clone()));
            assert!(matches!(result, Err(Refusal::MissingFinalizedAnimation)));
            publish_evaluation(source.controller, result);
            assert!(matches!(
                evidence::matching(source, after.current, &animation),
                Err(Refusal::MissingFinalizedAnimation)
            ));
        }
    }

    #[test]
    fn animation_adapter_publishes_a_range_including_the_camera() {
        let (evaluation, after, source, animation) = evaluation_fixture(911, 9);
        evidence::publish(source, evaluation.before.current, animation.clone());
        let previous = evidence::matching(source, evaluation.before.current, &animation)
            .unwrap()
            .serial;
        let result = evaluation
            .complete(after, source.camera_bone)
            .map(|()| (source, after.current, animation.clone()));
        assert!(result.is_ok());
        publish_evaluation(source.controller, result);
        assert!(
            evidence::matching(source, after.current, &animation)
                .unwrap()
                .serial
                > previous
        );
        evidence::invalidate(source.controller);
    }

    #[test]
    fn animation_adapter_invalidates_skipped_producer_work() {
        let (evaluation, _, source, animation) = evaluation_fixture(912, 9);
        let after = evaluation.before;
        evidence::publish(source, after.current, animation.clone());
        let result = evaluation
            .complete(after, source.camera_bone)
            .map(|()| (source, after.current, animation.clone()));
        assert!(matches!(result, Err(Refusal::MissingFinalizedAnimation)));
        publish_evaluation(source.controller, result);
        assert!(matches!(
            evidence::matching(source, after.current, &animation),
            Err(Refusal::MissingFinalizedAnimation)
        ));
    }

    fn evaluation_fixture(
        controller: u64,
        num_of_bones: i32,
    ) -> (AnimationEvaluation, ProducerBuffers, Source, Animation) {
        let before = ProducerBuffers {
            producer: controller + 1,
            owner: controller + 2,
            previous: controller + 3,
            current: controller + 4,
        };
        let after = ProducerBuffers {
            previous: before.current,
            current: before.previous,
            ..before
        };
        let source = Source {
            controller,
            producer: before.producer,
            producer_owner: before.owner,
            skeleton_resource: controller + 5,
            skeleton: controller + 6,
            driver: controller + 7,
            rule: controller + 8,
            blender: controller + 9,
            camera_bone: 8,
        };
        let clip = Clip {
            source: ClipSource {
                instance: controller + 10,
                owner: controller + 11,
                animation: controller + 12,
                binding: controller + 13,
            },
            time: 0.5,
            duration: 1.0,
            looping: false,
            overflows: 0,
            underflows: 0,
            weight: 1.0,
            camera_track_weight: 1.0,
        };
        (
            AnimationEvaluation {
                controller,
                num_of_bones,
                before,
            },
            after,
            source,
            Animation {
                state: Some(1),
                playing: vec![clip],
                samplers: vec![clip],
            },
        )
    }

    #[test]
    fn classifier_admits_the_exact_ground_sequence_and_rejects_ordinary_states() {
        let id = |name: &[u8]| {
            name.iter().fold(0_u32, |id, byte| {
                id.wrapping_mul(31).wrapping_add(u32::from(*byte))
            })
        };
        for (name, expected) in [
            (
                b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO".as_slice(),
                GraphState::GroundInto,
            ),
            (
                b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND".as_slice(),
                GraphState::GroundFollowOn,
            ),
        ] {
            assert_eq!(classify_with(Some(id(name)), id), Ok(expected));
        }
        for name in [
            b"S_IDLE".as_slice(),
            b"S_JUMP".as_slice(),
            b"S_RUN".as_slice(),
            b"S_FULL_RAGDOLL".as_slice(),
            b"S_CRASH_LAND_RAGDOLL".as_slice(),
            b"S_HEAVY_WEAPON_PICKUP".as_slice(),
            b"S_GRAPPLE_HANG".as_slice(),
            b"S_GRAPPLE_REEL_TO_LAND".as_slice(),
            b"S_JC_WINGSUIT_TO_LAND_SLIDE".as_slice(),
            b"S_GETUP_UNKNOWN".as_slice(),
        ] {
            assert_eq!(
                classify_with(Some(id(name)), id),
                Err(Refusal::OrdinaryOrUnsupportedState)
            );
        }
        assert_eq!(
            classify_with(Some(id(b"S_JC_WINGSUIT_IDLE")), id),
            Err(Refusal::IdleTailWeightNotBound)
        );
        assert_eq!(classify_with(None, id), Err(Refusal::MissingState));
    }

    #[test]
    fn classifier_admits_exact_landing_recovery_pickup_and_hanging_states() {
        let id = |name: &[u8]| {
            name.iter().fold(0_u32, |id, byte| {
                id.wrapping_mul(31).wrapping_add(u32::from(*byte))
            })
        };
        for (name, expected) in [
            (
                b"S_LAND_FAST_FALL".as_slice(),
                GraphState::Landing(LandingState::FastFall),
            ),
            (
                b"S_LAND_HARD".as_slice(),
                GraphState::Landing(LandingState::Hard),
            ),
            (
                b"S_LAND_IDLE_0".as_slice(),
                GraphState::Landing(LandingState::Idle),
            ),
            (
                b"S_LAND_IDLE_90L".as_slice(),
                GraphState::Landing(LandingState::IdleLeft),
            ),
            (
                b"S_LAND_IDLE_90R".as_slice(),
                GraphState::Landing(LandingState::IdleRight),
            ),
            (
                b"S_LAND_IDLE_180L".as_slice(),
                GraphState::Landing(LandingState::IdleBackLeft),
            ),
            (
                b"S_LAND_IDLE_180R".as_slice(),
                GraphState::Landing(LandingState::IdleBackRight),
            ),
            (
                b"S_LAND_RUN_0".as_slice(),
                GraphState::Landing(LandingState::Run),
            ),
            (
                b"S_GETUP_POSEMATCHING".as_slice(),
                GraphState::Recovery(RecoveryState::PoseMatching),
            ),
            (
                b"S_GETUP_BLENDIN".as_slice(),
                GraphState::Recovery(RecoveryState::BlendIn),
            ),
            (
                b"S_GETUP".as_slice(),
                GraphState::Recovery(RecoveryState::GetUp),
            ),
            (b"S_IDLE_PICKUP".as_slice(), GraphState::FloorPickup),
            (
                b"S_GRAPPLE_ATTACH_0".as_slice(),
                GraphState::Hanging(HangingState::GrappleAttached),
            ),
            (
                b"S_IDLE_HANG_STUNT".as_slice(),
                GraphState::Hanging(HangingState::StuntIdle),
            ),
            (
                b"S_IDLE_HANG_STUNT_120L".as_slice(),
                GraphState::Hanging(HangingState::StuntIdleLeft),
            ),
            (
                b"S_IDLE_HANG_STUNT_120R".as_slice(),
                GraphState::Hanging(HangingState::StuntIdleRight),
            ),
            (
                b"S_IDLE_HANG_STUNT2".as_slice(),
                GraphState::Hanging(HangingState::StuntIdleAlternate),
            ),
        ] {
            assert_eq!(classify_with(Some(id(name)), id), Ok(expected));
        }
    }
}
