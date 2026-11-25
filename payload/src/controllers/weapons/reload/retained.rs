//! Admission and transfer of a retained magazine without discarding incoming rounds.

use crate::controllers::weapons::reload::loose::Operation;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum SeatOutcome {
    Loaded,
    Rejected { rounds: i32, capacity: i32 },
    Deferred,
    Gone,
}

pub(super) fn seat(
    rounds: i32,
    capacity: i32,
    consume: impl FnOnce() -> Operation<i32>,
    load: impl FnOnce(i32),
) -> SeatOutcome {
    if rounds < 0 || (capacity >= 0 && rounds > capacity) {
        return SeatOutcome::Rejected { rounds, capacity };
    }
    match consume() {
        Operation::Ready(seated) => {
            // The held record is immutable between admission and consumption on the input thread.
            assert_eq!(seated, rounds);
            load(seated);
            SeatOutcome::Loaded
        }
        Operation::Deferred => SeatOutcome::Deferred,
        Operation::Gone => SeatOutcome::Gone,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::controllers::weapons::{
        dual_wield::single::magazine,
        reload::{
            ammo::{Ammo, NativeAmmo},
            chamber::Chamber,
        },
    };

    #[test]
    fn native_pair_magazine_refuses_single_capacity_before_consumption() {
        let held = Cell::new(Some(22));
        let recipient = Cell::new(Ammo::adopt(
            NativeAmmo {
                total: 5,
                out: false,
            },
            [Chamber::Chambered, Chamber::Empty],
        ));
        let reserve = Cell::new(70);
        let before = recipient.get();
        let outcome = seat(
            22,
            magazine::capacity(24),
            || Operation::Ready(held.take().unwrap()),
            |rounds| {
                let mut ammo = recipient.get();
                reserve.set(reserve.get() + ammo.insert(rounds));
                recipient.set(ammo);
            },
        );
        assert_eq!(
            outcome,
            SeatOutcome::Rejected {
                rounds: 22,
                capacity: 12
            }
        );
        assert_eq!(held.get(), Some(22));
        assert_eq!(recipient.get(), before);
        assert_eq!(reserve.get(), 70);
    }

    #[test]
    fn accepted_magazine_returns_only_recipient_rounds_and_keeps_chamber() {
        for capacity in [12, -1] {
            let held = Cell::new(Some(12));
            let mut recipient = Ammo::adopt(
                NativeAmmo {
                    total: 5,
                    out: false,
                },
                [Chamber::Chambered, Chamber::Empty],
            );
            let mut reserve = 70;
            assert_eq!(
                seat(
                    12,
                    capacity,
                    || Operation::Ready(held.take().unwrap()),
                    |rounds| {
                        reserve += recipient.insert(rounds);
                    }
                ),
                SeatOutcome::Loaded
            );
            assert_eq!(held.get(), None);
            assert_eq!(recipient.magazine, 12);
            assert_eq!(recipient.chamber_rounds(), 1);
            assert_eq!(reserve, 74);
            assert_eq!(recipient.total() + reserve, 87);
        }
    }
}
