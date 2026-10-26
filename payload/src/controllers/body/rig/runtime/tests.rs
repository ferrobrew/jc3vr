use super::*;
use crate::controllers::body::{
    procedural::{
        rig::{ArmRig, RigBone},
        skeleton::Transform,
        solver::{constrained::realize, pose::RigPose},
    },
    rig::estimate::{ContactState, MAX_COMPUTED_EVALUATIONS},
};

fn rig() -> Rig {
    let mut reference = [Transform {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    }; RigBone::COUNT];
    let mut set = |bone: RigBone, point| reference[bone.index()].translation = point;
    set(RigBone::Hips, Vec3::new(0.0, 0.93, 0.0));
    set(RigBone::Pelvis, Vec3::new(0.0, 0.93, 0.0));
    set(RigBone::Spine, Vec3::new(0.0, 1.02, 0.0));
    set(RigBone::Spine1, Vec3::new(0.0, 1.15, 0.0));
    set(RigBone::Spine2, Vec3::new(0.0, 1.28, 0.0));
    set(RigBone::Sternum, Vec3::new(0.0, 1.40, 0.0));
    set(RigBone::LeftUpLeg, Vec3::new(-0.09, 0.93, 0.0));
    set(RigBone::RightUpLeg, Vec3::new(0.09, 0.93, 0.0));
    set(RigBone::LeftLeg, Vec3::new(-0.09, 0.48, -0.02));
    set(RigBone::RightLeg, Vec3::new(0.09, 0.48, -0.02));
    set(RigBone::LeftFoot, Vec3::new(-0.09, 0.03, 0.0));
    set(RigBone::RightFoot, Vec3::new(0.09, 0.03, 0.0));
    set(RigBone::LeftToeBase, Vec3::new(-0.09, 0.01, -0.12));
    set(RigBone::RightToeBase, Vec3::new(0.09, 0.01, -0.12));
    set(RigBone::LeftShoulder, Vec3::new(-0.20, 1.44, 0.0));
    set(RigBone::RightShoulder, Vec3::new(0.20, 1.44, 0.0));
    set(RigBone::Neck, Vec3::new(0.0, 1.555, 0.0));
    set(RigBone::Head, Vec3::new(0.0, 1.63, -0.085));
    Rig {
        bones: [0; RigBone::COUNT],
        reference,
        arms: [ArmRig {
            shoulder_local: Vec3::ZERO,
            length: 0.65,
        }; 2],
    }
}

fn tracking(now: Instant, sequence: u64) -> Tracking {
    Tracking {
        head: TrackedPose {
            pose: Pose {
                position: Vec3::new(0.0, 1.63 - BodyDimensions::GENERIC_ADULT.eye_height, -0.085),
                orientation: Quat::IDENTITY,
            },
            position_valid: true,
            orientation_valid: true,
            ..TrackedPose::default()
        },
        grips: [None; 2],
        sequence,
        display_time_ns: sequence as i64 * 33_000_000,
        published_at: now,
        focused: true,
    }
}

fn input(rig: &Rig, now: Instant) -> GateInput<'_> {
    GateInput {
        rig,
        player: BodyDimensions::GENERIC_ADULT,
        previous_root: Mat4::IDENTITY,
        root: Mat4::IDENTITY,
        head_world: Vec3::new(0.0, 1.63, -0.085),
        view: Quat::IDENTITY,
        world_scale: 1.0,
        origin: 0,
        now,
        dt: 1.0 / 30.0,
    }
}

#[test]
fn committed_fit_reaches_the_pose_consumer_and_both_consumers_share_the_step() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    let fit = runtime
        .commit(Some(tracking(now, 1)), input(&rig, now))
        .unwrap();
    assert_eq!(runtime.status.step, fit.provenance.current.step);
    assert_eq!(runtime.status.fallback, None);
    let observed = runtime.estimator.snapshot().unwrap();
    assert_eq!(observed.provenance.sample.sequence, 1);
    assert!(fit.body.neck.distance(Vec3::new(0.0, 1.555, 0.0)) < 0.15);
    assert!(runtime.status.evaluations <= MAX_COMPUTED_EVALUATIONS);
    assert!(
        fit.contacts
            .iter()
            .all(|contact| matches!(contact, ContactState::Released(_)))
    );
    let animated = RigPose {
        bones: rig.reference,
    };
    let pose = realize(&rig, &animated, &fit, Mat4::IDENTITY, Quat::IDENTITY).unwrap();
    assert!(
        pose.position(RigBone::Hips)
            .abs_diff_eq(fit.body.pelvis, 1.0e-5)
    );
    assert!(pose.is_finite());
    let mut accepted = runtime.accepted.unwrap();
    accepted.0.root.displacement = Vec3::new(0.08, 0.0, 0.0);
    runtime.accepted = Some(accepted);
    let velocity = runtime.take_root(now, 1.0 / 30.0, Mat4::IDENTITY, true);
    assert!(velocity.x > 0.0 && velocity.length() <= 1.5);
    assert_eq!(runtime.consumed, fit.provenance.current.step);
    assert_eq!(
        runtime.take_root(now, 1.0 / 30.0, Mat4::IDENTITY, true),
        Vec2::ZERO
    );
}

#[test]
fn disabled_root_consumes_fit_request_without_replaying_when_reenabled() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    let fit = runtime
        .commit(Some(tracking(now, 1)), input(&rig, now))
        .unwrap();
    let mut accepted = runtime.accepted.unwrap();
    accepted.0.root.displacement = Vec3::new(0.08, 0.0, 0.0);
    runtime.accepted = Some(accepted);

    assert_eq!(
        runtime.take_root(now, 1.0 / 30.0, Mat4::IDENTITY, false),
        Vec2::ZERO
    );
    assert_eq!(runtime.consumed, fit.provenance.current.step);
    assert_eq!(
        runtime.take_root(now, 1.0 / 30.0, Mat4::IDENTITY, true),
        Vec2::ZERO
    );
}

#[test]
fn stale_or_invalid_tracking_withdraws_pose_and_root_without_mutating_inputs() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    let mut packet = tracking(now, 1);
    let original = packet.head;
    runtime.commit(Some(packet), input(&rig, now)).unwrap();
    let later = now + MAX_AGE + Duration::from_millis(1);
    assert_eq!(
        runtime.take_root(later, 0.03, Mat4::IDENTITY, true),
        Vec2::ZERO
    );
    packet.sequence += 1;
    packet.head.position_valid = false;
    assert!(runtime.commit(Some(packet), input(&rig, later)).is_none());
    assert!(matches!(
        runtime.status.fallback,
        Some(Fallback::Rejected(_))
    ));
    assert!(runtime.accepted.is_none());
    assert_eq!(original.pose, packet.head.pose);
    assert_eq!(
        runtime.take_root(later, 0.03, Mat4::IDENTITY, true),
        Vec2::ZERO
    );
}

#[test]
fn owner_handback_requires_a_new_commit_and_does_not_reuse_a_root_request() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    runtime
        .commit(Some(tracking(now, 1)), input(&rig, now))
        .unwrap();
    runtime.withdraw(Fallback::Owner);
    assert!(runtime.fresh(now).is_none());
    assert_eq!(
        runtime.take_root(now, 0.03, Mat4::IDENTITY, true),
        Vec2::ZERO
    );
    let later = now + Duration::from_millis(33);
    let next = runtime
        .commit(Some(tracking(later, 2)), input(&rig, later))
        .unwrap();
    assert_eq!(next.provenance.transition, Transition::Reseed);
    assert_eq!(next.provenance.current.step, 2);
}

#[test]
fn changed_dimensions_reseed_and_missing_packets_clear_authority() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    runtime
        .commit(Some(tracking(now, 1)), input(&rig, now))
        .unwrap();
    let later = now + Duration::from_millis(33);
    let mut changed = input(&rig, later);
    changed.player.shoulder_width *= 0.98;
    let fit = runtime.commit(Some(tracking(later, 2)), changed).unwrap();
    assert_eq!(fit.provenance.transition, Transition::Reseed);
    assert!(runtime.commit(None, input(&rig, later)).is_none());
    assert_eq!(runtime.status.fallback, Some(Fallback::TrackingUnavailable));
    assert_eq!(
        runtime.take_root(later, 0.03, Mat4::IDENTITY, true),
        Vec2::ZERO
    );
}

#[test]
fn changed_teleport_origin_reseeds_without_crediting_root_travel() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    runtime
        .commit(Some(tracking(now, 1)), input(&rig, now))
        .unwrap();
    let later = now + Duration::from_millis(33);
    let mut teleported = input(&rig, later);
    teleported.origin = 1;
    teleported.root = Mat4::from_translation(Vec3::new(8.0, 0.0, -5.0));
    teleported.head_world += teleported.root.w_axis.truncate();
    let fit = runtime
        .commit(Some(tracking(later, 2)), teleported)
        .unwrap();
    assert_eq!(fit.provenance.transition, Transition::Teleport);
    assert_eq!(fit.provenance.previous, fit.provenance.current);
    assert_eq!(fit.gait.completed_across_support, Vec3::ZERO);
    assert_eq!(fit.gait.distance, 0.0);
    assert!(
        runtime
            .estimator
            .rates_for_test()
            .into_iter()
            .all(|rate| rate == 0.0)
    );
}

#[test]
fn continuing_gate_commits_preserve_the_temporal_stream() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    runtime
        .commit(Some(tracking(now, 1)), input(&rig, now))
        .unwrap();
    let later = now + Duration::from_millis(33);
    let fit = runtime
        .commit(Some(tracking(later, 2)), input(&rig, later))
        .unwrap();
    assert_eq!(fit.provenance.transition, Transition::Continue);
    assert_eq!(fit.provenance.previous.step, 1);
    assert_eq!(fit.provenance.current.step, 2);
}
