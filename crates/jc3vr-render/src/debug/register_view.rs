//! The debug tools' taps on the VR runtime and the camera hooks, which sit below them: the render
//! trace on the render camera's setup, and the texture census on the engine resizes the
//! native-resolution driver requests.

use jc3vr_view::{
    hooks::camera::{CameraDebugTaps, register_camera_debug_taps},
    vr::{ResizeObserver, register_resize_observer},
};

use crate::debug::{
    texture_census,
    trace::{TraceEvent, TraceState},
};

pub fn register() {
    register_camera_debug_taps(CameraDebugTaps {
        setup_entered: || TraceState::record_eye(TraceEvent::SetupRenderCamera),
    });
    register_resize_observer(&RESIZE_OBSERVER);
}

static RESIZE_OBSERVER: ResizeObserver = ResizeObserver {
    engine_ready: texture_census::ensure_armed,
    resized: |width, height| texture_census::report(&format!("resize {width}x{height}")),
};
