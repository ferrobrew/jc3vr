use super::*;

#[test]
fn false_magazine_drain_vetoes_preparation_with_or_without_a_binder() {
    for installed in [false, true] {
        assert!(matches!(
            magazine_preparation(false, installed),
            PrepareOutcome::Deferred(DeferReason::MagazineBusy),
        ));
    }
}

#[test]
fn drained_installed_binder_still_requires_entry_exclusion() {
    assert!(matches!(
        magazine_preparation(true, true),
        PrepareOutcome::Deferred(DeferReason::HookEntryExclusionUnproven),
    ));
    assert!(matches!(
        magazine_preparation(true, false),
        PrepareOutcome::Ready(())
    ));
}
