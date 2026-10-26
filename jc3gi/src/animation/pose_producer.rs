#![cfg_attr(any(), rustfmt::skip)]
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How an animation instance treats playback at the end of its duration.
pub enum AnimLoopMode {
    Loop = 0isize as _,
    PlayOnce = 1isize as _,
}
fn _AnimLoopMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AnimLoopMode>([0u8; 0x4]);
    }
    unreachable!()
}
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
/// The binding between an animation's transform tracks and the skeleton's bones.
pub struct AnimationBinding {
    _field_0: [u8; 32],
    /// Maps each animation transform track to its corresponding skeleton bone index.
    pub m_TransformTrackToBoneIndices: crate::animation::skeleton::HkArray<i16>,
}
fn _AnimationBinding_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], AnimationBinding>([0u8; 0x30]);
    }
    unreachable!()
}
impl AnimationBinding {}
impl std::convert::AsRef<AnimationBinding> for AnimationBinding {
    fn as_ref(&self) -> &AnimationBinding {
        self
    }
}
impl std::convert::AsMut<AnimationBinding> for AnimationBinding {
    fn as_mut(&mut self) -> &mut AnimationBinding {
        self
    }
}
#[repr(C, align(8))]
/// An animation layer's clip driver (`CAnimationDriverTransition`): follows the layer's rule system,
/// starts and blends the clips of its current state, and owns the layer's [`Blender`](crate::animation::pose_producer::Blender). The clips it
/// plays send the layer's track messages and answer its segment queries whether or not the blender's
/// pose reaches the skeleton.
pub struct AnimationDriverTransition {
    _field_0: [u8; 48],
    /// The layer's animation rule system, which supplies the current state and transition choices.
    pub m_RuleSystem: crate::types::shared_ptr::SharedPtr<
        crate::animation::rule_system::AnimationRuleSystem,
    >,
    /// Animation instances selected by the driver for playback.
    pub m_PlayingAnimInstances: crate::types::std_vector::Vector<
        crate::types::shared_ptr::SharedPtr<
            crate::animation::pose_producer::AnimationInstance,
        >,
    >,
    _field_60: [u8; 120],
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
impl AnimationDriverTransition {
    pub const PlayCustomAnimation_ADDRESS: usize = 0x140474AB0;
    /// Starts `instance`, a clip chosen by code rather than by the layer's rule system, on the
    /// layer, blending it in over `blend` seconds.
    /// [`AnimationController::PlayCustomAnimation`](crate::character::character::AnimationController::PlayCustomAnimation)
    /// calls it.
    pub unsafe fn PlayCustomAnimation(
        &mut self,
        instance: *mut crate::types::shared_ptr::SharedPtr<
            crate::animation::pose_producer::AnimationInstance,
        >,
        blend: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::types::shared_ptr::SharedPtr<
                    crate::animation::pose_producer::AnimationInstance,
                >,
                blend: f32,
            ) = ::std::mem::transmute(Self::PlayCustomAnimation_ADDRESS);
            f(self as *mut Self as _, instance, blend)
        }
    }
}
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
/// One playing clip (`CAnimationInstance`): a clip resource bound to a skeleton, with its
/// playback time, weight, easing, and loop mode. A [`Blender`](crate::animation::pose_producer::Blender) samples its instances into a pose.
pub struct AnimationInstance {
    _field_0: [u8; 16],
    /// The animation resource handle retained by this instance.
    pub m_Animation: *mut ::std::ffi::c_void,
    _field_18: [u8; 8],
    /// The playback mode selected when this instance is constructed.
    pub m_LoopMode: crate::animation::pose_producer::AnimLoopMode,
    _field_24: [u8; 36],
    /// Per-transform-track weights used when the control contributes to a blended pose.
    pub m_TransformTrackWeights: crate::animation::skeleton::HkArray<u8>,
    _field_58: [u8; 16],
    /// The mapping from this animation's transform tracks to skeleton bones.
    pub m_Binding: *mut crate::animation::pose_producer::AnimationBinding,
    _field_70: [u8; 32],
    /// The number of positive playback-time wraps recorded by the embedded animation control.
    pub m_OverflowCount: u32,
    /// The number of negative playback-time wraps recorded by the embedded animation control.
    pub m_UnderflowCount: u32,
    _field_98: [u8; 136],
}
fn _AnimationInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x120], AnimationInstance>([0u8; 0x120]);
    }
    unreachable!()
}
impl AnimationInstance {
    pub const Construct_ADDRESS: usize = 0x141E68BD0;
    /// Constructs an instance of the clip resource `animation` (a `TResourceCachePtr<CAnimation>`,
    /// one resource handle) over the skeleton resource `skeleton`, configured by `context` (a
    /// `CAnimationInstance::SCreationContext`: its easing, start and finish trims, speed, mirror
    /// flag, and loop mode). A context with the mirror flag set binds the clip mirrored.
    pub unsafe fn Construct(
        &mut self,
        animation: *const *mut ::std::ffi::c_void,
        skeleton: *const *mut ::std::ffi::c_void,
        context: *const ::std::ffi::c_void,
    ) -> *mut crate::animation::pose_producer::AnimationInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                animation: *const *mut ::std::ffi::c_void,
                skeleton: *const *mut ::std::ffi::c_void,
                context: *const ::std::ffi::c_void,
            ) -> *mut crate::animation::pose_producer::AnimationInstance = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _, animation, skeleton, context)
        }
    }
    pub const GetTime_ADDRESS: usize = 0x141E66EA0;
    /// Returns the embedded animation control's current local playback time.
    pub unsafe fn GetTime(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetTime_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetTotalWeight_ADDRESS: usize = 0x141E66F00;
    /// Returns the embedded animation control's total playback weight.
    pub unsafe fn GetTotalWeight(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetTotalWeight_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetDuration_ADDRESS: usize = 0x141E67B10;
    /// Returns the duration of the animation resource retained by this instance.
    pub unsafe fn GetDuration(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetDuration_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<AnimationInstance> for AnimationInstance {
    fn as_ref(&self) -> &AnimationInstance {
        self
    }
}
impl std::convert::AsMut<AnimationInstance> for AnimationInstance {
    fn as_mut(&mut self) -> &mut AnimationInstance {
        self
    }
}
#[repr(C, align(8))]
/// One animation layer's sampler set (`CBlender`): wraps a Havok animated skeleton whose animation
/// controls are the layer driver's playing clips.
pub struct Blender {
    _field_0: [u8; 8],
    /// Animation instances whose controls are registered with the blender's animated skeleton.
    pub m_Samplers: crate::animation::skeleton::HkArray<
        crate::types::shared_ptr::SharedPtr<
            crate::animation::pose_producer::AnimationInstance,
        >,
    >,
}
fn _Blender_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], Blender>([0u8; 0x18]);
    }
    unreachable!()
}
impl Blender {
    pub const UpdatePose_ADDRESS: usize = 0x141E6BE10;
    /// Samples the blender's clips and blends them over `reference_pose` into `pose_out`, both `num_bones` local-space transforms, and likewise the float slots from `reference_floats` into `float_slots_out` (`num_float_slots` each). Where the clips' total weight on a bone is below the skeleton's reference-pose threshold, the reference fills in. `normal_blend_pose_out` is non-null only for the first layer, which also copies its result there (the pose producer's pre-additive pose). It writes nothing but its outputs. [`PoseProducer::UpdateBlendersPoseInRange`](crate::animation::pose_producer::PoseProducer::UpdateBlendersPoseInRange) calls it during synchronous updates and pose-job execution.
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
    /// Whether the blender has any animation controls. [`PoseProducer::UpdateBlendersPoseInRange`](crate::animation::pose_producer::PoseProducer::UpdateBlendersPoseInRange) skips a later layer's blender without them.
    pub unsafe fn HasSamplers(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::HasSamplers_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const AddSampler_ADDRESS: usize = 0x141E6B930;
    /// Adds `instance` to the blender's samplers: registers its Havok animation control with the
    /// blender's animated skeleton and keeps a reference to it.
    pub unsafe fn AddSampler(
        &mut self,
        instance: *mut crate::types::shared_ptr::SharedPtr<
            crate::animation::pose_producer::AnimationInstance,
        >,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::types::shared_ptr::SharedPtr<
                    crate::animation::pose_producer::AnimationInstance,
                >,
            ) = ::std::mem::transmute(Self::AddSampler_ADDRESS);
            f(self as *mut Self as _, instance)
        }
    }
    pub const ClearSamplers_ADDRESS: usize = 0x141E6B9B0;
    /// Removes every sampler's animation control from the animated skeleton and releases the
    /// samplers.
    pub unsafe fn ClearSamplers(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ClearSamplers_ADDRESS,
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
/// A pose producer (`CPoseProducer`): owns an animated model's blenders and the `T0`/`T1` poses the skinning palette is built from.
///
/// [`UpdateBlendersPoseInRange`](crate::animation::pose_producer::PoseProducer::UpdateBlendersPoseInRange) builds the local-space pose layer by layer. The first blender samples over the skeleton's reference pose. Each later blender that has samplers runs [`Blender::UpdatePose`](crate::animation::pose_producer::Blender::UpdatePose) with the previous blender's output as its reference pose, so a partial layer overlays the pose below it by its clips' per-bone weights, and an additive layer adds onto it. A later blender without samplers is skipped, leaving the pose below it unchanged. The blenders are added in the order of the animation controller's body parts ([`AnimationController::m_BodyParts`](crate::character::character::AnimationController::m_BodyParts)), one per layer.
///
/// The poses are reused buffers. [`Update`](crate::animation::pose_producer::PoseProducer::Update) exchanges their pointers before synchronously evaluating [`m_PoseT1`](crate::animation::pose_producer::PoseProducer::m_PoseT1). The asynchronous pose-job path (`CPoseProducer::KickUpdateJob`) also exchanges them before queuing `PoseUpdate_fragment` on a CPU fragment worker. [`m_PoseT0`](crate::animation::pose_producer::PoseProducer::m_PoseT0) receives the previously current pose, including changes made before the exchange, such as HumanIK results and joint writes. The skinning job blends `T0` to `T1`.
pub struct PoseProducer {
    _field_0: [u8; 32],
    /// The previously current pose after the latest buffer exchange. [`Update`](crate::animation::pose_producer::PoseProducer::Update) exchanges this pointer with [`m_PoseT1`](crate::animation::pose_producer::PoseProducer::m_PoseT1) before evaluating the new current output.
    pub m_PoseT0: *mut crate::animation::skeleton::HkaPose,
    /// The current pose buffer. [`UpdateBlendersPoseInRange`](crate::animation::pose_producer::PoseProducer::UpdateBlendersPoseInRange) writes its selected local transforms and float slots. The animation controller's joint accessors ([`AnimationController::GetJoint`](crate::character::character::AnimationController::GetJoint) and [`SetJoint`](crate::character::character::AnimationController::SetJoint)) read and write it in model space.
    pub m_PoseT1: *mut crate::animation::skeleton::HkaPose,
}
fn _PoseProducer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], PoseProducer>([0u8; 0x30]);
    }
    unreachable!()
}
impl PoseProducer {
    pub const Update_ADDRESS: usize = 0x141E6A9A0;
    /// Exchanges [`m_PoseT0`](crate::animation::pose_producer::PoseProducer::m_PoseT0) and [`m_PoseT1`](crate::animation::pose_producer::PoseProducer::m_PoseT1), advances each blender's time by `dt`, and synchronously evaluates all blenders through [`UpdateBlendersPoseInRange`](crate::animation::pose_producer::PoseProducer::UpdateBlendersPoseInRange) for `num_bones`. The selected local transforms and float slots are copied into the current pose before this call returns. The blender range is captured before the time updates.
    pub unsafe fn Update(&mut self, dt: f32, num_bones: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, num_bones: i32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt, num_bones)
        }
    }
    pub const UpdateBlendersPoseInRange_ADDRESS: usize = 0x141E6A090;
    /// Synchronously evaluates a blender range for `num_bones` local transforms and the skeleton's float slots. Clamps `to_blender` to the current blender count and returns without producing output when `from_blender` exceeds that end. A start at the first blender samples it over the skeleton's reference pose before the later-layer loop. A later start initializes the temporary output from the current [`m_PoseT1`](crate::animation::pose_producer::PoseProducer::m_PoseT1). The loop applies blenders below the clamped end. Copies the selected local transforms and float slots into the current pose, then frees the temporaries before returning. This operation does not exchange the pose pointers.
    pub unsafe fn UpdateBlendersPoseInRange(
        &mut self,
        from_blender: i32,
        to_blender: i32,
        num_bones: i32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                from_blender: i32,
                to_blender: i32,
                num_bones: i32,
            ) = ::std::mem::transmute(Self::UpdateBlendersPoseInRange_ADDRESS);
            f(self as *mut Self as _, from_blender, to_blender, num_bones)
        }
    }
    pub const GetJoint_ADDRESS: usize = 0x141E693D0;
    /// Copies bone `index` from the current [`m_PoseT1`](crate::animation::pose_producer::PoseProducer::m_PoseT1) into `result` in model space and returns `result`. A dirty model-space transform is calculated lazily before the complete joint is copied.
    ///
    /// The current pose must be live, `index` must be nonnegative and valid for its bone flags and model-space transforms, and `result` must point to writable aligned joint storage. There are no null or range guards. The lazy calculation can modify the pose caches, so the call requires serialized access to the current pose. It does not read [`m_PoseT0`](crate::animation::pose_producer::PoseProducer::m_PoseT0).
    ///
    /// IDA labels this address `TResourceCachePtr<CSkeleton>::operator->`; the body is the pose producer's joint accessor.
    pub unsafe fn GetJoint(
        &mut self,
        result: *mut crate::character::character::Joint,
        index: i32,
    ) -> *mut crate::character::character::Joint {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                result: *mut crate::character::character::Joint,
                index: i32,
            ) -> *mut crate::character::character::Joint = ::std::mem::transmute(
                Self::GetJoint_ADDRESS,
            );
            f(self as *mut Self as _, result, index)
        }
    }
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
pub const AquireAnimation_ADDRESS: usize = 0x1403EBFD0;
/// `IAnimationManager::AquireAnimation`: the clip resource for the clip path `path` (transformed
/// to its compiled name and hashed with `HashString`), from the global resource cache. It writes
/// the resource handle (a `TResourceCachePtr<CAnimation>`) into `result` and returns it.
pub unsafe fn AquireAnimation(
    result: *mut *mut ::std::ffi::c_void,
    path: *const u8,
) -> *mut *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            result: *mut *mut ::std::ffi::c_void,
            path: *const u8,
        ) -> *mut *mut ::std::ffi::c_void = ::std::mem::transmute(
            AquireAnimation_ADDRESS,
        );
        f(result, path)
    }
}
pub const AquireAnimationFromCache_ADDRESS: usize = 0x1404823D0;
/// `IAnimationManager::AquireAnimation` with an explicit resource cache: like [`AquireAnimation`](crate::animation::pose_producer::AquireAnimation)
/// but from `cache`, and writing a null handle for a null or empty `path`.
pub unsafe fn AquireAnimationFromCache(
    result: *mut *mut ::std::ffi::c_void,
    path: *const u8,
    cache: *mut ::std::ffi::c_void,
) -> *mut *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            result: *mut *mut ::std::ffi::c_void,
            path: *const u8,
            cache: *mut ::std::ffi::c_void,
        ) -> *mut *mut ::std::ffi::c_void = ::std::mem::transmute(
            AquireAnimationFromCache_ADDRESS,
        );
        f(result, path, cache)
    }
}
