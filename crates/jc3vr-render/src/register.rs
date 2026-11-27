//! Every registration the render crate makes with the layers below it, in one place: its two steps
//! at hook removal ([`jc3vr_core::hooks::register_pre_unpatch`]), the debug tools' taps on the
//! camera hooks and the VR runtime, and what the VR runtime's teardown releases on the render side.
//!
//! The payload's composition calls [`register`] once, before the hooks are installed. The two
//! removal steps run in the order they are registered, and a step that refuses stops the ones after
//! it, so stereo's goes first: the single-pass COM detours are torn down even when the texture
//! census then refuses the removal. The other registrations each fill a slot of their own, so their
//! order does not matter. [`register`] reports any slot still empty to the install's registration
//! check, which stops the startup naming it.

use std::sync::atomic::Ordering;

use jc3vr_core::{hooks::register_pre_unpatch, registration::RegistrationCheck};
use jc3vr_view::{
    hooks::camera::{CameraDebugTaps, camera_debug_taps_registered, register_camera_debug_taps},
    vr::{
        ResizeObserver, TeardownParticipant, register_resize_observer,
        register_teardown_participant, resize_observer_registered, teardown_participant_registered,
    },
};

use crate::{
    debug::{
        texture_census,
        trace::{TraceEvent, TraceState},
    },
    hooks::graphics_engine::engine::BLOCK_FLIP,
    stereo::single_pass,
};

/// Register the render crate with the layers below it, and report what is still missing to
/// `check`.
pub fn register(check: &mut RegistrationCheck) {
    // The single-pass COM-vtable detours inline-patch the DXVK functions to jump into payload code
    // and are managed outside the hook library, so hook removal tears them down explicitly before
    // the DLL unloads. Left standing, the dangling jumps crash the game on its next D3D call.
    register_pre_unpatch(single_pass::uninstall_com_detours);
    register_pre_unpatch(texture_census_unarmed);
    register_camera_debug_taps(CameraDebugTaps {
        setup_entered: || TraceState::record_eye(TraceEvent::SetupRenderCamera),
    });
    register_resize_observer(&RESIZE_OBSERVER);
    register_teardown_participant(&TEARDOWN);

    check.require(
        "camera",
        "the render trace's debug taps",
        camera_debug_taps_registered(),
    );
    check.require(
        "native resolution",
        "the texture census's resize observer",
        resize_observer_registered(),
    );
    check.require(
        "VR teardown",
        "the flip block's release",
        teardown_participant_registered(),
    );
}

/// The texture census's detours, once enabled, pin the payload in the process.
fn texture_census_unarmed() -> bool {
    if texture_census::armed() {
        tracing::error!(
            restart_required = true,
            "hook removal: the texture census's detours were enabled; the payload must remain mapped and the game must be restarted"
        );
        return false;
    }
    true
}

/// The texture census on the engine resizes the native-resolution driver requests.
static RESIZE_OBSERVER: ResizeObserver = ResizeObserver {
    engine_ready: texture_census::ensure_armed,
    resized: |width, height| texture_census::report(&format!("resize {width}x{height}")),
};

/// The flip block the frame top sets while a session presents, released by the VR runtime's
/// teardown.
static TEARDOWN: TeardownParticipant = TeardownParticipant {
    release_flip: || BLOCK_FLIP.store(false, Ordering::Relaxed),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registration_leaves_nothing_missing() {
        let mut check = RegistrationCheck::new();
        register(&mut check);
        assert_eq!(check.missing(), []);
    }
}
