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
        off.stick = Vec2::new(-0.4, 0.0);
        off.b = down();
        dom.b = down();
    });
    let step = b.step(car, &c, DT, &CONFIG);
    assert_eq!(player(&step, ACCELERATE), Some(Want::Analog(0.6)));
    assert_eq!(player(&step, TURN_LEFT), Some(Want::Analog(0.4)));
    assert_eq!(player(&step, TURN_RIGHT), None);
    assert_eq!(player(&step, HANDBRAKE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, USE_VEHICLE_MOD), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, SUBMERSIBLE_DIVE), None);
}

#[test]
fn the_plane_pitches_down_on_stick_forward() {
    let mut b = Bindings::default();
    let plane = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Plane,
        driver: true,
    });
    let c = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.0, 1.0));
    let step = b.step(plane, &c, DT, &CONFIG);
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
    let step = b.step(wingsuit, &flick, DT, &CONFIG);
    assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, OPEN_PARACHUTE), None);
    // The latch holds for the press even once the stick recentres.
    let held = controls(Hand::Right, |dom, _| dom.a = down());
    let step = b.step(wingsuit, &held, DT, &CONFIG);
    assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
    // A fresh press without the flick opens the parachute.
    b.step(wingsuit, &Controls::default(), DT, &CONFIG);
    let step = b.step(wingsuit, &held, DT, &CONFIG);
    assert_eq!(player(&step, OPEN_PARACHUTE), Some(Want::Hold(1.0)));
}

#[test]
fn a_stick_held_over_in_the_wingsuit_lets_a_open_the_parachute() {
    let mut b = Bindings::default();
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    let banked = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.9, 0.0));
    let ticks = (CONFIG.evade_window_s / DT).ceil() as usize + 2;
    for _ in 0..ticks {
        b.step(wingsuit, &banked, DT, &CONFIG);
    }
    let mut press = banked;
    press.hands[Hand::Right.index()].a = down();
    let step = b.step(wingsuit, &press, DT, &CONFIG);
    assert_eq!(player(&step, OPEN_PARACHUTE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, WINGSUIT_EVADE), None);
}

#[test]
fn a_flick_out_of_rest_evades_within_the_window() {
    let mut b = Bindings::default();
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    b.step(wingsuit, &Controls::default(), DT, &CONFIG);
    let pushed = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.0, -0.9));
    b.step(wingsuit, &pushed, DT, &CONFIG);
    let mut press = pushed;
    press.hands[Hand::Right.index()].a = down();
    let step = b.step(wingsuit, &press, DT, &CONFIG);
    assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
}

#[test]
fn falling_and_the_parachute_keep_the_hand_weapon_bindings() {
    let c = controls(Hand::Right, |dom, _| {
        dom.trigger_press = down();
        dom.b = down();
        dom.stick_click = down();
    });
    for variant in [AirborneVariant::Falling, AirborneVariant::Parachute] {
        let mut b = Bindings::default();
        let step = b.step(Mode::Airborne(variant), &c, DT, &CONFIG);
        for action in [FIRE_RIGHT, R2, RELOAD, THROW_GRENADE] {
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
    let step = b.step(Mode::Airborne(AirborneVariant::Wingsuit), &c, DT, &CONFIG);
    for action in [FIRE_WINGSUIT_WEAPON_MAIN, WINGSUIT_BOOST] {
        assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{action:?}");
    }
    for action in [FIRE_RIGHT, RELOAD, THROW_GRENADE] {
        assert_eq!(player(&step, action), None, "{action:?}");
    }
}
