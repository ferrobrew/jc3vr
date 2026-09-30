//! Menu bindings: the UI map, the pointer's trigger suppression, and the system button.

use super::*;

#[test]
fn pointer_suppresses_only_its_hand_in_either_layout() {
    for dominant in Hand::BOTH {
        let mut c = controls(dominant, |dom, off| {
            dom.trigger_press = down();
            off.trigger_press = down();
        });
        c.pointer_hand = Some(dominant);
        let step = Bindings::default().step(Mode::Ui, &c, DT, &CONFIG);
        let (owned, other) = match dominant {
            Hand::Left => (GUI_PAGE_PREV, GUI_PAGE_NEXT),
            Hand::Right => (GUI_PAGE_NEXT, GUI_PAGE_PREV),
        };
        assert_eq!(wants(&step, MapKind::Ui, owned), None);
        assert_eq!(wants(&step, MapKind::Ui, other), Some(Want::Hold(1.0)));
        assert!(step.desired.iter().all(|d| d.target.map == MapKind::Ui));
    }
}

#[test]
fn menus_use_the_ui_map_on_the_physical_hands() {
    let mut b = Bindings::default();
    let c = controls(Hand::Left, |dom, off| {
        // Left-handed: the dominant hand is the left one.
        dom.grip = down();
        off.a = down();
    });
    let step = b.step(Mode::Ui, &c, DT, &CONFIG);
    assert_eq!(
        wants(&step, MapKind::Ui, GUI_TAB_PREV),
        Some(Want::Hold(1.0))
    );
    assert_eq!(wants(&step, MapKind::Ui, GUI_OK), Some(Want::Hold(1.0)));
    assert!(step.desired.iter().all(|d| d.target.map == MapKind::Ui));
}

fn system(down: bool, pressed: bool, released: bool) -> Controls {
    let mut c = Controls::default();
    c.hands[0].system = ButtonControl {
        down,
        pressed,
        released,
    };
    c
}

fn clicked(step: &Step, action: Action) -> bool {
    wants(step, MapKind::PlayerThroughBlockers, action) == Some(Want::Click)
        && wants(step, MapKind::Ui, action) == Some(Want::Click)
}

#[test]
fn a_short_system_press_pauses_on_release() {
    let mut b = Bindings::default();
    let step = b.step(ON_FOOT, &system(true, true, false), DT, &CONFIG);
    assert!(step.desired.is_empty());
    let step = b.step(ON_FOOT, &system(false, false, true), DT, &CONFIG);
    assert!(clicked(&step, GUI_PAUSE));
    assert!(!clicked(&step, GUI_PDA));
}

#[test]
fn a_tap_between_ticks_still_pauses() {
    let mut b = Bindings::default();
    b.step(ON_FOOT, &system(false, true, true), DT, &CONFIG);
    let step = b.step(ON_FOOT, &Controls::default(), DT, &CONFIG);
    assert!(clicked(&step, GUI_PAUSE));
}

#[test]
fn a_long_system_press_opens_the_commlink_once() {
    let mut b = Bindings::default();
    let held = system(true, false, false);
    let ticks = (CONFIG.long_press_s / DT).ceil() as usize + 1;
    let mut commlinks = 0;
    for _ in 0..ticks * 2 {
        let step = b.step(ON_FOOT, &held, DT, &CONFIG);
        commlinks += usize::from(clicked(&step, GUI_PDA));
    }
    assert_eq!(commlinks, 1);
    let step = b.step(ON_FOOT, &system(false, false, true), DT, &CONFIG);
    assert!(!clicked(&step, GUI_PAUSE));
}

#[test]
fn the_system_button_works_in_menus() {
    let mut b = Bindings::default();
    b.step(Mode::Ui, &system(true, true, false), DT, &CONFIG);
    let step = b.step(Mode::Ui, &Controls::default(), DT, &CONFIG);
    assert!(clicked(&step, GUI_PAUSE));
}
