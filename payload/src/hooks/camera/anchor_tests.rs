use glam::{Mat4, Quat, Vec3};

use super::*;
use crate::headpose::{
    HeadPoseState,
    anchors::{
        History, Pair,
        tests::{context, identity_fixture, request, rooted},
    },
    xr::compose,
};

fn roots() -> [Mat4; 2] {
    [
        Mat4::from_rotation_translation(
            Quat::from_euler(glam::EulerRot::YXZ, 0.4, 0.2, -0.3),
            Vec3::new(10.0, 2.0, -3.0),
        ),
        Mat4::from_rotation_translation(
            Quat::from_euler(glam::EulerRot::YXZ, -0.7, 0.4, 0.2),
            Vec3::new(11.0, 2.4, -4.0),
        ),
    ]
}

fn settings() -> CameraConfig {
    CameraConfig {
        use_eye_matrices: true,
        head_offset: Vec3::ZERO,
        body_offset: Vec3::ZERO,
        ..CameraConfig::new()
    }
}

fn capture_pair(discontinuous: bool) -> Pair {
    let mut history = History::new();
    let id = identity_fixture();
    let [r0, r1] = roots();
    history
        .capture(request(id, 20, 1, r0, r0), || Some(rooted(0.0, r0)))
        .unwrap();
    let mut next = request(id, 21, 2, r0, r1);
    if discontinuous {
        next.context.teleport += 1;
    }
    history.capture(next, || Some(rooted(1.0, r1))).unwrap()
}

#[test]
fn history_publication_camera_uses_each_nonidentity_moving_root_and_complete_geometry() {
    let pair = capture_pair(false);
    let geometry = pair.geometry();
    let mut publication = HeadPoseState::default();
    publication.publish_anchors(geometry, Vec3::ZERO, None);
    let cockpit = Vec3::new(0.1, -0.03, 0.02);
    let follow = [Quat::from_rotation_y(0.3), Quat::from_rotation_y(-0.5)];
    let positions = std::array::from_fn::<_, 2, _>(|i| {
        let (_, body, _) = geometry[i].root.to_scale_rotation_translation();
        let pose = compose(
            cockpit,
            Quat::from_rotation_x(0.2),
            body * follow[i],
            geometry[i].head,
            1.0,
        );
        camera_position(&pose, follow[i], &settings(), geometry[i], Source::Vr)
    });
    let expected = std::array::from_fn::<_, 2, _>(|i| {
        let (_, body, _) = geometry[i].root.to_scale_rotation_translation();
        geometry[i].head
            + geometry[i].neck_delta
            + body * follow[i] * (cockpit + geometry[i].eye_arm)
    });
    for alpha in [0.0, 0.25, 0.5, 0.9, 1.0] {
        assert!(
            positions[0]
                .lerp(positions[1], alpha)
                .abs_diff_eq(expected[0].lerp(expected[1], alpha), 1.0e-5),
            "alpha {alpha}"
        );
    }
    assert_ne!(geometry[0].neck_delta, geometry[1].neck_delta);
    assert_ne!(geometry[0].eye_arm, geometry[1].eye_arm);
}

#[test]
fn discontinuity_collapses_whole_sim_pose_before_camera_interpolation() {
    let pair = capture_pair(true);
    assert!(pair.collapsed);
    let geometry = pair.geometry();
    let mut publication = HeadPoseState::default();
    let old = HeadPose {
        position: Vec3::new(20.0, -3.0, 8.0),
        orientation: Quat::from_rotation_z(1.1) * Quat::from_rotation_x(0.6),
    };
    publication.test_set_poses([old; 2]);
    let input = Quat::from_euler(glam::EulerRot::YXZ, 0.8, -0.3, 0.2);
    let (_, body, _) = pair.current.root.to_scale_rotation_translation();
    let current_orientation = body * input;
    let offset = Vec3::new(0.2, -0.1, 0.3);
    publication.publish_anchors(geometry, offset, Some(current_orientation));
    let poses = publication.test_poses();
    assert!(
        poses[0]
            .position
            .abs_diff_eq(geometry[1].head + current_orientation * offset, 1.0e-5)
    );
    assert!(
        poses[0]
            .orientation
            .abs_diff_eq(current_orientation, 1.0e-5)
    );
    assert!(!poses[0].orientation.abs_diff_eq(old.orientation, 1.0e-4));
    let positions = std::array::from_fn::<_, 2, _>(|i| {
        camera_position(
            &poses[i],
            Quat::IDENTITY,
            &settings(),
            geometry[i],
            Source::Sim,
        )
    });
    for alpha in [0.0, 0.25, 0.5, 0.9, 1.0] {
        assert!(
            poses[0]
                .position
                .lerp(poses[1].position, alpha)
                .abs_diff_eq(poses[1].position, 1.0e-5)
        );
        assert!(
            poses[0]
                .orientation
                .slerp(poses[1].orientation, alpha)
                .abs_diff_eq(current_orientation, 1.0e-5)
        );
        assert!(
            positions[0]
                .lerp(positions[1], alpha)
                .abs_diff_eq(positions[1], 1.0e-5)
        );
    }
}

#[test]
fn collapsed_vr_geometry_uses_current_follow_and_frame_for_both_endpoints() {
    let geometry = capture_pair(true).geometry();
    let old_follow = Quat::from_rotation_y(0.9);
    let current_follow = Quat::from_rotation_y(-0.4);
    let (_, old_body, _) = roots()[0].to_scale_rotation_translation();
    let (_, body, _) = geometry[1].root.to_scale_rotation_translation();
    let current_frame = body * current_follow;
    let previous_frame = geometry[0].previous_endpoint(old_body * old_follow, current_frame);
    let previous_follow = geometry[0].previous_endpoint(old_follow, current_follow);
    let cockpit = Vec3::new(0.1, -0.04, 0.05);
    let cockpit_previous = geometry[0].previous_endpoint(Vec3::new(-0.3, 0.2, 0.7), cockpit);
    let orientation = Quat::from_rotation_x(0.3) * Quat::from_rotation_z(-0.2);
    let poses = [
        compose(
            cockpit_previous,
            orientation,
            previous_frame,
            geometry[0].head,
            1.0,
        ),
        compose(cockpit, orientation, current_frame, geometry[1].head, 1.0),
    ];
    let mut publication = HeadPoseState::default();
    publication.publish_anchors(geometry, Vec3::ZERO, None);
    publication.test_set_poses(poses);
    let positions = [
        camera_position(
            &poses[0],
            previous_follow,
            &settings(),
            geometry[0],
            Source::Vr,
        ),
        camera_position(
            &poses[1],
            current_follow,
            &settings(),
            geometry[1],
            Source::Vr,
        ),
    ];
    assert!(geometry[0].neck_delta.length() > 0.0 && geometry[0].eye_arm.length() > 0.0);
    let wrong = camera_position(&poses[0], old_follow, &settings(), geometry[0], Source::Vr);
    assert!(!wrong.abs_diff_eq(positions[1], 1.0e-4));
    for alpha in [0.0, 0.25, 0.5, 0.9, 1.0] {
        assert!(
            positions[0]
                .lerp(positions[1], alpha)
                .abs_diff_eq(positions[1], 1.0e-5)
        );
        assert!(
            poses[0]
                .orientation
                .slerp(poses[1].orientation, alpha)
                .abs_diff_eq(poses[1].orientation, 1.0e-5)
        );
    }
}

#[test]
fn reused_receipt_changed_root_cannot_reach_publication_or_movement_then_fresh_receipt_resumes() {
    let id = identity_fixture();
    let [r0, r1] = roots();
    let mut history = History::new();
    history
        .capture(request(id, 20, 1, r0, r0), || Some(rooted(0.0, r0)))
        .unwrap();
    let mut publications = 0;
    if let Some(pair) = history.capture(request(id, 20, 1, r0, r1), || {
        panic!("duplicate root change rereads joints")
    }) {
        let mut state = HeadPoseState::default();
        state.publish_anchors(pair.geometry(), Vec3::ZERO, None);
        panic!("incoherent pair reached native capture or movement completion");
    }
    assert_eq!(publications, 0);
    let pair = history
        .capture(request(id, 21, 2, r0, r1), || Some(rooted(1.0, r1)))
        .unwrap();
    publications += 1;
    assert_eq!(publications, 1);
    assert!(pair.collapsed);
    assert_eq!(pair.current.root, r1);
    assert_eq!(context().source, Source::Sim);
}
