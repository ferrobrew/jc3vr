//! The VR runtime's side of the tracking recorder and the replay: what the frame loop records of
//! a frame, how a replay's head stands in for the located views, and the frame a replay drives
//! with no session running.
//!
//! The recorder and the replay work on the cockpit frame, which is what the frame loop publishes:
//! the eye views re-based against the recenter baseline, and the controllers sampled with them.
//! A replay therefore replaces the cockpit poses ([`EyeView::pose`]) and leaves the raw poses the
//! compositor is given alone, so a headset that is running shows the replayed view where it is.

use openxr as xr;

use crate::{
    recorder::tracking::{CaptureSample, format::EyeSample},
    testing::replay,
    vr::{
        EyeView, Fov, OffAxisProjection, VrConfig, begin_replay_frame,
        convert::{self, posef},
        stage,
    },
};

/// Replace the located views' cockpit poses with the replay's, while one plays. Called by the
/// frame loop under the runtime lock, after it located the views.
pub(super) fn replace_located_head(eyes: &mut [EyeView; 2]) {
    let Some(head) = replay::head() else {
        return;
    };
    for (eye, sample) in eyes.iter_mut().zip(&head.eyes) {
        eye.pose = posef(sample.pose.position, sample.pose.orientation);
    }
}

/// The frame loop's record of a frame, for the tracking recorder.
pub(super) fn capture_sample(
    display_time: xr::Time,
    should_render: bool,
    head_tracked: bool,
    located: Option<&[EyeView; 2]>,
) -> CaptureSample {
    CaptureSample {
        display_time_ns: display_time.as_nanos(),
        should_render,
        head_tracked,
        eyes: located.map(|eyes| eyes.map(|eye| eye_sample(&eye))),
    }
}

/// Drive a frame from a playing replay when the frame loop did not: publish the recorded frames
/// the input update did not take (there is no session, or it has no controller input), and, with
/// no VR frame rendering, publish the recorded head the way a VR frame publishes its views, less
/// the per-eye render parameters, so the desktop renders the recorded view. Game thread, in the
/// render update after the frame began.
pub fn drive_replay_frame(vr_frame_rendering: bool, cfg: &VrConfig) {
    let controllers = crate::config::Config::lock_query(|c| c.controllers);
    crate::vr::input::publish_replayed(&controllers);
    if vr_frame_rendering {
        return;
    }
    if let Some(info) = replay::stage() {
        stage::publish_replayed(info);
    }
    let Some(head) = replay::head() else {
        return;
    };
    let (near, far) = crate::hooks::camera::main_camera_planes_or(cfg.far_clip);
    let eyes = head.eyes.map(|sample| {
        let [left, right, up, down] = sample.fov;
        EyeView {
            pose: posef(sample.pose.position, sample.pose.orientation),
            raw_pose: posef(sample.pose.position, sample.pose.orientation),
            fov: xr::Fovf {
                angle_left: left,
                angle_right: right,
                angle_up: up,
                angle_down: down,
            },
            projection: OffAxisProjection::new(
                Fov {
                    left,
                    right,
                    up,
                    down,
                },
                near,
                far,
            ),
        }
    });
    begin_replay_frame(eyes, cfg);
}

fn eye_sample(eye: &EyeView) -> EyeSample {
    EyeSample {
        pose: convert::pose(eye.pose),
        fov: [
            eye.fov.angle_left,
            eye.fov.angle_right,
            eye.fov.angle_up,
            eye.fov.angle_down,
        ],
    }
}
