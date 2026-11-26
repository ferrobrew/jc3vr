//! Registers the debug UI as the input layer's capture driver
//! ([`crate::input_core::capture::CaptureDriver`]): the policy step runs against the live
//! [`EguiState`](crate::egui_impl::EguiState), and the desktop sample is the native one.

use crate::{
    egui_impl::{desktop_input::sample_desktop_input, update_debug_ui_capture},
    input_core::capture::{CaptureDriver, register_driver},
};

/// Register the driver. Runs before the debug UI's state exists, which is harmless: the step
/// returns `None` until the state is installed.
pub(crate) fn register() {
    register_driver(CaptureDriver {
        update: update_debug_ui_capture,
        sample_desktop: sample_desktop_input,
    });
}
