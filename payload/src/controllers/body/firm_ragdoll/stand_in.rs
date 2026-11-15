//! The stand-in pose producer the firm ragdoll's pre-physics step reads its muscle target from.
//!
//! The ragdoll's pre-physics step
//! (`CRagdollInstanceImpl::PrePhysicsUpdate`, `docs/engine/character/ragdoll.md`) reads only the
//! producer's current pose ([`PoseProducer::m_PoseT1`]) and maps it onto the ragdoll's skeleton.
//! Handing it a producer of the mod's own, whose current pose holds the muscle target
//! ([`crate::controllers::body::firm_ragdoll::target`]), retargets the keyframed bodies and the
//! motors without touching the game's producer, whose current pose the post-physics blend, the
//! HumanIK solve, and the skinning go on using.
//!
//! When the step runs, the producer has exchanged its poses: its previous pose
//! ([`PoseProducer::m_PoseT0`]) is the last tick's final pose, the HumanIK solve and the joint
//! writes included, whose model-space transforms the skinning was built from, and its current pose
//! holds this tick's animation in local space. The stand-in's pose is the game's current pose's
//! header with its arrays pointed at storage the mod owns, flagged so that nothing frees it
//! ([`HK_ARRAY_DONT_DEALLOCATE_FLAG`]), with the local and model-space transforms both written and
//! no bone marked stale.

use glam::Vec3;
use jc3gi::{
    animation::{
        pose_producer::{PoseProducer, QsTransform},
        skeleton::{HK_ARRAY_DONT_DEALLOCATE_FLAG, HkArray, HkaPose},
    },
    types::vector_math::{AlignedQuat, AlignedVector3},
};

use crate::controllers::body::{
    firm_ragdoll::target::{
        IkLegs, TargetBones, TargetInput, local_space, model_space, muscle_target,
    },
    procedural::skeleton::Transform,
};

/// How far the muscle target stands from the clip's pose, for the readout and the log.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TargetReport {
    /// The furthest any bone above the hips moved from the clip's pose to the drawn one, metres:
    /// how far the keyframed bodies are retargeted.
    pub upper_shift_m: f32,
    /// How far each thigh turned from the clip's, degrees, left then right.
    pub thigh_turn_deg: [f32; 2],
}

/// A pose producer of the mod's own, and the storage of its current pose.
pub(crate) struct StandIn {
    local: Vec<QsTransform>,
    model: Vec<QsTransform>,
    flags: Vec<u32>,
    /// Made on the first refresh, and reused.
    pose: Option<Box<PoseBuffer>>,
    producer: Option<Box<PoseProducer>>,
}

impl StandIn {
    pub(crate) const fn new() -> Self {
        Self {
            local: Vec::new(),
            model: Vec::new(),
            flags: Vec::new(),
            pose: None,
            producer: None,
        }
    }

    /// Compose the muscle target from `producer`'s poses with the bones `bones` and the IK body's
    /// legs `legs`, and return the stand-in producer whose current pose holds it, valid until the
    /// next refresh, with how far the target stands from the clip. `None` where the producer's
    /// poses cannot be read or do not fit its skeleton.
    ///
    /// # Safety
    ///
    /// `producer` must be the live producer of the character whose ragdoll steps, between its
    /// buffer exchange and the end of the ragdoll's pre-physics step, on the thread running it.
    pub(crate) unsafe fn refresh(
        &mut self,
        producer: &PoseProducer,
        bones: TargetBones,
        legs: Option<IkLegs>,
    ) -> Option<(*mut PoseProducer, TargetReport)> {
        let last = unsafe { producer.m_PoseT0.as_ref() }?;
        let current = unsafe { producer.m_PoseT1.as_ref() }?;
        if last.m_Skeleton != current.m_Skeleton {
            return None;
        }
        let skeleton = unsafe { current.m_Skeleton.as_ref() }?;
        let parents = unsafe { slice(&skeleton.m_ParentIndices) }?;
        let count = parents.len();
        let drawn = unsafe { slice(&last.m_ModelPose) }?.get(..count)?;
        let animated = unsafe { slice(&current.m_LocalPose) }?.get(..count)?;
        let drawn: Vec<Transform> = drawn.iter().map(Transform::from_qs).collect();
        let animated: Vec<Transform> = animated.iter().map(Transform::from_qs).collect();
        let last_final = local_space(parents, &drawn);
        let target = muscle_target(&TargetInput {
            parents,
            last_final: &last_final,
            animated: &animated,
            bones,
            legs,
        })?;
        let model = model_space(parents, &target);
        let report = report(parents, &model, &model_space(parents, &animated), bones);
        self.local.clear();
        self.local.extend(target.iter().map(to_qs));
        self.model.clear();
        self.model.extend(model.iter().map(to_qs));
        self.flags.clear();
        self.flags.resize(count, 0);
        // SAFETY: both are plain data whose all-zero pattern is valid: null pointers and empty
        // arrays; the pointers are filled below.
        let buffer = self
            .pose
            .get_or_insert_with(|| Box::new(unsafe { std::mem::zeroed() }));
        buffer.pose = unsafe { std::mem::zeroed() };
        buffer.pose.m_Skeleton = current.m_Skeleton;
        buffer.pose.m_LocalPose = borrowed(&mut self.local);
        buffer.pose.m_ModelPose = borrowed(&mut self.model);
        buffer.pose.m_BoneFlags = borrowed(&mut self.flags);
        let pose: *mut HkaPose = &mut buffer.pose;
        let stand_in = self
            .producer
            .get_or_insert_with(|| Box::new(unsafe { std::mem::zeroed() }));
        stand_in.m_PoseT0 = producer.m_PoseT0;
        stand_in.m_PoseT1 = pose;
        Some((&mut **stand_in, report))
    }
}

/// How far the target `target` stands from the clip `animated`, both model space.
fn report(
    parents: &[i16],
    target: &[Transform],
    animated: &[Transform],
    bones: TargetBones,
) -> TargetReport {
    let upper_shift_m = (0..target.len())
        .filter(|bone| !below_hips(parents, *bone, bones))
        .map(|bone| {
            target[bone]
                .translation
                .distance(animated[bone].translation)
        })
        .fold(0.0, f32::max);
    let thigh_turn_deg = bones.legs.map(|[up, knee, _]| {
        let thigh = |pose: &[Transform]| pose[knee].translation - pose[up].translation;
        thigh(target).angle_between(thigh(animated)).to_degrees()
    });
    TargetReport {
        upper_shift_m,
        thigh_turn_deg,
    }
}

/// Whether `bone` is the hips or hangs from them other than through the spine.
fn below_hips(parents: &[i16], bone: usize, bones: TargetBones) -> bool {
    let mut at = Some(bone);
    while let Some(b) = at {
        if b == bones.spine {
            return false;
        }
        if b == bones.hips {
            return true;
        }
        at = usize::try_from(parents[b]).ok();
    }
    false
}

// SAFETY: the stand-in's pointers point into its own storage and at the game's skeleton and pose,
// which outlive the character's passes; it is used only under its lock, on the thread running the
// character's physics passes.
unsafe impl Send for StandIn {}

/// The stand-in's pose header, padded past the bound fields with zeros in case the mapping reads
/// further into the header than the binding describes.
#[repr(C, align(16))]
struct PoseBuffer {
    pose: HkaPose,
    _tail: [u8; POSE_TAIL],
}

/// The zeroed bytes after the bound pose header.
const POSE_TAIL: usize = 0x48;

/// The elements of `array`, or `None` for a null or negative one.
///
/// # Safety
///
/// `array` must be a live Havok array whose elements stay put while the slice is used.
unsafe fn slice<T>(array: &HkArray<T>) -> Option<&[T]> {
    let size = usize::try_from(array.size).ok()?;
    if array.data.is_null() || size == 0 {
        return None;
    }
    // SAFETY: the caller's contract: `size` live elements at `data`.
    Some(unsafe { std::slice::from_raw_parts(array.data, size) })
}

/// A Havok array over `values`, marked so that nothing frees the storage.
fn borrowed<T>(values: &mut Vec<T>) -> HkArray<T> {
    let size = values.len() as i32;
    HkArray {
        data: values.as_mut_ptr(),
        size,
        capacity_and_flags: (size as u32 | HK_ARRAY_DONT_DEALLOCATE_FLAG) as i32,
    }
}

fn to_qs(t: &Transform) -> QsTransform {
    QsTransform {
        m_Translation: vector(t.translation),
        m_Rotation: AlignedQuat {
            data: t.rotation.to_array(),
        },
        m_Scale: vector(t.scale),
    }
}

fn vector(v: Vec3) -> AlignedVector3 {
    AlignedVector3 {
        data: v.to_array(),
        _w: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;
    use jc3gi::animation::skeleton::HkaSkeleton;

    use super::*;
    use crate::controllers::body::firm_ragdoll::target::tests::{BONES, PARENTS, standing};

    fn array<T>(values: &mut [T]) -> HkArray<T> {
        HkArray {
            data: values.as_mut_ptr(),
            size: values.len() as i32,
            capacity_and_flags: values.len() as i32,
        }
    }

    #[test]
    fn the_stand_in_holds_the_target_and_leaves_the_games_producer_alone() {
        let mut parents = PARENTS;
        // SAFETY: plain data; the zero pattern is valid.
        let mut skeleton: HkaSkeleton = unsafe { std::mem::zeroed() };
        skeleton.m_ParentIndices = array(&mut parents);
        let lean = Quat::from_rotation_x(0.4);
        let drawn_local = standing(lean);
        let animated_local = standing(Quat::IDENTITY);
        let mut drawn: Vec<QsTransform> = model_space(&PARENTS, &drawn_local)
            .iter()
            .map(to_qs)
            .collect();
        let mut animated: Vec<QsTransform> = animated_local.iter().map(to_qs).collect();
        let mut stale: Vec<QsTransform> = vec![QsTransform::default(); PARENTS.len()];
        let mut flags = vec![0u32; PARENTS.len()];
        let mut flags_current = vec![0u32; PARENTS.len()];
        // SAFETY: plain data; the zero pattern is valid.
        let mut last: HkaPose = unsafe { std::mem::zeroed() };
        last.m_Skeleton = &skeleton;
        last.m_ModelPose = array(&mut drawn);
        last.m_LocalPose = array(&mut stale);
        last.m_BoneFlags = array(&mut flags);
        // SAFETY: as above.
        let mut current: HkaPose = unsafe { std::mem::zeroed() };
        current.m_Skeleton = &skeleton;
        current.m_LocalPose = array(&mut animated);
        current.m_BoneFlags = array(&mut flags_current);
        // SAFETY: as above.
        let mut producer: PoseProducer = unsafe { std::mem::zeroed() };
        producer.m_PoseT0 = &mut last;
        producer.m_PoseT1 = &mut current;

        let mut stand_in = StandIn::new();
        let (fed, report) = unsafe { stand_in.refresh(&producer, BONES, None) }.unwrap();
        let fed = unsafe { &*fed };
        // The drawn lean moves the chest off the clip's, and the hips and the legs hang from the
        // drawn spine, leaning with it.
        assert!(report.upper_shift_m > 0.1, "{report:?}");
        let lean_deg = 0.4f32.to_degrees();
        assert!(
            report
                .thigh_turn_deg
                .iter()
                .all(|turn| (turn - lean_deg).abs() < 0.5),
            "{report:?}"
        );
        assert!(std::ptr::eq(producer.m_PoseT1, &current));
        assert!(!std::ptr::eq(fed.m_PoseT1, &current));
        let pose = unsafe { &*fed.m_PoseT1 };
        assert!(std::ptr::eq(pose.m_Skeleton, &skeleton));
        assert_eq!(pose.m_LocalPose.size, PARENTS.len() as i32);
        assert_ne!(
            pose.m_LocalPose.capacity_and_flags as u32 & HK_ARRAY_DONT_DEALLOCATE_FLAG,
            0
        );
        // The upper body is held where the last tick drew it.
        let model = unsafe { slice(&pose.m_ModelPose) }.unwrap();
        let drawn_model = model_space(&PARENTS, &drawn_local);
        let chest = Transform::from_qs(&model[3]);
        assert!(
            chest
                .translation
                .abs_diff_eq(drawn_model[3].translation, 1e-5)
        );
        // The local and model poses agree.
        let local: Vec<Transform> = unsafe { slice(&pose.m_LocalPose) }
            .unwrap()
            .iter()
            .map(Transform::from_qs)
            .collect();
        let rebuilt = model_space(&PARENTS, &local);
        for (a, b) in rebuilt.iter().zip(model) {
            assert!(
                a.translation
                    .abs_diff_eq(Transform::from_qs(b).translation, 1e-5)
            );
        }
        // A producer whose poses are missing feeds nothing.
        producer.m_PoseT0 = std::ptr::null_mut();
        assert!(unsafe { stand_in.refresh(&producer, BONES, None) }.is_none());
    }
}
