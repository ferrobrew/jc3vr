use super::*;
use crate::controllers::weapons::{
    pause,
    reload::{
        chamber::ActionKind,
        travel::{ActionTravel, TravelGrip},
    },
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
                assert!(!watch.observe_release(
                    hand,
                    Some(1),
                    grips(hand, false, true),
                    active,
                    None
                ));
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
            assert!(!watch.observe_release(
                hand,
                Some(1),
                grips(hand, false, true),
                [true; 2],
                None
            ));
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

#[test]
fn a_stand_down_ends_with_the_grip_as_it_is_then() {
    // One test drives every stand-down route, since each goes through the shared release state.
    for route in StandDown::ALL {
        for hand in Hand::BOTH {
            // Held through the stand-down: the hold's grip reads open until the hand opens, so the
            // controller's grip keeps the gun, and its opening lets go of it, once.
            let mut watch = route.through(hand, arm(hand));
            let closed = grips(hand, true, false);
            assert_eq!(
                watch.observe(hand, Some(1), masked(closed), [false; 2], None),
                GripRelease::Kept
            );
            assert_eq!(
                watch.observe(hand, Some(1), masked(closed), [false; 2], None),
                GripRelease::Kept
            );
            assert_eq!(
                watch.observe(hand, Some(1), masked([false; 2]), [false; 2], None),
                GripRelease::LetGo
            );
            assert_eq!(
                watch.observe(hand, Some(1), masked([false; 2]), [false; 2], None),
                GripRelease::Kept
            );
            // Held through, and the hold's grip reads the hand again: the ordinary watch takes over.
            let mut watch = route.through(hand, arm(hand));
            assert_eq!(
                watch.observe(hand, Some(1), masked(closed), [false; 2], None),
                GripRelease::Kept
            );
            assert_eq!(
                watch.observe(hand, Some(1), unmasked(closed), [false; 2], None),
                GripRelease::Kept
            );
            assert_eq!(
                watch.observe(hand, Some(1), unmasked([false; 2]), [false; 2], None),
                GripRelease::LetGo
            );
            // Opened during the stand-down: the gun goes back on the body, once.
            let mut watch = route.through(hand, arm(hand));
            assert_eq!(
                watch.observe(hand, Some(1), masked([false; 2]), [false; 2], None),
                GripRelease::Stow
            );
            assert_eq!(
                watch.observe(hand, Some(1), masked([false; 2]), [false; 2], None),
                GripRelease::Kept
            );
            // A stand-down with nothing in the hand changes nothing.
            let mut watch = route.through(hand, Watch::new());
            assert_eq!(
                watch.observe(hand, None, masked([false; 2]), [false; 2], None),
                GripRelease::Kept
            );
            assert_eq!(
                watch.observe(hand, Some(1), masked([false; 2]), [false; 2], None),
                GripRelease::Kept
            );
        }
    }
}

/// A returning hand's grips: the hold's reads open, the controller's are `physical`.
fn masked(physical: [bool; 2]) -> GripReading {
    GripReading {
        grips: [false; 2],
        physical,
    }
}

/// Grips the hold reads as the controller's are.
fn unmasked(grips: [bool; 2]) -> GripReading {
    GripReading {
        grips,
        physical: grips,
    }
}

#[test]
fn a_grab_waits_for_its_weapon_to_reach_the_hand() {
    for hand in Hand::BOTH {
        let mut watch = Watch::new();
        watch.pending = true;
        assert!(!watch.observe_release(hand, None, grips(hand, true, false), [false; 2], None));
        assert!(!watch.observe_release(hand, Some(1), grips(hand, true, false), [false; 2], None));
        assert!(watch.observe_release(hand, Some(1), [false; 2], [false; 2], None));
    }
}

#[test]
fn a_stowed_gun_goes_back_where_it_was_drawn_from_when_free() {
    let left = Socket::Hip(Hand::Left);
    let right = Socket::Hip(Hand::Right);
    assert_eq!(stow_socket(Some(left), |_| true), Some(left));
    assert_eq!(stow_socket(Some(left), |s| s == right), Some(right));
    assert_eq!(stow_socket(None, |s| s == right), Some(right));
    assert_eq!(stow_socket(Some(left), |_| false), None);
}

/// A way the grip release stands down.
#[derive(Debug, Copy, Clone)]
enum StandDown {
    /// The input route stands the release down ([`cancel_releases`]).
    Cancelled,
    /// The weapon handling stands down: the debug panel, a focus loss, the controllers lost
    /// ([`crate::controllers::weapons::stand_down`], which runs [`clear`]).
    Cleared,
    /// A game menu pauses the weapon handling ([`pause`]).
    Paused,
}

impl StandDown {
    const ALL: [Self; 3] = [Self::Cancelled, Self::Cleared, Self::Paused];

    /// `watch` as `hand`'s watch after this stand-down.
    fn through(self, hand: Hand, watch: Watch) -> Watch {
        STATE.lock().watches[hand.index()] = watch;
        match self {
            Self::Cancelled => cancel_releases(),
            Self::Cleared => clear(),
            Self::Paused => pause(),
        }
        STATE.lock().watches[hand.index()]
    }
}
