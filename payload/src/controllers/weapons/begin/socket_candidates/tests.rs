use std::time::{Duration, Instant};

use glam::{Quat, Vec3};

use super::*;
use crate::controllers::{
    body::{
        mirror::{Plane, attachment_markers},
        rig::frames::SkeletonLandmarks,
    },
    frame::{Hand, Pose},
    overlay::primitive::{Anchor, Primitive},
    weapons::{
        docks::attachments::{Attachment, MAX_AGE, Missing, Occupancy, PoseSource, TargetPolicy},
        volumes,
    },
};

fn body(bend: Quat) -> BodyFrame {
    let pelvis = Vec3::new(0.0, 0.9, 0.1);
    let shoulders = [
        pelvis + bend * Vec3::new(-0.2, 0.5, 0.0),
        pelvis + bend * Vec3::new(0.2, 0.5, 0.0),
    ];
    BodyFrame::on(SkeletonLandmarks {
        head: pelvis + bend * Vec3::new(0.0, 0.75, 0.0),
        pelvis,
        hips: [pelvis - Vec3::X * 0.1, pelvis + Vec3::X * 0.1],
        shoulders,
    })
    .unwrap()
}

fn frames(character: Mat4) -> Frames {
    Frames {
        head_world: character.transform_point3(Vec3::new(0.0, 1.7, 0.0)),
        head_cockpit: Vec3::new(0.0, 1.7, 0.0),
        view: Quat::from_rotation_y(0.7),
        world_scale: 1.3,
        character,
    }
}

fn native_sample(frames: &Frames) -> Sample {
    Sample {
        owner: 7,
        at: Instant::now(),
        character: frames.character,
        policy: TargetPolicy::Native,
        attachments: Socket::ALL
            .into_iter()
            .enumerate()
            .map(|(index, socket)| {
                let occupied = index % 2 == 0;
                Attachment {
                    socket,
                    hand: Some(Hand::Left),
                    usage: Some(if occupied {
                        SocketUse::Draw
                    } else {
                        SocketUse::Dock
                    }),
                    occupancy: if occupied {
                        Occupancy::Occupied { object: Some(11) }
                    } else {
                        Occupancy::Empty
                    },
                    item: None,
                    source: PoseSource::VisibleGrip { object: 11 },
                    pose: Ok(Pose {
                        position: frames.cockpit_to_world(Vec3::new(index as f32, 0.2, 0.3)),
                        orientation: Quat::IDENTITY,
                    }),
                    native: None,
                }
            })
            .collect(),
    }
}

fn assert_provider_and_mirror(
    sample: &Sample,
    draws: &[Candidate],
    others: &[(Candidate, SocketUse)],
    frames: &Frames,
) {
    let plane = Plane::ahead_of(&sample.character, 1.5).unwrap();
    let head = frames.head_world;
    let mut primitives = Vec::new();
    attachment_markers(sample, plane, head, &mut primitives);
    let points = sample.points();
    assert_eq!(points.len(), draws.len() + others.len());
    assert_eq!(primitives.len(), points.len() * 2);
    for (point, marker) in points.iter().zip(primitives.as_chunks::<2>().0) {
        let candidate = draws
            .iter()
            .chain(others.iter().map(|(candidate, _)| candidate))
            .find(|candidate| {
                candidate.volume == GrabVolume::Socket(point.socket)
                    && candidate.hand == Some(point.hand)
            })
            .unwrap();
        assert!(
            frames
                .cockpit_to_world(candidate.centre)
                .abs_diff_eq(point.point, 1e-5)
        );
        let Primitive::Sprite {
            at: Anchor::Head(offset),
            ..
        } = marker[0]
        else {
            panic!("the production mirror emits a target ring");
        };
        assert!((head + offset).abs_diff_eq(plane.reflect(point.point), 1e-5));
    }
}

#[test]
fn landmark_provider_and_mirror_share_bent_body_targets_and_configured_offsets() {
    let frames = frames(Mat4::from_rotation_translation(
        Quat::from_rotation_y(-0.4),
        Vec3::new(4.0, 2.0, -3.0),
    ));
    for bend in [
        Quat::IDENTITY,
        Quat::from_rotation_x(0.8),
        Quat::from_rotation_z(-0.5),
    ] {
        for offset in [Vec3::ZERO, Vec3::new(0.04, -0.03, 0.12)] {
            let body = body(bend);
            let mut holsters = HolstersConfig::new();
            holsters.follow_docks = false;
            holsters.hip.offset_m = offset;
            holsters.shoulder.offset_m = offset;
            holsters.chest.offset_m = offset;
            let native = native_sample(&frames);
            let original = native.clone();
            let (sample, draws, others) = provide(&native, &frames, Some(&body), &holsters);
            assert_eq!(native, original);
            assert_eq!(sample.at, native.at);
            assert_eq!(sample.policy, TargetPolicy::BodyLandmarks);
            assert!(sample.matches_policy(false));
            assert!(!sample.matches_policy(true));
            let landmarks = volumes::socket_landmarks(&body, &holsters);
            for (target, observed) in sample.attachments.iter().zip(&native.attachments) {
                assert_eq!(target.occupancy, observed.occupancy);
                assert_eq!(target.usage, observed.usage);
                assert_eq!(target.hand, observed.hand);
                if let Some(landmark) = landmarks
                    .iter()
                    .find(|c| c.volume == GrabVolume::Socket(target.socket))
                {
                    assert_eq!(target.source, PoseSource::BodyLandmark);
                    assert!(
                        target
                            .pose
                            .unwrap()
                            .position
                            .abs_diff_eq(frames.cockpit_to_world(landmark.centre), 1e-5)
                    );
                    let observed_native = target.native.unwrap();
                    assert_eq!(observed_native.source, observed.source);
                    assert_eq!(observed_native.pose, observed.pose);
                } else {
                    assert_eq!(target, observed);
                }
            }
            assert_provider_and_mirror(&sample, &draws, &others, &frames);
        }
    }
}

#[test]
fn missing_native_poses_do_not_create_landmark_offers_and_no_fit_keeps_occupancy() {
    let frames = frames(Mat4::IDENTITY);
    let mut native = native_sample(&frames);
    native.attachments[0].source = PoseSource::UnavailableWeapon { object: Some(11) };
    native.attachments[0].pose = Err(Missing::Weapon);
    native.attachments[1].pose = Err(Missing::Bone);
    native.attachments[2].usage = Some(SocketUse::NoFit);
    let mut holsters = HolstersConfig::new();
    holsters.follow_docks = false;
    let (sample, draws, others) = provide(&native, &frames, Some(&body(Quat::IDENTITY)), &holsters);
    assert_eq!(sample.attachments[0], native.attachments[0]);
    assert_eq!(sample.attachments[1], native.attachments[1]);
    assert_eq!(sample.attachments[2].usage, Some(SocketUse::NoFit));
    assert_eq!(
        sample.attachments[2].occupancy,
        native.attachments[2].occupancy
    );
    assert_provider_and_mirror(&sample, &draws, &others, &frames);
}

#[test]
fn missing_body_and_follow_docks_keep_the_existing_native_targets() {
    let frames = frames(Mat4::IDENTITY);
    let native = native_sample(&frames);
    for follow_docks in [false, true] {
        let mut holsters = HolstersConfig::new();
        holsters.follow_docks = follow_docks;
        let (sample, draws, others) = provide(&native, &frames, None, &holsters);
        assert_eq!(sample.attachments, native.attachments);
        assert_eq!(
            sample.policy,
            if follow_docks {
                TargetPolicy::Native
            } else {
                TargetPolicy::NativeWithoutBody
            }
        );
        assert!(sample.matches_policy(follow_docks));
        assert_provider_and_mirror(&sample, &draws, &others, &frames);
    }
}

#[test]
fn platform_rebase_carries_target_and_native_observations_without_refreshing_age() {
    let source = frames(Mat4::from_translation(Vec3::new(3.0, 0.0, 1.0)));
    let native = native_sample(&source);
    let mut holsters = HolstersConfig::new();
    holsters.follow_docks = false;
    let (sample, _, _) = provide(
        &native,
        &source,
        Some(&body(Quat::from_rotation_x(0.6))),
        &holsters,
    );
    let destination =
        Mat4::from_rotation_translation(Quat::from_rotation_y(1.1), Vec3::new(20.0, 4.0, -7.0));
    let rebased = sample
        .in_world(
            sample.owner,
            destination,
            sample.at + Duration::from_millis(20),
        )
        .unwrap();
    assert_eq!(rebased.at, sample.at);
    let change = destination * source.character.inverse();
    for (before, after) in sample.attachments.iter().zip(&rebased.attachments) {
        assert!(
            after
                .pose
                .unwrap()
                .position
                .abs_diff_eq(change.transform_point3(before.pose.unwrap().position), 1e-5)
        );
        if let Some(before_native) = before.native {
            assert!(after.native.unwrap().pose.unwrap().position.abs_diff_eq(
                change.transform_point3(before_native.pose.unwrap().position),
                1e-5
            ));
        }
    }
    let destination_frames = frames(destination);
    let (draws, others) = candidates_from_sample(&rebased, &destination_frames, &holsters);
    assert_provider_and_mirror(&rebased, &draws, &others, &destination_frames);
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
}
