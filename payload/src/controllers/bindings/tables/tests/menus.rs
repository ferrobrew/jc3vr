//! Menu bindings: the UI map, both hands' navigation and grip modifier, the pointer's trigger
//! suppression, the system button, and the stick click chord.

use super::*;

#[test]
fn pointer_suppresses_only_its_hand_in_either_layout() {
    for dominant in Hand::BOTH {
        let mut c = controls(dominant, |dom, off| {
            dom.trigger_press = down();
            off.trigger_press = down();
        });
        c.pointer_hand = Some(dominant);
        let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
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
fn the_other_trigger_with_its_grip_turns_the_pointers_page() {
    for dominant in Hand::BOTH {
        let mut c = controls(dominant, |_, off| {
            off.grip = down();
            off.trigger_press = down();
            off.trigger = 1.0;
        });
        c.pointer_hand = Some(dominant);
        let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
        let (pointers_page, own_page) = match dominant {
            Hand::Left => (GUI_PAGE_PREV, GUI_PAGE_NEXT),
            Hand::Right => (GUI_PAGE_NEXT, GUI_PAGE_PREV),
        };
        assert_eq!(
            wants(&step, MapKind::Ui, pointers_page),
            Some(Want::Hold(1.0))
        );
        assert_eq!(wants(&step, MapKind::Ui, own_page), None);
    }
}

#[test]
fn either_hand_confirms_and_goes_back_in_either_layout() {
    for dominant in Hand::BOTH {
        for hand in Hand::BOTH {
            let mut c = Controls {
                dominant,
                ..Default::default()
            };
            c.hands[hand.index()].a = down();
            let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
            assert_eq!(wants(&step, MapKind::Ui, GUI_OK), Some(Want::Hold(1.0)));
            assert_eq!(wants(&step, MapKind::Ui, GUI_CANCEL), None);
            assert_eq!(wants(&step, MapKind::Ui, GUI_SPECIAL_BUTTON), None);
            assert!(step.desired.iter().all(|d| d.target.map == MapKind::Ui));

            let mut c = Controls {
                dominant,
                ..Default::default()
            };
            c.hands[hand.index()].b = down();
            let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
            assert_eq!(wants(&step, MapKind::Ui, GUI_CANCEL), Some(Want::Hold(1.0)));
            assert_eq!(wants(&step, MapKind::Ui, GUI_OK), None);
            assert_eq!(wants(&step, MapKind::Ui, GUI_USE_BUTTON), None);
        }
    }
}

#[test]
fn the_grip_turns_a_and_b_into_the_gamepads_x_and_y() {
    for hand in Hand::BOTH {
        let mut c = Controls::default();
        let h = &mut c.hands[hand.index()];
        h.grip = down();
        h.a = down();
        h.b = down();
        let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
        assert_eq!(
            wants(&step, MapKind::Ui, GUI_SPECIAL_BUTTON),
            Some(Want::Hold(1.0))
        );
        assert_eq!(
            wants(&step, MapKind::Ui, GUI_USE_BUTTON),
            Some(Want::Hold(1.0))
        );
        assert_eq!(wants(&step, MapKind::Ui, GUI_OK), None);
        assert_eq!(wants(&step, MapKind::Ui, GUI_CANCEL), None);
    }
}

#[test]
fn the_grips_alone_step_no_tabs() {
    let mut c = Controls::default();
    c.hands[0].grip = down();
    c.hands[1].grip = down();
    let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
    assert!(step.desired.is_empty());
}

#[test]
fn a_press_keeps_the_grip_it_began_with() {
    let mut b = Bindings::default();
    let mut c = Controls::default();
    c.hands[1].a = down();
    b.sim_step(Mode::Ui, &c, DT, &CONFIG);
    // Squeezing the grip mid-press does not turn the confirm into the X button.
    c.hands[1].a = ButtonControl {
        down: true,
        pressed: false,
        released: false,
    };
    c.hands[1].grip = down();
    let step = b.sim_step(Mode::Ui, &c, DT, &CONFIG);
    assert_eq!(wants(&step, MapKind::Ui, GUI_OK), Some(Want::Hold(1.0)));
    assert_eq!(wants(&step, MapKind::Ui, GUI_SPECIAL_BUTTON), None);
}

fn stick(hand: Hand, x: f32, y: f32, grip: bool) -> Controls {
    let mut c = Controls::default();
    let h = &mut c.hands[hand.index()];
    h.stick_raw = Vec2::new(x, y);
    h.stick = h.stick_raw;
    if grip {
        h.grip = down();
    }
    c
}

/// The UI actions wanted for one tick of `controls` on fresh bindings, sorted.
fn ui_actions(controls: &Controls) -> Vec<Action> {
    let step = Bindings::default().sim_step(Mode::Ui, controls, DT, &CONFIG);
    let mut actions: Vec<_> = step
        .desired
        .iter()
        .filter(|d| d.target.map == MapKind::Ui && d.want == Want::Hold(1.0))
        .map(|d| d.target.action)
        .collect();
    actions.sort_by_key(|a| *a as i32);
    actions
}

#[test]
fn either_stick_steps_one_direction_at_a_time() {
    for hand in Hand::BOTH {
        assert_eq!(ui_actions(&stick(hand, 0.0, 0.9, false)), [GUI_LSTICK_UP]);
        assert_eq!(
            ui_actions(&stick(hand, 0.0, -0.9, false)),
            [GUI_LSTICK_DOWN]
        );
        // Mostly sideways with a little lift is sideways only.
        assert_eq!(
            ui_actions(&stick(hand, 0.9, 0.4, false)),
            [GUI_LSTICK_RIGHT]
        );
        assert_eq!(
            ui_actions(&stick(hand, -0.9, -0.4, false)),
            [GUI_LSTICK_LEFT]
        );
        // Mostly up with a little drift is up only.
        assert_eq!(ui_actions(&stick(hand, 0.3, 0.8, false)), [GUI_LSTICK_UP]);
        // A stick resting short of the push does nothing.
        assert!(ui_actions(&stick(hand, 0.2, 0.3, false)).is_empty());
        assert!(ui_actions(&stick(hand, 0.2, 0.3, true)).is_empty());
    }
}

#[test]
fn a_held_stick_holds_its_step_for_the_game_to_repeat() {
    let mut b = Bindings::default();
    let c = stick(Hand::Left, 0.0, -0.9, false);
    for _ in 0..30 {
        let step = b.sim_step(Mode::Ui, &c, DT, &CONFIG);
        assert_eq!(
            wants(&step, MapKind::Ui, GUI_LSTICK_DOWN),
            Some(Want::Hold(1.0))
        );
    }
    // Easing off past half the push still holds the step; resting lets go.
    let eased = stick(Hand::Left, 0.0, -CONFIG.menu_stick_push * 0.6, false);
    let step = b.sim_step(Mode::Ui, &eased, DT, &CONFIG);
    assert_eq!(
        wants(&step, MapKind::Ui, GUI_LSTICK_DOWN),
        Some(Want::Hold(1.0))
    );
    let step = b.sim_step(Mode::Ui, &Controls::default(), DT, &CONFIG);
    assert!(step.desired.is_empty());
}

#[test]
fn with_its_grip_held_a_sticks_sideways_push_steps_the_tabs() {
    for hand in Hand::BOTH {
        assert_eq!(ui_actions(&stick(hand, 0.9, 0.0, true)), [GUI_TAB_NEXT]);
        assert_eq!(ui_actions(&stick(hand, -0.9, 0.0, true)), [GUI_TAB_PREV]);
        // Up and down still navigate with the grip held.
        assert_eq!(ui_actions(&stick(hand, 0.0, 0.9, true)), [GUI_LSTICK_UP]);
        // The other hand's grip does not modify this stick.
        let mut c = stick(hand, 0.9, 0.0, false);
        c.hands[hand.other().index()].grip = down();
        assert_eq!(ui_actions(&c), [GUI_LSTICK_RIGHT]);
    }
}

#[test]
fn a_tab_push_stays_a_tab_push_when_the_grip_lets_go() {
    let mut b = Bindings::default();
    let step = b.sim_step(Mode::Ui, &stick(Hand::Right, 0.9, 0.0, true), DT, &CONFIG);
    assert_eq!(
        wants(&step, MapKind::Ui, GUI_TAB_NEXT),
        Some(Want::Hold(1.0))
    );
    let step = b.sim_step(Mode::Ui, &stick(Hand::Right, 0.9, 0.0, false), DT, &CONFIG);
    assert_eq!(
        wants(&step, MapKind::Ui, GUI_TAB_NEXT),
        Some(Want::Hold(1.0))
    );
    assert_eq!(wants(&step, MapKind::Ui, GUI_LSTICK_RIGHT), None);
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
    let step = b.sim_step(ON_FOOT, &system(true, true, false), DT, &CONFIG);
    assert!(step.desired.is_empty());
    let step = b.sim_step(ON_FOOT, &system(false, false, true), DT, &CONFIG);
    assert!(clicked(&step, GUI_PAUSE));
    assert!(!clicked(&step, GUI_PDA));
}

#[test]
fn a_tap_between_ticks_still_pauses() {
    let mut b = Bindings::default();
    b.sim_step(ON_FOOT, &system(false, true, true), DT, &CONFIG);
    let step = b.sim_step(ON_FOOT, &Controls::default(), DT, &CONFIG);
    assert!(clicked(&step, GUI_PAUSE));
}

#[test]
fn a_long_system_press_opens_the_commlink_once() {
    let mut b = Bindings::default();
    let held = system(true, false, false);
    let ticks = (CONFIG.long_press_s / DT).ceil() as usize + 1;
    let mut commlinks = 0;
    for _ in 0..ticks * 2 {
        let step = b.sim_step(ON_FOOT, &held, DT, &CONFIG);
        commlinks += usize::from(clicked(&step, GUI_PDA));
    }
    assert_eq!(commlinks, 1);
    let step = b.sim_step(ON_FOOT, &system(false, false, true), DT, &CONFIG);
    assert!(!clicked(&step, GUI_PAUSE));
}

#[test]
fn the_system_button_works_in_menus() {
    let mut b = Bindings::default();
    b.sim_step(Mode::Ui, &system(true, true, false), DT, &CONFIG);
    let step = b.sim_step(Mode::Ui, &Controls::default(), DT, &CONFIG);
    assert!(clicked(&step, GUI_PAUSE));
}

#[test]
fn with_trackpads_either_hands_halves_page_and_the_grips_do_not() {
    for dominant in Hand::BOTH {
        let mut c = controls(dominant, |dom, off| {
            dom.grip = down();
            off.grip = down();
        });
        c.trackpad_paging = true;
        let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
        assert_eq!(wants(&step, MapKind::Ui, GUI_TAB_PREV), None);
        assert_eq!(wants(&step, MapKind::Ui, GUI_TAB_NEXT), None);

        for hand in Hand::BOTH {
            let mut c = Controls {
                dominant,
                trackpad_paging: true,
                ..Default::default()
            };
            c.hands[hand.index()].trackpad_left = down();
            let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
            assert_eq!(
                wants(&step, MapKind::Ui, GUI_TAB_PREV),
                Some(Want::Hold(1.0))
            );
            assert_eq!(wants(&step, MapKind::Ui, GUI_TAB_NEXT), None);

            let mut c = Controls {
                dominant,
                trackpad_paging: true,
                ..Default::default()
            };
            c.hands[hand.index()].trackpad_right = down();
            let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
            assert_eq!(
                wants(&step, MapKind::Ui, GUI_TAB_NEXT),
                Some(Want::Hold(1.0))
            );
            assert_eq!(wants(&step, MapKind::Ui, GUI_TAB_PREV), None);
        }
    }
}

#[test]
fn without_trackpads_the_trackpad_halves_page_nothing() {
    let mut c = Controls::default();
    c.hands[0].trackpad_left = down();
    c.hands[1].trackpad_right = down();
    let step = Bindings::default().sim_step(Mode::Ui, &c, DT, &CONFIG);
    assert!(step.desired.is_empty());
}

#[test]
fn the_tables_consume_accepted_clicks_without_recognizing_a_second_chord() {
    let c = controls(Hand::Right, |dom, off| {
        dom.stick_click = down();
        off.stick_click = down();
    });
    let step = Bindings::default().sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, DETONATE_EXPLOSIVE), Some(Want::Hold(1.0)));
}
