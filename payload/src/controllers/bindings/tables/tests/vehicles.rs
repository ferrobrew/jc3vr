//! Vehicle, wingsuit, and parachute bindings.

use super::*;
use crate::controllers::bindings::{
    bail::{self, BailLatch, BailTick},
    hold::{ActionWrite, HoldTracker, Phase},
};

#[test]
fn vehicle_stunt_routes_physical_right_a_in_land_helicopter_and_plane_modes() {
    for dominant in Hand::BOTH {
        for class in [
            VehicleClass::Car,
            VehicleClass::Helicopter,
            VehicleClass::Plane,
        ] {
            let mode = Mode::Vehicle(VehicleSeat {
                class,
                driver: true,
            });
            let mut c = Controls {
                dominant,
                ..Default::default()
            };
            c.hands[Hand::Right.index()].a = down();
            let mut b = Bindings::default();
            let pressed = b.sim_step(mode, &c, DT, &CONFIG);
            assert_eq!(
                player(&pressed, STUNT_JUMP),
                None,
                "a stunt waits for the release"
            );
            c.hands[Hand::Right.index()].a = up();
            let step = b.sim_step(mode, &c, DT, &CONFIG);
            assert_eq!(
                player(&step, STUNT_JUMP),
                Some(Want::Hold(1.0)),
                "physical right A in {class:?} with {dominant:?} dominance",
            );

            let mut c = Controls {
                dominant,
                ..Default::default()
            };
            c.hands[Hand::Left.index()].a = down();
            let mut b = Bindings::default();
            b.sim_step(mode, &c, DT, &CONFIG);
            c.hands[Hand::Left.index()].a = up();
            let step = b.sim_step(mode, &c, DT, &CONFIG);
            assert_eq!(
                player(&step, STUNT_JUMP),
                None,
                "physical left A in {class:?} with {dominant:?} dominance",
            );
        }
    }
}

#[test]
fn the_car_reads_left_a_for_handbrake_and_right_a_for_stunt() {
    let mode = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    });
    for dominant in Hand::BOTH {
        let mut c = Controls {
            dominant,
            ..Default::default()
        };
        c.hands[Hand::Left.index()].a = down();
        c.hands[Hand::Right.index()].a = down();
        let mut b = Bindings::default();
        let step = b.sim_step(mode, &c, DT, &CONFIG);
        assert_eq!(player(&step, HANDBRAKE), Some(Want::Hold(1.0)));
        c.hands[Hand::Right.index()].a = up();
        let step = b.sim_step(mode, &c, DT, &CONFIG);
        assert_eq!(player(&step, STUNT_JUMP), Some(Want::Hold(1.0)));
    }
}

/// The right A: a tap takes the stunt position on its release, and a hold exits at the moment the
/// press becomes long, with a one-tick tap of the gamepad's Y however long the press goes on: the
/// game's exit task leaves the seat only on a release held no longer than its time.
#[test]
fn the_right_a_taps_for_the_stunt_and_holds_to_exit() {
    for class in [
        VehicleClass::Car,
        VehicleClass::Motorcycle,
        VehicleClass::Sea,
        VehicleClass::Helicopter,
        VehicleClass::Plane,
    ] {
        for driver in [false, true] {
            let mode = Mode::Vehicle(VehicleSeat { class, driver });
            for dominant in Hand::BOTH {
                let held = {
                    let mut c = Controls {
                        dominant,
                        ..Default::default()
                    };
                    c.hands[Hand::Right.index()].a = down();
                    c
                };
                let released = {
                    let mut c = held;
                    c.hands[Hand::Right.index()].a = up();
                    c
                };
                // A short press: no exit, the stunt on the release.
                let mut b = Bindings::default();
                let first = b.sim_step(mode, &held, DT, &CONFIG);
                assert_eq!(player(&first, EXIT_VEHICLE), None);
                assert_eq!(player(&first, STUNT_JUMP), None);
                let last = b.sim_step(mode, &released, DT, &CONFIG);
                assert_eq!(player(&last, STUNT_JUMP), Some(Want::Hold(1.0)), "{mode:?}");
                assert_eq!(player(&last, EXIT_VEHICLE), None);
                // A long press: the gamepad's Y on the one tick the press becomes long, and no
                // stunt on the release.
                let mut b = Bindings::default();
                let mut t = 0.0;
                let mut exits = 0;
                while t < CONFIG.long_press_s * 4.0 {
                    let step = b.sim_step(mode, &held, DT, &CONFIG);
                    let exit = player(&step, EXIT_VEHICLE).is_some();
                    let becomes_long = t >= CONFIG.long_press_s && t - DT < CONFIG.long_press_s;
                    assert_eq!(exit, becomes_long, "{mode:?} at {t} s");
                    for action in [ENTER_VEHICLE, BTN_Y] {
                        assert_eq!(player(&step, action).is_some(), exit, "{action:?} at {t} s");
                    }
                    assert_eq!(player(&step, STUNT_JUMP), None);
                    exits += usize::from(exit);
                    t += DT;
                }
                assert_eq!(exits, 1, "{mode:?}");
                let last = b.sim_step(mode, &released, DT, &CONFIG);
                assert_eq!(player(&last, STUNT_JUMP), None);
                assert_eq!(player(&last, EXIT_VEHICLE), None);
            }
        }
    }
}

/// Holds the right A for four long-press times in `mode`, the bail's choice for each tick coming
/// from `bail` given the elapsed time and whether the press had become long by the last tick, then
/// lets go, returning for each tick whether `OPEN_PARACHUTE` and whether the exit (`EXIT_VEHICLE`
/// or the gamepad's Y) were held.
fn long_exit(mode: Mode, mut bail: impl FnMut(f32, bool) -> bool) -> Vec<(f32, bool, bool)> {
    let Mode::Vehicle(seat) = mode else {
        panic!("{mode:?} is not a seat");
    };
    let mut c = Controls::default();
    c.hands[Hand::Right.index()].a = down();
    let mut b = Bindings::default();
    let mut ticks = Vec::new();
    let mut t = 0.0;
    let mut step = |b: &mut Bindings, c: &mut Controls, t: f32| {
        c.bail = bail(t, b.exit_press_long(seat, CONFIG.long_press_s));
        let step = b.sim_step(mode, c, DT, &CONFIG);
        (
            t,
            player(&step, OPEN_PARACHUTE).is_some(),
            player(&step, EXIT_VEHICLE).is_some() || player(&step, BTN_Y).is_some(),
        )
    };
    while t < CONFIG.long_press_s * 4.0 {
        ticks.push(step(&mut b, &mut c, t));
        t += DT;
    }
    c.hands[Hand::Right.index()].a = up();
    ticks.push(step(&mut b, &mut c, t));
    ticks
}

/// Where the long press bails, the long right A holds `OPEN_PARACHUTE` from the moment the press
/// becomes long until it is let go, the game's parachute bail, and never taps the gamepad's Y,
/// whose exit the game would queue first.
#[test]
fn a_long_right_a_that_bails_holds_open_parachute() {
    for (class, driver) in [
        (VehicleClass::Car, true),
        (VehicleClass::Motorcycle, true),
        (VehicleClass::Car, false),
    ] {
        let mode = Mode::Vehicle(VehicleSeat { class, driver });
        let ticks = long_exit(mode, |_, _| true);
        let (last, held) = ticks.split_last().expect("ticks");
        for &(t, parachute, exit) in held {
            assert!(!exit, "{mode:?} exits at {t} s");
            assert_eq!(parachute, t >= CONFIG.long_press_s, "{mode:?} at {t} s");
        }
        assert_eq!(*last, (last.0, false, false), "{mode:?} on the release");
    }
}

/// Where it does not, the long right A taps the gamepad's Y as ever, and holds no
/// `OPEN_PARACHUTE`.
#[test]
fn a_long_right_a_that_does_not_bail_exits() {
    let mode = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    });
    let ticks = long_exit(mode, |_, _| false);
    assert!(ticks.iter().all(|&(_, parachute, _)| !parachute));
    assert_eq!(ticks.iter().filter(|&&(_, _, exit)| exit).count(), 1);
}

/// The speed the vehicle has `t` seconds into the press, for a press that crosses the bail speed
/// on the way from `from` to `to` just after it becomes long.
fn crossing(from: f32, to: f32) -> impl Fn(f32) -> f32 {
    move |t| {
        if t < CONFIG.long_press_s + DT * 1.5 {
            from
        } else {
            to
        }
    }
}

/// The bail's choice is the one made when the press became long: braking below the bail speed
/// through the hold keeps holding `OPEN_PARACHUTE` and never taps Y, and speeding up past it after
/// the exit's tap never holds `OPEN_PARACHUTE`. A vehicle hovering about the bail speed neither
/// restarts the game's hold nor taps Y.
#[test]
fn the_bail_or_the_exit_is_chosen_once_per_press() {
    let seat = VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    };
    let mode = Mode::Vehicle(seat);
    let fast = bail::PARACHUTE_BAIL_MIN_SPEED_MPS + bail::BAIL_ENTER_MARGIN_MPS + 1.0;
    let slow = bail::PARACHUTE_BAIL_MIN_SPEED_MPS - 1.0;
    // Inside the hysteresis band, a vehicle that was fast stays fast.
    let near = bail::PARACHUTE_BAIL_MIN_SPEED_MPS + bail::BAIL_LEAVE_MARGIN_MPS + 0.1;
    let hover = move |t: f32| {
        if ((t / DT) as u32).is_multiple_of(2) {
            fast
        } else {
            near
        }
    };
    let run = |speed: &dyn Fn(f32) -> f32| {
        let mut latch = BailLatch::new();
        long_exit(mode, |t, press_long| {
            latch.step(BailTick {
                seat_bails: true,
                speed_mps: Some(speed(t)),
                press_long,
            })
        })
    };
    for (name, speed, bails) in [
        (
            "braking",
            &crossing(fast, slow) as &dyn Fn(f32) -> f32,
            true,
        ),
        ("speeding up", &crossing(slow, fast), false),
        ("hovering", &hover, true),
    ] {
        let ticks = run(speed);
        let (_, held) = ticks.split_last().expect("ticks");
        for &(t, parachute, exit) in held {
            let long = t >= CONFIG.long_press_s;
            assert_eq!(parachute, bails && long, "{name}: the parachute at {t} s");
            if bails {
                assert!(!exit, "{name}: an exit at {t} s");
            }
        }
        let exits = ticks.iter().filter(|&&(_, _, exit)| exit).count();
        assert_eq!(exits, usize::from(!bails), "{name}: the exits");
    }
}

#[test]
fn the_bail_seats_are_the_drivers_riders_and_gunners_of_land_vehicles() {
    let seat = |class, driver| VehicleSeat { class, driver };
    assert!(bail::bails_by_parachute(
        seat(VehicleClass::Car, true),
        false
    ));
    assert!(bail::bails_by_parachute(
        seat(VehicleClass::Motorcycle, true),
        false
    ));
    assert!(bail::bails_by_parachute(
        seat(VehicleClass::Car, false),
        true
    ));
    assert!(!bail::bails_by_parachute(
        seat(VehicleClass::Car, false),
        false
    ));
    assert!(!bail::bails_by_parachute(
        seat(VehicleClass::Sea, true),
        false
    ));
    assert!(!bail::bails_by_parachute(
        seat(VehicleClass::Mech, true),
        false
    ));
}

/// However long the right A is held, the game sees the exit as a tap: the press edge on the tick
/// the press becomes long, and the release on the next, after one tick's hold.
#[test]
fn a_long_hold_writes_the_exit_as_a_tap() {
    let mode = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    });
    let mut c = Controls::default();
    c.hands[Hand::Right.index()].a = down();
    let mut b = Bindings::default();
    let mut tracker = HoldTracker::new();
    let exit = |writes: &[ActionWrite]| {
        writes
            .iter()
            .find(|w| {
                w.target
                    == Target {
                        map: MapKind::Player,
                        action: EXIT_VEHICLE,
                    }
            })
            .map(|w| w.phase)
    };
    let mut phases = Vec::new();
    let ticks = (CONFIG.long_press_s * 4.0 / DT) as usize;
    for _ in 0..ticks {
        let step = b.sim_step(mode, &c, DT, &CONFIG);
        phases.extend(exit(&tracker.resolve(&step.desired, DT)));
    }
    c.hands[Hand::Right.index()].a = up();
    let step = b.sim_step(mode, &c, DT, &CONFIG);
    phases.extend(exit(&tracker.resolve(&step.desired, DT)));
    assert_eq!(
        phases,
        [Phase::Clicked, Phase::Released { seconds: DT }],
        "the exit is a one-tick tap",
    );
}

/// The upper faces fire vehicle weapons 1 and 2 while held, and on a hand holding a gun they fire
/// nothing of the vehicle's; out of the driver's seat they reach the game's fire there.
#[test]
fn the_upper_faces_fire_the_vehicle_weapons_unless_the_hand_holds_a_gun() {
    for (class, primary, secondary) in [
        (
            VehicleClass::Car,
            FIRE_VEHICLE_WEAPON_PRIMARY,
            FIRE_VEHICLE_WEAPON_SECONDARY,
        ),
        (
            VehicleClass::Helicopter,
            FIRE_VEHICLE_WEAPON_PRIMARY,
            FIRE_VEHICLE_WEAPON_SECONDARY,
        ),
        (
            VehicleClass::Plane,
            FIRE_VEHICLE_WEAPON_PRIMARY,
            FIRE_VEHICLE_WEAPON_SECONDARY,
        ),
    ] {
        for driver in [false, true] {
            let mode = Mode::Vehicle(VehicleSeat { class, driver });
            for dominant in Hand::BOTH {
                for armed in [[false, false], [true, false], [false, true], [true, true]] {
                    let mut c = controls(dominant, |dom, off| {
                        dom.b = down();
                        off.b = down();
                    });
                    c.armed = armed;
                    let mut b = Bindings::default();
                    for _ in 0..3 {
                        let step = b.sim_step(mode, &c, DT, &CONFIG);
                        let dom_armed = armed[dominant.index()];
                        let off_armed = armed[dominant.other().index()];
                        assert_eq!(
                            player(&step, primary).is_some(),
                            !dom_armed,
                            "{mode:?} {dominant:?} {armed:?}"
                        );
                        assert_eq!(
                            player(&step, secondary).is_some(),
                            !off_armed,
                            "{mode:?} {dominant:?} {armed:?}"
                        );
                        assert_eq!(
                            player(&step, FIRE_RIGHT).is_some(),
                            !driver && (dom_armed || off_armed),
                            "{mode:?} {dominant:?} {armed:?}"
                        );
                    }
                }
            }
        }
    }
}

/// The grips fire nothing in a vehicle: they only grip.
#[test]
fn the_grips_fire_nothing_in_a_vehicle() {
    for class in [
        VehicleClass::Car,
        VehicleClass::Motorcycle,
        VehicleClass::Sea,
        VehicleClass::Helicopter,
        VehicleClass::Plane,
        VehicleClass::Mech,
    ] {
        for driver in [false, true] {
            let mode = Mode::Vehicle(VehicleSeat { class, driver });
            let c = controls(Hand::Right, |dom, off| {
                dom.grip = down();
                off.grip = down();
            });
            let step = Bindings::default().sim_step(mode, &c, DT, &CONFIG);
            assert!(step.desired.is_empty(), "{mode:?}: {:?}", step.desired);
        }
    }
}

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
        off.stick_click = down();
    });
    let step = b.sim_step(car, &c, DT, &CONFIG);
    assert_eq!(player(&step, ACCELERATE), Some(Want::Analog(0.6)));
    assert_eq!(player(&step, TURN_LEFT), Some(Want::Analog(0.4)));
    assert_eq!(player(&step, TURN_RIGHT), None);
    assert_eq!(player(&step, HANDBRAKE), Some(Want::Hold(1.0)));
    // Nitrous is the dominant stick click and the grapple the off-hand's; the horn is unbound.
    assert_eq!(player(&step, USE_VEHICLE_MOD), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, SOUND_HORN_SIREN), None);
    assert_eq!(player(&step, SUBMERSIBLE_DIVE), None);
    // The off-hand's upper face fires the secondary weapon alone, not the grapple.
    let mut b = Bindings::default();
    let face = controls(Hand::Right, |_, off| off.b = down());
    let step = b.sim_step(car, &face, DT, &CONFIG);
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
        off.stick = Vec2::new(0.0, 1.0);
    });
    let step = Bindings::default().sim_step(mech(true), &c, DT, &CONFIG);
    for action in [
        MECH_FIRE_RIGHT_HAND_WEAPON,
        MECH_FIRE_GRAVITY_WEAPON_PRIMARY,
    ] {
        assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{action:?}");
    }
    // A tap of the right A jumps, on its release.
    let mut b = Bindings::default();
    b.sim_step(
        mech(true),
        &controls(Hand::Right, |dom, _| dom.a = down()),
        DT,
        &CONFIG,
    );
    let jump = b.sim_step(
        mech(true),
        &controls(Hand::Right, |dom, _| dom.a = up()),
        DT,
        &CONFIG,
    );
    assert_eq!(player(&jump, MECH_JUMP), Some(Want::Hold(1.0)));
    // The stick walks it forward, as the mech's move reads the plane's pitch.
    assert_eq!(player(&step, PLANE_PITCH_DOWN), Some(Want::Analog(1.0)));
    for action in [ACCELERATE, REVERSE, HANDBRAKE] {
        assert_eq!(player(&step, action), None, "{action:?}");
    }
    let faces = controls(Hand::Right, |dom, off| {
        dom.b = down();
        off.b = down();
    });
    let step = Bindings::default().sim_step(mech(true), &faces, DT, &CONFIG);
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
        let mut c = controls(Hand::Right, |_, off| off.stick_click = b);
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
fn a_flicked_a_in_the_wingsuit_opens_the_parachute_and_never_evades() {
    let mut b = Bindings::default();
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    b.sim_step(wingsuit, &Controls::default(), DT, &CONFIG);
    let flick = controls(Hand::Right, |dom, off| {
        dom.a = down();
        off.stick = Vec2::new(0.9, 0.0);
    });
    let step = b.sim_step(wingsuit, &flick, DT, &CONFIG);
    assert_eq!(player(&step, OPEN_PARACHUTE), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, WINGSUIT_EVADE), None);
}

#[test]
fn the_wingsuit_triggers_keep_physical_sides_and_grips_fire_no_rockets() {
    let mode = Mode::Airborne(AirborneVariant::Wingsuit);
    for dominant in Hand::BOTH {
        for hand in Hand::BOTH {
            let mut c = Controls {
                dominant,
                ..Default::default()
            };
            c.hands[hand.index()].trigger_press = down();
            let step = Bindings::default().sim_step(mode, &c, DT, &CONFIG);
            let (own, other) = if hand == Hand::Left {
                (FIRE_WINGSUIT_WEAPON_MAIN, FIRE_WINGSUIT_WEAPON_SECONDARY)
            } else {
                (FIRE_WINGSUIT_WEAPON_SECONDARY, FIRE_WINGSUIT_WEAPON_MAIN)
            };
            assert_eq!(player(&step, own), Some(Want::Hold(1.0)));
            for action in [other, R2, CANCEL, WINGSUIT_AIRBRAKE] {
                assert_eq!(
                    player(&step, action),
                    None,
                    "{dominant:?}, {hand:?}, {action:?}"
                );
            }

            let mut c = Controls {
                dominant,
                ..Default::default()
            };
            c.hands[hand.index()].grip = down();
            let step = Bindings::default().sim_step(mode, &c, DT, &CONFIG);
            assert_eq!(player(&step, FIRE_WINGSUIT_WEAPON_SECONDARY), None);
        }
    }
}

#[test]
fn the_wingsuit_uses_physical_right_b_for_boost_and_a_for_canopy() {
    let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
    for dominant in Hand::BOTH {
        let mut c = Controls {
            dominant,
            ..Default::default()
        };
        c.hands[Hand::Right.index()].b = down();
        let step = Bindings::default().sim_step(wingsuit, &c, DT, &CONFIG);
        assert_eq!(player(&step, WINGSUIT_BOOST), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, OPEN_WINGSUIT), None);
        assert_eq!(player(&step, FIRE_GRAPPLE), None);

        let mut c = Controls {
            dominant,
            ..Default::default()
        };
        c.hands[Hand::Right.index()].a = down();
        let step = Bindings::default().sim_step(wingsuit, &c, DT, &CONFIG);
        assert_eq!(player(&step, OPEN_PARACHUTE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, OPEN_WINGSUIT), None);

        let click = controls(dominant, |dom, _| dom.stick_click = down());
        let step = Bindings::default().sim_step(wingsuit, &click, DT, &CONFIG);
        assert_eq!(player(&step, WINGSUIT_BOOST), None);
    }
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
    let c = controls(Hand::Right, |dom, off| {
        dom.trigger_press = down();
        off.trigger_press = down();
        dom.b = down();
    });
    let step = b.sim_step(Mode::Airborne(AirborneVariant::Wingsuit), &c, DT, &CONFIG);
    for action in [
        FIRE_WINGSUIT_WEAPON_MAIN,
        FIRE_WINGSUIT_WEAPON_SECONDARY,
        WINGSUIT_BOOST,
    ] {
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

#[test]
fn the_modes_whose_triggers_fire_the_hand_weapons() {
    let seat = |driver| {
        Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver,
        })
    };
    let cases = [
        (Mode::OnFoot(OnFootVariant::Ground), true),
        (Mode::OnFoot(OnFootVariant::Swimming), true),
        (Mode::Airborne(AirborneVariant::Falling), true),
        (Mode::Airborne(AirborneVariant::Parachute), true),
        (Mode::Airborne(AirborneVariant::Wingsuit), false),
        (seat(false), true),
        (seat(true), false),
        (Mode::Ui, false),
    ];
    for (mode, fires) in cases {
        assert_eq!(triggers_fire_hand_weapons(mode), fires, "{mode:?}");
    }
}
