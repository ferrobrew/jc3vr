#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The camera modifier that applies the local player's weapon to the camera
/// (`CWeaponCameraModifier`). Its layout is unmapped.
pub struct WeaponCameraModifier {}
impl WeaponCameraModifier {
    pub const ProcessCameraContext_ADDRESS: usize = 0x1407EC200;
    /// The modifier's camera pass. With a local player character it runs the snap-aim camera
    /// assist when the modifier's settings enable it, turns the pipeline's camera boom by the held
    /// weapon's per-tick recoil
    /// ([`WeaponBase::m_RecoilYaw`](crate::equipment::weapon::WeaponBase::m_RecoilYaw) and
    /// [`WeaponBase::m_RecoilPitch`](crate::equipment::weapon::WeaponBase::m_RecoilPitch), through
    /// [`BoomTransform::DeltaTransform`](crate::camera::input_to_orbit::BoomTransform::DeltaTransform)) for
    /// the weapon being planted or the selected weapon while wielded, dual-wielded, or mounted, and
    /// then writes the pipeline's camera transform into `out`
    /// ([`CameraPipelineContext::CalculateCameraTransform`](crate::camera::camera_context::CameraPipelineContext::CalculateCameraTransform)). Without a local player it does
    /// nothing. Because the boom persists, the recoil also turns the camera's lasting yaw and pitch.
    pub unsafe fn ProcessCameraContext(
        &mut self,
        pipeline: *mut crate::camera::camera_context::CameraPipelineContext,
        previous: *const crate::camera::camera_context::CameraContext,
        out: *mut crate::camera::camera_context::CameraContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pipeline: *mut crate::camera::camera_context::CameraPipelineContext,
                previous: *const crate::camera::camera_context::CameraContext,
                out: *mut crate::camera::camera_context::CameraContext,
            ) = ::std::mem::transmute(Self::ProcessCameraContext_ADDRESS);
            f(self as *mut Self as _, pipeline, previous, out)
        }
    }
}
impl std::convert::AsRef<WeaponCameraModifier> for WeaponCameraModifier {
    fn as_ref(&self) -> &WeaponCameraModifier {
        self
    }
}
impl std::convert::AsMut<WeaponCameraModifier> for WeaponCameraModifier {
    fn as_mut(&mut self) -> &mut WeaponCameraModifier {
        self
    }
}
