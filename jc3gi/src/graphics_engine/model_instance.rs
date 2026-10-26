#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// An axis-aligned box (`CAABox`), stored as its least and greatest corners.
pub struct AABox {
    pub min: crate::types::math::Vector3,
    pub max: crate::types::math::Vector3,
}
fn _AABox_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], AABox>([0u8; 0x18]);
    }
    unreachable!()
}
impl AABox {}
impl std::convert::AsRef<AABox> for AABox {
    fn as_ref(&self) -> &AABox {
        self
    }
}
impl std::convert::AsMut<AABox> for AABox {
    fn as_mut(&mut self) -> &mut AABox {
        self
    }
}
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
/// A shared model resource (`NModelSystem::CModel`): the render blocks of up to five LODs, the
/// bounding box the instances transform, and the model's id. Instances hold it by
/// [`SharedPtr`](crate::types::shared_ptr::SharedPtr); the [`ModelManager`](crate::graphics_engine::model_instance::ModelManager)'s cache holds it weakly, so a model lives as long as an
/// instance, a resource, or another owner keeps a reference. A `.rbm` loads as a single-mesh
/// model; a `.lod` loads as a LOD model whose levels are `.rbm` meshes. Only some members are
/// mapped.
pub struct Model {
    vftable: *const crate::graphics_engine::model_instance::ModelVftable,
    _field_8: [u8; 28],
    /// The hash of the model's compiled resource name.
    pub m_Id: crate::hash::HashString,
}
fn _Model_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x28], Model>([0u8; 0x28]);
    }
    unreachable!()
}
impl Model {
    pub fn vftable(
        &self,
    ) -> *const crate::graphics_engine::model_instance::ModelVftable {
        self.vftable as *const crate::graphics_engine::model_instance::ModelVftable
    }
    /// `GetShowLod`: whether the model defines a mesh for LOD `lod`, that is, whether the
    /// LOD's entry in the model's LOD list names a mesh. The dynamic draw-list build
    /// ([`ModelInstanceManager::ShowUpdate`](crate::graphics_engine::model_instance::ModelInstanceManager::ShowUpdate)) adds nothing to the passes for an instance
    /// whose current LOD this reports false for.
    pub unsafe fn GetShowLod(&self, lod: u8) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).GetShowLod).read();
            f(self as *const Self as _, lod)
        }
    }
}
impl std::convert::AsRef<Model> for Model {
    fn as_ref(&self) -> &Model {
        self
    }
}
impl std::convert::AsMut<Model> for Model {
    fn as_mut(&mut self) -> &mut Model {
        self
    }
}
#[repr(C, align(8))]
/// A collection of model instances loaded together from a location resource. Its instance pointers
/// are owned by the collection and are retired by [`Destruct`](crate::graphics_engine::model_instance::ModelCollection::Destruct); they must not outlive
/// their entry in this collection.
pub struct ModelCollection {
    pub m_Vftable: *mut *mut ::std::ffi::c_void,
    /// The number of model instances in [`m_ModelInstances`](crate::graphics_engine::model_instance::ModelCollection::m_ModelInstances).
    pub m_NumInstances: u32,
    _field_c: [u8; 4],
    /// The model instances in file order.
    pub m_ModelInstances: *mut *mut crate::graphics_engine::model_instance::ModelInstance,
    _field_18: [u8; 8],
    /// The broad-phase culling context shared by this collection's instances.
    pub m_Bfbc: *mut crate::graphics_engine::model_instance::BFBCContext,
    /// The collection's axis-aligned bounds.
    pub m_AABB: crate::graphics_engine::model_instance::AABox,
    _field_40: [u8; 8],
}
fn _ModelCollection_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x48], ModelCollection>([0u8; 0x48]);
    }
    unreachable!()
}
impl ModelCollection {
    pub const Destruct_ADDRESS: usize = 0x140508A00;
    /// Releases the instances and resources owned by the collection, and clears its instance array.
    pub unsafe fn Destruct(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Destruct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
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
impl ModelCollection {
    /// The stream boundary used when advancing between aligned data sections.
    pub const FILE_ALIGNMENT: u32 = 16;
    /// The four-byte identifier at the start of the model-collection stream.
    pub const FILE_MAGIC: u32 = 4801613;
    /// The stream format version accepted by the collection loader.
    pub const FILE_VERSION: u32 = 12;
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
/// instance info and world transform. Every creator allocates it at this size and constructs it
/// from a [`Model`](crate::graphics_engine::model_instance::Model); the [`ModelInstanceManager`](crate::graphics_engine::model_instance::ModelInstanceManager) is the usual factory, and
/// [`ModelInstanceManager::AddToDeleteQueue`](crate::graphics_engine::model_instance::ModelInstanceManager::AddToDeleteQueue) its release. An instance is drawn while it is in
/// the world (registered with a BFBC context through [`AddToWorld`](crate::graphics_engine::model_instance::ModelInstance::AddToWorld)) and
/// [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE): the model instance manager's dynamic cull finds it
/// there each frame and adds its render blocks to the passes, so the owner only keeps its matrix
/// current with [`UpdateMatrix`](crate::graphics_engine::model_instance::ModelInstance::UpdateMatrix). Only some members are mapped.
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
    _field_10: [u8; 2],
    /// The instance's entry in its BFBC context ([`m_BFBC`](crate::graphics_engine::model_instance::ModelInstance::m_BFBC)), or all bits set while
    /// the instance is not in the world. [`AddToWorld`](crate::graphics_engine::model_instance::ModelInstance::AddToWorld) sets it and
    /// [`RemoveFromWorld`](crate::graphics_engine::model_instance::ModelInstance::RemoveFromWorld) resets it.
    pub m_BFBC_id: u16,
    _field_14: [u8; 4],
    /// The BFBC context the instance is registered with while in the world, or null. The model
    /// instance manager's creators and [`ModelInstanceManager::ShowModelInstance`](crate::graphics_engine::model_instance::ModelInstanceManager::ShowModelInstance) assign the
    /// manager's [`m_BFBC`](crate::graphics_engine::model_instance::ModelInstanceManager::m_BFBC).
    pub m_BFBC: *mut crate::graphics_engine::model_instance::BFBCContext,
    /// The model the instance draws. The constructor takes a reference to it, so the model lives
    /// at least as long as the instance.
    pub m_spModel: crate::types::shared_ptr::SharedPtr<
        crate::graphics_engine::model_instance::Model,
    >,
    _field_30: [u8; 64],
    /// A dirty bit for the instance's render info, set (atomically) whenever
    /// [`m_FadeAlpha`](crate::graphics_engine::model_instance::ModelInstance::m_FadeAlpha) is written.
    pub m_RenderInfoDirty: u32,
    _field_74: [u8; 11],
    /// The instance's fade alpha, 255 for fully shown. Showing without a fade sets it to 255 at
    /// once; showing with a fade sets it to zero and starts the model instance manager's fade.
    /// Hiding leaves it as it was.
    pub m_FadeAlpha: u8,
    _field_80: [u8; 160],
}
fn _ModelInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x120], ModelInstance>([0u8; 0x120]);
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
    /// bounding box (a tail call to `UpdateMatrixAndAABB`). The transform goes into the render
    /// info's slot for the current render frame, and while the instance is in the world the new
    /// box is pushed to its BFBC entry, so the next cull tests the moved box. The engine's owners
    /// of moving instances (weapons, explosives, the C4 placement preview) call it from their
    /// render-phase `UpdateRender`.
    pub unsafe fn UpdateMatrix(&mut self, matrix: *const crate::types::math::Matrix4) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                matrix: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::UpdateMatrix_ADDRESS);
            f(self as *mut Self as _, matrix)
        }
    }
    pub const GetMatrix_ADDRESS: usize = 0x140315D60;
    /// Returns the world transform for the current update frame through `result`, and returns the
    /// same pointer. The transform comes from the instance's render information.
    pub unsafe fn GetMatrix(
        &self,
        result: *mut crate::types::math::Matrix4,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                result: *mut crate::types::math::Matrix4,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::GetMatrix_ADDRESS,
            );
            f(self as *const Self as _, result)
        }
    }
    pub const AddToWorld_ADDRESS: usize = 0x140319B60;
    /// Registers the instance with its BFBC context ([`m_BFBC`](crate::graphics_engine::model_instance::ModelInstance::m_BFBC), which must be
    /// set) unless it already is: recomputes its world bounding box from its current transform,
    /// adds the box to the context, stores the entry in [`m_BFBC_id`](crate::graphics_engine::model_instance::ModelInstance::m_BFBC_id), applies
    /// [`NO_SSSIZECULLING`](crate::graphics_engine::model_instance::ModelInstanceFlags::NO_SSSIZECULLING), and sets
    /// [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE) and
    /// [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED). The BFBC context is not locked:
    /// registration is main-thread work, and [`ModelInstanceManager::AddToDeleteQueue`](crate::graphics_engine::model_instance::ModelInstanceManager::AddToDeleteQueue) defers a
    /// removal requested from any other thread to the main thread.
    pub unsafe fn AddToWorld(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::AddToWorld_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RemoveFromWorld_ADDRESS: usize = 0x140313E70;
    /// Removes the instance from its BFBC context when it is in the world, resetting
    /// [`m_BFBC_id`](crate::graphics_engine::model_instance::ModelInstance::m_BFBC_id), and clears [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE) and
    /// [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED) either way. It keeps
    /// [`m_BFBC`](crate::graphics_engine::model_instance::ModelInstance::m_BFBC).
    pub unsafe fn RemoveFromWorld(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RemoveFromWorld_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SetBFBC_ADDRESS: usize = 0x140311620;
    /// Stores `bfbc` in [`m_BFBC`](crate::graphics_engine::model_instance::ModelInstance::m_BFBC) and nothing else; an instance that is in the
    /// world must be removed first.
    pub unsafe fn SetBFBC(
        &mut self,
        bfbc: *mut crate::graphics_engine::model_instance::BFBCContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                bfbc: *mut crate::graphics_engine::model_instance::BFBCContext,
            ) = ::std::mem::transmute(Self::SetBFBC_ADDRESS);
            f(self as *mut Self as _, bfbc)
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
    " in [`ModelInstance::m_Flags`](crate::graphics_engine::model_instance::ModelInstance::m_Flags). Only some bits are listed. A newly constructed instance has"]
    #[doc =
    " [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED), [`FLAG_CLIPONDISTANCE`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_CLIPONDISTANCE),"]
    #[doc =
    " [`FLAG_UPDATELOD`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_UPDATELOD), [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE),"]
    #[doc =
    " [`DYNAMIC`](crate::graphics_engine::model_instance::ModelInstanceFlags::DYNAMIC), and [`SHADOW`](crate::graphics_engine::model_instance::ModelInstanceFlags::SHADOW) set, among debug bits."]
    pub struct ModelInstanceFlags : u32 { const FLAG_ENABLED = 1usize as _; const
    FLAG_CLIPONDISTANCE = 2usize as _; const FLAG_UPDATELOD = 4usize as _; const
    FLAG_DELETED = 8usize as _; const VISIBLE = 16usize as _; const DYNAMIC = 32usize as
    _; const SHADOW = 128usize as _; const REFLECTION = 256usize as _; const
    FADE_OUT_ON_DELETE = 2048usize as _; const NO_SSSIZECULLING = 16384usize as _; }
}
fn _ModelInstanceFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ModelInstanceFlags>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The model instance manager (`CModelInstanceManager`, a `Base::CSingle` singleton): the factory
/// and release point for game-object model instances, the owner of the dynamic BFBC context their
/// instances are culled in, and the per-frame cull and draw-list build over them, with its fade
/// queue. Only some members are mapped.
///
/// An instance it creates with `add_to_world` set, or shows with
/// [`ShowModelInstance`](crate::graphics_engine::model_instance::ModelInstanceManager::ShowModelInstance), is in [`m_BFBC`](crate::graphics_engine::model_instance::ModelInstanceManager::m_BFBC) and is drawn
/// each frame while [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE), from the transform its owner last
/// pushed with [`ModelInstance::UpdateMatrix`](crate::graphics_engine::model_instance::ModelInstance::UpdateMatrix); the owner never adds it to a pass itself.
pub struct ModelInstanceManager {
    _field_0: [u8; 544],
    /// The dynamic BFBC context: the one [`ShowModelInstance`](crate::graphics_engine::model_instance::ModelInstanceManager::ShowModelInstance) and the
    /// creators register instances with, and whose per-frame cull hands each surviving
    /// [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE) instance to [`ShowUpdate`](crate::graphics_engine::model_instance::ModelInstanceManager::ShowUpdate).
    pub m_BFBC: *mut crate::graphics_engine::model_instance::BFBCContext,
}
fn _ModelInstanceManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x228], ModelInstanceManager>([0u8; 0x228]);
    }
    unreachable!()
}
impl ModelInstanceManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418086168usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl ModelInstanceManager {
    pub const CreateInstanceFromName_ADDRESS: usize = 0x1404E1790;
    /// `CreateInstance(const char*, ...)`: creates an instance of the model at `name` through
    /// [`ModelManager::GetModelInstanceFromName`](crate::graphics_engine::model_instance::ModelManager::GetModelInstanceFromName) with `load_flags`, so a name that fails to load
    /// yields an instance of the fail model rather than null. It then initialises it at
    /// `world_matrix` ([`InitModelInstance`](crate::graphics_engine::model_instance::ModelInstanceManager::InitModelInstance)) and, with `add_to_world`,
    /// assigns [`m_BFBC`](crate::graphics_engine::model_instance::ModelInstanceManager::m_BFBC) and adds it to the world
    /// ([`ModelInstance::AddToWorld`](crate::graphics_engine::model_instance::ModelInstance::AddToWorld)). `name` may be an intermediate (`.model_xml`) or a compiled
    /// (`.lod`, `.rbm`) path. Returns null only when no instance could be allocated.
    pub unsafe fn CreateInstanceFromName(
        &mut self,
        name: *const u8,
        world_matrix: *const crate::types::math::Matrix4,
        casts_shadow: bool,
        reflect: bool,
        load_flags: crate::graphics_engine::model_instance::ModelLoadFlags,
        add_to_world: bool,
    ) -> *mut crate::graphics_engine::model_instance::ModelInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                name: *const u8,
                world_matrix: *const crate::types::math::Matrix4,
                casts_shadow: bool,
                reflect: bool,
                load_flags: crate::graphics_engine::model_instance::ModelLoadFlags,
                add_to_world: bool,
            ) -> *mut crate::graphics_engine::model_instance::ModelInstance = ::std::mem::transmute(
                Self::CreateInstanceFromName_ADDRESS,
            );
            f(
                self as *mut Self as _,
                name,
                world_matrix,
                casts_shadow,
                reflect,
                load_flags,
                add_to_world,
            )
        }
    }
    pub const CreateInstanceFromHash_ADDRESS: usize = 0x1404E16E0;
    /// `CreateInstance(unsigned int, ...)`: as
    /// [`CreateInstanceFromName`](crate::graphics_engine::model_instance::ModelInstanceManager::CreateInstanceFromName) for the model whose compiled-name
    /// hash is `name_hash`, through [`ModelManager::GetModelInstanceFromHash`](crate::graphics_engine::model_instance::ModelManager::GetModelInstanceFromHash). It loads nothing:
    /// it returns null unless the model is in the model manager's cache.
    pub unsafe fn CreateInstanceFromHash(
        &mut self,
        name_hash: u32,
        world_matrix: *const crate::types::math::Matrix4,
        casts_shadow: bool,
        reflect: bool,
        add_to_world: bool,
    ) -> *mut crate::graphics_engine::model_instance::ModelInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                name_hash: u32,
                world_matrix: *const crate::types::math::Matrix4,
                casts_shadow: bool,
                reflect: bool,
                add_to_world: bool,
            ) -> *mut crate::graphics_engine::model_instance::ModelInstance = ::std::mem::transmute(
                Self::CreateInstanceFromHash_ADDRESS,
            );
            f(
                self as *mut Self as _,
                name_hash,
                world_matrix,
                casts_shadow,
                reflect,
                add_to_world,
            )
        }
    }
    pub const CreateInstanceFromModel_ADDRESS: usize = 0x1404E1620;
    /// `CreateInstance(const boost::shared_ptr<CModel>&, ...)`: allocates an instance of `model`
    /// directly, then initialises it and optionally adds it to the world as
    /// [`CreateInstanceFromName`](crate::graphics_engine::model_instance::ModelInstanceManager::CreateInstanceFromName) does. IDA labels this address
    /// `CModelManager::GetModelInstance(const char*, int)`; it is this overload.
    pub unsafe fn CreateInstanceFromModel(
        &mut self,
        model: *const crate::types::shared_ptr::SharedPtr<
            crate::graphics_engine::model_instance::Model,
        >,
        world_matrix: *const crate::types::math::Matrix4,
        casts_shadow: bool,
        reflect: bool,
        add_to_world: bool,
    ) -> *mut crate::graphics_engine::model_instance::ModelInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                model: *const crate::types::shared_ptr::SharedPtr<
                    crate::graphics_engine::model_instance::Model,
                >,
                world_matrix: *const crate::types::math::Matrix4,
                casts_shadow: bool,
                reflect: bool,
                add_to_world: bool,
            ) -> *mut crate::graphics_engine::model_instance::ModelInstance = ::std::mem::transmute(
                Self::CreateInstanceFromModel_ADDRESS,
            );
            f(
                self as *mut Self as _,
                model,
                world_matrix,
                casts_shadow,
                reflect,
                add_to_world,
            )
        }
    }
    pub const InitModelInstance_ADDRESS: usize = 0x1404CFAA0;
    /// The creators' common initialisation: sets the instance's transform (`InitMatrix`), its
    /// sand or snow tint from the landscape at the transform's position, and
    /// [`SHADOW`](crate::graphics_engine::model_instance::ModelInstanceFlags::SHADOW) and [`REFLECTION`](crate::graphics_engine::model_instance::ModelInstanceFlags::REFLECTION)
    /// from `casts_shadow` and `reflect`. Does nothing for a null instance. Static.
    pub unsafe fn InitModelInstance(
        instance: *mut crate::graphics_engine::model_instance::ModelInstance,
        world_matrix: *const crate::types::math::Matrix4,
        casts_shadow: bool,
        reflect: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                instance: *mut crate::graphics_engine::model_instance::ModelInstance,
                world_matrix: *const crate::types::math::Matrix4,
                casts_shadow: bool,
                reflect: bool,
            ) = ::std::mem::transmute(Self::InitModelInstance_ADDRESS);
            f(instance, world_matrix, casts_shadow, reflect)
        }
    }
    pub const CreateStaticInstance_ADDRESS: usize = 0x1404DB450;
    /// Creates an instance of `model` for the static world: as
    /// [`CreateInstanceFromModel`](crate::graphics_engine::model_instance::ModelInstanceManager::CreateInstanceFromModel) without adding it to the world,
    /// and with [`DYNAMIC`](crate::graphics_engine::model_instance::ModelInstanceFlags::DYNAMIC) cleared. Static.
    pub unsafe fn CreateStaticInstance(
        model: *const crate::types::shared_ptr::SharedPtr<
            crate::graphics_engine::model_instance::Model,
        >,
        world_matrix: *const crate::types::math::Matrix4,
        casts_shadow: bool,
        reflect: bool,
    ) -> *mut crate::graphics_engine::model_instance::ModelInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                model: *const crate::types::shared_ptr::SharedPtr<
                    crate::graphics_engine::model_instance::Model,
                >,
                world_matrix: *const crate::types::math::Matrix4,
                casts_shadow: bool,
                reflect: bool,
            ) -> *mut crate::graphics_engine::model_instance::ModelInstance = ::std::mem::transmute(
                Self::CreateStaticInstance_ADDRESS,
            );
            f(model, world_matrix, casts_shadow, reflect)
        }
    }
    pub const ShowModelInstance_ADDRESS: usize = 0x1404DB4E0;
    /// Shows an instance: sets [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE) and
    /// [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED), assigns [`m_BFBC`](crate::graphics_engine::model_instance::ModelInstanceManager::m_BFBC) when
    /// the instance has no context, and adds it to the world ([`ModelInstance::AddToWorld`](crate::graphics_engine::model_instance::ModelInstance::AddToWorld)),
    /// which does nothing for an instance already there.
    pub unsafe fn ShowModelInstance(
        &mut self,
        instance: *mut crate::graphics_engine::model_instance::ModelInstance,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::graphics_engine::model_instance::ModelInstance,
            ) = ::std::mem::transmute(Self::ShowModelInstance_ADDRESS);
            f(self as *mut Self as _, instance)
        }
    }
    pub const HideModelInstance_ADDRESS: usize = 0x1404DB510;
    /// Hides an instance: clears [`VISIBLE`](crate::graphics_engine::model_instance::ModelInstanceFlags::VISIBLE) and
    /// [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED) and removes it from the world
    /// ([`ModelInstance::RemoveFromWorld`](crate::graphics_engine::model_instance::ModelInstance::RemoveFromWorld)). The instance keeps its context, so
    /// [`ShowModelInstance`](crate::graphics_engine::model_instance::ModelInstanceManager::ShowModelInstance) brings it back.
    pub unsafe fn HideModelInstance(
        &mut self,
        instance: *mut crate::graphics_engine::model_instance::ModelInstance,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::graphics_engine::model_instance::ModelInstance,
            ) = ::std::mem::transmute(Self::HideModelInstance_ADDRESS);
            f(self as *mut Self as _, instance)
        }
    }
    pub const AddToDeleteQueue_ADDRESS: usize = 0x140505F40;
    /// Releases an instance, the counterpart of the creators. Does nothing for null. On the main
    /// thread, without `always_defer`, it destroys the instance at once
    /// ([`DeleteInstance`](crate::graphics_engine::model_instance::ModelInstanceManager::DeleteInstance)); from another thread, or with `always_defer`, it
    /// queues an immediate fade whose completion destroys it on the main thread. An instance with
    /// [`FADE_OUT_ON_DELETE`](crate::graphics_engine::model_instance::ModelInstanceFlags::FADE_OUT_ON_DELETE) and
    /// [`FLAG_CLIPONDISTANCE`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_CLIPONDISTANCE), while the manager's
    /// fading is on, is moved into [`m_BFBC`](crate::graphics_engine::model_instance::ModelInstanceManager::m_BFBC) and faded out first. The caller must
    /// not touch the instance afterwards.
    pub unsafe fn AddToDeleteQueue(
        &mut self,
        instance: *mut crate::graphics_engine::model_instance::ModelInstance,
        always_defer: bool,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::graphics_engine::model_instance::ModelInstance,
                always_defer: bool,
            ) -> bool = ::std::mem::transmute(Self::AddToDeleteQueue_ADDRESS);
            f(self as *mut Self as _, instance, always_defer)
        }
    }
    pub const DeleteInstance_ADDRESS: usize = 0x1404F5930;
    /// Destroys an instance: clears [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED), removes
    /// it from the world when it is there, clears its context, drops any fade, and hands it to
    /// [`ModelManager::RemoveModelInstance`](crate::graphics_engine::model_instance::ModelManager::RemoveModelInstance), which frees it later.
    pub unsafe fn DeleteInstance(
        &mut self,
        instance: *mut crate::graphics_engine::model_instance::ModelInstance,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::graphics_engine::model_instance::ModelInstance,
            ) = ::std::mem::transmute(Self::DeleteInstance_ADDRESS);
            f(self as *mut Self as _, instance)
        }
    }
    pub const ShowUpdate_ADDRESS: usize = 0x1404E1840;
    /// The dynamic draw-list build for one culled instance (the dynamic cull's per-instance step).
    /// It sets [`FLAG_ENABLED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_ENABLED), then returns false, adding
    /// nothing, when the instance is clipped by distance
    /// ([`FLAG_CLIPONDISTANCE`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_CLIPONDISTANCE)) or its model reports
    /// no mesh for the chosen LOD ([`Model::GetShowLod`](crate::graphics_engine::model_instance::Model::GetShowLod)). Otherwise it picks the LOD
    /// ([`FLAG_UPDATELOD`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_UPDATELOD)), updates the instance's constant
    /// buffer, and adds it to `opaque_pass` and `transparent_pass` (and the effect and
    /// reflection passes its flags select) through [`ModelInstance::AddToRender`](crate::graphics_engine::model_instance::ModelInstance::AddToRender).
    pub unsafe fn ShowUpdate(
        &mut self,
        instance: *mut crate::graphics_engine::model_instance::ModelInstance,
        opaque_pass: *mut crate::graphics_engine::render_pass::RenderPass,
        transparent_pass: *mut crate::graphics_engine::render_pass::RenderPass,
        camera_position: *const crate::types::math::Vector3,
        sq_distance: f32,
        use_velocity: bool,
        global_lod_factor: f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::graphics_engine::model_instance::ModelInstance,
                opaque_pass: *mut crate::graphics_engine::render_pass::RenderPass,
                transparent_pass: *mut crate::graphics_engine::render_pass::RenderPass,
                camera_position: *const crate::types::math::Vector3,
                sq_distance: f32,
                use_velocity: bool,
                global_lod_factor: f32,
            ) -> bool = ::std::mem::transmute(Self::ShowUpdate_ADDRESS);
            f(
                self as *mut Self as _,
                instance,
                opaque_pass,
                transparent_pass,
                camera_position,
                sq_distance,
                use_velocity,
                global_lod_factor,
            )
        }
    }
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
crate::__bitflags! {
    #[doc =
    " The load flags [`ModelManager::GetModel`](crate::graphics_engine::model_instance::ModelManager::GetModel) takes (`FLAG_LOAD`, `FLAG_HOLD`)."]
    pub struct ModelLoadFlags : u32 { const FLAG_LOAD = 1usize as _; const FLAG_HOLD =
    2usize as _; }
}
fn _ModelLoadFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ModelLoadFlags>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The model manager (`NModelSystem::CModelManager`, a `Base::CSingle` singleton): the loader and
/// cache of [`Model`](crate::graphics_engine::model_instance::Model)s, keyed by the hash of their compiled names, and the deferred destruction
/// of [`ModelInstance`](crate::graphics_engine::model_instance::ModelInstance)s. The cache holds models weakly. The model resource factory
/// (`CModel::CreateResource`) builds each model of a loading resource bundle from the bundle's
/// in-memory data through [`GetModel`](crate::graphics_engine::model_instance::ModelManager::GetModel), which caches it, and the resource keeps a
/// reference; so while a bundle is loaded its models are found in the cache by name or hash. Only
/// some members are mapped.
pub struct ModelManager {
    _field_0: [u8; 304],
    /// The fail model (a pink box): the model
    /// [`GetModelInstanceFromName`](crate::graphics_engine::model_instance::ModelManager::GetModelInstanceFromName) instances when a name does
    /// not load, and the mesh a LOD model's top LOD falls back to when its mesh is missing.
    pub m_FailModel: crate::types::shared_ptr::SharedPtr<
        crate::graphics_engine::model_instance::Model,
    >,
}
fn _ModelManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x140], ModelManager>([0u8; 0x140]);
    }
    unreachable!()
}
impl ModelManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417914048usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl ModelManager {
    pub const GetModel_ADDRESS: usize = 0x140321D70;
    /// Returns, through `result`, the model at `name`. The name is first transformed to its
    /// compiled form (`TransformToCompiledName`, so a `.model_xml` becomes a `.lod`) and hashed,
    /// and a cached model under that hash is returned as is. Otherwise a `.rbm` is read from
    /// `memory` (`size` bytes) or, without it, synchronously from the virtual file system, built
    /// (`CMeshModel::ReadLod`), and cached; a `.lod` is built from the LOD list in `memory`, or
    /// read from the file system under [`FLAG_LOAD`](crate::graphics_engine::model_instance::ModelLoadFlags::FLAG_LOAD), its LOD meshes
    /// loaded or looked up per `flags`, and cached. Any other extension, an empty name, or a
    /// failed read yields null.
    pub unsafe fn GetModel(
        &mut self,
        result: *mut crate::types::shared_ptr::SharedPtr<
            crate::graphics_engine::model_instance::Model,
        >,
        name: *const u8,
        memory: *mut u8,
        size: u32,
        flags: crate::graphics_engine::model_instance::ModelLoadFlags,
    ) -> *mut crate::types::shared_ptr::SharedPtr<
        crate::graphics_engine::model_instance::Model,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                result: *mut crate::types::shared_ptr::SharedPtr<
                    crate::graphics_engine::model_instance::Model,
                >,
                name: *const u8,
                memory: *mut u8,
                size: u32,
                flags: crate::graphics_engine::model_instance::ModelLoadFlags,
            ) -> *mut crate::types::shared_ptr::SharedPtr<
                crate::graphics_engine::model_instance::Model,
            > = ::std::mem::transmute(Self::GetModel_ADDRESS);
            f(self as *mut Self as _, result, name, memory, size, flags)
        }
    }
    pub const GetCachedModel_ADDRESS: usize = 0x14031A100;
    /// Returns, through `result`, the cached model whose compiled-name hash is `name_hash`, or
    /// null when the cache has none or its last owner has released it. Loads nothing. The cache
    /// is guarded for concurrent readers.
    pub unsafe fn GetCachedModel(
        &self,
        result: *mut crate::types::shared_ptr::SharedPtr<
            crate::graphics_engine::model_instance::Model,
        >,
        name_hash: u32,
    ) -> *mut crate::types::shared_ptr::SharedPtr<
        crate::graphics_engine::model_instance::Model,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                result: *mut crate::types::shared_ptr::SharedPtr<
                    crate::graphics_engine::model_instance::Model,
                >,
                name_hash: u32,
            ) -> *mut crate::types::shared_ptr::SharedPtr<
                crate::graphics_engine::model_instance::Model,
            > = ::std::mem::transmute(Self::GetCachedModel_ADDRESS);
            f(self as *const Self as _, result, name_hash)
        }
    }
    pub const GetModelInstanceFromName_ADDRESS: usize = 0x140322F70;
    /// `GetModelInstance(const char*, int)`: gets the model at `name` with
    /// [`GetModel`](crate::graphics_engine::model_instance::ModelManager::GetModel) and `flags` and allocates an instance of it, or of
    /// [`m_FailModel`](crate::graphics_engine::model_instance::ModelManager::m_FailModel) when the model does not load. The instance is not
    /// initialised or added to the world. IDA labels this address
    /// `CModelInstanceManager::CreateInstance(const boost::shared_ptr<CModel>&, ...)`; it is this
    /// function.
    pub unsafe fn GetModelInstanceFromName(
        &mut self,
        name: *const u8,
        flags: crate::graphics_engine::model_instance::ModelLoadFlags,
    ) -> *mut crate::graphics_engine::model_instance::ModelInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                name: *const u8,
                flags: crate::graphics_engine::model_instance::ModelLoadFlags,
            ) -> *mut crate::graphics_engine::model_instance::ModelInstance = ::std::mem::transmute(
                Self::GetModelInstanceFromName_ADDRESS,
            );
            f(self as *mut Self as _, name, flags)
        }
    }
    pub const GetModelInstanceFromHash_ADDRESS: usize = 0x14031FFC0;
    /// `GetModelInstance(unsigned int)`: allocates an instance of the cached model whose
    /// compiled-name hash is `name_hash` ([`GetCachedModel`](crate::graphics_engine::model_instance::ModelManager::GetCachedModel)), or returns
    /// null when it is not cached.
    pub unsafe fn GetModelInstanceFromHash(
        &mut self,
        name_hash: u32,
    ) -> *mut crate::graphics_engine::model_instance::ModelInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                name_hash: u32,
            ) -> *mut crate::graphics_engine::model_instance::ModelInstance = ::std::mem::transmute(
                Self::GetModelInstanceFromHash_ADDRESS,
            );
            f(self as *mut Self as _, name_hash)
        }
    }
    pub const RemoveModelInstance_ADDRESS: usize = 0x14031E450;
    /// Queues an instance for destruction: sets
    /// [`FLAG_DELETED`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_DELETED), calls the instance-deletion callback
    /// when one is set, and appends the instance, under a lock, to the current half of the
    /// double-buffered removal list, whose instances are freed on a later frame. The instance
    /// must be out of the world.
    pub unsafe fn RemoveModelInstance(
        &mut self,
        instance: *mut crate::graphics_engine::model_instance::ModelInstance,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                instance: *mut crate::graphics_engine::model_instance::ModelInstance,
            ) = ::std::mem::transmute(Self::RemoveModelInstance_ADDRESS);
            f(self as *mut Self as _, instance)
        }
    }
}
impl std::convert::AsRef<ModelManager> for ModelManager {
    fn as_ref(&self) -> &ModelManager {
        self
    }
}
impl std::convert::AsMut<ModelManager> for ModelManager {
    fn as_mut(&mut self) -> &mut ModelManager {
        self
    }
}
#[repr(C, align(8))]
pub struct ModelVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_1: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_2: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_3: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_4: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_5: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::model_instance::Model,
    ),
    /// `GetShowLod`: whether the model defines a mesh for LOD `lod`, that is, whether the
    /// LOD's entry in the model's LOD list names a mesh. The dynamic draw-list build
    /// ([`ModelInstanceManager::ShowUpdate`](crate::graphics_engine::model_instance::ModelInstanceManager::ShowUpdate)) adds nothing to the passes for an instance
    /// whose current LOD this reports false for.
    pub GetShowLod: unsafe extern "system" fn(
        this: *const crate::graphics_engine::model_instance::Model,
        lod: u8,
    ) -> bool,
}
fn _ModelVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x80], ModelVftable>([0u8; 0x80]);
    }
    unreachable!()
}
impl ModelVftable {}
impl std::convert::AsRef<ModelVftable> for ModelVftable {
    fn as_ref(&self) -> &ModelVftable {
        self
    }
}
impl std::convert::AsMut<ModelVftable> for ModelVftable {
    fn as_mut(&mut self) -> &mut ModelVftable {
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
