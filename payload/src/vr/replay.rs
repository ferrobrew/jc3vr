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
    controllers::frame::Pose,
    recorder::tracking::{CaptureSample, format::EyeSample},
    testing::replay,
    vr::{
        EyeView, Fov, OffAxisProjection, VrConfig, begin_replay_frame,
        pose_control::{pose_orientation, pose_position},
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
        eye.pose = posef(&sample.pose);
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
    let (near, far) = crate::hooks::camera::main_camera_planes_or((cfg.near_clip, cfg.far_clip));
    let eyes = head.eyes.map(|sample| {
        let [left, right, up, down] = sample.fov;
        EyeView {
            pose: posef(&sample.pose),
            raw_pose: posef(&sample.pose),
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
        pose: Pose {
            position: pose_position(eye.pose),
            orientation: pose_orientation(eye.pose),
        },
        fov: [
            eye.fov.angle_left,
            eye.fov.angle_right,
            eye.fov.angle_up,
            eye.fov.angle_down,
        ],
    }
}

fn posef(pose: &Pose) -> xr::Posef {
    let p = pose.position;
    let q = pose.orientation;
    xr::Posef {
        orientation: xr::Quaternionf {
            x: q.x,
            y: q.y,
            z: q.z,
            w: q.w,
        },
        position: xr::Vector3f {
            x: p.x,
            y: p.y,
            z: p.z,
        },
    }
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

    #[test]
    fn a_pose_survives_the_round_trip_through_openxr() {
        let pose = Pose {
            position: Vec3::new(0.1, 1.6, -0.2),
            orientation: Quat::from_rotation_y(0.7),
        };
        let back = posef(&pose);
        assert_eq!(pose_position(back), pose.position);
        assert_eq!(pose_orientation(back), pose.orientation);
    }
}
