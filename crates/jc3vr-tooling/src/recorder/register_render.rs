//! Registers the recorder with the render layer: as the observer of the desktop mirror's presents,
//! and as the post-draw's eye-frame recorder.

use crate::{
    hooks::graphics_engine::post_draw::{EyeFrameRecorder, register_eye_frame_recorder},
    recorder::{
        capture_frame, frame_rendered, note_mirror, note_present, records_mirror, release_capture,
        wants_eye_captures,
    },
    vr::{MirrorObserver, register_mirror_observer},
};

pub(crate) fn register() {
    register_mirror_observer(MirrorObserver {
        records_mirror,
        note_mirror,
        note_present,
    });
    register_eye_frame_recorder(EyeFrameRecorder {
        wants_eye_captures,
        frame_rendered,
        capture_frame,
        release_capture,
    });
}
