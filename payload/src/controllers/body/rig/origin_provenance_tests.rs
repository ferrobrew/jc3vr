use super::*;

#[test]
fn unchanged_source_preserves_the_acquired_identity() {
    let source = Source::new();
    let acquired = source.capture();
    assert!(matches!(acquired, OriginEvidence::Known(_)));
    assert_eq!(acquired.finish_with(&source), acquired);
    assert_eq!(source.capture(), acquired);
}

#[test]
fn changes_during_acquisition_and_composition_never_relabel_a_token() {
    let source = Source::new();
    let acquired = source.capture();
    let mutation = source.changing();
    assert_eq!(
        acquired.finish_with(&source),
        OriginEvidence::Unavailable(OriginUnavailable::MutationInProgress),
    );
    drop(mutation);
    assert_eq!(
        acquired.finish_with(&source),
        OriginEvidence::Unavailable(OriginUnavailable::ChangedDuringAcquisition),
    );
    let composed = source.capture();
    assert_ne!(composed, acquired);
    drop(source.changing());
    assert_eq!(
        composed.finish_with(&source),
        OriginEvidence::Unavailable(OriginUnavailable::ChangedDuringAcquisition),
    );
}

#[test]
fn overlapping_mutations_do_not_expose_a_stable_intermediate_generation() {
    let source = Source::new();
    let before = source.capture();
    let first = source.changing();
    let second = source.changing();
    drop(first);
    assert_eq!(
        source.capture(),
        OriginEvidence::Unavailable(OriginUnavailable::MutationInProgress),
    );
    drop(second);
    assert!(matches!(source.capture(), OriginEvidence::Known(_)));
    assert_ne!(source.capture(), before);
}

#[test]
fn runtime_resets_do_not_reuse_a_source_identity_or_clear_exhaustion() {
    let source = Source::new();
    let first_runtime = source.capture();
    let mut references = ReferenceSpaceChanges::new();
    references.note_pending_with(100, &source);
    drop(source.changing());
    references = ReferenceSpaceChanges::new();
    assert_eq!(references.prepare_with(1, &source), Ok(()));
    assert_ne!(source.capture(), first_runtime);

    source.state.store(!ACTIVE_MASK, Ordering::SeqCst);
    drop(source.changing());
    assert_eq!(
        source.capture(),
        OriginEvidence::Unavailable(OriginUnavailable::GenerationExhausted)
    );
    drop(source.changing());
    references = ReferenceSpaceChanges::new();
    assert_eq!(references.prepare_with(1, &source), Ok(()));
    assert_eq!(
        source.capture(),
        OriginEvidence::Unavailable(OriginUnavailable::GenerationExhausted)
    );
}

#[test]
fn active_writer_capacity_exhaustion_is_terminal() {
    let source = Source::new();
    source
        .state
        .store(GENERATION_STEP | ACTIVE_MASK, Ordering::SeqCst);
    drop(source.changing());
    assert_eq!(
        source.capture(),
        OriginEvidence::Unavailable(OriginUnavailable::GenerationExhausted)
    );
}

#[test]
fn pending_local_change_rejects_until_its_effective_display_time_then_advances() {
    let source = Source::new();
    let mut references = ReferenceSpaceChanges::new();
    let before = source.capture();
    references.note_pending_with(100, &source);
    assert_ne!(source.capture(), before);
    assert_eq!(
        references.prepare_with(99, &source),
        Err(OriginUnavailable::ReferenceSpacePending)
    );
    let pending_generation = source.capture();
    assert_eq!(references.prepare_with(100, &source), Ok(()));
    let after = source.capture();
    assert_ne!(after, pending_generation);
    assert_eq!(references.prepare_with(101, &source), Ok(()));
    assert_eq!(source.capture(), after);
}

#[test]
fn out_of_order_pending_times_and_late_notifications_are_resolved_without_history() {
    let source = Source::new();
    let mut references = ReferenceSpaceChanges::new();
    references.note_pending_with(200, &source);
    references.note_pending_with(100, &source);
    assert_eq!(
        references.prepare_with(150, &source),
        Err(OriginUnavailable::ReferenceSpacePending)
    );
    assert_eq!(references.prepare_with(200, &source), Ok(()));
    let before_late = source.capture();
    references.note_pending_with(175, &source);
    assert_eq!(references.prepare_with(201, &source), Ok(()));
    assert_ne!(source.capture(), before_late);
}

#[test]
fn capacity_loss_event_loss_and_clock_regression_remain_unknown_until_runtime_reset() {
    let source = Source::new();
    let mut references = ReferenceSpaceChanges::new();
    for time in 0..=PENDING_CAPACITY {
        references.note_pending_with(time as i64, &source);
    }
    assert_eq!(
        references.prepare_with(100, &source),
        Err(OriginUnavailable::ReferenceSpaceUnknown)
    );
    assert_eq!(
        references.prepare_with(200, &source),
        Err(OriginUnavailable::ReferenceSpaceUnknown)
    );

    references = ReferenceSpaceChanges::new();
    references.events_unknown_with(&source);
    assert_eq!(
        references.prepare_with(200, &source),
        Err(OriginUnavailable::ReferenceSpaceUnknown)
    );

    references = ReferenceSpaceChanges::new();
    assert_eq!(references.prepare_with(200, &source), Ok(()));
    assert_eq!(
        references.prepare_with(199, &source),
        Err(OriginUnavailable::ReferenceSpaceUnknown)
    );
    assert_eq!(
        references.prepare_with(201, &source),
        Err(OriginUnavailable::ReferenceSpaceUnknown)
    );

    drop(source.changing());
    references = ReferenceSpaceChanges::new();
    assert_eq!(references.prepare_with(1, &source), Ok(()));
}

#[test]
fn only_local_changes_for_the_acquiring_session_invalidate_tracking_origin() {
    let source = Source::new();
    let mut references = ReferenceSpaceChanges::new();
    let first = source.capture();
    references.note_event_with(false, xr::ReferenceSpaceType::LOCAL, 100, &source);
    references.note_event_with(true, xr::ReferenceSpaceType::STAGE, 100, &source);
    references.note_event_with(true, xr::ReferenceSpaceType::VIEW, 100, &source);
    assert_eq!(references.prepare_with(50, &source), Ok(()));
    assert_eq!(source.capture(), first);
    references.note_event_with(true, xr::ReferenceSpaceType::LOCAL, 100, &source);
    assert_ne!(source.capture(), first);
    assert_eq!(
        references.prepare_with(99, &source),
        Err(OriginUnavailable::ReferenceSpacePending)
    );
    assert_eq!(references.prepare_with(100, &source), Ok(()));
}

#[test]
fn an_unknown_acquisition_cannot_be_promoted_by_a_later_stable_source() {
    let source = Source::new();
    for reason in [
        OriginUnavailable::SourceAbsent,
        OriginUnavailable::ReferenceSpacePending,
        OriginUnavailable::ReferenceSpaceUnknown,
    ] {
        let absent = OriginEvidence::Unavailable(reason);
        drop(source.changing());
        assert_eq!(absent.finish_with(&source), absent);
    }
}
