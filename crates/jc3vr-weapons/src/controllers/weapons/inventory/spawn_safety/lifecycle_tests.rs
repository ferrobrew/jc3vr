//! Inventory spawn safety as a shutdown participant, alongside the others, in a lifecycle of its
//! own.

use std::sync::Arc;

use jc3vr_core::lifecycle::{
    ParticipantId,
    test_support::{Answer, FakeParticipant, Harness},
};

use crate::controllers::weapons::inventory::spawn_safety::{RequestKind, SpawnSafety};

#[test]
fn exposed_inventory_requests_do_not_block_shutdown() {
    for kind in [RequestKind::Restore, RequestKind::Twin] {
        let harness = Harness::new();
        let safety = Arc::new(SpawnSafety::for_test());
        safety.activate_for_test();
        harness.register_participant(ParticipantId::InventorySpawn, safety.clone());
        let magazine = FakeParticipant::new(ParticipantId::MagazineExecutor, [Answer::Ready]);
        harness.register(&magazine);
        harness.add_cleanup("cleanup");
        safety.submit_for_test(kind, || ()).unwrap();
        harness.request();

        assert!(harness.frame());
        assert!(harness.is_shutting_down());
        assert_eq!(magazine.calls(), 1);
        assert_eq!(harness.cleanup_calls(), 1);
        assert!(safety.post_unpatch_for_test().is_ok());
        assert!(safety.submit_for_test(kind, || 37).is_err());
    }
}

#[test]
fn later_participant_deferral_reopens_inventory_admission() {
    let harness = Harness::new();
    let safety = Arc::new(SpawnSafety::for_test());
    safety.activate_for_test();
    harness.register_participant(ParticipantId::InventorySpawn, safety.clone());
    harness.register(&FakeParticipant::new(
        ParticipantId::MagazineExecutor,
        [Answer::Deferred],
    ));
    harness.add_cleanup("cleanup");
    harness.request();

    assert!(!harness.frame());
    assert!(!harness.is_shutting_down());
    assert_eq!(harness.pending_cleanups(), 1);
    assert_eq!(safety.submit_for_test(RequestKind::Restore, || 41), Ok(41));
}

#[test]
fn unexposed_inventory_preparation_commits_without_reopening_admission() {
    let harness = Harness::new();
    let safety = Arc::new(SpawnSafety::for_test());
    safety.activate_for_test();
    harness.register_participant(ParticipantId::InventorySpawn, safety.clone());
    harness.add_cleanup("cleanup");
    harness.request();
    assert!(harness.frame());
    assert!(harness.is_shutting_down());
    assert_eq!(harness.cleanup_calls(), 1);
    assert!(safety.post_unpatch_for_test().is_ok());
    assert!(
        safety
            .submit_for_test(RequestKind::Twin, || panic!("native call after commit"))
            .is_err()
    );
}
