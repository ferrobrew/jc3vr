use super::*;

#[test]
fn pending_magazine_work_vetoes_preparation_with_or_without_an_entry() {
    for entry_active in [false, true] {
        assert!(matches!(
            magazine_preparation(false, entry_active),
            PrepareOutcome::Deferred(DeferReason::MagazineBusy),
        ));
    }
}

#[test]
fn drained_magazine_work_waits_for_the_native_callback_to_return() {
    assert!(matches!(
        magazine_preparation(true, true),
        PrepareOutcome::Deferred(DeferReason::MagazineBusy),
    ));
    assert!(matches!(
        magazine_preparation(true, false),
        PrepareOutcome::Ready(())
    ));
}
