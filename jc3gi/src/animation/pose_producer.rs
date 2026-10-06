#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// One animation layer of an animation controller (`CAnimatedBodyPart`): the layer's rule system and
/// its animation driver. Created by `CAnimationControl::AddAnimatedBodyPart`, which also adds the
/// driver's [`Blender`](crate::animation::pose_producer::Blender) to the controller's [`PoseProducer`](crate::animation::pose_producer::PoseProducer).
pub struct AnimatedBodyPart {
    _field_0: [u8; 24],
    /// The driver that selects and plays the layer's clips from its rule system's state.
    pub m_AnimationDriver: *mut crate::animation::pose_producer::AnimationDriverTransition,
}
fn _AnimatedBodyPart_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], AnimatedBodyPart>([0u8; 0x20]);
    }
    unreachable!()
}
impl AnimatedBodyPart {}
impl std::convert::AsRef<AnimatedBodyPart> for AnimatedBodyPart {
    fn as_ref(&self) -> &AnimatedBodyPart {
        self
    }
}
impl std::convert::AsMut<AnimatedBodyPart> for AnimatedBodyPart {
    fn as_mut(&mut self) -> &mut AnimatedBodyPart {
        self
    }
}
#[repr(C, align(8))]
/// An animation layer's clip driver (`CAnimationDriverTransition`): follows the layer's rule system,
/// starts and blends the clips of its current state, and owns the layer's [`Blender`](crate::animation::pose_producer::Blender). The clips it
/// plays send the layer's track messages and answer its segment queries whether or not the blender's
/// pose reaches the skeleton.
pub struct AnimationDriverTransition {
    _field_0: [u8; 216],
    /// The layer's blender, added to the pose producer when the body part is created.
    pub m_Blender: crate::types::shared_ptr::SharedPtr<
        crate::animation::pose_producer::Blender,
    >,
}
fn _AnimationDriverTransition_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE8], AnimationDriverTransition>([0u8; 0xE8]);
    }
    unreachable!()
}
impl AnimationDriverTransition {}
impl std::convert::AsRef<AnimationDriverTransition> for AnimationDriverTransition {
    fn as_ref(&self) -> &AnimationDriverTransition {
        self
    }
}
impl std::convert::AsMut<AnimationDriverTransition> for AnimationDriverTransition {
    fn as_mut(&mut self) -> &mut AnimationDriverTransition {
        self
    }
}
#[repr(C, align(8))]
/// One animation layer's sampler set (`CBlender`): wraps a Havok animated skeleton whose animation
/// controls are the layer driver's playing clips.
pub struct Blender {}
impl Blender {
    pub const UpdatePose_ADDRESS: usize = 0x141E6BE10;
    /// Samples the blender's clips and blends them over `reference_pose` into `pose_out`, both
    /// `num_bones` local-space transforms, and likewise the float slots from `reference_floats` into
    /// `float_slots_out` (`num_float_slots` each). Where the clips' total weight on a bone is below
    /// the skeleton's reference-pose threshold, the reference fills in. `normal_blend_pose_out` is
    /// non-null only for the first layer, which also copies its result there (the pose producer's
    /// pre-additive pose). It writes nothing but its outputs. Called by the pose job on a worker
    /// thread, once per layer that has samplers (see [`PoseProducer`](crate::animation::pose_producer::PoseProducer)).
    pub unsafe fn UpdatePose(
        &mut self,
        pose_out: *mut crate::animation::pose_producer::QsTransform,
        reference_pose: *mut crate::animation::pose_producer::QsTransform,
        normal_blend_pose_out: *mut crate::animation::pose_producer::QsTransform,
        num_bones: u32,
        float_slots_out: *mut f32,
        reference_floats: *mut f32,
        num_float_slots: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pose_out: *mut crate::animation::pose_producer::QsTransform,
                reference_pose: *mut crate::animation::pose_producer::QsTransform,
                normal_blend_pose_out: *mut crate::animation::pose_producer::QsTransform,
                num_bones: u32,
                float_slots_out: *mut f32,
                reference_floats: *mut f32,
                num_float_slots: u32,
            ) = ::std::mem::transmute(Self::UpdatePose_ADDRESS);
            f(
                self as *mut Self as _,
                pose_out,
                reference_pose,
                normal_blend_pose_out,
                num_bones,
                float_slots_out,
                reference_floats,
                num_float_slots,
            )
        }
    }
    pub const HasSamplers_ADDRESS: usize = 0x141E6B1C0;
    /// Whether the blender has any animation controls; the pose job skips a later layer's blender
    /// without them.
    pub unsafe fn HasSamplers(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::HasSamplers_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<Blender> for Blender {
    fn as_ref(&self) -> &Blender {
        self
    }
}
impl std::convert::AsMut<Blender> for Blender {
    fn as_mut(&mut self) -> &mut Blender {
        self
    }
}
#[repr(C, align(8))]
/// A pose producer (`CPoseProducer`): owns an animated model's blenders and the `T0`/`T1` poses
/// the skinning palette is built from.
///
/// The pose job (`CPoseProducer::UpdateBlendersPoseInRange`) builds the local-space pose layer by
/// layer: the first blender samples over the skeleton's reference pose, and each later blender that
/// has samplers runs [`Blender::UpdatePose`](crate::animation::pose_producer::Blender::UpdatePose) with the previous blender's output as its reference
/// pose, so a partial layer overlays the pose below it by its clips' per-bone weights and an
/// additive layer adds onto it. A blender without samplers is skipped, leaving the pose below it
/// unchanged. The blenders are added in the order of the animation controller's body parts
/// ([`AnimationController::m_BodyParts`](crate::character::character::AnimationController::m_BodyParts)),
/// one per layer.
///
/// The two poses are a tick-spaced pair. Kicking the pose job (`CPoseProducer::KickUpdateJob`)
/// swaps them and queues `PoseUpdate_fragment` on a CPU fragment worker, so the pose the job
/// writes this tick is the buffer that held the tick before last, and the previous
/// tick's result (with every change made to it after the job, the HumanIK solve and joint writes
/// included) becomes [`m_PoseT0`](crate::animation::pose_producer::PoseProducer::m_PoseT0) untouched. The skinning job blends `T0` to `T1`.
pub struct PoseProducer {
    _field_0: [u8; 32],
    /// The previous tick's pose.
    pub m_PoseT0: *mut crate::animation::skeleton::HkaPose,
    /// The current tick's pose: the one the pose job writes, and the one the animation controller's
    /// joint accessors
    /// ([`AnimationController::GetJoint`](crate::character::character::AnimationController::GetJoint) and
    /// [`SetJoint`](crate::character::character::AnimationController::SetJoint)) read and write in model
    /// space.
    pub m_PoseT1: *mut crate::animation::skeleton::HkaPose,
}
fn _PoseProducer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], PoseProducer>([0u8; 0x30]);
    }
    unreachable!()
}
impl PoseProducer {
    pub const MakeSkinningPalette_ADDRESS: usize = 0x141E69600;
    /// Queues a skinning-palette job for this producer: advances the time of any time-updatable
    /// blender, then records the skeleton's bone count, `dtf`, and the two poses into the next slot
    /// of the global skinning job table (up to 0x1000 jobs per frame) together with the output
    /// `palette` buffer. The skinning job later blends the `T0` and `T1` poses by `dtf` into the
    /// palette. Reached from
    /// [`AnimationController::UpdateSkinning`](crate::character::character::AnimationController::UpdateSkinning)
    /// in the render phase.
    pub unsafe fn MakeSkinningPalette(
        &mut self,
        dtf: f32,
        palette: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dtf: f32,
                palette: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::MakeSkinningPalette_ADDRESS);
            f(self as *mut Self as _, dtf, palette)
        }
    }
}
impl std::convert::AsRef<PoseProducer> for PoseProducer {
    fn as_ref(&self) -> &PoseProducer {
        self
    }
}
impl std::convert::AsMut<PoseProducer> for PoseProducer {
    fn as_mut(&mut self) -> &mut PoseProducer {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(16))]
/// One bone's local-space transform in a Havok pose (`hkQsTransformf`): translation, rotation, and
/// scale, each a 16-byte vector.
pub struct QsTransform {
    pub m_Translation: crate::types::vector_math::AlignedVector3,
    pub m_Rotation: crate::types::vector_math::AlignedQuat,
    pub m_Scale: crate::types::vector_math::AlignedVector3,
}
fn _QsTransform_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], QsTransform>([0u8; 0x30]);
    }
    unreachable!()
}
impl QsTransform {}
impl std::convert::AsRef<QsTransform> for QsTransform {
    fn as_ref(&self) -> &QsTransform {
        self
    }
}
impl std::convert::AsMut<QsTransform> for QsTransform {
    fn as_mut(&mut self) -> &mut QsTransform {
        self
    }
}
