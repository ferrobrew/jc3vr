use super::*;

#[test]
fn explicit_resets_preserve_the_temporal_watermark() {
    for reason in [
        ResetReason::MissingOrStaleInput,
        ResetReason::BindingUnresolved,
        ResetReason::BindingChanged,
        ResetReason::MaskedObservation,
        ResetReason::ClockDiscontinuity,
    ] {
        {
            let mut history = History::new();
            let accepted = history.step(100, B);
            let Some(SingleEvent::Pressed { press, .. }) = accepted.singles[2] else {
                panic!("B must be owned");
            };
            let watermark = 100;
            let reset = history.arbiter.reset(ResetRequest { reason });
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
