use super::*;
use crate::{
    controllers::loadout::Socket,
    lifecycle::{Preparation, PrepareOutcome},
};

#[test]
fn clear_reset_and_expiry_never_erase_callback_exposure_or_block_preparation() {
    spawn_safety::activate();
    spawn_safety::submit(RequestKind::Restore, || ()).unwrap();
    let participant = spawn_safety::participant();
    // Exposure warns rather than defers, so each preparation is ready and dropping it aborts.
    let still_exposed = || {
        assert!(matches!(
            participant.prepare(Preparation::for_test()),
            PrepareOutcome::Ready(_)
        ));
        assert!(spawn_safety::exposed());
    };
    still_exposed();
    clear();
    still_exposed();
    on_pre_reset(Vec::new(), false, false);
    on_post_reset();
    on_frontend();
    still_exposed();

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
    still_exposed();
    clear();
}
