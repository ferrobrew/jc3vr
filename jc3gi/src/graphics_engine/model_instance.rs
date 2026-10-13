#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The draw-list build context `CModelInstance::AddToRender` fills and hands to the
/// `ForEachRb` instantiations (`CModelInstance::SAddtoRenderContext`): the target passes and the
/// camera the render blocks are tested against. Opaque.
pub struct AddToRenderContext {}
impl AddToRenderContext {}
impl std::convert::AsRef<AddToRenderContext> for AddToRenderContext {
    fn as_ref(&self) -> &AddToRenderContext {
        self
    }
}
impl std::convert::AsMut<AddToRenderContext> for AddToRenderContext {
    fn as_mut(&mut self) -> &mut AddToRenderContext {
        self
    }
}
#[repr(C, align(8))]
/// The BFBC result context the model collection cull runs in. Opaque.
pub struct BFBCContext {}
impl BFBCContext {}
impl std::convert::AsRef<BFBCContext> for BFBCContext {
    fn as_ref(&self) -> &BFBCContext {
        self
    }
}
impl std::convert::AsMut<BFBCContext> for BFBCContext {
    fn as_mut(&mut self) -> &mut BFBCContext {
        self
    }
}
#[repr(C, align(8))]
/// A model collection (`CModelCollection`). Opaque.
pub struct ModelCollection {}
impl ModelCollection {
    pub const DoCulling_ADDRESS: usize = 0x1404E7950;
    /// The instance-level broad-phase cull: runs a BFBC frustum and distance process over the model
    /// collections against the occluder manager's main cull camera (its cached parameters from
    /// `GetBFBCFrustumParamsForCameraAndTime`), passing each survivor on through
    /// `CModelCollection::CollectionCallback` and `CModelInstanceManager::BFBCCallback`, and
    /// accumulating `cull_stat`. Static. IDA labels this address
    /// `CAOVolumeManager::AddToRenderThread`; it is `CModelCollection::DoCulling`.
    pub unsafe fn DoCulling(
        bfbc: *mut crate::graphics_engine::model_instance::BFBCContext,
        cull_stat: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                bfbc: *mut crate::graphics_engine::model_instance::BFBCContext,
                cull_stat: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::DoCulling_ADDRESS);
            f(bfbc, cull_stat)
        }
    }
}
impl std::convert::AsRef<ModelCollection> for ModelCollection {
    fn as_ref(&self) -> &ModelCollection {
        self
    }
}
impl std::convert::AsMut<ModelCollection> for ModelCollection {
    fn as_mut(&mut self) -> &mut ModelCollection {
        self
    }
}
#[repr(C, align(8))]
/// A placed model instance (`NModelSystem::CModelInstance`): a shared model plus its render-block
/// instance info and world transform. Only some members are mapped.
pub struct ModelInstance {
    _field_0: [u8; 12],
    /// The instance's status bits (`m_Flags`). The weapon's visibility virtual
    /// ([`WeaponBase::SetVisible`](crate::equipment::weapon::WeaponBase::SetVisible)) and the model
    /// parts' ([`WeaponComponentBase::SetVisible`](crate::equipment::weapon::WeaponComponentBase::SetVisible))
    /// show an instance by setting [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE) and
    /// [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED) and hide it by clearing both, writing
    /// only when [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE) differs from the wanted state. Adding the
    /// instance to the world sets both, and removing it clears both.
    pub m_Flags: crate::graphics_engine::model_instance::ModelInstanceFlags,
    _field_10: [u8; 96],
    /// A dirty bit for the instance's render info, set (atomically) whenever
    /// [`m_FadeAlpha`](crate::graphics_engine::model_instance::ModelInstance::m_FadeAlpha) is written.
    pub m_RenderInfoDirty: u32,
    _field_74: [u8; 11],
    /// The instance's fade alpha, 255 for fully shown. Showing without a fade sets it to 255 at
    /// once; showing with a fade sets it to zero and starts the model instance manager's fade.
    /// Hiding leaves it as it was.
    pub m_FadeAlpha: u8,
}
fn _ModelInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x80], ModelInstance>([0u8; 0x80]);
    }
    unreachable!()
}
impl ModelInstance {
    pub const AddToRender_ADDRESS: usize = 0x140319BD0;
    /// Builds the per-pass draw lists for this instance's render blocks. It fills an
    /// [`AddToRenderContext`](crate::graphics_engine::model_instance::AddToRenderContext) whose test camera is the camera manager's active camera, then
    /// dispatches to the `ForEachRb` instantiation selected by [`enable_coarse_pre_z_models`](get_enable_coarse_pre_z_models) and
    /// [`use_bounding_box_model_rb_view`](get_use_bounding_box_model_rb_view). IDA labels this address
    /// `CModelInstance::AddToPass`; it is `AddToRender`.
    pub unsafe fn AddToRender(
        &mut self,
        opaque_pass: *mut crate::graphics_engine::render_pass::RenderPass,
        transparent_pass: *mut crate::graphics_engine::render_pass::RenderPass,
        allow_pre_z: bool,
        allow_velocity_buffer: bool,
        allow_culling: bool,
        refract_pass: *mut crate::graphics_engine::render_pass::RenderPass,
        sq_distance: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                opaque_pass: *mut crate::graphics_engine::render_pass::RenderPass,
                transparent_pass: *mut crate::graphics_engine::render_pass::RenderPass,
                allow_pre_z: bool,
                allow_velocity_buffer: bool,
                allow_culling: bool,
                refract_pass: *mut crate::graphics_engine::render_pass::RenderPass,
                sq_distance: f32,
            ) = ::std::mem::transmute(Self::AddToRender_ADDRESS);
            f(
                self as *mut Self as _,
                opaque_pass,
                transparent_pass,
                allow_pre_z,
                allow_velocity_buffer,
                allow_culling,
                refract_pass,
                sq_distance,
            )
        }
    }
    pub const UpdateMatrix_ADDRESS: usize = 0x1403165A0;
    /// Sets the instance's world transform and recomputes its world bounding box from the model's
    /// bounding box (a tail call to `UpdateMatrixAndAABB`).
    pub unsafe fn UpdateMatrix(&mut self, matrix: *const crate::types::math::Matrix4) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                matrix: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::UpdateMatrix_ADDRESS);
            f(self as *mut Self as _, matrix)
        }
    }
    pub const GetModelSpaceBoundingBox_ADDRESS: usize = 0x140313F70;
    /// Writes the model's bounding box, in the model's own space, to `min` and `max`. It is the
    /// box the shared model resource carries, and the box [`UpdateMatrix`](crate::graphics_engine::model_instance::ModelInstance::UpdateMatrix)
    /// transforms into the instance's world bounding box.
    pub unsafe fn GetModelSpaceBoundingBox(
        &self,
        min: *mut crate::types::math::Vector3,
        max: *mut crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                min: *mut crate::types::math::Vector3,
                max: *mut crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::GetModelSpaceBoundingBox_ADDRESS);
            f(self as *const Self as _, min, max)
        }
    }
    pub const ForEachRb_1_1_ADDRESS: usize = 0x140316E50;
    /// `NModelSystem::ForEachRb<1, 1>`: walks `count` render blocks, frustum-tests each block's
    /// world bounding box against the context camera (`CCamera::IsBoxVisible`), skipping failing
    /// blocks for every pass, and adds survivors to their passes, routing near, large blocks to
    /// the coarse pre-Z list (see [`coarse_pre_z_models_distance_threshold`](get_coarse_pre_z_models_distance_threshold) and
    /// [`coarse_pre_z_models_area_threshold`](get_coarse_pre_z_models_area_threshold)). The instantiation used when both selector globals
    /// are set, as they are by default. Static.
    pub unsafe fn ForEachRb_1_1(
        context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
        info: *mut crate::graphics_engine::render_pass::RBIInfo,
        render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
        count: u32,
        flags: u32,
        sq_distance: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
                info: *mut crate::graphics_engine::render_pass::RBIInfo,
                render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
                count: u32,
                flags: u32,
                sq_distance: f32,
            ) = ::std::mem::transmute(Self::ForEachRb_1_1_ADDRESS);
            f(context, info, render_blocks, count, flags, sq_distance)
        }
    }
    pub const ForEachRb_0_1_ADDRESS: usize = 0x140317650;
    /// `NModelSystem::ForEachRb<0, 1>`: as [`ForEachRb_1_1`](crate::graphics_engine::model_instance::ModelInstance::ForEachRb_1_1), selected when
    /// [`use_bounding_box_model_rb_view`](get_use_bounding_box_model_rb_view) is clear and [`enable_coarse_pre_z_models`](get_enable_coarse_pre_z_models) is set. It still
    /// frustum-tests each block. Static.
    pub unsafe fn ForEachRb_0_1(
        context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
        info: *mut crate::graphics_engine::render_pass::RBIInfo,
        render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
        count: u32,
        flags: u32,
        sq_distance: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
                info: *mut crate::graphics_engine::render_pass::RBIInfo,
                render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
                count: u32,
                flags: u32,
                sq_distance: f32,
            ) = ::std::mem::transmute(Self::ForEachRb_0_1_ADDRESS);
            f(context, info, render_blocks, count, flags, sq_distance)
        }
    }
    pub const ForEachRb_1_0_ADDRESS: usize = 0x140317E50;
    /// `NModelSystem::ForEachRb<1, 0>`: the frustum-testing walk without the coarse pre-Z routing,
    /// selected when [`enable_coarse_pre_z_models`](get_enable_coarse_pre_z_models) is clear. Static.
    pub unsafe fn ForEachRb_1_0(
        context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
        info: *mut crate::graphics_engine::render_pass::RBIInfo,
        render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
        count: u32,
        flags: u32,
        sq_distance: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
                info: *mut crate::graphics_engine::render_pass::RBIInfo,
                render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
                count: u32,
                flags: u32,
                sq_distance: f32,
            ) = ::std::mem::transmute(Self::ForEachRb_1_0_ADDRESS);
            f(context, info, render_blocks, count, flags, sq_distance)
        }
    }
    pub const ForEachRb_0_0_ADDRESS: usize = 0x1403182C0;
    /// `NModelSystem::ForEachRb<0, 0>`: adds every render block to its passes with neither the
    /// per-block frustum test nor the coarse pre-Z routing. Static.
    pub unsafe fn ForEachRb_0_0(
        context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
        info: *mut crate::graphics_engine::render_pass::RBIInfo,
        render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
        count: u32,
        flags: u32,
        sq_distance: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                context: *mut crate::graphics_engine::model_instance::AddToRenderContext,
                info: *mut crate::graphics_engine::render_pass::RBIInfo,
                render_blocks: *mut *mut crate::graphics_engine::render_pass::RenderBlock,
                count: u32,
                flags: u32,
                sq_distance: f32,
            ) = ::std::mem::transmute(Self::ForEachRb_0_0_ADDRESS);
            f(context, info, render_blocks, count, flags, sq_distance)
        }
    }
}
impl std::convert::AsRef<ModelInstance> for ModelInstance {
    fn as_ref(&self) -> &ModelInstance {
        self
    }
}
impl std::convert::AsMut<ModelInstance> for ModelInstance {
    fn as_mut(&mut self) -> &mut ModelInstance {
        self
    }
}
crate::__bitflags! {
    #[doc =
    " A model instance's status bits (`NModelSystem::CModelInstance::EModelInstanceStatus`), stored"]
    #[doc =
    " in [`ModelInstance::m_Flags`](crate::graphics_engine::model_instance::ModelInstance::m_Flags). Only the bits the engine's show and hide touch are listed."]
    pub struct ModelInstanceFlags : u32 { const FLAG_ENABLED = 1usize as _; const VISIBLE
    = 16usize as _; }
}
fn _ModelInstanceFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ModelInstanceFlags>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The model instance manager (`CModelInstanceManager`). Opaque.
pub struct ModelInstanceManager {}
impl ModelInstanceManager {
    pub const TextureStreamingTask_ADDRESS: usize = 0x1404F3010;
    /// The per-frame texture-streaming distance task: runs a BFBC squared-distance process over the
    /// model instances from the occluder manager's camera position (feeding
    /// `CModelInstanceManager::BFBCCallbackDistSq`), then kicks the streaming update jobs. Static.
    /// IDA labels this address `CModelInstanceManager::DumpModelInstanceHistogram`; it is
    /// `CModelInstanceManager::TextureStreamingTask`.
    pub unsafe fn TextureStreamingTask(
        bfbc: *mut crate::graphics_engine::model_instance::BFBCContext,
    ) -> i64 {
        unsafe {
            let f: unsafe extern "system" fn(
                bfbc: *mut crate::graphics_engine::model_instance::BFBCContext,
            ) -> i64 = ::std::mem::transmute(Self::TextureStreamingTask_ADDRESS);
            f(bfbc)
        }
    }
}
impl std::convert::AsRef<ModelInstanceManager> for ModelInstanceManager {
    fn as_ref(&self) -> &ModelInstanceManager {
        self
    }
}
impl std::convert::AsMut<ModelInstanceManager> for ModelInstanceManager {
    fn as_mut(&mut self) -> &mut ModelInstanceManager {
        self
    }
}
#[repr(C, align(8))]
/// The road mesh manager (`CRoadMeshManager`). Opaque.
pub struct RoadMeshManager {}
impl RoadMeshManager {
    pub const UpdateAddToRenderer_ADDRESS: usize = 0x140533300;
    /// Adds the visible road render blocks to the road passes: each block's bounding box is tested
    /// against the camera manager's active camera (`CCamera::IsBoxVisible`), and survivors are added
    /// to the road surface pass and, when the road's visual type has shader type
    /// [`ShaderType_Simple`](crate::graphics_engine::model_instance::SplineRoadShaderType::ShaderType_Simple), to the second road pass.
    pub unsafe fn UpdateAddToRenderer(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateAddToRenderer_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RoadMeshManager> for RoadMeshManager {
    fn as_ref(&self) -> &RoadMeshManager {
        self
    }
}
impl std::convert::AsMut<RoadMeshManager> for RoadMeshManager {
    fn as_mut(&mut self) -> &mut RoadMeshManager {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The spline-road shader variant (`CRenderBlockTypeSplineRoad::EShaderType`). Each road visual type
/// (`CRoadMeshManager::SRoadType`) carries one, loaded from the road's shipped data and stored as a
/// byte, and `CRenderBlockSplineRoad::SetShaderType` copies it onto the road's render block.
pub enum SplineRoadShaderType {
    ShaderType_Simple = 0isize as _,
    ShaderType_Dirt = 1isize as _,
    ShaderType_Layered = 2isize as _,
}
fn _SplineRoadShaderType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SplineRoadShaderType>([0u8; 0x4]);
    }
    unreachable!()
}
/// The coarse pre-Z screen-area threshold (`g_CoarsePreZModelsAreaThreshold`, `0.01`): a render
/// block whose projected bounding-box area does not exceed this fraction of the screen is not a
/// coarse pre-Z candidate.
pub unsafe fn get_coarse_pre_z_models_area_threshold() -> &'static mut f32 {
    unsafe { &mut *(0x142D4B7BC as *mut f32) }
}
/// The coarse pre-Z distance threshold (`g_CoarsePreZModelsDistanceThreshold`, `0.15`): a render
/// block whose projected centre has `w` at or beyond this value times 1000 is not a coarse pre-Z
/// candidate.
pub unsafe fn get_coarse_pre_z_models_distance_threshold() -> &'static mut f32 {
    unsafe { &mut *(0x142D4B7B8 as *mut f32) }
}
/// Whether the draw-list build also routes qualifying model render blocks to the coarse pre-Z
/// (depth prepass) list (`g_EnableCoarsePreZModels`). Statically initialised to `true`.
pub unsafe fn get_enable_coarse_pre_z_models() -> &'static mut bool {
    unsafe { &mut *(0x142D4B7B4 as *mut bool) }
}
/// Whether the per-render-block draw-list build frustum-tests each render block's world bounding
/// box (`g_useBoundingBoxModelRBView`). Together with [`enable_coarse_pre_z_models`](get_enable_coarse_pre_z_models) it selects which
/// [`ForEachRb`](crate::graphics_engine::model_instance::ModelInstance::ForEachRb_1_1) instantiation
/// [`ModelInstance::AddToRender`](crate::graphics_engine::model_instance::ModelInstance::AddToRender) runs. Statically initialised to `true`.
pub unsafe fn get_use_bounding_box_model_rb_view() -> &'static mut bool {
    unsafe { &mut *(0x142D4B7B5 as *mut bool) }
}
