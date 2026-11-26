use super::*;
use crate::controllers::physical_hands::targets::test_support::DoorProvider;

#[test]
fn only_fresh_unheld_latched_doors_receive_provider_eligibility() {
    for hand in Hand::BOTH {
        let mut provider = DoorProvider::new();
        let latched = provider.door(hand, true);
        assert!(provider.current(hand, latched));
        assert!(!provider.current(hand.other(), latched));
        assert!(latched.observed.is_some());
        let open = provider.door(hand, false);
        assert!(!provider.current(hand, open));
        let prop = provider.target(hand, TargetKind::Prop { mass_kg: 1.0 });
        assert!(prop.door_eligibility.is_none());
        assert!(prop.observed.is_some());
        let mut provider = DoorProvider::new();
        provider.hold(hand);
        assert!(provider.door(hand, true).door_eligibility.is_none());
        let mut provider = DoorProvider::new();
        provider.expire();
        assert!(provider.door(hand, true).door_eligibility.is_none());
    }
}

#[test]
fn final_provider_validation_rejects_source_and_claim_changes() {
    for hand in Hand::BOTH {
        for change in 0..10 {
            let mut provider = DoorProvider::new();
            let target = provider.door(hand, true);
            assert!(provider.current(hand, target));
            match change {
                0 => provider.expire(),
                1 => provider.replace_source(),
                2 => provider.replace_sample(),
                3 => provider.remove_reach(hand),
                4 => provider.claim(hand),
                5 => provider.latch(hand),
                6 => provider.hold(hand),
                7 => provider.replace_door(hand),
                8 => provider.claim_same_door(hand),
                _ => {
                    provider.door(hand, false);
                }
            }
            assert!(!provider.current(hand, target), "change {change}");
        }
    }
}
