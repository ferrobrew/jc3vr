use super::*;

#[test]
fn ordinary_observed_and_explicit_resets_preserve_the_temporal_watermark() {
    for reason in [
        ResetReason::FocusLost,
        ResetReason::TrackingLost,
        ResetReason::MissingOrStaleInput,
        ResetReason::ControllerDisabled,
        ResetReason::SchemeChanged,
        ResetReason::BindingUnresolved,
        ResetReason::BindingChanged,
        ResetReason::MaskedObservation,
        ResetReason::ClockDiscontinuity,
    ] {
        for observed in [false, true] {
            let mut history = History::new();
            let accepted = history.step(100, B);
            let Some(SingleEvent::Pressed { press, .. }) = accepted.singles[2] else {
                panic!("B must be owned");
            };
            let watermark = if observed { 110 } else { 100 };
            let reset = if observed {
                history.evidence(110, B, InputEvidence::Unavailable(reason))
            } else {
                history.arbiter.reset(ResetRequest { reason })
            };
            assert_eq!(reset.status, ObservationStatus::Reset(reason));
            assert_eq!(
                reset.singles[2],
                Some(SingleEvent::Released {
                    press,
                    at: ms(watermark)
                })
            );
            let stale = history.step(watermark, UP);
            assert_eq!(
                stale.status,
                ObservationStatus::Reset(ResetReason::InvalidHistory)
            );
            no_singles(&stale);
            let held = history.step(watermark + 1, B);
            assert_eq!(held.status, ObservationStatus::AwaitingNeutral);
            no_singles(&held);
            let recovered = history.step(watermark + 2, UP);
            assert_eq!(recovered.status, ObservationStatus::Recovered);
            no_singles(&recovered);
            assert!(history.step(watermark + 3, B).sources[2].single_down);
        }
    }
}

#[test]
fn validated_binding_reset_preserves_its_stamp_until_newer_neutral() {
    let mut history = History::new();
    let accepted = history.step(100, B);
    let Some(SingleEvent::Pressed { press, .. }) = accepted.singles[2] else {
        panic!("B must be owned");
    };
    history.generation = BindingGeneration(2);
    let changed = history.step(110, B);
    assert_eq!(
        changed.status,
        ObservationStatus::Reset(ResetReason::BindingChanged)
    );
    assert_eq!(
        changed.singles[2],
        Some(SingleEvent::Released { press, at: ms(110) })
    );
    for at in [60, 100, 110] {
        let stale = history.step(at, UP);
        assert_eq!(
            stale.status,
            ObservationStatus::Reset(ResetReason::InvalidHistory)
        );
        no_singles(&stale);
    }
    assert_eq!(
        history.step(111, B).status,
        ObservationStatus::AwaitingNeutral
    );
    assert_eq!(history.step(112, UP).status, ObservationStatus::Recovered);
    assert!(history.step(113, B).sources[2].single_down);
}

#[test]
fn clock_epoch_replacement_is_explicit_and_rejects_old_queued_history() {
    let mut history = History::new();
    let accepted = history.step(100, B);
    let Some(SingleEvent::Pressed { press, .. }) = accepted.singles[2] else {
        panic!("B must be owned");
    };
    let changed = history.arbiter.begin_clock_epoch(ClockEpochRequest {
        next: ClockEpoch(2),
    });
    assert_eq!(
        changed.status,
        ObservationStatus::Reset(ResetReason::ClockDiscontinuity)
    );
    assert_eq!(
        changed.singles[2],
        Some(SingleEvent::Released { press, at: ms(100) })
    );
    let new_stamp = ObservationStamp {
        epoch: ClockEpoch(2),
        at: ms(1),
        sync: 1,
        sequence: 1,
    };
    let held = history.stamped(new_stamp, B, InputEvidence::VerifiedUnmasked);
    assert_eq!(held.status, ObservationStatus::AwaitingNeutral);
    no_singles(&held);
    let old = history.stamped(
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(1000),
            sync: 100,
            sequence: 100,
        },
        UP,
        InputEvidence::VerifiedUnmasked,
    );
    assert_eq!(
        old.status,
        ObservationStatus::Reset(ResetReason::InvalidHistory)
    );
    no_singles(&old);
    let duplicate = history.stamped(new_stamp, UP, InputEvidence::VerifiedUnmasked);
    assert_eq!(
        duplicate.status,
        ObservationStatus::Reset(ResetReason::InvalidHistory)
    );
    for next in [ClockEpoch(1), ClockEpoch(2)] {
        let rejected = history
            .arbiter
            .begin_clock_epoch(ClockEpochRequest { next });
        assert_eq!(
            rejected.status,
            ObservationStatus::Reset(ResetReason::InvalidHistory)
        );
        no_singles(&rejected);
    }
    history.epoch = ClockEpoch(2);
    history.sequence = 1;
    assert_eq!(history.step(2, UP).status, ObservationStatus::Recovered);
    let fresh = history.step(3, B);
    assert!(
        matches!(fresh.singles[2], Some(SingleEvent::Pressed { press, accepted_at }) if press.onset.epoch == ClockEpoch(2) && accepted_at == ms(3))
    );
}
