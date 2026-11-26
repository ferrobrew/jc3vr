use jc3gi::{
    animation::skeleton::{HkaSkeleton, Skeleton},
    character::character::AnimationController,
};

use super::*;

pub(crate) fn identity_fixture() -> Identity {
    let mut havok: HkaSkeleton = unsafe { std::mem::zeroed() };
    havok.m_Bones.size = 5;
    let mut skeleton: Skeleton = unsafe { std::mem::zeroed() };
    skeleton.m_HavokSkeleton = &mut havok;
    let mut controller: AnimationController = unsafe { std::mem::zeroed() };
    controller.m_Skeleton = &mut skeleton;
    Identity {
        character: 1,
        skeleton: unsafe { SkeletonKey::of(&controller) }.unwrap(),
        producer: 2,
        bones: [1, 2, 3, 4],
    }
}

pub(crate) fn context() -> Context {
    Context {
        source: Source::Sim,
        gameplay: true,
        teleport: 0,
        recenter: 0,
        world_scale: 1.0,
        tracking: None,
    }
}

pub(crate) fn sample(step: f32) -> Sample {
    rooted(step, Mat4::IDENTITY)
}

pub(crate) fn rooted(step: f32, root: Mat4) -> Sample {
    Sample::animated(
        root,
        [
            Vec3::new(0.02 * step, 1.7 + 0.1 * step, 0.0),
            Vec3::new(0.0, 1.5 + 0.04 * step, 0.01 * step),
            Vec3::new(0.01 * step, 1.72 + 0.08 * step, -0.1 - 0.03 * step),
        ],
        [
            Quat::from_rotation_x(0.1 * step),
            Quat::from_rotation_y(0.2 * step),
        ],
    )
}

pub(crate) fn request(
    identity: Identity,
    tick: u64,
    serial: u64,
    previous_root: Mat4,
    current_root: Mat4,
) -> Capture {
    Capture {
        identity,
        tick,
        serial: Some(serial),
        context: context(),
        previous_root,
        current_root,
    }
}

#[test]
fn unchanged_publication_never_rereads_even_when_dispatch_advances() {
    let mut history = History::new();
    let id = identity_fixture();
    let first = history
        .capture(request(id, 10, 1, Mat4::IDENTITY, Mat4::IDENTITY), || {
            Some(sample(0.0))
        })
        .unwrap();
    assert_eq!(first.previous, first.current);
    let pair = history
        .capture(request(id, 11, 2, Mat4::IDENTITY, Mat4::IDENTITY), || {
            Some(sample(1.0))
        })
        .unwrap();
    assert_eq!(pair.previous, sample(0.0));
    assert_ne!(
        pair.previous.geometry.neck_delta,
        pair.current.geometry.neck_delta
    );
    assert_ne!(
        pair.previous.geometry.eye_arm,
        pair.current.geometry.eye_arm
    );
    for tick in [11, 12, 30] {
        assert_eq!(
            history.capture(
                request(id, tick, 2, Mat4::IDENTITY, Mat4::IDENTITY),
                || panic!("reused publication rereads joints or advances the stabilizer")
            ),
            Some(pair)
        );
    }
}

#[test]
fn captured_roots_stay_with_their_joints_without_double_interpolation() {
    let mut history = History::new();
    let id = identity_fixture();
    let first = rooted(
        0.0,
        Mat4::from_rotation_translation(Quat::from_rotation_x(0.3), Vec3::new(3.0, 2.0, -1.0)),
    );
    history
        .capture(request(id, 10, 1, first.root, first.root), || Some(first))
        .unwrap();
    let second = rooted(
        1.0,
        Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(4.0, 2.3, -2.0)),
    );
    let pair = history
        .capture(request(id, 11, 2, first.root, second.root), || Some(second))
        .unwrap();
    assert!(!pair.collapsed);
    assert_eq!(pair.previous, first);
    assert_eq!(pair.current, second);
    assert_eq!(
        pair.current.geometry.head,
        second.root.transform_point3(second.head_model)
    );
    assert_ne!(
        pair.previous.geometry.head,
        first.root.transform_point3(second.head_model)
    );
}

#[test]
fn discontinuities_and_replacements_collapse_only_with_a_fresh_publication() {
    let id = identity_fixture();
    for kind in 0..9 {
        let mut history = History::new();
        history
            .capture(request(id, 10, 1, Mat4::IDENTITY, Mat4::IDENTITY), || {
                Some(sample(0.0))
            })
            .unwrap();
        let mut next = request(id, 11, 2, Mat4::IDENTITY, Mat4::IDENTITY);
        match kind {
            0 => next.context.teleport += 1,
            1 => next.context.recenter += 1,
            2 => next.identity.character += 1,
            3 => next.identity.producer += 1,
            4 => next.identity.bones.swap(2, 3),
            5 => next.tick += 1,
            6 => next.previous_root = Mat4::from_translation(Vec3::X),
            7 => next.context.world_scale = 2.0,
            _ => next.context.gameplay = false,
        }
        let pair = history.capture(next, || Some(sample(1.0))).unwrap();
        assert!(pair.collapsed, "case {kind}");
        assert_eq!(pair.previous, pair.current);
    }
}

#[test]
fn rejected_complete_sample_consumes_receipt_and_recovery_requires_new_publication() {
    let mut history = History::new();
    let id = identity_fixture();
    history
        .capture(request(id, 10, 1, Mat4::IDENTITY, Mat4::IDENTITY), || {
            Some(sample(0.0))
        })
        .unwrap();
    assert!(
        history
            .capture(request(id, 11, 2, Mat4::IDENTITY, Mat4::IDENTITY), || {
                let mut invalid = sample(1.0);
                invalid.geometry.eye_arm = Vec3::NAN;
                Some(invalid)
            })
            .is_none()
    );
    assert!(
        history
            .capture(
                request(id, 12, 2, Mat4::IDENTITY, Mat4::IDENTITY),
                || panic!("failed receipt reused")
            )
            .is_none()
    );
    let pair = history
        .capture(request(id, 12, 3, Mat4::IDENTITY, Mat4::IDENTITY), || {
            Some(sample(2.0))
        })
        .unwrap();
    assert!(pair.collapsed);
    history.clear();
    assert!(
        history
            .capture(
                request(id, 13, 3, Mat4::IDENTITY, Mat4::IDENTITY),
                || panic!("clear must not unconsume receipt")
            )
            .is_none()
    );
}

#[test]
fn reused_receipt_root_or_context_changes_withdraw_without_native_capture() {
    let id = identity_fixture();
    for kind in 0..5 {
        let mut history = History::new();
        history
            .capture(request(id, 10, 1, Mat4::IDENTITY, Mat4::IDENTITY), || {
                Some(sample(0.0))
            })
            .unwrap();
        let mut changed = request(id, 10, 1, Mat4::IDENTITY, Mat4::IDENTITY);
        match kind {
            0 => changed.current_root = Mat4::from_translation(Vec3::X),
            1 => changed.previous_root = Mat4::from_rotation_x(0.2),
            2 => changed.context.teleport += 1,
            3 => changed.context.recenter += 1,
            _ => changed.context.source = Source::Vr,
        }
        let next_root = changed.current_root;
        assert!(
            history
                .capture(changed, || panic!(
                    "incoherent reuse reads overridden joints"
                ))
                .is_none()
        );
        let fresh = history
            .capture(request(id, 11, 2, Mat4::IDENTITY, next_root), || {
                Some(rooted(1.0, next_root))
            })
            .unwrap();
        assert!(fresh.collapsed);
    }
}

#[test]
fn no_publication_and_regressed_serial_never_read() {
    let id = identity_fixture();
    let mut history = History::new();
    let mut missing = request(id, 1, 1, Mat4::IDENTITY, Mat4::IDENTITY);
    missing.serial = None;
    assert!(
        history
            .capture(missing, || panic!("time-only path reads joints"))
            .is_none()
    );
    history
        .capture(request(id, 2, 3, Mat4::IDENTITY, Mat4::IDENTITY), || {
            Some(sample(0.0))
        })
        .unwrap();
    assert!(
        history
            .capture(
                request(id, 3, 2, Mat4::IDENTITY, Mat4::IDENTITY),
                || panic!("regressed receipt reads joints")
            )
            .is_none()
    );
}

#[test]
fn vr_tracking_discontinuities_collapse_new_publications() {
    let id = identity_fixture();
    let mut initial = context();
    initial.source = Source::Vr;
    initial.tracking = Some(Publication {
        sequence: 10,
        display_time_ns: 100,
        published_at: Instant::now(),
        origin: crate::controllers::body::rig::origin_provenance::begin(Ok(())),
    });
    for case in 0..4 {
        let mut history = History::new();
        let mut first = request(id, 10, 1, Mat4::IDENTITY, Mat4::IDENTITY);
        first.context = initial;
        history.capture(first, || Some(sample(0.0))).unwrap();
        let mut next = request(id, 11, 2, Mat4::IDENTITY, Mat4::IDENTITY);
        next.context = initial;
        match case {
            0 => next.context.tracking = None,
            1 => next.context.tracking.as_mut().unwrap().sequence -= 1,
            2 => next.context.tracking.as_mut().unwrap().display_time_ns -= 1,
            _ => next.context.tracking.as_mut().unwrap().origin = OriginEvidence::Unavailable(crate::controllers::body::rig::origin_provenance::OriginUnavailable::ChangedDuringAcquisition),
        }
        let pair = history.capture(next, || Some(sample(1.0))).unwrap();
        assert!(pair.collapsed, "case {case}");
    }
}
