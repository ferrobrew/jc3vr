use super::*;
use crate::controllers::loadout::Socket;

fn pending() -> Pending {
    Pending {
        records: vec![Record {
            tag_hash: 7,
            magazine: 5,
            socket: Socket::BackOuter,
            single: false,
        }],
        slot_pistol_single: None,
        at: Instant::now(),
        character: 0,
    }
}

#[test]
fn due_plan_is_unchanged_until_table_ready_and_consumed_once() {
    clear();
    {
        let mut plan = PLAN.lock();
        plan.from_load = Some(pending());
        plan.due = true;
        assert!(plan.take_due(false, 1).is_none());
        assert!(plan.due);
        assert_eq!(plan.from_load.as_ref().unwrap().records.len(), 1);
        let chosen = plan.take_due(true, 1).unwrap();
        assert_eq!(chosen.records.len(), 1);
        assert!(plan.take_due(true, 1).is_none());
    }
    clear();
}

#[test]
fn reset_and_clear_invalidate_stale_pending_work_without_resurrection() {
    clear();
    PLAN.lock().from_load = Some(pending());
    PLAN.lock().due = true;
    let before = PLAN.lock().generation;
    on_pre_reset(Vec::new(), false, false);
    assert_ne!(PLAN.lock().generation, before);
    assert!(!PLAN.lock().due);
    assert!(PLAN.lock().take_due(true, 1).is_none());
    on_post_reset();
    clear();
    assert!(PLAN.lock().from_load.is_none());
    assert!(PLAN.lock().from_reset.is_none());
    assert!(PLAN.lock().take_due(true, 1).is_none());
    assert!(SPAWNED.lock().is_empty());
    assert!(TWINS.lock().is_empty());
}
