//! Native reads at the camera-tree seam, before the tracked contexts replace the engine output.

use glam::Mat4;
use jc3gi::{character::character::Character, hash::hashlittle, types::math::Matrix4};

use crate::hooks::{
    animation::active_state_type_id,
    camera::authored::{Refusal, composition::Trs, policy::GraphState},
};

/// All native pointers are used only during capture; publication contains owned values.
pub(super) struct NativeSample {
    pub character: u64,
    pub source: Source,
    pub pose_buffer: u64,
    pub state: GraphState,
    pub root: Trs,
    pub local: Trs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Source {
    pub controller: u64,
    pub producer: u64,
    pub skeleton: u64,
    pub camera_bone: u32,
}

/// The tree's native attachment modifiers read these same model-space bones before this call.
pub(super) unsafe fn read(character: &Character) -> Result<NativeSample, Refusal> {
    unsafe {
        let state = classify(active_state_type_id(character))?;
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
        let mut matrix = Matrix4::default();
        controller.GetBoneMatrix(index, &mut matrix);
        Ok(NativeSample {
            character: character as *const Character as u64,
            source: Source {
                controller: controller as *const _ as u64,
                producer: producer as *const _ as u64,
                skeleton: skeleton.m_HavokSkeleton as u64,
                camera_bone: index,
            },
            pose_buffer: producer.m_PoseT1 as u64,
            state,
            root: trs(Mat4::from(character.m_WorldMatrixT1))?,
            local: trs(Mat4::from(matrix))?,
        })
    }
}

fn classify(hash: Option<u32>) -> Result<GraphState, Refusal> {
    classify_with(hash, |name| hashlittle(name) as u32)
}

fn classify_with(hash: Option<u32>, name_id: impl Fn(&[u8]) -> u32) -> Result<GraphState, Refusal> {
    let hash = hash.ok_or(Refusal::MissingState)?;
    if hash == name_id(b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO") {
        Ok(GraphState::GroundInto)
    } else if hash == name_id(b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND") {
        Ok(GraphState::GroundFollowOn)
    } else if hash == name_id(b"S_JC_WINGSUIT_IDLE") {
        Err(Refusal::IdleTailWeightNotBound)
    } else if hash == name_id(b"S_IDLE_PICKUP") {
        Err(Refusal::PickupNotVerified)
    } else if [
        b"S_GRAPPLE_HANG".as_slice(),
        b"S_IDLE_HANG_STUNT".as_slice(),
    ]
    .iter()
    .any(|name| hash == name_id(name))
    {
        Err(Refusal::HangingNotVerified)
    } else {
        Err(Refusal::OrdinaryOrUnsupportedState)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_admits_only_the_exact_ground_sequence() {
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
}
