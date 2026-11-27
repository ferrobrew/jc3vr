//! Every registration the tooling makes with the layers below it, in one place: the video recorder
//! as the observer of the desktop mirror's presents and the post-draw's eye-frame recorder, the
//! session recording as the handler of take-marker requests, the diagnostics samplers at the
//! character hooks' pose-finalization observer (a field of the body's contributions) and after
//! each simulation step, and the directing tools as the source of a tracking recording's start
//! marker.
//!
//! The payload's composition calls [`register`] once, before the hooks are installed. Each
//! registration fills a slot of its own, so their order does not matter. [`register`] reports any
//! slot still empty to the install's registration check, which stops the startup naming it; the
//! body's registration reports the pose observer.

use jc3vr_body::{contributions::BodyContributions, hooks::character::PoseObserver};
use jc3vr_core::{
    registration::RegistrationCheck,
    session::{mark_handler_registered, set_mark_handler},
};
use jc3vr_gameplay::hooks::sim::{post_step_registered, register_post_step};
use jc3vr_view::{
    hooks::graphics_engine::post_draw::{
        EyeFrameRecorder, eye_frame_recorder_registered, register_eye_frame_recorder,
    },
    recorder::{NamedPlace, StartSource, register_start_source, start_source_registered},
    vr::{MirrorObserver, mirror_observer_registered, register_mirror_observer},
};

use crate::{
    diagnostics::{bodies, pose},
    director::teleport,
    recorder,
    recorder::session_recording,
};

/// Register the tooling with the layers below it, filling its field of `body`, and report what is
/// still missing to `check`.
pub fn register(body: &mut BodyContributions, check: &mut RegistrationCheck) {
    register_mirror_observer(MirrorObserver {
        records_mirror: recorder::records_mirror,
        note_mirror: recorder::note_mirror,
        note_present: recorder::note_present,
    });
    register_eye_frame_recorder(EyeFrameRecorder {
        wants_eye_captures: recorder::wants_eye_captures,
        frame_rendered: recorder::frame_rendered,
        capture_frame: recorder::capture_frame,
        release_capture: recorder::release_capture,
    });
    set_mark_handler(session_recording::mark);
    body.pose_observer = Some(PoseObserver {
        before_solve: pose::sample_pre_ik,
        after_pass: pose::sample_final,
    });
    register_post_step(bodies::after_step);
    register_start_source(StartSource {
        player_transform: teleport::player_transform,
        places: bookmark_places,
    });

    check.require(
        "desktop mirror",
        "the video recorder's observer",
        mirror_observer_registered(),
    );
    check.require(
        "post-draw",
        "the video recorder's eye-frame recorder",
        eye_frame_recorder_registered(),
    );
    check.require(
        "session",
        "the session recording's take-marker handler",
        mark_handler_registered(),
    );
    check.require(
        "sim",
        "the diagnostics' post-step sampler",
        post_step_registered(),
    );
    check.require(
        "tracking recorder",
        "the directing tools' start-marker source",
        start_source_registered(),
    );
}

/// The teleport bookmarks, as the named places a start marker near one is labelled with.
fn bookmark_places() -> Vec<NamedPlace> {
    teleport::bookmarks()
        .into_iter()
        .map(|bookmark| NamedPlace {
            position: bookmark.position(),
            name: bookmark.name,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registration_leaves_nothing_missing() {
        let mut check = RegistrationCheck::new();
        let mut body = BodyContributions::default();
        register(&mut body, &mut check);
        assert_eq!(check.missing(), []);
        assert!(body.pose_observer.is_some());
    }
}
