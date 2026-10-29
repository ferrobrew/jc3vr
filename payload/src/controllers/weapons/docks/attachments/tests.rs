use glam::Quat;

use super::*;
use crate::controllers::{
    body::{
        mirror::{Plane, attachment_markers},
        rig::frames::Frames,
    },
    overlay::primitive::{Anchor, Primitive},
};

fn observation(placement: Placement, bone: Option<Mat4>, occupancy: Occupancy) -> Sample {
    let (source, pose) = socket_pose(Mat4::IDENTITY, bone, placement);
    Sample {
        owner: 7,
        at: Instant::now(),
        character: Mat4::IDENTITY,
        attachments: vec![Attachment {
            socket: Socket::Shoulder(Hand::Right),
            hand: Some(Hand::Left),
            usage: Some(SocketUse::Dock),
            occupancy,
            item: None,
            source,
            pose,
        }],
    }
}

fn assert_docking_and_mirror(sample: &Sample, expected: Vec3) {
    let points = sample.points();
    assert_eq!(points.len(), 1);
    assert!(points[0].point.abs_diff_eq(expected, 1e-5));
    let plane = Plane::ahead_of(&sample.character, 1.5).unwrap();
    let head = Vec3::new(2.0, 1.8, -3.0);
    let mut primitives = Vec::new();
    attachment_markers(sample, plane, head, &mut primitives);
    assert_eq!(primitives.len(), 2);
    let Primitive::Sprite {
        at: Anchor::Head(offset),
        ..
    } = primitives[0]
    else {
        panic!("the actual mirror renderer must emit the socket ring");
    };
    assert!((head + offset).abs_diff_eq(plane.reflect(points[0].point), 1e-5));
    let Primitive::Line {
        from: Anchor::Head(from),
        ..
    } = primitives[1]
    else {
        panic!("the actual mirror renderer must emit the socket orientation");
    };
    assert!(from.abs_diff_eq(offset, 1e-5));
}

#[test]
fn remembered_grips_follow_body_poses_in_both_production_consumers() {
    let local = Vec3::new(0.03, -0.08, 0.04);
    for rotation in [
        Quat::IDENTITY,
        Quat::from_rotation_x(0.8),
        Quat::from_rotation_z(-0.6),
        Quat::from_rotation_y(1.2),
    ] {
        let bone = Mat4::from_rotation_translation(rotation, Vec3::new(0.2, 1.1, 0.1));
        let sample = observation(
            Placement::Free {
                object: Some(11),
                remembered: Some(local),
                native: false,
                offset: Vec3::splat(0.3),
            },
            Some(bone),
            Occupancy::Empty,
        );
        assert_eq!(
            sample.attachments[0].source,
            PoseSource::RememberedGrip { object: 11 }
        );
        assert_docking_and_mirror(&sample, bone.transform_point3(local));
    }
}

#[test]
fn visible_grips_do_not_become_bone_origins_and_keep_occupancy_identity() {
    let occupancy = Occupancy::Occupied { object: Some(13) };
    let grip = Vec3::new(-0.2, 0.9, 0.05);
    for bone in [None, Some(Mat4::from_translation(Vec3::new(0.2, 1.2, 0.2)))] {
        let mut sample = observation(
            Placement::Visible {
                object: 13,
                grip: Some(grip),
            },
            bone,
            occupancy,
        );
        sample.attachments[0].usage = Some(SocketUse::Draw);
        assert_eq!(sample.attachments[0].occupancy, occupancy);
        assert_eq!(
            sample.attachments[0].source,
            PoseSource::VisibleGrip { object: 13 }
        );
        assert_docking_and_mirror(&sample, grip);
    }
}

#[test]
fn missing_bones_and_grips_produce_neither_docking_points_nor_mirror_markers() {
    for (placement, expected) in [
        (
            Placement::Visible {
                object: 11,
                grip: None,
            },
            Missing::Grip,
        ),
        (
            Placement::Free {
                object: Some(11),
                remembered: None,
                native: false,
                offset: Vec3::ZERO,
            },
            Missing::Bone,
        ),
        (
            Placement::Visible {
                object: 11,
                grip: Some(Vec3::NAN),
            },
            Missing::Transform,
        ),
    ] {
        let sample = observation(placement, None, Occupancy::Empty);
        assert_eq!(sample.attachments[0].pose, Err(expected));
        assert!(sample.points().is_empty());
        let mut primitives = Vec::new();
        attachment_markers(
            &sample,
            Plane::ahead_of(&Mat4::IDENTITY, 1.0).unwrap(),
            Vec3::ZERO,
            &mut primitives,
        );
        assert!(primitives.is_empty());
    }
}

#[test]
fn native_and_configured_fallbacks_use_the_same_character_frame_as_docking() {
    let character =
        Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(4.0, 2.0, -5.0));
    let bone = character
        * Mat4::from_rotation_translation(Quat::from_rotation_x(0.8), Vec3::new(0.1, 1.3, 0.2));
    let offset = Vec3::new(0.0, -0.05, 0.14);
    for native in [false, true] {
        let (source, pose) = socket_pose(
            character,
            Some(bone),
            Placement::Free {
                object: Some(11),
                remembered: None,
                native,
                offset,
            },
        );
        let mut sample = observation(
            Placement::Visible {
                object: 11,
                grip: None,
            },
            None,
            Occupancy::Empty,
        );
        sample.character = character;
        sample.attachments[0].source = source;
        sample.attachments[0].pose = pose;
        let expected = bone.w_axis.truncate()
            + if native {
                Vec3::ZERO
            } else {
                character.transform_vector3(offset)
            };
        assert_docking_and_mirror(&sample, expected);
    }
}

#[test]
fn a_platform_move_rebases_the_same_sample_without_refreshing_its_age() {
    let source =
        Mat4::from_rotation_translation(Quat::from_rotation_y(-0.4), Vec3::new(3.0, 0.0, 1.0));
    let destination =
        Mat4::from_rotation_translation(Quat::from_rotation_y(1.0), Vec3::new(20.0, 4.0, -7.0));
    let local = Vec3::new(0.2, 1.1, 0.15);
    let mut sample = observation(
        Placement::Visible {
            object: 11,
            grip: Some(source.transform_point3(local)),
        },
        None,
        Occupancy::Empty,
    );
    sample.character = source;
    let original = sample.clone();
    let rebased = sample
        .in_world(7, destination, sample.at + Duration::from_millis(20))
        .unwrap();
    assert_eq!(rebased.at, sample.at);
    assert_eq!(sample, original);
    assert_docking_and_mirror(&rebased, destination.transform_point3(local));
    let frames = Frames {
        head_world: Vec3::new(19.0, 5.5, -8.0),
        head_cockpit: Vec3::new(0.0, 1.7, 0.0),
        view: Quat::from_rotation_y(0.5),
        world_scale: 1.4,
        character: destination,
    };
    let point = rebased.points()[0].point;
    assert!(
        frames
            .cockpit_to_world(frames.world_to_cockpit(point))
            .abs_diff_eq(point, 1e-5)
    );
    assert!(sample.in_world(8, destination, sample.at).is_none());
    assert!(
        sample
            .in_world(
                7,
                destination,
                sample.at + MAX_AGE + Duration::from_nanos(1)
            )
            .is_none()
    );
    assert!(
        sample
            .in_world(7, destination, sample.at - Duration::from_millis(1))
            .is_none()
    );
    assert!(sample.in_world(7, Mat4::ZERO, sample.at).is_none());
    assert!(
        sample
            .in_world(7, Mat4::from_translation(Vec3::NAN), sample.at)
            .is_none()
    );
}

#[test]
fn mirror_prefers_actual_hand_offers_without_hiding_an_unoffered_socket() {
    let mut sample = observation(
        Placement::Visible {
            object: 11,
            grip: Some(Vec3::Y),
        },
        None,
        Occupancy::Empty,
    );
    let mut base = sample.attachments[0];
    base.hand = None;
    base.usage = None;
    sample.attachments.insert(0, base);
    let mut unoffered = base;
    unoffered.socket = Socket::Chest;
    unoffered.source = PoseSource::UnavailableWeapon { object: None };
    unoffered.pose = Err(Missing::Weapon);
    sample.attachments.push(unoffered);
    let markers: Vec<_> = sample.markers().collect();
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].hand, Some(Hand::Left));
    assert_eq!(markers[1].pose, Err(Missing::Weapon));
    assert_docking_and_mirror(&sample, Vec3::Y);
}
