use std::sync::{
    Arc, Weak,
    atomic::{AtomicUsize, Ordering},
};

use super::*;

struct Owner {
    ledger: Weak<Mutex<Ledger<Owner>>>,
    released: Arc<AtomicUsize>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        let ledger = self.ledger.upgrade().unwrap();
        assert!(
            ledger.try_lock().is_some(),
            "native owner release must not hold the ledger lock"
        );
        self.released.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn reset_releases_every_ledger_owner_outside_lock_and_reused_address_has_no_partition() {
    let ledger = Arc::new(Mutex::new(Ledger {
        entries: Vec::new(),
    }));
    let released = Arc::new(AtomicUsize::new(0));
    for weapon in 1..=MAX_RECORDS {
        ledger.lock().entries.push(Entry {
            weapon,
            owner: Owner {
                ledger: Arc::downgrade(&ledger),
                released: released.clone(),
            },
            user: 1,
            runtime: Runtime::new(
                Ammo::adopt(
                    NativeAmmo {
                        total: 1,
                        out: true,
                    },
                    [Chamber::Chambered, Chamber::Empty],
                ),
                [Some((0, false)), None],
            ),
            magazine_only: false,
        });
    }
    for entry in &mut ledger.lock().entries {
        entry.runtime.suspend();
    }
    assert_eq!(released.load(Ordering::Relaxed), 0);
    release_for_reset(&ledger);
    assert_eq!(released.load(Ordering::Relaxed), MAX_RECORDS);
    assert!(ledger.lock().entries.is_empty());
    release_for_reset(&ledger);
    assert_eq!(released.load(Ordering::Relaxed), MAX_RECORDS);
    ledger.lock().entries.push(Entry {
        weapon: 1,
        owner: Owner {
            ledger: Arc::downgrade(&ledger),
            released: released.clone(),
        },
        user: 2,
        runtime: Runtime::new(
            Ammo::adopt(
                NativeAmmo {
                    total: 0,
                    out: true,
                },
                [Chamber::Empty; 2],
            ),
            [Some((0, false)), None],
        ),
        magazine_only: false,
    });
    assert_eq!(ledger.lock().entries[0].runtime.ammo.chamber_rounds(), 0);
    release_for_reset(&ledger);
    assert_eq!(released.load(Ordering::Relaxed), MAX_RECORDS + 1);
}
