//! Stereo's step at hook removal ([`jc3vr_core::hooks::register_pre_unpatch`]).
//!
//! The single-pass COM-vtable detours inline-patch the DXVK functions to jump into payload code and
//! are managed outside the hook library, so hook removal tears them down explicitly before the DLL
//! unloads. Left standing, the dangling jumps crash the game on its next D3D call.

pub fn register() {
    jc3vr_core::hooks::register_pre_unpatch(crate::stereo::single_pass::uninstall_com_detours);
}
