#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The scene light manager: gathers the frame's visible lights and prepares the per-frame lighting
/// render state. Only the fields below are verified.
pub struct LightManager {
    _field_0: [u8; 12],
    /// The global light's (sun or moon) diffuse colour, the `Diffuse` of the embedded global light's
    /// `SLight`. `CRenderPass::SetupRenderContext` copies it into
    /// [`RenderContext::m_GlobalLightDiffuse`](crate::graphics_engine::graphics_engine::RenderContext::m_GlobalLightDiffuse).
    pub m_GlobalLightDiffuse: crate::types::math::Vector3,
    _field_18: [u8; 12],
    /// The direction toward the global light, the `Direction` of the embedded global light's
    /// `SLight`. `CRenderPass::SetupRenderContext` copies it into
    /// [`RenderContext::m_GlobalLightVector`](crate::graphics_engine::graphics_engine::RenderContext::m_GlobalLightVector),
    /// and `CRenderEngine::SetGlobalShaderConstants` stages it (raw and elevation-clamped) as the
    /// shaders' sun direction.
    pub m_GlobalLightDirection: crate::types::math::Vector3,
    _field_30: [u8; 69792],
    /// The 24-entry per-hour night-light reference table (`SetNightLightReferences`).
    pub m_NightLightReferences: *const f32,
    /// The time-of-day light modulator, returned by `GetTimeOfDayLightModulator`.
    /// `CRenderPass::SetupRenderContext` copies it into
    /// [`RenderContext::m_TimeOfDayEmissiveScale`](crate::graphics_engine::graphics_engine::RenderContext::m_TimeOfDayEmissiveScale).
    pub m_TimeOfDayLightModulator: f32,
    _field_110dc: [u8; 4],
    /// The environment-light intensity scale (`SetEnvLightIntensityScaleFactor`).
    pub m_EnvLightIntensityScaleFactor: f32,
    /// The VFX-light intensity scale (`SetVFXLightIntensityScaleFactor` /
    /// `GetVFXLightIntensityScaleFactor`).
    pub m_VFXLightIntensityScaleFactor: f32,
    _field_110e8: [u8; 1759560],
    /// The clustered-lighting frame constant buffer, copied into
    /// [`RenderContext::m_LightBuffer`](crate::graphics_engine::graphics_engine::RenderContext::m_LightBuffer).
    pub m_LightBuffer: *mut ::std::ffi::c_void,
    /// The per-frame light-grid lookup ring, indexed by the render frame modulo the ring length.
    pub m_LightLookup: [*mut ::std::ffi::c_void; 3],
    /// The light-grid lookup the clustered light assignment writes; used instead of
    /// [`m_LightLookup`](crate::graphics_engine::light_manager::LightManager::m_LightLookup) while
    /// [`m_UseClusteredLightAssignment`](crate::graphics_engine::light_manager::LightManager::m_UseClusteredLightAssignment) is set.
    pub m_LightAssignmentLookup: *mut ::std::ffi::c_void,
    _field_1bea58: [u8; 8],
    /// The fill-phase render setup of the clustered light assignment.
    pub m_LightAssignmentFillRenderSetup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
    /// The first light-assignment phase's output texture.
    pub m_LightAssignmentTexture: *mut crate::graphics_engine::texture::Texture,
    _field_1bea70: [u8; 8],
    /// The first light-assignment phase's render setup.
    pub m_LightAssignmentRenderSetup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
    /// Whether the clustered light-assignment path is active. `UpdateRenderContext` mirrors it into
    /// [`RenderContextFlags::m_UseClusteredLightAssignment`](crate::graphics_engine::graphics_engine::RenderContextFlags::m_UseClusteredLightAssignment).
    pub m_UseClusteredLightAssignment: bool,
    _field_1bea81: [u8; 7],
}
fn _LightManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1BEA88], LightManager>([0u8; 0x1BEA88]);
    }
    unreachable!()
}
impl LightManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417799280usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl LightManager {
    pub const UpdateRenderContext_ADDRESS: usize = 0x1400A5090;
    /// Copies the light manager's per-frame lighting resources into `ctx`: the frame constant
    /// buffer, the light-grid lookup and light-assignment targets, the frame's point and spot light
    /// records and counts, the night-light reference, the light falloff texture, and the lighting
    /// samplers.
    pub unsafe fn UpdateRenderContext(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::UpdateRenderContext_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const SetupForwardLightingResources_ADDRESS: usize = 0x1400B98B0;
    /// Binds the forward-lighting resources
    /// ([`RenderBlockTypeBase::SetupForwardLightingResources`](crate::graphics_engine::render_engine::RenderBlockTypeBase::SetupForwardLightingResources)),
    /// then the global-illumination textures when GI is active.
    pub unsafe fn SetupForwardLightingResources(
        &self,
        ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::SetupForwardLightingResources_ADDRESS);
            f(self as *const Self as _, ctx)
        }
    }
    pub const SetupLightingTextures_ADDRESS: usize = 0x1400B9910;
    /// Binds the lighting textures
    /// ([`RenderBlockTypeBase::SetupLightingTextures`](crate::graphics_engine::render_engine::RenderBlockTypeBase::SetupLightingTextures)),
    /// then the global-illumination textures when GI is active.
    pub unsafe fn SetupLightingTextures(
        &self,
        ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::SetupLightingTextures_ADDRESS);
            f(self as *const Self as _, ctx)
        }
    }
    pub const SetupLightingData_ADDRESS: usize = 0x1400C60D0;
    /// Fills the clustered-lighting frame constant buffer (`LightingFrameConsts`, the buffer
    /// [`RenderContext::m_LightBuffer`](crate::graphics_engine::graphics_engine::RenderContext::m_LightBuffer)
    /// carries) for `ctx` from the frame's point and spot light records. Each point light takes
    /// three rows of `PointLights` and each spot light four rows of `SpotLights`; the first row's
    /// `.xyz` is the light's world position minus `ctx`'s
    /// [`m_RenderCameraPosition`](crate::graphics_engine::graphics_engine::RenderContext::m_RenderCameraPosition),
    /// so every light position in the buffer is relative to the render camera. Each spot light that
    /// casts a projected texture also gets a `Projections` matrix: the offset world-view-projection
    /// of the light's frustum built with a world transform translated to the same render camera
    /// position, so it maps a camera-relative position rather than a world position. The pixel
    /// shaders that read the buffer subtract a camera-relative pixel position from the light
    /// positions and apply the projections to that same position.
    pub unsafe fn SetupLightingData(
        &mut self,
        ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
        point_lights: *const ::std::ffi::c_void,
        point_light_count: u32,
        spot_lights: *const ::std::ffi::c_void,
        spot_light_count: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
                point_lights: *const ::std::ffi::c_void,
                point_light_count: u32,
                spot_lights: *const ::std::ffi::c_void,
                spot_light_count: u32,
            ) = ::std::mem::transmute(Self::SetupLightingData_ADDRESS);
            f(
                self as *mut Self as _,
                ctx,
                point_lights,
                point_light_count,
                spot_lights,
                spot_light_count,
            )
        }
    }
    pub const ApplyDynamicLights_ADDRESS: usize = 0x1400D0270;
    /// Rebuilds the lighting frame constant buffer for `ctx` through
    /// [`SetupLightingData`](crate::graphics_engine::light_manager::LightManager::SetupLightingData), from the light records of the current
    /// render-context frame's slot in the light manager's three-deep ring, then refreshes the light
    /// lists. Called by
    /// [`RenderEngine::SetGlobalShaderConstants`](crate::graphics_engine::render_engine::RenderEngine::SetGlobalShaderConstants)
    /// before it copies the buffer into the context, so the buffer is rebuilt from the render camera
    /// position each time the global constants are staged.
    pub unsafe fn ApplyDynamicLights(
        &mut self,
        ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *const crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::ApplyDynamicLights_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const AddFarLightsToPass_ADDRESS: usize = 0x1400B9C10;
    /// Adds the far lights to `render_pass`, culled against the active camera.
    pub unsafe fn AddFarLightsToPass(
        &mut self,
        render_pass: *mut crate::graphics_engine::render_pass::RenderPass,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                render_pass: *mut crate::graphics_engine::render_pass::RenderPass,
            ) = ::std::mem::transmute(Self::AddFarLightsToPass_ADDRESS);
            f(self as *mut Self as _, render_pass)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1400CFEE0;
    /// The per-frame render-side update of the light manager; `sun_shadow_cascades` is the number
    /// of shadow-atlas slices the sun cascades occupy.
    pub unsafe fn UpdateRender(
        &mut self,
        dt: f32,
        unused: f32,
        sun_shadow_cascades: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                unused: f32,
                sun_shadow_cascades: u32,
            ) = ::std::mem::transmute(Self::UpdateRender_ADDRESS);
            f(self as *mut Self as _, dt, unused, sun_shadow_cascades)
        }
    }
    pub const InitDeferredLighting_ADDRESS: usize = 0x1400E9C10;
    /// Creates the deferred-lighting render block and pass.
    pub unsafe fn InitDeferredLighting(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::InitDeferredLighting_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const CopyLightsToUpdate_ADDRESS: usize = 0x1400C6860;
    /// Copies the frame's visible point and spot lights into the parity-buffered update lists and
    /// configures the volumetric-lighting passes. Reads
    /// [`enable_low_res_spot_light_volume`](get_enable_low_res_spot_light_volume) (and whether any volumetric spot lights are present this
    /// frame) to select the reduced- or full-resolution spot-light-cone render setup, toggle the
    /// low-res upsampling pass, and set the spot-light-cone block type's low-res flag. `reserved_slots`
    /// is the count of shadow-atlas slices reserved for other casters; `dt` drives the per-light fade.
    pub unsafe fn CopyLightsToUpdate(&mut self, reserved_slots: u32, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                reserved_slots: u32,
                dt: f32,
            ) = ::std::mem::transmute(Self::CopyLightsToUpdate_ADDRESS);
            f(self as *mut Self as _, reserved_slots, dt)
        }
    }
}
impl LightManager {
    /// The edge length in pixels of the clustered light grid's screen tiles:
    /// `CRenderBlockDeferredLighting::DrawClustered` divides the display size by it to get the tile
    /// counts, and the light-grid lookup shader addresses tiles by shifting `SV_Position` right by
    /// its base-2 logarithm.
    pub const LIGHT_GRID_TILE_SIZE: u32 = 64;
}
impl std::convert::AsRef<LightManager> for LightManager {
    fn as_ref(&self) -> &LightManager {
        self
    }
}
impl std::convert::AsMut<LightManager> for LightManager {
    fn as_mut(&mut self) -> &mut LightManager {
        self
    }
}
/// Whether the engine renders volumetric spot-light cones at reduced resolution (the engine's
/// `g_EnableLowResSpotLightVolume`). When set — the default — the per-frame light gather routes the
/// spot-light-cone render block through a quarter-resolution render setup, enables the low-res
/// upsampling pass, and sets the cone block type's low-res flag; when clear, the cones render at full
/// resolution into the main render setup. It is also treated as clear for any frame that has no
/// visible volumetric spot lights. Read once per frame by
/// [`LightManager::CopyLightsToUpdate`](crate::graphics_engine::light_manager::LightManager::CopyLightsToUpdate).
pub unsafe fn get_enable_low_res_spot_light_volume() -> &'static mut bool {
    unsafe { &mut *(0x142D3A6F2 as *mut bool) }
}
