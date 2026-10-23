//! On-foot bindings: face buttons, sticks, grab volumes, weapons, the grapple, crouch, and the
//! sight chord.

use super::*;

#[test]
fn nothing_pressed_wants_nothing() {
    let mut b = Bindings::default();
    let step = b.sim_step(ON_FOOT, &Controls::default(), DT, &CONFIG);
    assert!(step.desired.is_empty());
    assert_eq!(step.body_turn, 0.0);
}

#[test]
fn on_foot_face_buttons_follow_the_plan() {
    let mut b = Bindings::default();
    let c = controls(Hand::Right, |dom, off| {
        dom.a = down();
        off.b = down();
    });
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    for action in [JUMP, OPEN_PARACHUTE, STUNT_JUMP, FIRE_GRAPPLE, L1] {
        assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{action:?}");
    }
    // The off-hand's upper face is the grapple; the tethers' release is its double tap.
    for action in [RELEASE_GRAPPLE, CANCEL, PUSH_GRAPPLE, RELOAD] {
        assert_eq!(player(&step, action), None, "{action:?}");
    }
}

#[test]
fn the_dominant_stick_click_only_detonates() {
    for dominant in Hand::BOTH {
        let c = controls(dominant, |dom, _| dom.stick_click = down());
        for mode in [
            ON_FOOT,
            Mode::Airborne(AirborneVariant::Falling),
            Mode::Airborne(AirborneVariant::Parachute),
        ] {
            let mut b = Bindings::default();
            let step = b.sim_step(mode, &c, DT, &CONFIG);
            assert_eq!(
                player(&step, DETONATE_EXPLOSIVE),
                Some(Want::Hold(1.0)),
                "{mode:?}"
            );
            // Grenades come from the pouch and C4 from its own pouch: neither is on the click.
            for action in [THROW_GRENADE, PLANT_EXPLOSIVE, DETONATE_EXPLOSIVE_TAP, R1] {
                assert_eq!(player(&step, action), None, "{mode:?}: {action:?}");
            }
        }
    }
}

#[test]
fn the_dominant_hand_mirrors_the_layout() {
    let mut b = Bindings::default();
    let right_handed = controls(Hand::Right, |dom, off| {
        dom.trigger_press = down();
        off.b = down();
    });
    let left_handed = controls(Hand::Left, |dom, off| {
        dom.trigger_press = down();
        off.b = down();
    });
    // The same logical presses land on opposite physical hands, and fire the same actions.
    assert!(right_handed.hands[1].trigger_press.down);
    assert!(left_handed.hands[0].trigger_press.down);
    for c in [right_handed, left_handed] {
        let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
        assert_eq!(player(&step, FIRE_RIGHT), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
        b = Bindings::default();
    }
}

#[test]
fn the_move_stick_splits_into_half_axes_and_rotates() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.0, 0.5));
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, MOVE_FORWARD), Some(Want::Analog(0.5)));
    assert_eq!(player(&step, MOVE_BACKWARD), None);
    assert_eq!(player(&step, MOVE_ALL), Some(Want::Analog(0.5)));

    c.move_rotation = std::f32::consts::FRAC_PI_2;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    let Some(Want::Analog(left)) = player(&step, MOVE_LEFT) else {
        panic!("no left move");
    };
    assert!((left - 0.5).abs() < 1e-5);
    assert!(matches!(
        player(&step, MOVE_FORWARD),
        None | Some(Want::Analog(_))
    ));
}

#[test]
fn the_right_stick_turns_the_body_on_foot_but_yaws_a_helicopter() {
    let mut b = Bindings::default();
    let c = controls(Hand::Right, |dom, _| {
        dom.stick_raw = Vec2::new(0.8, 0.0);
        dom.stick = Vec2::new(0.76, 0.0);
    });
    assert_eq!(b.sim_step(ON_FOOT, &c, DT, &CONFIG).body_turn, 0.8);
    let heli = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Helicopter,
        driver: true,
    });
    let step = b.sim_step(heli, &c, DT, &CONFIG);
    assert_eq!(step.body_turn, 0.0);
    assert_eq!(player(&step, HELI_TURN_RIGHT), Some(Want::Analog(0.76)));
}

#[test]
fn a_grip_press_in_reach_grabs_once_and_holds_through_the_press() {
    let mut b = Bindings::default();
    let grab = controls(Hand::Right, |_, off| {
        off.grip = down();
        off.grab_volume = Some(GrabVolume::Holster(Socket::Chest));
    });
    let step = b.sim_step(ON_FOOT, &grab, DT, &CONFIG);
    assert!(step.desired.is_empty(), "{:?}", step.desired);
    assert_eq!(
        step.grabs[Hand::Left.index()],
        Some(GrabVolume::Holster(Socket::Chest))
    );
    // Leaving the target mid-press keeps the press a grab, reported once.
    let held = |c: &mut HandControls| {
        c.grip = ButtonControl {
            down: true,
            ..Default::default()
        };
        c.grab_slow = true;
    };
    let left_volume = controls(Hand::Right, |_, off| held(off));
    let step = b.sim_step(ON_FOOT, &left_volume, DT, &CONFIG);
    assert!(step.desired.is_empty());
    assert_eq!(step.grabs, [None; 2]);
}

#[test]
fn an_empty_grip_on_foot_is_a_fist_that_takes_what_it_comes_to() {
    let mut b = Bindings::default();
    let fist = controls(Hand::Right, |_, off| off.grip = down());
    let step = b.sim_step(ON_FOOT, &fist, DT, &CONFIG);
    assert!(
        step.desired.is_empty(),
        "a fist fires nothing: {:?}",
        step.desired
    );
    assert_eq!(step.grabs, [None; 2]);
    // Still closed, laid on the grenade pouch: it takes it.
    let on_pouch = controls(Hand::Right, |_, off| {
        off.grip = ButtonControl {
            down: true,
            ..Default::default()
        };
        off.grab_volume = Some(GrabVolume::GrenadePouch);
        off.grab_slow = true;
    });
    let step = b.sim_step(ON_FOOT, &on_pouch, DT, &CONFIG);
    assert_eq!(
        step.grabs[Hand::Left.index()],
        Some(GrabVolume::GrenadePouch)
    );
    assert!(step.desired.is_empty());
}

#[test]
fn a_tap_between_ticks_in_a_volume_grabs_once() {
    let mut b = Bindings::default();
    let tap = controls(Hand::Right, |dom, _| {
        dom.grip = ButtonControl {
            down: false,
            pressed: true,
            released: true,
        };
        dom.grab_volume = Some(GrabVolume::Foregrip);
    });
    let step = b.sim_step(ON_FOOT, &tap, DT, &CONFIG);
    assert_eq!(step.grabs[Hand::Right.index()], Some(GrabVolume::Foregrip));
    let step = b.sim_step(ON_FOOT, &Controls::default(), DT, &CONFIG);
    assert_eq!(step.grabs, [None; 2]);
}

#[test]
fn the_stick_click_cycles_weapons_only_as_the_fallback() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |_, off| off.stick_click = down());
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, NEXT_WEAPON), None);
    c.weapon_cycle = true;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, NEXT_WEAPON), Some(Want::Click));
}

#[test]
fn dual_wielding_moves_the_off_trigger_to_its_pistol_and_retract_to_the_stick_click() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |_, off| off.trigger_press = down());
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, RETRACT_GRAPPLE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, FIRE_RIGHT), None);

    c.dual_wield = true;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, FIRE_RIGHT), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, RETRACT_GRAPPLE), None);

    let mut c = controls(Hand::Right, |_, off| off.stick_click = down());
    c.dual_wield = true;
    c.weapon_cycle = true;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, RETRACT_GRAPPLE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, NEXT_WEAPON), None);
}

#[test]
fn a_yank_presses_the_grapple_for_one_tick() {
    let mut b = Bindings::default();
    let mut c = Controls {
        yank: true,
        ..Default::default()
    };
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
    c.yank = false;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(player(&step, FIRE_GRAPPLE), None);
}

#[test]
fn a_crouch_flick_fires_once_until_the_stick_returns() {
    let mut fired = false;
    assert!(!flick_down(&mut fired, -0.5, 0.8));
    assert!(flick_down(&mut fired, -0.9, 0.8));
    assert!(!flick_down(&mut fired, -1.0, 0.8));
    assert!(!flick_down(&mut fired, -0.3, 0.8));
    assert!(flick_down(&mut fired, -0.85, 0.8));
    assert!(!flick_down(&mut fired, f32::NAN, 0.8));
}

#[test]
fn the_sight_chord_captures_without_jumping_or_grappling() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |dom, off| {
        dom.grip = down();
        off.grip = down();
        dom.a = down();
    });
    c.sight_chord = true;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert!(step.sight_capture);
    assert_eq!(player(&step, JUMP), None);
    assert_eq!(player(&step, FIRE_GRAPPLE), None);
}

#[test]
fn the_sight_chord_captures_only_on_the_press() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |dom, off| {
        dom.grip = down();
        off.grip = down();
        dom.a = ButtonControl {
            down: true,
            ..Default::default()
        };
    });
    c.sight_chord = true;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert!(!step.sight_capture);
    assert_eq!(player(&step, JUMP), None);
}

#[test]
fn a_disarmed_sight_chord_leaves_the_bindings_alone() {
    let mut b = Bindings::default();
    let c = controls(Hand::Right, |dom, off| {
        dom.grip = down();
        off.grip = down();
        dom.a = down();
    });
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert!(!step.sight_capture);
    assert_eq!(player(&step, JUMP), Some(Want::Hold(1.0)));
    // The grips are fists, which fire nothing.
    assert_eq!(player(&step, FIRE_GRAPPLE), None);
}

#[test]
fn the_sight_chord_keeps_the_foregrip() {
    let chord = |armed: bool| {
        let mut c = controls(Hand::Right, |dom, off| {
            dom.grip = ButtonControl {
                down: true,
                ..Default::default()
            };
            off.grip = down();
            off.grab_volume = Some(GrabVolume::Foregrip);
        });
        c.sight_chord = armed;
        c
    };
    let mut b = Bindings::default();
    let step = b.sim_step(ON_FOOT, &chord(true), DT, &CONFIG);
    assert_eq!(step.grabs, [None; 2]);
    assert_eq!(player(&step, FIRE_GRAPPLE), None);
    // Without the chord armed, the same press is a foregrip grab.
    let mut b = Bindings::default();
    let step = b.sim_step(ON_FOOT, &chord(false), DT, &CONFIG);
    assert_eq!(step.grabs[Hand::Left.index()], Some(GrabVolume::Foregrip));
}

#[test]
fn the_sight_chord_needs_both_grips() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |_, off| {
        off.grip = down();
    });
    c.hands[1].a = down();
    c.sight_chord = true;
    let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert!(!step.sight_capture);
    assert_eq!(player(&step, JUMP), Some(Want::Hold(1.0)));
}

#[test]
fn the_use_button_steps_off_ledges_and_keeps_a_ragdoll() {
    for dominant in Hand::BOTH {
        let mut c = controls(dominant, |dom, _| dom.b = down());
        let step = Bindings::default().sim_step(ON_FOOT, &c, DT, &CONFIG);
        assert_eq!(player(&step, BYPASS_FALL_PREVENTION), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, ENTER_VEHICLE), Some(Want::Hold(1.0)));
        // Cancel only while a ragdoll: elsewhere it cancels other things.
        assert_eq!(player(&step, CANCEL), None);
        c.ragdolling = true;
        for mode in [ON_FOOT, Mode::Airborne(AirborneVariant::Falling)] {
            let step = Bindings::default().sim_step(mode, &c, DT, &CONFIG);
            assert_eq!(player(&step, CANCEL), Some(Want::Hold(1.0)), "{mode:?}");
        }
    }
}

#[test]
fn the_right_b_held_skips_a_cutscene_outside_the_menus() {
    for dominant in Hand::BOTH {
        let c = controls(dominant, |dom, off| match dominant {
            Hand::Right => dom.b = down(),
            Hand::Left => off.b = down(),
        });
        for mode in [
            ON_FOOT,
            Mode::Airborne(AirborneVariant::Wingsuit),
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Helicopter,
                driver: true,
            }),
        ] {
            let mut b = Bindings::default();
            let step = b.sim_step(mode, &c, DT, &CONFIG);
            assert_eq!(
                wants(&step, MapKind::Ui, SKIP_CUTSCENE),
                Some(Want::Hold(1.0)),
                "{dominant:?} {mode:?}"
            );
        }
        let mut b = Bindings::default();
        let step = b.sim_step(Mode::Ui, &c, DT, &CONFIG);
        assert_eq!(wants(&step, MapKind::Ui, SKIP_CUTSCENE), None);
    }
    // The left B is not the skip.
    let c = controls(Hand::Right, |_, off| off.b = down());
    let step = Bindings::default().sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert_eq!(wants(&step, MapKind::Ui, SKIP_CUTSCENE), None);
}
