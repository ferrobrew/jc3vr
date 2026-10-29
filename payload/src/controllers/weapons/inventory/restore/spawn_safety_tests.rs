use super::*;
use crate::{
    controllers::loadout::Socket,
    lifecycle::{DeferReason, Preparation, PrepareOutcome},
};

#[test]
fn actual_clear_reset_and_expiry_never_erase_callback_exposure() {
    spawn_safety::activate();
    spawn_safety::submit(RequestKind::Restore, || ()).unwrap();
    let participant = spawn_safety::participant();
    let blocked = || {
        assert!(matches!(
            participant.prepare(Preparation::for_test()),
            PrepareOutcome::Deferred(DeferReason::SpawnCallbacksUnproven)
        ));
        assert!(spawn_safety::post_unpatch_check().is_err());
    };
    blocked();
    clear();
    blocked();
    on_pre_reset(Vec::new(), false, false);
    on_post_reset();
    on_frontend();
    blocked();

    let record = Record {
        tag_hash: 7,
        magazine: 5,
        socket: Socket::BackOuter,
        single: false,
    };
    PLAN.lock().spawning.push(Spawning {
        record,
        at: Instant::now() - SPAWN_WAIT - Duration::from_secs(1),
    });
    assert_eq!(expire_spawning(), vec![record]);
    assert!(PLAN.lock().spawning.is_empty());
    blocked();
    clear();
}
