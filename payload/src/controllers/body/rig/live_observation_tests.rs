use glam::Vec2;

use super::*;
use crate::controllers::{
    body::rig::root::{
        ledger::{Movement, Requests},
        owner::RootOwner,
    },
    frame::ActionSetKind,
};

#[test]
fn raw_packet_retains_actual_receipt_and_independent_hand_validity() {
    let now = Instant::now();
    let mut controllers = controllers(now);
    controllers.hands[0].active = true;
    controllers.hands[0].grip.position_valid = true;
    controllers.hands[0].grip.pose.position = Vec3::X;
    let mut located = located();
    located.flags = xr::ViewStateFlags::POSITION_VALID;
    let mut publisher = Publisher::new();
    publisher.publish_tracking(Some(located), Some(controllers), origin());

    let status = publisher.read(now, Duration::ZERO, origin());
    let RawAvailability::Observed { age } = status.raw.availability else {
        panic!("the actual live receipt must be observable");
    };
    let packet = status
        .raw
        .packet
        .expect("observed tracking retains its owned packet");
    assert_eq!(packet.sequence, 17);
    assert_eq!(packet.display_time_ns, 400);
    assert_eq!(packet.published_at, now);
    assert_eq!(age, Duration::ZERO);
    assert!(packet.head.position_valid);
    assert!(!packet.head.orientation_valid);
    assert!(!packet.head.position_tracked);
    assert_eq!(packet.grips[0].unwrap().pose.position, Vec3::X);
    assert!(!packet.grips[0].unwrap().orientation_valid);
    assert_eq!(packet.grips[1], None);
    assert_eq!(status.missing, MISSING);
}

#[test]
fn mismatched_or_missing_live_source_replaces_the_previous_packet() {
    let now = Instant::now();
    let mut publisher = Publisher::new();
    publisher.publish_tracking(Some(located()), Some(controllers(now)), origin());
    let mut mismatch = controllers(now);
    mismatch.display_time_ns += 1;
    publisher.publish_tracking(Some(located()), Some(mismatch), origin());
    let mismatched = publisher.read(now, Duration::ZERO, origin()).raw;
    assert_eq!(
        mismatched.availability,
        RawAvailability::Unavailable(Unavailable::MixedDisplayTimes),
    );
    assert_eq!(mismatched.packet, None);
    publisher.publish_tracking(Some(located()), None, origin());
    let missing = publisher.read(now, Duration::ZERO, origin()).raw;
    assert_eq!(
        missing.availability,
        RawAvailability::Unavailable(Unavailable::NoLiveTracking),
    );
    assert_eq!(missing.packet, None);
}

#[test]
fn independent_source_ages_do_not_create_a_temporal_pair() {
    let now = Instant::now();
    let earlier = now - Duration::from_millis(20);
    let mut publisher = Publisher::new();
    publisher.publish_native(native(), earlier, origin());
    publisher.publish_tracking(Some(located()), Some(controllers(now)), origin());
    let status = publisher.read(now, Duration::from_millis(10), origin());
    assert!(matches!(
        status.raw.availability,
        RawAvailability::Observed { .. }
    ));
    assert!(status.raw.packet.is_some());
    assert_eq!(
        status.native,
        NativeObservation::Unavailable(Unavailable::Stale {
            age: Duration::from_millis(20),
        }),
    );
    assert!(
        status
            .missing
            .contains(&MissingEvidence::CompletedLedgerAndShiftPair)
    );
    assert!(
        status
            .missing
            .contains(&MissingEvidence::PreviousNativePose)
    );
    assert!(
        status
            .missing
            .contains(&MissingEvidence::CalibrationAndScaleEpochs)
    );
}

#[test]
fn future_publications_and_changed_origin_counters_are_unavailable() {
    let now = Instant::now();
    let mut publisher = Publisher::new();
    publisher.publish_tracking(Some(located()), Some(controllers(now)), origin());
    publisher.publish_native(native(), now, origin());
    let before = publisher.read(now - Duration::from_nanos(1), Duration::MAX, origin());
    assert_eq!(
        before.raw.availability,
        RawAvailability::Unavailable(Unavailable::FuturePublication)
    );
    assert_eq!(before.raw.packet, None);
    assert_eq!(
        before.native,
        NativeObservation::Unavailable(Unavailable::FuturePublication)
    );

    let changed = OriginCounters {
        recenters: 1,
        ..origin()
    };
    let status = publisher.read(now, Duration::MAX, changed);
    assert_eq!(
        status.raw.availability,
        RawAvailability::Unavailable(Unavailable::OriginCountersChanged)
    );
    assert_eq!(status.raw.packet, None);
    assert_eq!(
        status.native,
        NativeObservation::Unavailable(Unavailable::OriginCountersChanged)
    );
    assert!(status.missing.contains(&MissingEvidence::OriginEpoch));
}

#[test]
fn latest_native_capture_is_owned_and_invalid_capture_does_not_reuse_the_old_one() {
    let now = Instant::now();
    let mut publisher = Publisher::new();
    let first = native();
    publisher.publish_native(first, now, origin());
    let held = publisher.read(now, Duration::ZERO, origin());
    let mut invalid = first;
    invalid.current_root = Mat4::ZERO;
    publisher.publish_native(invalid, now, origin());
    assert_eq!(
        publisher.read(now, Duration::ZERO, origin()).native,
        NativeObservation::Unavailable(Unavailable::InvalidCapture),
    );
    assert_eq!(
        held.native,
        NativeObservation::Observed {
            capture: first,
            age: Duration::ZERO
        },
    );
}

#[test]
fn repeated_status_reads_preserve_owned_raw_evidence_without_mutating_the_publication() {
    let now = Instant::now();
    let mut publisher = Publisher::new();
    publisher.publish_tracking(Some(located()), Some(controllers(now)), origin());
    let held = publisher.read(now, Duration::ZERO, origin());
    assert_eq!(publisher.read(now, Duration::ZERO, origin()), held);

    publisher.publish_tracking(None, None, origin());
    assert_eq!(held.raw.packet.unwrap().sequence, 17);
    assert_eq!(
        held.raw.availability,
        RawAvailability::Observed {
            age: Duration::ZERO
        },
    );
    let unavailable = publisher.read(now, Duration::ZERO, origin());
    assert_eq!(
        unavailable.raw.availability,
        RawAvailability::Unavailable(Unavailable::NoLiveTracking),
    );
    assert_eq!(unavailable.raw.packet, None);
}

#[test]
fn completion_freshness_is_read_only_and_does_not_join_equal_host_times() {
    let now = Instant::now();
    let completion = completion(now);
    let snapshot = CompletionSnapshot::completed(completion);
    let observed = read_completion(snapshot, now, Duration::ZERO);
    assert_eq!(
        observed.availability(),
        CompletionAvailability::Observed {
            age: Duration::ZERO
        },
    );
    assert_eq!(observed.completion(), Some(completion));
    assert_eq!(
        observed,
        CompletionObservation::observed(completion, Duration::ZERO)
    );
    let future = read_completion(snapshot, now - Duration::from_nanos(1), Duration::MAX);
    assert_eq!(
        future,
        CompletionObservation::unavailable(CompletionRejection::Diagnostic(
            Unavailable::FuturePublication,
        )),
    );
    assert_eq!(future.completion(), None);
    let stale = read_completion(
        snapshot,
        now + Duration::from_millis(2),
        Duration::from_millis(1),
    );
    assert_eq!(
        stale.availability(),
        CompletionAvailability::Unavailable(CompletionRejection::Diagnostic(Unavailable::Stale {
            age: Duration::from_millis(2),
        })),
    );
    assert_eq!(stale.completion(), None);
    assert_eq!(read_completion(snapshot, now, Duration::ZERO), observed);
    assert_eq!(snapshot, CompletionSnapshot::completed(completion));

    let mut publisher = Publisher::new();
    publisher.publish_native(native(), now, origin());
    publisher.publish_tracking(Some(located()), Some(controllers(now)), origin());
    let status = publisher.read_completion_status(now, Duration::ZERO, origin(), snapshot);
    assert_eq!(status.missing, MISSING);
    assert_eq!(status.completion, observed);
    assert!(
        status
            .missing
            .contains(&MissingEvidence::CompletedLedgerAndShiftPair)
    );
    assert!(status.missing.contains(&MissingEvidence::OwnerAtCompletion));
    assert!(
        status
            .missing
            .contains(&MissingEvidence::CompositionBasisPair)
    );
    let changed_origin = OriginCounters {
        recenters: 1,
        ..origin()
    };
    let changed = publisher.read_completion_status(now, Duration::ZERO, changed_origin, snapshot);
    assert_eq!(changed.completion, observed);
    assert_eq!(
        changed.native,
        NativeObservation::Unavailable(Unavailable::OriginCountersChanged)
    );
    assert_eq!(changed.missing, MISSING);
}

#[test]
fn completion_replacement_and_invalidations_do_not_reuse_an_older_record() {
    let now = Instant::now();
    let first = completion(now);
    let held = read_completion(CompletionSnapshot::completed(first), now, Duration::ZERO);
    let mut invalid = first;
    invalid.sequence += 1;
    invalid.current_root = Mat4::ZERO;
    let replaced = read_completion(CompletionSnapshot::completed(invalid), now, Duration::ZERO);
    assert_eq!(
        replaced,
        CompletionObservation::unavailable(CompletionRejection::Diagnostic(
            Unavailable::InvalidCapture,
        )),
    );
    assert_eq!(replaced.completion(), None);
    for reason in [
        CompletionUnavailable::NotCompleted,
        CompletionUnavailable::Cleared,
        CompletionUnavailable::Teleported,
        CompletionUnavailable::SourceUnavailable,
        CompletionUnavailable::SequenceExhausted,
    ] {
        let unavailable =
            read_completion(CompletionSnapshot::unavailable(reason), now, Duration::ZERO);
        assert_eq!(
            unavailable.availability(),
            CompletionAvailability::Unavailable(CompletionRejection::Source(reason)),
        );
        assert_eq!(unavailable.completion(), None);
    }
    assert_eq!(held, CompletionObservation::observed(first, Duration::ZERO));
    assert_eq!(held.completion(), Some(first));
    assert_eq!(first.ledger.request.unwrap().owner, Some(RootOwner::Player));
}

fn completion(now: Instant) -> MovementCompletion {
    let mut requests = Requests::new();
    requests.submit_owned(Movement::new(Vec2::ZERO, Vec2::X, 1.0), RootOwner::Player);
    let ledger = requests.complete(Vec3::ZERO, Vec3::X);
    MovementCompletion {
        sequence: 1,
        completed_at: now,
        previous_root: Mat4::IDENTITY,
        current_root: Mat4::from_translation(Vec3::X),
        ledger,
    }
}

fn origin() -> OriginCounters {
    OriginCounters {
        teleports: 0,
        recenters: 0,
    }
}

fn located() -> LocatedTracking {
    LocatedTracking {
        head: xr::Posef::IDENTITY,
        flags: xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID,
        display_time_ns: 400,
        cockpit_from_local: xr::Posef::IDENTITY,
    }
}

fn controllers(now: Instant) -> ControllerFrame {
    ControllerFrame {
        sequence: 17,
        display_time_ns: 400,
        published_at: now,
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    }
}

fn native() -> NativeCapture {
    NativeCapture {
        previous_root: Mat4::IDENTITY,
        current_root: Mat4::from_translation(Vec3::X),
        previous_anchor: Vec3::Y,
        current_anchor: Vec3::X + Vec3::Y,
        current_head_model: Vec3::Y,
        current_neck_model: Vec3::Y * 0.9,
        current_eye_mid_model: Vec3::Y,
    }
}
