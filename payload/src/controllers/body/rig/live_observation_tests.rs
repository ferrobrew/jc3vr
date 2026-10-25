use super::*;
use crate::controllers::{
    body::rig::root::{owner::RootOwner, test_source, test_source_with_root},
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
    publisher.publish_tracking(Some(located), Some(controllers));

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
    publisher.publish_tracking(Some(located()), Some(controllers(now)));
    let mut mismatch = controllers(now);
    mismatch.display_time_ns += 1;
    publisher.publish_tracking(Some(located()), Some(mismatch));
    let mismatched = publisher.read(now, Duration::ZERO, origin()).raw;
    assert_eq!(
        mismatched.availability,
        RawAvailability::Unavailable(Unavailable::MixedDisplayTimes),
    );
    assert_eq!(mismatched.packet, None);
    publisher.publish_tracking(Some(located()), None);
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
    publisher.publish_tracking(Some(located()), Some(controllers(now)));
    let status = publisher.read(now, Duration::from_millis(10), origin());
    assert!(matches!(
        status.raw.availability,
        RawAvailability::Observed { .. }
    ));
    assert!(status.raw.packet.is_some());
    assert_eq!(
        status.native,
        NativeObservation::unavailable(Unavailable::Stale {
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
    publisher.publish_tracking(Some(located()), Some(controllers(now)));
    publisher.publish_native(native(), now, origin());
    let before = publisher.read(now - Duration::from_nanos(1), Duration::MAX, origin());
    assert_eq!(
        before.raw.availability,
        RawAvailability::Unavailable(Unavailable::FuturePublication)
    );
    assert_eq!(before.raw.packet, None);
    assert_eq!(
        before.native,
        NativeObservation::unavailable(Unavailable::FuturePublication)
    );

    let changed = OriginCounters {
        recenters: 1,
        ..origin()
    };
    let status = publisher.read(now, Duration::MAX, changed);
    assert_eq!(
        status.raw.availability,
        RawAvailability::Observed {
            age: Duration::ZERO
        },
    );
    assert!(status.raw.packet.is_some());
    assert_eq!(
        status.native,
        NativeObservation::unavailable(Unavailable::OriginCountersChanged)
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
        NativeObservation::unavailable(Unavailable::InvalidCapture),
    );
    assert_eq!(
        held.native,
        NativeObservation::observed(first, Duration::ZERO),
    );
    assert_eq!(held.native.capture(), Some(first));
    assert_eq!(
        held.native.availability(),
        NativeAvailability::Observed {
            age: Duration::ZERO
        },
    );
    assert_eq!(
        publisher
            .read(now, Duration::ZERO, origin())
            .native
            .capture(),
        None
    );
}

#[test]
fn repeated_status_reads_preserve_owned_raw_evidence_without_mutating_the_publication() {
    let now = Instant::now();
    let mut publisher = Publisher::new();
    publisher.publish_tracking(Some(located()), Some(controllers(now)));
    let held = publisher.read(now, Duration::ZERO, origin());
    assert_eq!(publisher.read(now, Duration::ZERO, origin()), held);

    publisher.publish_tracking(None, None);
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
    let snapshot = test_source(1, now).1.completion();
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
    assert_eq!(snapshot.completion(), Some(completion));

    let mut publisher = Publisher::new();
    publisher.publish_native(native(), now, origin());
    publisher.publish_tracking(Some(located()), Some(controllers(now)));
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
        NativeObservation::unavailable(Unavailable::OriginCountersChanged)
    );
    assert_eq!(changed.missing, MISSING);
}

#[test]
fn completion_replacement_and_invalidations_do_not_reuse_an_older_record() {
    let now = Instant::now();
    let first = completion(now);
    let held = read_completion(test_source(1, now).1.completion(), now, Duration::ZERO);
    let invalid = test_source_with_root(2, now, Mat4::ZERO).1.completion();
    let replaced = read_completion(invalid, now, Duration::ZERO);
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

#[test]
fn production_publisher_matches_exact_identity_across_independent_times_and_repeated_frames() {
    let completed_at = Instant::now();
    let (token, source) = test_source(7, completed_at);
    let native_at = completed_at + Duration::from_millis(1);
    let frame_at = completed_at + Duration::from_millis(3);
    let mut publisher = Publisher::new();
    let capture = NativeCapture {
        completion: Some(token),
        ..native()
    };
    publisher.publish_native(capture, native_at, origin());
    for frame in 0..3 {
        let now = frame_at + Duration::from_millis(frame);
        publisher.publish_composition(source, now, origin());
        let status =
            publisher.read_completion_status(now, Duration::MAX, origin(), source.completion());
        assert_eq!(
            status.frame_match.availability(),
            FrameMatchAvailability::Matched
        );
        let matched = status.frame_match.matched_source().unwrap();
        assert_eq!(matched.source, source);
        assert_eq!(matched.native, capture);
        assert_eq!(matched.source.current_basis(), source.current_basis());
        assert_eq!(matched.source.shift(), source.shift());
        assert_eq!(status.missing, MISSING_AFTER_MATCH);
        assert!(
            !status
                .missing
                .contains(&MissingEvidence::CompletedLedgerAndShiftPair)
        );
        assert!(
            status
                .missing
                .contains(&MissingEvidence::CompositionBasisPair)
        );
        assert!(status.missing.contains(&MissingEvidence::OwnerAtCompletion));
    }
}

#[test]
fn equal_clocks_missing_tokens_and_new_completions_do_not_match_the_held_frame() {
    let now = Instant::now();
    let (first, source) = test_source(1, now);
    let (second, replacement) = test_source(2, now);
    let mut publisher = Publisher::new();
    publisher.publish_composition(source, now, origin());
    publisher.publish_native(
        NativeCapture {
            completion: Some(second),
            ..native()
        },
        now,
        origin(),
    );
    let mismatch =
        publisher.read_completion_status(now, Duration::ZERO, origin(), source.completion());
    assert_eq!(
        mismatch.frame_match.availability(),
        FrameMatchAvailability::Unavailable(MatchUnavailable::DifferentCompletion)
    );
    assert_eq!(mismatch.frame_match.matched_source(), None);
    publisher.publish_native(native(), now, origin());
    let absent =
        publisher.read_completion_status(now, Duration::ZERO, origin(), source.completion());
    assert_eq!(
        absent.frame_match.availability(),
        FrameMatchAvailability::Unavailable(MatchUnavailable::MissingNativeToken)
    );
    publisher.publish_native(
        NativeCapture {
            completion: Some(first),
            ..native()
        },
        now,
        origin(),
    );
    let replaced =
        publisher.read_completion_status(now, Duration::ZERO, origin(), replacement.completion());
    assert_eq!(
        replaced.frame_match.availability(),
        FrameMatchAvailability::Unavailable(MatchUnavailable::DifferentCompletion)
    );
    publisher.publish_native(
        NativeCapture {
            completion: Some(second),
            ..native()
        },
        now,
        origin(),
    );
    publisher.publish_composition(replacement, now, origin());
    let matched =
        publisher.read_completion_status(now, Duration::ZERO, origin(), replacement.completion());
    assert_eq!(
        matched.frame_match.availability(),
        FrameMatchAvailability::Matched
    );
    assert_eq!(
        matched.frame_match.matched_source().unwrap().source,
        replacement
    );
}

#[test]
fn clears_teleports_and_unavailable_sources_replace_a_previously_matched_result() {
    let now = Instant::now();
    let (token, source) = test_source(1, now);
    let mut publisher = Publisher::new();
    publisher.publish_native(
        NativeCapture {
            completion: Some(token),
            ..native()
        },
        now,
        origin(),
    );
    publisher.publish_composition(source, now, origin());
    let held = publisher.read_completion_status(now, Duration::ZERO, origin(), source.completion());
    assert!(held.frame_match.matched_source().is_some());
    for reason in [
        CompletionUnavailable::Cleared,
        CompletionUnavailable::Teleported,
        CompletionUnavailable::SourceUnavailable,
        CompletionUnavailable::SequenceExhausted,
    ] {
        let invalid = publisher.read_completion_status(
            now,
            Duration::ZERO,
            origin(),
            CompletionSnapshot::unavailable(reason),
        );
        assert_eq!(
            invalid.frame_match.availability(),
            FrameMatchAvailability::Unavailable(MatchUnavailable::Completion(
                CompletionRejection::Source(reason)
            ))
        );
        assert_eq!(invalid.frame_match.matched_source(), None);
        assert_eq!(invalid.missing, MISSING);
    }
    publisher.clear_native();
    let missing =
        publisher.read_completion_status(now, Duration::ZERO, origin(), source.completion());
    assert_eq!(
        missing.frame_match.availability(),
        FrameMatchAvailability::Unavailable(MatchUnavailable::Native(Unavailable::NotCaptured))
    );
    publisher.clear_composition();
    let no_frame =
        publisher.read_completion_status(now, Duration::ZERO, origin(), source.completion());
    assert_eq!(
        no_frame.frame_match.availability(),
        FrameMatchAvailability::Unavailable(MatchUnavailable::NoFrame)
    );
    assert_eq!(no_frame.frame_match.matched_source(), None);
    assert!(held.frame_match.matched_source().is_some());
}

#[test]
fn frame_age_and_origin_counters_reject_without_joining_sources() {
    let now = Instant::now();
    let (token, source) = test_source(1, now);
    let mut publisher = Publisher::new();
    publisher.publish_native(
        NativeCapture {
            completion: Some(token),
            ..native()
        },
        now,
        origin(),
    );
    publisher.publish_composition(source, now, origin());
    let changed = OriginCounters {
        recenters: 1,
        ..origin()
    };
    let invalid =
        publisher.read_completion_status(now, Duration::ZERO, changed, source.completion());
    assert_eq!(
        invalid.frame_match.availability(),
        FrameMatchAvailability::Unavailable(MatchUnavailable::FrameDiagnostic(
            Unavailable::OriginCountersChanged
        ))
    );
    let later = now + Duration::from_millis(2);
    let stale = publisher.read_completion_status(
        later,
        Duration::from_millis(1),
        origin(),
        source.completion(),
    );
    assert_eq!(
        stale.frame_match.availability(),
        FrameMatchAvailability::Unavailable(MatchUnavailable::FrameDiagnostic(
            Unavailable::Stale {
                age: Duration::from_millis(2)
            }
        ))
    );
    assert_eq!(stale.frame_match.matched_source(), None);
}

#[test]
fn tracking_origin_matches_held_sources_without_native_or_latest_global_labels() {
    let now = Instant::now();
    let [first, second] = crate::controllers::body::rig::origin_provenance::test_origins();
    let mut head = located();
    head.origin = first;
    let mut publisher = Publisher::new();
    publisher.publish_tracking(Some(head), Some(controllers(now)));
    let receipt = Some(TrackingReceipt::from_frame(&controllers(now)));
    let source = test_source(1, now).1.with_tracking_origin(first, receipt);
    publisher.publish_composition(source, now, origin());
    let changed_counters = OriginCounters {
        recenters: 100,
        teleports: 100,
    };
    let status = publisher.read(now, Duration::ZERO, changed_counters);
    let OriginEvidence::Known(token) = first else {
        panic!("the source must be known");
    };
    assert_eq!(
        status.tracking_origin,
        TrackingOriginAvailability::SameOrigin(token)
    );
    assert!(status.native.capture().is_none());
    assert!(status.missing.contains(&MissingEvidence::OriginEpoch));
    assert_eq!(
        publisher
            .read(now, Duration::ZERO, changed_counters)
            .tracking_origin,
        status.tracking_origin
    );

    publisher.publish_composition(source.with_tracking_origin(first, None), now, origin());
    assert_eq!(
        publisher
            .read(now, Duration::ZERO, origin())
            .tracking_origin,
        TrackingOriginAvailability::DifferentTrackingPacket
    );
    publisher.publish_composition(source.with_tracking_origin(second, receipt), now, origin());
    assert_eq!(
        publisher
            .read(now, Duration::ZERO, origin())
            .tracking_origin,
        TrackingOriginAvailability::DifferentOrigins
    );
    publisher.publish_composition(
        source.with_tracking_origin(
            OriginEvidence::Unavailable(OriginUnavailable::ReferenceSpacePending),
            receipt,
        ),
        now,
        origin(),
    );
    assert_eq!(
        publisher
            .read(now, Duration::ZERO, origin())
            .tracking_origin,
        TrackingOriginAvailability::Unavailable(OriginUnavailable::ReferenceSpacePending)
    );
    publisher.clear_composition();
    assert_eq!(
        publisher
            .read(now, Duration::ZERO, origin())
            .tracking_origin,
        TrackingOriginAvailability::Unavailable(OriginUnavailable::SourceAbsent)
    );
}

#[test]
fn absent_origin_is_retained_as_unknown_without_dropping_raw_tracking() {
    let now = Instant::now();
    let mut head = located();
    head.origin = OriginEvidence::Unavailable(OriginUnavailable::ChangedDuringAcquisition);
    let packet = make_raw_packet(Some(head), Some(controllers(now))).unwrap();
    assert_eq!(
        packet.origin,
        OriginEvidence::Unavailable(OriginUnavailable::ChangedDuringAcquisition)
    );
    let source = test_source(1, now).1;
    assert_eq!(
        source.tracking_origin(),
        OriginEvidence::Unavailable(OriginUnavailable::SourceAbsent)
    );
    assert_eq!(
        match_tracking_origins(packet.origin, source.tracking_origin()),
        TrackingOriginAvailability::Unavailable(OriginUnavailable::ChangedDuringAcquisition)
    );
}

fn completion(now: Instant) -> MovementCompletion {
    test_source(1, now).1.completion().completion().unwrap()
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
        origin: OriginEvidence::Unavailable(OriginUnavailable::SourceAbsent),
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
        completion: None,
        previous_root: Mat4::IDENTITY,
        current_root: Mat4::from_translation(Vec3::X),
        previous_anchor: Vec3::Y,
        current_anchor: Vec3::X + Vec3::Y,
        current_head_model: Vec3::Y,
        current_neck_model: Vec3::Y * 0.9,
        current_eye_mid_model: Vec3::Y,
    }
}
