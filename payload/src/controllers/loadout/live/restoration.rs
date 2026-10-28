//! Complete native resets and cache restoration before accepting carried results.

use std::time::Instant;

use crate::controllers::{
    frame::Hand,
    loadout::{
        BodyLayout, CacheKind, Event, ResetKind, SlotObject,
        live::{Hold, Live, ResetWatch, SETTLE_WAIT},
    },
};

impl Live {
    pub(super) fn begin_restoration(
        &mut self,
        dominant: Hand,
        slots: &[SlotObject],
        spawning: bool,
        now: Instant,
    ) -> bool {
        self.table.set_body(BodyLayout::full(dominant));
        if !self.started {
            let since = *self.reset.completed_at.get_or_insert(now);
            if spawning && now.saturating_duration_since(since) < SETTLE_WAIT {
                return false;
            }
            self.started = true;
            self.pending = None;
            self.origins = [None; 2];
            self.last = None;
            self.reset = ResetWatch::default();
            self.fold(Event::ResetCompleted {
                kind: ResetKind::Load,
                slots: slots.to_vec(),
            });
            self.publish_places();
            return true;
        }
        let settled = |since: Option<Instant>| {
            !spawning || since.is_some_and(|at| now.saturating_duration_since(at) >= SETTLE_WAIT)
        };
        if self.reset.completed {
            let since = *self.reset.completed_at.get_or_insert(now);
            if settled(Some(since)) {
                let kind = self.reset.kind();
                let checkpoint = self.reset.checkpoint && kind == ResetKind::Respawn;
                self.reset = ResetWatch::default();
                self.fold(Event::ResetCompleted {
                    kind,
                    slots: slots.to_vec(),
                });
                if checkpoint {
                    self.fold(Event::CacheRestored {
                        cache: CacheKind::Checkpoint,
                        slots: slots.to_vec(),
                    });
                }
            }
        }
        // A give-back waits for the reset too, when one is under way.
        if !(self.reset.began || self.reset.completed) {
            for cache in [CacheKind::Mission, CacheKind::Challenge] {
                if let Hold::Restoring(since) = self.held_by(cache)
                    && settled(Some(since))
                {
                    *self.hold_mut(cache) = Hold::Idle;
                    self.fold(Event::CacheRestored {
                        cache,
                        slots: slots.to_vec(),
                    });
                }
            }
        }
        self.publish_places();
        self.restore_ready()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};

    use super::*;
    use crate::controllers::loadout::{
        ObjectAddress, Restored, Socket, WeaponType, binding::Binding,
    };

    #[test]
    fn first_start_waits_for_pending_slots_before_publishing_restore_readiness() {
        let mut live = Live::new();
        let now = Instant::now();
        live.reset.completed = true;
        assert!(!live.begin_restoration(Hand::Right, &[], true, now));
        assert!(!live.started);
        assert!(!live.restore_ready());
        assert!(live.begin_restoration(Hand::Right, &[], false, now));
        assert!(live.started);
        assert!(live.restore_ready());
    }

    #[test]
    fn carried_callback_waits_for_delayed_native_slot_then_survives_later_ticks() {
        let mut live = Live::new();
        let now = Instant::now();
        assert!(live.begin_restoration(Hand::Right, &[], false, now));
        live.reset.began = true;
        live.reset.completed = true;
        live.reset.loaded = true;
        let weapon = WeaponType::new(
            1,
            WeaponSlot::E_SLOT_TWO_HANDED,
            WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE as i32,
        )
        .unwrap();
        let restored = Restored {
            object: ObjectAddress::new(NonZeroUsize::new(100).unwrap()),
            weapon,
            socket: Socket::BackOuter,
        };
        let mut callback = Some(restored);
        assert!(!live.begin_restoration(Hand::Right, &[], true, now));
        assert!(!live.restore_ready());
        assert!(callback.is_some());
        assert!(
            !live
                .table
                .items()
                .any(|item| item.binding == Binding::Live(restored.object))
        );
        let slots = [SlotObject {
            object: ObjectAddress::new(NonZeroUsize::new(200).unwrap()),
            weapon,
        }];
        assert!(live.begin_restoration(Hand::Right, &slots, false, now));
        let admission = live.table.admit_restore(callback.take().unwrap()).unwrap();
        live.table.commit_restore(admission).unwrap();
        assert!(live.begin_restoration(Hand::Right, &slots, false, now));
        assert!(
            live.table
                .items()
                .any(|item| item.binding == Binding::Live(restored.object))
        );
        assert!(callback.is_none());
    }
}
