use super::*;
use crate::controllers::bindings::tables::declare::WEAPON_SELECT;

const ALL_OFFERED: [bool; 4] = [true; 4];

const PRESSED: ButtonControl = ButtonControl {
    down: true,
    pressed: true,
    released: false,
};
const HELD: ButtonControl = ButtonControl {
    down: true,
    pressed: false,
    released: false,
};
const RELEASED: ButtonControl = ButtonControl {
    down: false,
    pressed: false,
    released: true,
};
const TAPPED: ButtonControl = ButtonControl {
    down: false,
    pressed: true,
    released: true,
};

fn config() -> WheelConfig {
    WheelConfig::new()
}

/// An aim pose turned `right_deg` to the right and `up_deg` up from straight ahead.
fn aimed(right_deg: f32, up_deg: f32) -> HandPose {
    HandPose {
        aim: Pose {
            position: Vec3::ZERO,
            orientation: Quat::from_rotation_y(-right_deg.to_radians())
                * Quat::from_rotation_x(up_deg.to_radians()),
        },
        grip: Vec3::new(-0.2, 1.2, -0.3),
    }
}

/// A tick on the left hand: its trackpad press and pose, with a sword wielded in the two-handed
/// slot and every arm offered.
fn input(pad: ButtonControl, zone: Option<PadZone>, pose: Option<HandPose>) -> WheelInput {
    let wielded = Some(WeaponSlot::E_SLOT_TWO_HANDED);
    WheelInput {
        acting: true,
        presses: [
            WheelPress {
                pad,
                zone,
                stick: 0.0,
            },
            WheelPress::default(),
        ],
        head: Quat::IDENTITY,
        hands: [pose, None],
        gate: true,
        offered: ALL_OFFERED,
        wielded,
        resting: resting(wielded, wielded, &ALL_OFFERED),
    }
}

fn open_centre(wheel: &mut Wheel) -> WheelOutput {
    wheel.step(&input(PRESSED, None, Some(aimed(0.0, 0.0))), &config())
}

#[test]
fn opening_holds_the_wielded_slot_so_nothing_changes_yet() {
    let mut wheel = Wheel::new();
    let out = open_centre(&mut wheel);
    assert_eq!(out.opened, Some((Opener::Trackpad, Hand::Left)));
    assert_eq!(out.hold, Some(WheelSlot::TwoHanded));
    assert_eq!(out.chosen, None);
    let out = wheel.step(&input(RELEASED, None, Some(aimed(0.0, 0.0))), &config());
    assert_eq!(out.closed, Some(Close::Cancel));
    assert_eq!(out.hold, None);
}

#[test]
fn pointing_at_an_arm_commits_it_on_release() {
    for (right, up, arm) in [
        (0.0, 20.0, WheelSlot::DualWield),
        (-20.0, 0.0, WheelSlot::Special),
        (0.0, -20.0, WheelSlot::Explosives),
    ] {
        let mut wheel = Wheel::new();
        open_centre(&mut wheel);
        let out = wheel.step(&input(HELD, None, Some(aimed(right, up))), &config());
        assert_eq!(out.hold, Some(arm));
        assert!(out.changed);
        let out = wheel.step(&input(RELEASED, None, Some(aimed(right, up))), &config());
        assert_eq!(out.closed, Some(Close::Commit(arm)));
    }
}

#[test]
fn retargeting_switches_the_held_action_without_a_tick_of_none() {
    let mut wheel = Wheel::new();
    open_centre(&mut wheel);
    let mut held = Vec::new();
    for (right, up) in [(0.0, 20.0), (25.0, 5.0), (5.0, -25.0)] {
        let out = wheel.step(&input(HELD, None, Some(aimed(right, up))), &config());
        held.push(out.hold);
    }
    assert_eq!(
        held,
        vec![
            Some(WheelSlot::DualWield),
            Some(WheelSlot::TwoHanded),
            Some(WheelSlot::Explosives)
        ]
    );
}

#[test]
fn returning_to_the_centre_cancels() {
    let mut wheel = Wheel::new();
    open_centre(&mut wheel);
    wheel.step(&input(HELD, None, Some(aimed(0.0, 20.0))), &config());
    let out = wheel.step(&input(HELD, None, Some(aimed(0.0, 2.0))), &config());
    assert!(out.changed);
    assert_eq!(out.chosen, None);
    assert_eq!(out.hold, Some(WheelSlot::TwoHanded));
    let out = wheel.step(&input(RELEASED, None, Some(aimed(0.0, 2.0))), &config());
    assert_eq!(out.closed, Some(Close::Cancel));
}

#[test]
fn releasing_on_the_wielded_slot_cancels() {
    let mut wheel = Wheel::new();
    open_centre(&mut wheel);
    wheel.step(&input(HELD, None, Some(aimed(20.0, 0.0))), &config());
    let out = wheel.step(&input(RELEASED, None, Some(aimed(20.0, 0.0))), &config());
    assert_eq!(out.closed, Some(Close::Cancel));
}

#[test]
fn an_arm_the_game_does_not_offer_counts_as_the_centre() {
    let mut wheel = Wheel::new();
    let mut tick = |pad, pose| {
        let mut i = input(pad, None, Some(pose));
        i.offered = [true, true, false, true];
        wheel.step(&i, &config())
    };
    tick(PRESSED, aimed(0.0, 0.0));
    let out = tick(HELD, aimed(-20.0, 0.0));
    assert_eq!(out.chosen, None);
    assert_eq!(out.hold, Some(WheelSlot::TwoHanded));
    let out = tick(RELEASED, aimed(-20.0, 0.0));
    assert_eq!(out.closed, Some(Close::Cancel));
}

#[test]
fn a_quadrant_tap_selects_its_arm_at_once() {
    let mut wheel = Wheel::new();
    let out = wheel.step(
        &input(TAPPED, Some(PadZone::Up), Some(aimed(0.0, 0.0))),
        &config(),
    );
    assert_eq!(out.hold, Some(WheelSlot::DualWield));
    let out = wheel.step(&input(RELEASED, None, Some(aimed(0.0, 0.0))), &config());
    assert_eq!(out.closed, Some(Close::Commit(WheelSlot::DualWield)));
}

#[test]
fn the_quadrant_seed_ends_once_the_hand_leaves_the_centre() {
    let mut wheel = Wheel::new();
    wheel.step(
        &input(PRESSED, Some(PadZone::Down), Some(aimed(0.0, 0.0))),
        &config(),
    );
    let out = wheel.step(
        &input(HELD, Some(PadZone::Down), Some(aimed(-20.0, 0.0))),
        &config(),
    );
    assert_eq!(out.chosen, Some(WheelSlot::Special));
    let out = wheel.step(
        &input(HELD, Some(PadZone::Down), Some(aimed(0.0, 0.0))),
        &config(),
    );
    assert_eq!(out.chosen, None);
}

#[test]
fn a_press_on_a_quadrant_line_seeds_once_the_thumb_moves_off() {
    let mut wheel = Wheel::new();
    wheel.step(&input(PRESSED, None, Some(aimed(0.0, 0.0))), &config());
    let out = wheel.step(
        &input(HELD, Some(PadZone::Left), Some(aimed(0.0, 0.0))),
        &config(),
    );
    assert_eq!(out.chosen, Some(WheelSlot::Special));
}

#[test]
fn a_closed_gate_refuses_until_the_opener_is_released() {
    let mut wheel = Wheel::new();
    let mut closed = input(PRESSED, None, Some(aimed(0.0, 0.0)));
    closed.gate = false;
    assert_eq!(wheel.step(&closed, &config()), WheelOutput::default());
    // The gate opening mid-press does not open the wheel.
    let out = wheel.step(&input(HELD, None, Some(aimed(0.0, 0.0))), &config());
    assert_eq!(out, WheelOutput::default());
    wheel.step(&input(RELEASED, None, None), &config());
    assert!(open_centre(&mut wheel).opened.is_some());
}

#[test]
fn the_gate_closing_mid_hold_cancels() {
    let mut wheel = Wheel::new();
    open_centre(&mut wheel);
    wheel.step(&input(HELD, None, Some(aimed(0.0, 20.0))), &config());
    let mut closed = input(HELD, None, Some(aimed(0.0, 20.0)));
    closed.gate = false;
    let out = wheel.step(&closed, &config());
    assert_eq!(out.closed, Some(Close::Cancel));
    assert_eq!(out.hold, None);
    assert!(!wheel.is_open());
}

#[test]
fn a_mode_change_mid_hold_cancels() {
    let mut wheel = Wheel::new();
    open_centre(&mut wheel);
    let mut away = input(HELD, None, Some(aimed(0.0, 20.0)));
    away.acting = false;
    let out = wheel.step(&away, &config());
    assert_eq!(out.closed, Some(Close::Cancel));
    assert_eq!(out.hold, None);
}

#[test]
fn an_untracked_hand_steers_from_where_it_is_first_seen() {
    let mut wheel = Wheel::new();
    wheel.step(&input(PRESSED, None, None), &config());
    let out = wheel.step(&input(HELD, None, Some(aimed(30.0, 0.0))), &config());
    assert_eq!(out.chosen, None);
    let out = wheel.step(&input(HELD, None, Some(aimed(30.0, 20.0))), &config());
    assert_eq!(out.chosen, Some(WheelSlot::DualWield));
}

#[test]
fn the_stick_opens_past_its_threshold_and_holds_down_to_its_release() {
    let config = config();
    let mut wheel = Wheel::new();
    let mut tick = |stick: f32| {
        let mut i = input(ButtonControl::default(), None, Some(aimed(0.0, 0.0)));
        i.presses = [
            WheelPress::default(),
            WheelPress {
                stick,
                ..Default::default()
            },
        ];
        i.hands = [None, Some(aimed(0.0, 0.0))];
        wheel.step(&i, &config)
    };
    assert_eq!(tick(0.6).opened, None);
    let out = tick(0.8);
    assert_eq!(out.opened, Some((Opener::Stick, Hand::Right)));
    assert!(out.by_stick);
    assert!(tick(0.5).closed.is_none());
    assert_eq!(tick(0.2).closed, Some(Close::Cancel));
}

#[test]
fn a_disabled_opener_never_opens() {
    let mut config = config();
    config.trackpad = false;
    let mut wheel = Wheel::new();
    let out = wheel.step(&input(PRESSED, Some(PadZone::Up), None), &config);
    assert_eq!(out, WheelOutput::default());
}

#[test]
fn the_resting_slot() {
    for (wielded, selected, offered, expected) in [
        // The wielded arm.
        (
            Some(WeaponSlot::E_SLOT_TWO_HANDED),
            None,
            [true; 4],
            Some(WheelSlot::TwoHanded),
        ),
        // The selected arm with nothing wielded.
        (
            None,
            Some(WeaponSlot::E_SLOT_DUAL_WIELD),
            [true; 4],
            Some(WheelSlot::DualWield),
        ),
        // The first offered arm under a mounted gun.
        (
            Some(WeaponSlot::E_SLOT_HEAVY),
            None,
            [false, false, true, true],
            Some(WheelSlot::Special),
        ),
        // Nothing with nothing offered.
        (None, None, [false; 4], None),
    ] {
        assert_eq!(
            resting(wielded, selected, &offered),
            expected,
            "{wielded:?}, {selected:?}"
        );
    }
}

#[test]
fn every_arm_round_trips_through_its_action_slot_and_zone() {
    for arm in WheelSlot::ALL {
        assert_eq!(WheelSlot::of_action(arm.action()), Some(arm));
        assert_eq!(WheelSlot::of_inventory(arm.inventory_slot()), Some(arm));
    }
    for (zone, action) in WEAPON_SELECT {
        assert_eq!(WheelSlot::of_zone(zone).action(), action);
    }
}
