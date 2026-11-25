#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The global-illumination render pass (`RP_GLOBAL_ILLUMINATION`), owning the LPV [`GISolver`](crate::graphics_engine::gi::GISolver).
pub struct GIPass {
    _field_0: [u8; 2240],
    /// The LPV solver the pass delegates its GI work to.
    pub m_pGISolver: *mut crate::graphics_engine::gi::GISolver,
}
fn _GIPass_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8C8], GIPass>([0u8; 0x8C8]);
    }
    unreachable!()
}
impl GIPass {}
impl std::convert::AsRef<GIPass> for GIPass {
    fn as_ref(&self) -> &GIPass {
        self
    }
}
impl std::convert::AsMut<GIPass> for GIPass {
    fn as_mut(&mut self) -> &mut GIPass {
        self
    }
}
#[repr(C, align(8))]
/// The light-propagation-volume solver: RSM inject + LPV propagation for the global-illumination pass.
/// `CGISolver::Execute` refreshes one of the two LPV cascades each dispatch and toggles
/// [`m_CascadeToUpdate`](crate::graphics_engine::gi::GISolver::m_CascadeToUpdate) afterward, so the cascades are refreshed in
/// alternation across frames.
pub struct GISolver {
    _field_0: [u8; 984],
    /// Selects which of the two LPV cascades is freshly injected + propagated this dispatch. Toggles
    /// `0 <-> 1` once per `Execute` (i.e. once per `Draw`).
    pub m_CascadeToUpdate: u32,
    _field_3dc: [u8; 4],
}
fn _GISolver_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3E0], GISolver>([0u8; 0x3E0]);
    }
    unreachable!()
}
impl GISolver {}
impl std::convert::AsRef<GISolver> for GISolver {
    fn as_ref(&self) -> &GISolver {
        self
    }
}
impl std::convert::AsMut<GISolver> for GISolver {
    fn as_mut(&mut self) -> &mut GISolver {
        self
    }
}
#[repr(C, align(8))]
/// The scene light manager, owning the global-illumination pass. The singleton address holds a pointer
/// to the manager, which is null until the light system initializes.
pub struct LightManager {
    _field_0: [u8; 12],
    /// The diffuse colour of the embedded global light (the sun, or the moon at night) -- the head
    /// of `m_GlobalLight.m_Light.Diffuse`. The atmospheric-scattering sky-lighting compute and the
    /// deferred sun term both read it.
    pub m_GlobalLightDiffuse: crate::types::math::Vector3,
    _field_18: [u8; 1829104],
    /// The global-illumination render pass.
    pub m_GIPass: *mut crate::graphics_engine::gi::GIPass,
}
fn _LightManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1BE910], LightManager>([0u8; 0x1BE910]);
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
    pub const UpdatePlayerLightingTransforms_ADDRESS: usize = 0x1400D0250;
    /// Places the light-propagation volume around a camera: hands `camera_transform` (a camera's
    /// world transform) to the [`m_GIPass`](crate::graphics_engine::gi::LightManager::m_GIPass), which keeps it and repositions the
    /// cascade [`GISolver::m_CascadeToUpdate`](crate::graphics_engine::gi::GISolver::m_CascadeToUpdate) names at the camera's position, rotated to its
    /// forward axis. `CGameCameraManager::UpdateRender` calls it once per frame with the game
    /// camera's render context (or a special camera's, when one is the lighting reference), not
    /// with the render camera, so the volume follows the game camera rather than the camera a
    /// pass renders from.
    pub unsafe fn UpdatePlayerLightingTransforms(
        &mut self,
        camera_transform: *const crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                camera_transform: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::UpdatePlayerLightingTransforms_ADDRESS);
            f(self as *mut Self as _, camera_transform)
        }
    }
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
