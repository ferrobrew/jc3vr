use super::*;
use crate::controllers::weapons::reload::{
    chamber::ActionKind,
    travel::{ActionTravel, TravelGrip},
};

fn grips(hand: Hand, gun: bool, off: bool) -> [bool; 2] {
    Hand::BOTH.map(|h| if h == hand { gun } else { off })
}

fn held_action(kind: ActionKind, position: f32) -> [bool; 2] {
    let travel = ActionTravel {
        position,
        grip: Some(TravelGrip {
            along_m: 0.0,
            angle_rad: 0.0,
        }),
        ..ActionTravel::new()
    };
    [travel.holds_release(kind); 2]
}

fn arm(hand: Hand) -> Watch {
    let mut watch = Watch::new();
    watch.pending = true;
    assert!(!watch.observe_release(hand, Some(1), grips(hand, true, true), [false; 2], None));
    watch
}

#[test]
fn gun_open_during_held_slide_or_bolt_releases_once_after_action_exit() {
    for hand in Hand::BOTH {
        for kind in [ActionKind::Slide, ActionKind::Bolt] {
            let mut watch = arm(hand);
            let active = held_action(kind, 0.7);
            for _ in 0..3 {
                assert!(!watch.observe_release(hand, Some(1), grips(hand, false, true), active, None));
            }
            assert!(watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
            assert!(!watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
        }
    }
}

#[test]
fn regrip_cancels_deferred_intent_until_another_opening() {
    for hand in Hand::BOTH {
        let mut watch = arm(hand);
        let active = held_action(ActionKind::Slide, 1.0);
        assert!(!watch.observe_release(hand, Some(1), grips(hand, false, true), active, None));
        assert!(!watch.observe_release(hand, Some(1), grips(hand, true, true), active, None));
        assert!(!watch.observe_release(hand, Some(1), grips(hand, true, false), [false; 2], None));
        assert!(watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
    }
}

#[test]
fn opening_both_hands_releases_without_an_extra_edge() {
    for hand in Hand::BOTH {
        let mut watch = arm(hand);
        assert!(watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
        assert!(!watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
    }
}

#[test]
fn ownership_change_and_standdown_cancel_the_exact_weapons_deferred_release() {
    for hand in Hand::BOTH {
        for replacement in [None, Some(2)] {
            let mut watch = arm(hand);
            assert!(!watch.observe_release(hand, Some(1), grips(hand, false, true), [true; 2], None));
            assert!(!watch.observe_release(hand, replacement, [false; 2], [false; 2], None));
            assert!(!watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
        }
        let mut watch = arm(hand);
        assert!(!watch.observe_release(hand, Some(1), grips(hand, false, true), [true; 2], None));
        watch.release.disarm();
        watch.weapon = None;
        assert!(!watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
    }
}

#[test]
fn ready_pump_foregrip_at_home_uses_normal_brace_or_drop_release() {
    for hand in Hand::BOTH {
        let mut watch = arm(hand);
        let home = held_action(ActionKind::Pump, 0.0);
        assert_eq!(home, [false; 2]);
        assert!(watch.observe_release(hand, Some(1), grips(hand, false, true), home, None));
        let mut watch = arm(hand);
        let moving = held_action(ActionKind::Pump, 0.2);
        assert_eq!(moving, [true; 2]);
        assert!(!watch.observe_release(hand, Some(1), grips(hand, false, true), moving, None));
        assert!(watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
    }
}
