//! Vehicle, wingsuit, and parachute bindings.

use super::*;

#[test]
fn the_car_uses_triggers_as_throttle() {
    let mut b = Bindings::default();
    let car = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    });
    let c = controls(Hand::Right, |dom, off| {
        dom.trigger = 0.6;
        dom.stick_click = down();
        off.stick = Vec2::new(-0.4, 0.0);
        off.a = down();
        off.b = down();
    });
    let step = b.sim_step(car, &c, DT, &CONFIG);
    assert_eq!(player(&step, ACCELERATE), Some(Want::Analog(0.6)));
    assert_eq!(player(&step, TURN_LEFT), Some(Want::Analog(0.4)));
    assert_eq!(player(&step, TURN_RIGHT), None);
    assert_eq!(player(&step, HANDBRAKE), Some(Want::Hold(1.0)));
    // Nitrous moved to the dominant stick click; the off-hand's upper face is the grapple.
    assert_eq!(player(&step, USE_VEHICLE_MOD), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, SUBMERSIBLE_DIVE), None);
    // The off-hand's grip fires the secondary weapon alone, not the grapple.
    let mut b = Bindings::default();
    let grip = controls(Hand::Right, |_, off| off.grip = down());
    let step = b.sim_step(car, &grip, DT, &CONFIG);
    assert_eq!(
        player(&step, FIRE_VEHICLE_WEAPON_SECONDARY),
        Some(Want::Hold(1.0))
    );
    assert_eq!(player(&step, FIRE_GRAPPLE), None);
}

#[test]
fn the_mech_punches_jumps_and_fires_at_the_gamepads_places() {
    let mech = |driver| {
        Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Mech,
            driver,
        })
    };
    let c = controls(Hand::Right, |dom, off| {
        dom.trigger = 1.0;
        dom.trigger_press = down();
        off.trigger = 1.0;
        off.trigger_press = down();
        dom.a = down();
        off.stick = Vec2::new(0.0, 1.0);
    });
    let step = Bindings::default().sim_step(mech(true), &c, DT, &CONFIG);
    for action in [
        MECH_FIRE_RIGHT_HAND_WEAPON,
        MECH_FIRE_GRAVITY_WEAPON_PRIMARY,
        MECH_JUMP,
    ] {
        assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{action:?}");
    }
    // The stick walks it forward, as the mech's move reads the plane's pitch.
    assert_eq!(player(&step, PLANE_PITCH_DOWN), Some(Want::Analog(1.0)));
    for action in [ACCELERATE, REVERSE, HANDBRAKE] {
        assert_eq!(player(&step, action), None, "{action:?}");
    }
    let grips = controls(Hand::Right, |dom, off| {
        dom.grip = down();
        off.grip = down();
    });
    let step = Bindings::default().sim_step(mech(true), &grips, DT, &CONFIG);
    assert_eq!(player(&step, MECH_PUNCH), Some(Want::Hold(1.0)));
    assert_eq!(
        player(&step, MECH_FIRE_GRAVITY_WEAPON_SECONDARY),
        Some(Want::Hold(1.0))
    );
    // Out of the driver's seat the mech's own actions are not the player's.
    let step = Bindings::default().sim_step(mech(false), &c, DT, &CONFIG);
    assert_eq!(player(&step, MECH_FIRE_RIGHT_HAND_WEAPON), None);
}

#[test]
fn a_double_tap_in_a_vehicle_releases_the_vehicles_tethers() {
    let car = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    });
    let up = ButtonControl {
        down: false,
        pressed: false,
        released: true,
    };
    let tick = |b| {
        let mut c = controls(Hand::Right, |_, off| off.b = b);
        c.grapple.tethers = true;
        c
    };
    let mut bindings = Bindings::default();
    let released: Vec<Step> = [down(), up, down(), up]
        .into_iter()
        .map(|b| bindings.sim_step(car, &tick(b), DT, &CONFIG))
        .collect();
    for action in [VEHICLE_RELEASE_GRAPPLE, RELEASE_GRAPPLE] {
        assert!(
            released
                .iter()
                .any(|s| player(s, action) == Some(Want::Click)),
            "{action:?}"
        );
    }
    assert!(released.iter().all(|s| player(s, FIRE_GRAPPLE).is_none()));
}

#[test]
fn the_plane_pitches_down_on_stick_forward() {
    let mut b = Bindings::default();
    let plane = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Plane,
        driver: true,
    });
    let c = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.0, 1.0));
    let step = b.sim_step(plane, &c, DT, &CONFIG);
    assert_eq!(player(&step, PLANE_PITCH_DOWN), Some(Want::Analog(1.0)));
    assert_eq!(player(&step, PLANE_PITCH_UP), None);
}

#[test]
fn the_wingsuit_evades_on_a_flicked_a() {
    let mut b = Bindings::default();
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    let flick = controls(Hand::Right, |dom, off| {
        dom.a = down();
        off.stick = Vec2::new(0.9, 0.0);
    });
    let step = b.sim_step(wingsuit, &flick, DT, &CONFIG);
    assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, OPEN_PARACHUTE), None);
    // The latch holds for the press even once the stick recentres.
    let held = controls(Hand::Right, |dom, _| dom.a = down());
    let step = b.sim_step(wingsuit, &held, DT, &CONFIG);
    assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
    // A fresh press without the flick opens the parachute.
    b.sim_step(wingsuit, &Controls::default(), DT, &CONFIG);
    let step = b.sim_step(wingsuit, &held, DT, &CONFIG);
    assert_eq!(player(&step, OPEN_PARACHUTE), Some(Want::Hold(1.0)));
}

#[test]
fn the_wingsuit_closes_on_the_button_that_opened_it_and_boosts_on_the_stick_click() {
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    for dominant in Hand::BOTH {
        let b_press = controls(dominant, |dom, _| dom.b = down());
        let on_foot = Bindings::default().sim_step(ON_FOOT, &b_press, DT, &CONFIG);
        assert_eq!(player(&on_foot, OPEN_WINGSUIT), Some(Want::Hold(1.0)));
        let step = Bindings::default().sim_step(wingsuit, &b_press, DT, &CONFIG);
        assert_eq!(player(&step, OPEN_WINGSUIT), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, WINGSUIT_BOOST), None);

        let click = controls(dominant, |dom, _| dom.stick_click = down());
        let step = Bindings::default().sim_step(wingsuit, &click, DT, &CONFIG);
        assert_eq!(player(&step, WINGSUIT_BOOST), Some(Want::Hold(1.0)));

        // The off-hand A, the reload on foot, does nothing in the suit.
        let x = controls(dominant, |_, off| off.a = down());
        let step = Bindings::default().sim_step(wingsuit, &x, DT, &CONFIG);
        assert!(step.desired.is_empty(), "{:?}", step.desired);
    }
}

#[test]
fn a_stick_held_over_in_the_wingsuit_lets_a_open_the_parachute() {
    let mut b = Bindings::default();
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    let banked = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.9, 0.0));
    let ticks = (CONFIG.evade_window_s / DT).ceil() as usize + 2;
    for _ in 0..ticks {
        b.sim_step(wingsuit, &banked, DT, &CONFIG);
    }
    let mut press = banked;
    press.hands[Hand::Right.index()].a = down();
    let step = b.sim_step(wingsuit, &press, DT, &CONFIG);
    assert_eq!(player(&step, OPEN_PARACHUTE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, WINGSUIT_EVADE), None);
}

#[test]
fn a_flick_out_of_rest_evades_within_the_window() {
    let mut b = Bindings::default();
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    b.sim_step(wingsuit, &Controls::default(), DT, &CONFIG);
    let pushed = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.0, -0.9));
    b.sim_step(wingsuit, &pushed, DT, &CONFIG);
    let mut press = pushed;
    press.hands[Hand::Right.index()].a = down();
    let step = b.sim_step(wingsuit, &press, DT, &CONFIG);
    assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
}

#[test]
fn falling_and_the_parachute_keep_the_hand_weapon_bindings() {
    let c = controls(Hand::Right, |dom, off| {
        dom.trigger_press = down();
        off.a = down();
        dom.stick_click = down();
    });
    for variant in [AirborneVariant::Falling, AirborneVariant::Parachute] {
        let mut b = Bindings::default();
        let step = b.sim_step(Mode::Airborne(variant), &c, DT, &CONFIG);
        for action in [FIRE_RIGHT, R2, RELOAD, DETONATE_EXPLOSIVE] {
            assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{action:?}");
        }
        for action in [FIRE_WINGSUIT_WEAPON_MAIN, WINGSUIT_BOOST] {
            assert_eq!(player(&step, action), None, "{action:?}");
        }
    }
}

#[test]
fn the_wingsuit_fires_its_own_weapons_and_boosts() {
    let mut b = Bindings::default();
    let c = controls(Hand::Right, |dom, _| {
        dom.trigger_press = down();
        dom.b = down();
        dom.stick_click = down();
    });
    let step = b.sim_step(Mode::Airborne(AirborneVariant::Wingsuit), &c, DT, &CONFIG);
    for action in [FIRE_WINGSUIT_WEAPON_MAIN, WINGSUIT_BOOST] {
        assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{action:?}");
    }
    for action in [FIRE_RIGHT, RELOAD, DETONATE_EXPLOSIVE] {
        assert_eq!(player(&step, action), None, "{action:?}");
    }
}

#[test]
fn dual_wielding_in_the_air_moves_the_off_trigger_to_its_pistol() {
    for variant in [AirborneVariant::Falling, AirborneVariant::Parachute] {
        let mode = Mode::Airborne(variant);
        let mut b = Bindings::default();
        let mut c = controls(Hand::Right, |_, off| off.trigger_press = down());
        let step = b.sim_step(mode, &c, DT, &CONFIG);
        assert_eq!(player(&step, RETRACT_GRAPPLE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, FIRE_RIGHT), None);

        c.dual_wield = true;
        let step = b.sim_step(mode, &c, DT, &CONFIG);
        assert_eq!(player(&step, FIRE_RIGHT), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, RETRACT_GRAPPLE), None);

        let mut c = controls(Hand::Right, |_, off| off.stick_click = down());
        c.dual_wield = true;
        let step = b.sim_step(mode, &c, DT, &CONFIG);
        assert_eq!(player(&step, RETRACT_GRAPPLE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, FIRE_RIGHT), None);
    }
}

#[test]
fn a_dual_wielding_passenger_fires_from_either_trigger() {
    let mode = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: false,
    });
    let mut b = Bindings::default();
    let mut c = controls(Hand::Left, |_, off| off.trigger_press = down());
    let step = b.sim_step(mode, &c, DT, &CONFIG);
    assert_eq!(player(&step, FIRE_RIGHT), None);
    c.dual_wield = true;
    let step = b.sim_step(mode, &c, DT, &CONFIG);
    assert_eq!(player(&step, FIRE_RIGHT), Some(Want::Hold(1.0)));
}
