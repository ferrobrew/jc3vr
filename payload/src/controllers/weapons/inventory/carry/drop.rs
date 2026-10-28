//! Carry-off ordering keeps the retained actor alive through every owner transition.

use crate::controllers::frame::Hand;

pub(super) trait Adapter {
    fn disarm(&mut self, hand: Hand);
    fn table_drop(&mut self);
    fn native_drop(&mut self);
}

pub(super) fn run(adapter: &mut impl Adapter) {
    for hand in Hand::BOTH {
        adapter.disarm(hand);
    }
    adapter.table_drop();
    adapter.native_drop();
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        hands: [bool; 2],
        table_world: bool,
        holstered: bool,
        retained: bool,
        drops: usize,
    }

    impl Adapter for Fake {
        fn disarm(&mut self, hand: Hand) {
            assert!(self.retained);
            self.hands[hand.index()] = false;
        }
        fn table_drop(&mut self) {
            assert!(self.retained);
            assert_eq!(self.hands, [false; 2]);
            self.table_world = true;
        }
        fn native_drop(&mut self) {
            assert!(self.retained && self.table_world);
            self.drops += usize::from(self.holstered);
        }
    }

    #[test]
    fn holstered_and_off_slot_hands_release_before_native_drop_and_owner_release() {
        for hands in [[false, false], [true, false], [false, true], [true, true]] {
            for holstered in [false, true] {
                let mut native = Fake {
                    hands,
                    table_world: false,
                    holstered,
                    retained: true,
                    drops: 0,
                };
                run(&mut native);
                native.retained = false;
                assert_eq!(native.hands, [false; 2]);
                assert!(native.table_world);
                assert_eq!(native.drops, usize::from(holstered));
            }
        }
    }
}
