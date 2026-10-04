use std::time::Instant;

use jc3gi::input::input_action_map::Action;

use super::*;
use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            intents::simulate::simulate,
            tables::{Bindings, Step},
        },
        config::BindingsConfig,
        frame::ActionSetKind,
        mode::{Mode, OnFootVariant, VehicleClass, VehicleSeat},
    },
    vr::input::semantic::{SemanticSet, defaults::default_binding},
};

const DT: f32 = 0.05;
const WINDOW: f32 = 0.35;

const UP: ButtonControl = ButtonControl {
    down: false,
    pressed: false,
    released: false,
};
const PRESS: ButtonControl = ButtonControl {
    down: true,
    pressed: true,
    released: false,
};
const HELD: ButtonControl = ButtonControl {
    down: true,
    pressed: false,
    released: false,
};
const RELEASE: ButtonControl = ButtonControl {
    down: false,
    pressed: false,
    released: true,
};

fn input(click: ButtonControl, b: ButtonControl) -> ChordInput {
    ChordInput {
        click,
        other_click: UP,
        b,
    }
}

#[test]
fn a_b_press_within_the_window_toggles_once_and_masks_both_until_released() {
    let mut chord = PanelChord::new();
    assert_eq!(
        chord.step(input(PRESS, UP), DT, WINDOW),
        ChordTick::default()
    );
    assert_eq!(
        chord.step(input(HELD, UP), DT, WINDOW),
        ChordTick::default()
    );
    let fired = chord.step(input(HELD, PRESS), DT, WINDOW);
    assert_eq!(
        fired,
        ChordTick {
            toggle: true,
            mask_click: true,
            mask_b: true,
        }
    );
    let held = chord.step(input(HELD, HELD), DT, WINDOW);
    assert!(!held.toggle && held.mask_click && held.mask_b);
    // The B's release tick is masked too, so the bindings never see its release edge.
    let b_up = chord.step(input(HELD, RELEASE), DT, WINDOW);
    assert!(!b_up.toggle && b_up.mask_click && b_up.mask_b);
    // A second B press on the same stick click is the B's own.
    let again = chord.step(input(HELD, PRESS), DT, WINDOW);
    assert!(!again.toggle && again.mask_click && !again.mask_b);
    let click_up = chord.step(input(RELEASE, HELD), DT, WINDOW);
    assert!(click_up.mask_click);
    assert_eq!(chord.step(input(UP, UP), DT, WINDOW), ChordTick::default());
}

#[test]
fn a_click_and_b_pressed_together_are_the_chord() {
    let mut chord = PanelChord::new();
    assert!(chord.step(input(PRESS, PRESS), DT, WINDOW).toggle);
}

#[test]
fn a_b_press_after_the_window_is_not_the_chord() {
    let mut chord = PanelChord::new();
    chord.step(input(PRESS, UP), DT, WINDOW);
    let window_ticks = (WINDOW / DT).ceil() as usize;
    for _ in 0..window_ticks {
        chord.step(input(HELD, UP), DT, WINDOW);
    }
    assert_eq!(
        chord.step(input(HELD, PRESS), DT, WINDOW),
        ChordTick::default()
    );
}

#[test]
fn b_alone_or_first_is_not_the_chord() {
    let mut chord = PanelChord::new();
    assert_eq!(
        chord.step(input(UP, PRESS), DT, WINDOW),
        ChordTick::default()
    );
    assert_eq!(
        chord.step(input(PRESS, HELD), DT, WINDOW),
        ChordTick::default()
    );
}

#[test]
fn the_recenter_chord_takes_the_stick_click() {
    let mut chord = PanelChord::new();
    chord.step(input(PRESS, UP), DT, WINDOW);
    let both = ChordInput {
        click: HELD,
        other_click: PRESS,
        b: UP,
    };
    chord.step(both, DT, WINDOW);
    let late_b = ChordInput {
        b: PRESS,
        other_click: HELD,
        ..both
    };
    assert_eq!(chord.step(late_b, DT, WINDOW), ChordTick::default());
    // Even with the other stick click let go, the stick click stays the recenter chord's.
    assert_eq!(
        chord.step(input(HELD, PRESS), DT, WINDOW),
        ChordTick::default()
    );
}

#[test]
fn masking_clears_the_buttons_and_their_edges() {
    let mut frame = frame();
    let mut edges = Edges::default();
    for hand in Hand::BOTH {
        let i = hand.index();
        for button in [Button::ThumbstickClick, Button::BClick] {
            frame.hands[i].buttons.set(button, true);
            edges.pressed[i].set(button, true);
        }
    }
    mask(
        &mut frame,
        &mut edges,
        ChordTick {
            toggle: false,
            mask_click: true,
            mask_b: true,
        },
    );
    assert!(
        !frame
            .hand(CLICK_HAND)
            .buttons
            .contains(Button::ThumbstickClick)
    );
    assert!(!edges.pressed(CLICK_HAND, Button::ThumbstickClick));
    assert!(!frame.hand(B_HAND).buttons.contains(Button::BClick));
    assert!(!edges.pressed(B_HAND, Button::BClick));
    // The other hand's stick click and B are not the chord's.
    assert!(frame.hand(B_HAND).buttons.contains(Button::ThumbstickClick));
    assert!(frame.hand(CLICK_HAND).buttons.contains(Button::BClick));
}

/// The physical buttons on one tick: the left stick click, the right stick click, and the right B.
type Buttons = (bool, bool, bool);

/// Run `ticks` through the chord and the tables as the input tick does, and return each tick's
/// table step and whether the chord toggled the panel.
fn run(mode: Mode, dominant: Hand, ticks: &[Buttons]) -> Vec<(Step, bool)> {
    let config = BindingsConfig::new();
    let mut chord = PanelChord::new();
    let mut bindings = Bindings::new();
    let mut previous = (false, false, false);
    let mut out = Vec::new();
    for &(left, right, b) in ticks {
        let mut frame = frame();
        let mut edges = Edges::default();
        let mut set = |hand: Hand, button: Button, now: bool, before: bool| {
            let i = hand.index();
            frame.hands[i].buttons.set(button, now);
            edges.pressed[i].set(button, now && !before);
            edges.released[i].set(button, !now && before);
        };
        set(Hand::Left, Button::ThumbstickClick, left, previous.0);
        set(Hand::Right, Button::ThumbstickClick, right, previous.1);
        set(Hand::Right, Button::BClick, b, previous.2);
        previous = (left, right, b);

        let tick = chord.step(
            ChordInput::of(&frame, &edges),
            DT,
            config.panel_chord_window_s,
        );
        if tick.toggle {
            bindings.cancel_stick_clicks();
        }
        mask(&mut frame, &mut edges, tick);
        let mut controls = Controls::from_frame(&frame, &edges, &config, dominant, 0.0, [None; 2]);
        controls.weapon_cycle = true;
        controls.masked_inputs = masked_inputs(tick);
        let input = simulate(
            &controls,
            SemanticSet::of(mode.action_set()),
            default_binding,
        );
        out.push((
            bindings.step(mode, &controls, &input, DT, &config),
            tick.toggle,
        ));
    }
    out
}

fn frame() -> ControllerFrame {
    ControllerFrame {
        sequence: 0,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    }
}

/// The left stick click pressed and held, the right B pressed within the window, held, and let go,
/// and then the stick click let go, over many ticks.
const CHORD: &[Buttons] = &[
    (true, false, false),
    (true, false, false),
    (true, false, true),
    (true, false, true),
    (true, false, true),
    (true, false, false),
    (true, false, false),
    (true, false, false),
    (true, false, false),
    (true, false, false),
    (true, false, false),
    (false, false, false),
    (false, false, false),
];

fn modes() -> [Mode; 3] {
    [
        Mode::OnFoot(OnFootVariant::Ground),
        Mode::Ui,
        Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver: true,
        }),
    ]
}

#[test]
fn the_chord_toggles_once_and_leaks_neither_press_in_any_mode() {
    for mode in modes() {
        for dominant in Hand::BOTH {
            let steps = run(mode, dominant, CHORD);
            assert_eq!(
                steps.iter().filter(|(_, toggled)| *toggled).count(),
                1,
                "{mode:?} {dominant:?}"
            );
            for (step, _) in &steps {
                assert!(
                    step.desired.is_empty(),
                    "{mode:?} {dominant:?}: {:?}",
                    step.desired
                );
                assert!(!step.recenter);
            }
        }
    }
}

#[test]
fn a_lone_left_stick_click_still_acts_after_the_window() {
    let mut ticks = vec![(true, false, false); 12];
    ticks.push((false, false, false));
    let steps = run(Mode::OnFoot(OnFootVariant::Ground), Hand::Right, &ticks);
    assert!(steps.iter().all(|(_, toggled)| !toggled));
    let stepped = steps
        .iter()
        .position(|(step, _)| {
            step.desired
                .iter()
                .any(|d| d.target.action == Action::NEXT_WEAPON)
        })
        .expect("the weapon steps");
    assert!(stepped as f32 * DT >= BindingsConfig::new().panel_chord_window_s - DT);
}

#[test]
fn a_late_b_is_the_b_and_both_stick_clicks_still_recenter() {
    let mut late = vec![(true, false, false); 12];
    late.extend([
        (true, false, true),
        (true, false, false),
        (false, false, false),
    ]);
    let steps = run(Mode::Ui, Hand::Right, &late);
    assert!(steps.iter().all(|(_, toggled)| !toggled));
    assert!(steps.iter().any(|(step, _)| {
        step.desired
            .iter()
            .any(|d| d.target.action == Action::GUI_CANCEL)
    }));

    let mut recenter = vec![(true, false, false), (true, true, false)];
    recenter.extend(std::iter::repeat_n((true, true, false), 30));
    recenter.push((true, true, true));
    recenter.extend([(false, false, false); 2]);
    let steps = run(Mode::OnFoot(OnFootVariant::Ground), Hand::Right, &recenter);
    assert!(steps.iter().all(|(_, toggled)| !toggled));
    assert_eq!(steps.iter().filter(|(step, _)| step.recenter).count(), 1);
}
